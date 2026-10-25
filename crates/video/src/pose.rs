//! Pose (keypoint) model outputs → keypoints in frame pixels, plus pose-derived
//! features for the behavior classifier. Inference runs in Core ML (Swift); this
//! module owns the decoding math so it is identical everywhere and testable.

use crate::Pose;

/// Peak of each keypoint heatmap with quadratic subpixel refinement.
/// `data` is `[K, H, W]` row-major (or `[H, W, K]` when `channels_last`).
/// Returns `(x, y, confidence)` in heatmap pixels; confidence is the peak value.
pub fn decode_heatmaps(data: &[f32], k: usize, h: usize, w: usize, channels_last: bool) -> Result<Vec<(f64, f64, f64)>, String> {
    if data.len() != k * h * w || h < 2 || w < 2 {
        return Err(format!("heatmap has {} values, expected {k}×{h}×{w}", data.len()));
    }
    let at = |c: usize, y: usize, x: usize| if channels_last { data[(y * w + x) * k + c] } else { data[(c * h + y) * w + x] };
    Ok((0..k)
        .map(|c| {
            let (mut by, mut bx, mut best) = (0, 0, f32::NEG_INFINITY);
            for y in 0..h {
                for x in 0..w {
                    let v = at(c, y, x);
                    if v > best {
                        (best, by, bx) = (v, y, x);
                    }
                }
            }
            // Fit a parabola through the peak and its neighbours on each axis.
            let refine = |m: f32, l: f32, r: f32| {
                let d = l - 2.0 * m + r;
                if d < 0.0 { (0.5 * (l - r) / d).clamp(-0.5, 0.5) as f64 } else { 0.0 }
            };
            let dx = if bx > 0 && bx + 1 < w { refine(best, at(c, by, bx - 1), at(c, by, bx + 1)) } else { 0.0 };
            let dy = if by > 0 && by + 1 < h { refine(best, at(c, by - 1, bx), at(c, by + 1, bx)) } else { 0.0 };
            (bx as f64 + dx, by as f64 + dy, best as f64)
        })
        .collect())
}

/// SimCC decoding (RTMPose): each keypoint has 1-D logit vectors over x and y bins at
/// `split` bins per input pixel. Location = argmax / split (input pixels); confidence =
/// the smaller of the two peak logits, as MMPose does. `x` is `[K, Wx]`, `y` is `[K, Wy]`.
pub fn decode_simcc(x: &[f32], y: &[f32], k: usize, split: f64) -> Result<Vec<(f64, f64, f64)>, String> {
    if k == 0 || !x.len().is_multiple_of(k) || !y.len().is_multiple_of(k) || split <= 0.0 {
        return Err("SimCC outputs do not match the keypoint count".into());
    }
    let (wx, wy) = (x.len() / k, y.len() / k);
    let peak = |v: &[f32]| v.iter().enumerate().fold((0usize, f32::NEG_INFINITY), |b, (i, &e)| if e > b.1 { (i, e) } else { b });
    Ok((0..k)
        .map(|c| {
            let (ix, vx) = peak(&x[c * wx..(c + 1) * wx]);
            let (iy, vy) = peak(&y[c * wy..(c + 1) * wy]);
            (ix as f64 / split, iy as f64 / split, vx.min(vy) as f64)
        })
        .collect())
}

/// Map a point in model-input pixels (`in_w × in_h`) to frame pixels via the crop.
pub fn input_to_frame(p: (f64, f64), crop: [f64; 4], in_w: usize, in_h: usize) -> (f64, f64) {
    (crop[0] + p.0 / in_w as f64 * (crop[2] - crop[0]), crop[1] + p.1 / in_h as f64 * (crop[3] - crop[1]))
}

/// Map a keypoint from heatmap pixels back to frame pixels, given the crop the
/// model saw (`[x0, y0, x1, y1]` in frame pixels) and the heatmap size.
pub fn to_frame(p: (f64, f64), crop: [f64; 4], hm_w: usize, hm_h: usize) -> (f64, f64) {
    // Heatmap pixel centres span the crop: (i + 0.5) / n of its width.
    (crop[0] + (p.0 + 0.5) / hm_w as f64 * (crop[2] - crop[0]), crop[1] + (p.1 + 0.5) / hm_h as f64 * (crop[3] - crop[1]))
}

