//! Classical single-animal tracking on grayscale frames (no model, no training):
//! median background, robust per-frame threshold, morphological opening (drops a
//! thin patch cord), largest connected blob → centroid, area, orientation; plus
//! frame-difference motion energy. This is the always-available baseline the
//! learned models are compared against (ezTrack-style; Pennington et al. 2019).
//!
//! Frames are row-major `u8` luma, typically decoded small (e.g. 160×120): a mouse
//! is still dozens of pixels and every step stays O(pixels).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackParams {
    /// Animal darker than the background (black mouse on white floor).
    pub dark_animal: bool,
    /// Fixed foreground threshold (gray levels); `None` = robust automatic per frame.
    #[serde(default)]
    pub threshold: Option<u8>,
    /// Blobs smaller than this fraction of the ROI are ignored (noise, cord, droppings).
    pub min_area_frac: f64,
    /// Region of interest `[x0, y0, x1, y1]` in frame pixels (e.g. the arena floor).
    #[serde(default)]
    pub roi: Option<[usize; 4]>,
}

impl Default for TrackParams {
    fn default() -> Self {
        TrackParams { dark_animal: true, threshold: None, min_area_frac: 0.002, roi: None }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Detection {
    /// Centroid (frame pixels); NaN when no animal-sized blob was found.
    pub x: f64,
    pub y: f64,
    /// Blob area in pixels (0 when not found).
    pub area: f64,
    /// Body axis angle (radians, image coordinates) from second moments.
    pub angle: f64,
    /// Mean absolute luma change from the previous frame inside the ROI (gray levels).
    pub motion: f64,
    /// Fraction of ROI pixels that changed by more than the threshold since the previous frame.
    pub motion_frac: f64,
}

/// Per-pixel median of sampled frames: with the animal moving, the median is the empty arena.
pub fn median_background(samples: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let n = samples.first().map(Vec::len).ok_or("no background samples")?;
    if samples.iter().any(|s| s.len() != n) {
        return Err("background samples differ in size".into());
    }
    let mut col = vec![0u8; samples.len()];
    Ok((0..n)
        .map(|i| {
            samples.iter().zip(col.iter_mut()).for_each(|(s, c)| *c = s[i]);
            let (_, m, _) = col.select_nth_unstable(samples.len() / 2);
            *m
        })
        .collect())
}

/// Find a bright arena floor in the background: Otsu split of luma, largest bright
/// 4-connected region, its bounding box shrunk by `margin` px. `None` when no
/// region covers at least 5 % of the frame (dark arenas: draw the ROI by hand).
pub fn auto_arena(bg: &[u8], w: usize, h: usize, margin: usize) -> Option<[usize; 4]> {
    let mut hist = [0usize; 256];
    bg.iter().for_each(|&p| hist[p as usize] += 1);
    let total = bg.len() as f64;
    let sum_all: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();
    let (mut wb, mut sb, mut best, mut cut) = (0.0, 0.0, -1.0, 128);
    for (i, &c) in hist.iter().enumerate() {
        wb += c as f64;
        sb += i as f64 * c as f64;
        let wf = total - wb;
        if wb == 0.0 || wf == 0.0 {
            continue;
        }
        let between = wb * wf * (sb / wb - (sum_all - sb) / wf).powi(2);
        if between > best {
            best = between;
            cut = i;
        }
    }
    let mut mask: Vec<bool> = bg.iter().map(|&p| p as usize > cut).collect();
    // Erode enough to cut thin bright bridges to the bench or floor, keep the
    // largest core, then grow it back the same amount (constrained to bright pixels).
    let r = (w / 40).max(2);
    let bright = mask.clone();
    for _ in 0..r {
        mask = morph(&mask, w, h, false);
    }
    // An arena is enclosed in the view; floors and benches run off the frame edge.
    let core = blobs(&mask, w, h).into_iter().filter(|b| !b.iter().any(|&i| i % w <= r || i % w + r + 1 >= w || i / w <= r || i / w + r + 1 >= h)).max_by_key(Vec::len)?;
    let mut keep = vec![false; bg.len()];
    core.iter().for_each(|&i| keep[i] = true);
    for _ in 0..r {
        keep = morph(&keep, w, h, true).iter().zip(&bright).map(|(a, b)| *a && *b).collect();
    }
    let blob: Vec<usize> = (0..bg.len()).filter(|&i| keep[i]).collect();
    if (blob.len() as f64) < 0.05 * total {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for &i in &blob {
        let (x, y) = (i % w, i / w);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
    }
    (x1 > x0 + 2 * margin + 4 && y1 > y0 + 2 * margin + 4).then_some([x0 + margin, y0 + margin, x1 - margin, y1 - margin])
}

pub struct Tracker {
    w: usize,
    h: usize,
    bg: Vec<u8>,
    prev: Option<Vec<u8>>,
    p: TrackParams,
    roi: [usize; 4],
}

impl Tracker {
    pub fn new(bg: Vec<u8>, w: usize, h: usize, p: TrackParams) -> Result<Tracker, String> {
        if bg.len() != w * h || w < 8 || h < 8 {
            return Err(format!("background is {} bytes, expected {w}×{h}", bg.len()));
        }
        let roi = p.roi.unwrap_or([0, 0, w, h]);
        if !(roi[0] < roi[2] && roi[1] < roi[3] && roi[2] <= w && roi[3] <= h) {
            return Err(format!("ROI {roi:?} is outside the {w}×{h} frame"));
        }
        if !(0.0..0.5).contains(&p.min_area_frac) {
            return Err("min_area_frac must be in [0, 0.5)".into());
        }
        Ok(Tracker { w, h, bg, prev: None, p, roi })
    }

    fn in_roi(&self, x: usize, y: usize) -> bool {
        x >= self.roi[0] && x < self.roi[2] && y >= self.roi[1] && y < self.roi[3]
    }

    pub fn step(&mut self, frame: &[u8]) -> Result<Detection, String> {
        let (w, h) = (self.w, self.h);
        if frame.len() != w * h {
            return Err(format!("frame is {} bytes, expected {}", frame.len(), w * h));
        }
        let roi_px = (self.roi[2] - self.roi[0]) * (self.roi[3] - self.roi[1]);
        // Signed difference toward the animal's polarity, minus the frame's global
        // shift (camera auto-exposure moves the whole arena, which is not an animal).
        let raw: Vec<i16> = frame.iter().zip(&self.bg).map(|(&f, &b)| if self.p.dark_animal { b as i16 - f as i16 } else { f as i16 - b as i16 }).collect();
        let shift = {
            let mut v: Vec<i16> = (0..w * h).filter(|&i| self.in_roi(i % w, i / w)).map(|i| raw[i]).collect();
            let k = v.len() / 2;
            *v.select_nth_unstable(k).1
        };
        let diff: Vec<i16> = raw.iter().map(|d| d - shift).collect();
        let thr = match self.p.threshold {
            Some(t) => t as i16,
            None => {
                // Most of the ROI is background: robust noise scale of |diff| sets the bar.
                let mut v: Vec<i16> = (0..w * h).filter(|&i| self.in_roi(i % w, i / w)).map(|i| diff[i].abs()).collect();
                let k = v.len() / 2;
                let (_, med, _) = v.select_nth_unstable(k);
                // Floor of 30 gray levels: a real animal contrasts far more than sensor or
                // compression noise on a flat floor ever does.
                (*med as f64 * 1.4826 * 6.0).clamp(30.0, 80.0) as i16
            }
        };
        let mut mask: Vec<bool> = (0..w * h).map(|i| diff[i] > thr && self.in_roi(i % w, i / w)).collect();
        mask = morph(&morph(&mask, w, h, false), w, h, true); // opening: erode then dilate
        let blob = largest_blob(&mask, w, h);
        let min_area = (self.p.min_area_frac * roi_px as f64).max(4.0);
        let (x, y, area, angle) = match blob {
            Some(b) if b.len() as f64 >= min_area => {
                let n = b.len() as f64;
                let (mx, my) = (b.iter().map(|&i| (i % w) as f64).sum::<f64>() / n, b.iter().map(|&i| (i / w) as f64).sum::<f64>() / n);
                let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
                for &i in &b {
                    let (dx, dy) = ((i % w) as f64 - mx, (i / w) as f64 - my);
                    sxx += dx * dx;
                    syy += dy * dy;
                    sxy += dx * dy;
                }
                (mx, my, n, 0.5 * (2.0 * sxy).atan2(sxx - syy))
            }
            _ => (f64::NAN, f64::NAN, 0.0, f64::NAN),
        };
        let (motion, motion_frac) = match &self.prev {
            Some(prev) => {
                let (mut sum, mut changed) = (0.0, 0usize);
                for yy in self.roi[1]..self.roi[3] {
                    for xx in self.roi[0]..self.roi[2] {
                        let d = (frame[yy * w + xx] as i16 - prev[yy * w + xx] as i16).unsigned_abs();
                        sum += d as f64;
                        changed += (d as i16 > thr) as usize;
                    }
                }
                (sum / roi_px as f64, changed as f64 / roi_px as f64)
            }
            None => (f64::NAN, f64::NAN),
        };
        self.prev = Some(frame.to_vec());
        Ok(Detection { x, y, area, angle, motion, motion_frac })
    }
}

/// 3×3 erosion (`dilate = false`) or dilation of a binary mask.
fn morph(m: &[bool], w: usize, h: usize, dilate: bool) -> Vec<bool> {
    let mut out = vec![false; m.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc = !dilate;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                    let v = xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h && m[yy as usize * w + xx as usize];
                    acc = if dilate { acc || v } else { acc && v };
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Pixel indices of the largest 4-connected component.
fn largest_blob(m: &[bool], w: usize, h: usize) -> Option<Vec<usize>> {
    blobs(m, w, h).into_iter().max_by_key(Vec::len)
}

/// All 4-connected components.
fn blobs(m: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; m.len()];
    let mut all = vec![];
    let mut stack = vec![];
    for start in 0..m.len() {
        if !m[start] || seen[start] {
            continue;
        }
        let mut comp = vec![];
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if m[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        all.push(comp);
    }
    all
}

/// Bouts (frame index ranges, end exclusive) where `motion` stays below `threshold`
/// for at least `min_frames`. Standard automated immobility scoring (e.g. tail
/// suspension, freezing); the threshold is a calibration knob, check it against a
/// human scorer with [`crate::frame_agreement`].
pub fn immobility(motion: &[f64], threshold: f64, min_frames: usize) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start = None;
    for (i, &m) in motion.iter().chain(std::iter::once(&f64::INFINITY)).enumerate() {
        let still = m.is_finite() && m < threshold;
        match (still, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min_frames.max(1) {
                    out.push((s, i));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// A data-driven immobility threshold: the valley between the "still" and "moving"
/// modes of log motion (Otsu on the log histogram). A starting point to review, not ground truth.
pub fn suggest_threshold(motion: &[f64]) -> Option<f64> {
    let v: Vec<f64> = motion.iter().filter(|m| m.is_finite() && **m > 0.0).map(|m| m.ln()).collect();
    if v.len() < 20 {
        return None;
    }
    let (lo, hi) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    if hi - lo < 1e-9 {
        return None;
    }
    const B: usize = 128;
    let mut hist = [0usize; B];
    for x in &v {
        hist[(((x - lo) / (hi - lo)) * (B - 1) as f64) as usize] += 1;
    }
    let total = v.len() as f64;
    let sum_all: f64 = (0..B).map(|i| i as f64 * hist[i] as f64).sum();
    let (mut wb, mut sb, mut best, mut cut) = (0.0, 0.0, -1.0, 0);
    for (i, &c) in hist.iter().enumerate() {
        wb += c as f64;
        sb += i as f64 * c as f64;
        let wf = total - wb;
        if wb == 0.0 || wf == 0.0 {
            continue;
        }
        let between = wb * wf * (sb / wb - (sum_all - sb) / wf).powi(2);
        if between > best {
            best = between;
            cut = i;
        }
    }
    Some((lo + (cut as f64 + 0.5) / (B - 1) as f64 * (hi - lo)).exp())
}

/// Per-frame behavior features on the block clock (only frames the camera epoc timestamps).
#[derive(Debug, Clone, Serialize)]
pub struct Features {
    pub t: Vec<f64>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Centroid speed (px/s, or units/s after `scale`), NaN across lost frames.
    pub speed: Vec<f64>,
    pub motion: Vec<f64>,
    pub immobile: Vec<bool>,
    pub immobility_threshold: f64,
    /// Block times of immobility bouts `(start, end)`.
    pub bouts: Vec<(f64, f64)>,
    /// Fraction of frames with no animal-sized blob.
    pub lost_frac: f64,
}

/// Derive features from detections. `threshold = None` uses [`suggest_threshold`].
pub fn features(clock: &crate::FrameClock, dets: &[Detection], scale: f64, threshold: Option<f64>, min_immobile_s: f64) -> Result<Features, String> {
    let n = clock.frames().map_or(dets.len(), |f| f.min(dets.len()));
    let t: Vec<f64> = (0..n).map(|k| clock.time(k).unwrap_or(f64::NAN)).collect();
    let (x, y): (Vec<f64>, Vec<f64>) = dets[..n].iter().map(|d| (d.x, d.y)).unzip();
    // No animal in view means no mobility measure (an empty arena is not "immobile").
    let motion: Vec<f64> = dets[..n].iter().map(|d| if d.x.is_nan() { f64::NAN } else { d.motion }).collect();
    let thr = threshold.or_else(|| suggest_threshold(&motion)).ok_or("not enough frames to suggest an immobility threshold")?;
    let min_frames = (min_immobile_s * clock.fps).round().max(1.0) as usize;
    let idx = immobility(&motion, thr, min_frames);
    let mut immobile = vec![false; n];
    idx.iter().for_each(|&(a, b)| immobile[a..b].iter_mut().for_each(|v| *v = true));
    let end_t = |b: usize| if b < n { t[b] } else { t[n - 1] + 1.0 / clock.fps };
    Ok(Features {
        speed: crate::speed(&x, &y, clock, scale),
        bouts: idx.iter().map(|&(a, b)| (t[a], end_t(b))).collect(),
        lost_frac: x.iter().filter(|v| v.is_nan()).count() as f64 / n.max(1) as f64,
        t,
        x,
        y,
        motion,
        immobile,
        immobility_threshold: thr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: usize, h: usize, mouse: Option<(usize, usize)>, cord: bool) -> Vec<u8> {
        let mut f = vec![200u8; w * h];
        if cord {
            (0..h).for_each(|y| f[y * w + w / 2] = 40); // 1-px dark patch cord across the frame
        }
        if let Some((cx, cy)) = mouse {
            for y in cy - 4..=cy + 4 {
                for x in cx - 7..=cx + 7 {
                    f[y * w + x] = 30;
                }
            }
        }
        f
    }

    #[test]
    fn finds_mouse_ignores_cord_and_measures_motion() {
        let (w, h) = (160, 120);
        let samples: Vec<Vec<u8>> = (0..9).map(|k| frame(w, h, Some((20 + 14 * k, 30 + 6 * k)), false)).collect();
        let bg = median_background(&samples).unwrap();
        assert!(bg.iter().all(|&p| p == 200), "median removes the moving animal");
        // A bright floor surrounded by a dark bench is found as the arena.
        let mut scene = vec![20u8; w * h];
        (30..90).for_each(|y| (50..130).for_each(|x| scene[y * w + x] = 220));
        // A thin bright bridge to the frame edge must not enlarge the arena,
        // and a brighter region touching the edge (lab floor) is not the arena.
        (0..50).for_each(|x| scene[60 * w + x] = 220);
        (0..h).for_each(|y| (140..w).for_each(|x| scene[y * w + x] = 255));
        assert_eq!(auto_arena(&scene, w, h, 2), Some([52, 32, 128, 88]));
        assert_eq!(auto_arena(&vec![20u8; w * h], w, h, 2), None);
        let mut t = Tracker::new(bg, w, h, TrackParams::default()).unwrap();
        let d0 = t.step(&frame(w, h, Some((40, 60)), true)).unwrap();
        assert!((d0.x - 40.0).abs() < 0.5 && (d0.y - 60.0).abs() < 0.5, "{d0:?}");
        assert!(d0.motion.is_nan());
        assert!(d0.angle.abs() < 0.1, "elongated along x: {}", d0.angle);
        let d1 = t.step(&frame(w, h, Some((40, 60)), true)).unwrap();
        assert_eq!(d1.motion, 0.0);
        let d2 = t.step(&frame(w, h, Some((60, 60)), true)).unwrap();
        assert!(d2.motion > 0.0 && (d2.x - 60.0).abs() < 0.5);
        let gone = t.step(&frame(w, h, None, true)).unwrap();
        assert!(gone.x.is_nan(), "a thin cord is not an animal");
    }

    #[test]
    fn immobility_bouts_and_threshold() {
        let m: Vec<f64> = (0..100).map(|i| if (20..50).contains(&i) || (70..72).contains(&i) { 0.2 } else { 5.0 }).collect();
        assert_eq!(immobility(&m, 1.0, 10), vec![(20, 50)]);
        let t = suggest_threshold(&m).unwrap();
        assert!(t > 0.2 && t < 5.0, "{t}");
        assert!(Tracker::new(vec![0; 10], 160, 120, TrackParams::default()).is_err());
    }
}