/// Extra per-frame classifier features from a pose: speed of each keypoint (scaled by
/// `diag`, the arena diagonal), and the distance between the first and last keypoints
/// (e.g. nose to tail base: body length). Low-confidence points become NaN.
pub fn pose_features(p: &Pose, fps: f64, diag: f64, min_conf: f64) -> Vec<(String, Vec<f64>)> {
    let n = p.frames();
    let ok = |b: usize, i: usize| p.likelihood[b][i] >= min_conf;
    let mut out = vec![];
    for (b, name) in p.bodyparts.iter().enumerate() {
        out.push((
            format!("{name}_speed"),
            (0..n)
                .map(|i| {
                    let (a, c) = (i.saturating_sub(1), (i + 1).min(n - 1));
                    if c > a && ok(b, a) && ok(b, c) { (p.x[b][c] - p.x[b][a]).hypot(p.y[b][c] - p.y[b][a]) / diag * fps / (c - a) as f64 } else { f64::NAN }
                })
                .collect(),
        ));
    }
    if p.bodyparts.len() >= 2 {
        let l = p.bodyparts.len() - 1;
        out.push((
            format!("{}_{}_dist", p.bodyparts[0], p.bodyparts[l]),
            (0..n).map(|i| if ok(0, i) && ok(l, i) { (p.x[0][i] - p.x[l][i]).hypot(p.y[0][i] - p.y[l][i]) / diag } else { f64::NAN }).collect(),
        ));
    }
    out
}

/// Single-animal DeepLabCut-style CSV, readable by [`crate::read_dlc_csv`] and DLC tools.
pub fn write_dlc_csv(p: &Pose, scorer: &str) -> String {
    let mut s = String::from("scorer");
    p.bodyparts.iter().for_each(|_| s.push_str(&format!(",{scorer},{scorer},{scorer}")));
    s.push_str("\nbodyparts");
    p.bodyparts.iter().for_each(|b| s.push_str(&format!(",{b},{b},{b}")));
    s.push_str("\ncoords");
    p.bodyparts.iter().for_each(|_| s.push_str(",x,y,likelihood"));
    s.push('\n');
    let num = |v: f64| if v.is_finite() { format!("{v:.3}") } else { String::new() };
    for i in 0..p.frames() {
        s.push_str(&i.to_string());
        for b in 0..p.bodyparts.len() {
            s.push_str(&format!(",{},{},{}", num(p.x[b][i]), num(p.y[b][i]), num(p.likelihood[b][i])));
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subpixel_peak_and_frame_mapping() {
        let (h, w) = (16, 16);
        // Gaussian centred at (5.3, 9.7), sampled on the grid.
        let hm: Vec<f32> = (0..h * w).map(|i| { let (x, y) = ((i % w) as f64, (i / w) as f64); (-((x - 5.3).powi(2) + (y - 9.7).powi(2)) / 4.0).exp() as f32 }).collect();
        let p = decode_heatmaps(&hm, 1, h, w, false).unwrap()[0];
        assert!((p.0 - 5.3).abs() < 0.1 && (p.1 - 9.7).abs() < 0.1, "{p:?}");
        // Channels-last layout gives the same answer.
        assert_eq!(decode_heatmaps(&hm, 1, h, w, true).unwrap()[0], p);
        let (fx, fy) = to_frame((7.5, 7.5), [100.0, 50.0, 164.0, 114.0], 16, 16);
        assert!((fx - 132.0).abs() < 1e-9 && (fy - 82.0).abs() < 1e-9, "centre of the crop");
        assert!(decode_heatmaps(&hm, 2, h, w, false).is_err());
    }

    #[test]
    fn simcc_decoding() {
        let (k, w) = (2, 512);
        let mut x = vec![0.0f32; k * w];
        let mut y = vec![0.0f32; k * w];
        x[100] = 0.9; y[300] = 0.7; // keypoint 0 at (50, 150) input px, conf 0.7
        x[w + 511] = 0.2; y[w] = 0.4;
        let d = decode_simcc(&x, &y, k, 2.0).unwrap();
        assert_eq!(d[0], (50.0, 150.0, 0.699999988079071));
        assert!((d[1].0 - 255.5).abs() < 1e-9 && d[1].2 < 0.21);
        assert_eq!(input_to_frame((128.0, 64.0), [100.0, 100.0, 164.0, 164.0], 256, 256), (132.0, 116.0));
        assert!(decode_simcc(&x, &y[..11], k, 2.0).is_err(), "uneven per-keypoint length");
    }

    #[test]
    fn pose_features_and_dlc_roundtrip() {
        let p = Pose { bodyparts: vec!["nose".into(), "tail_base".into()], x: vec![vec![0.0, 3.0, 6.0], vec![0.0, 0.0, 0.0]], y: vec![vec![4.0, 4.0, 4.0], vec![0.0, 0.0, 0.0]], likelihood: vec![vec![0.9; 3], vec![0.9, 0.1, 0.9]] };
        let f = pose_features(&p, 10.0, 1.0, 0.5);
        assert_eq!(f[0].0, "nose_speed");
        assert!((f[0].1[1] - 30.0).abs() < 1e-9);
        assert!(f[2].1[1].is_nan(), "low-confidence tail base");
        assert!((f[2].1[0] - 4.0).abs() < 1e-9);
        let back = crate::read_dlc_csv(&write_dlc_csv(&p, "simplefp")).unwrap();
        assert_eq!(back.bodyparts, p.bodyparts);
        assert!((back.x[0][2] - 6.0).abs() < 1e-9);
    }
}
