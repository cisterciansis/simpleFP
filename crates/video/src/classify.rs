//! Supervised behavior detection trained on a lab's own scoring.
//!
//! frames → windowed kinematic features → gradient-boosted trees (binary, one per
//! behavior) → per-frame probability → hysteresis bouts → model proposals that a
//! person reviews. Evaluation holds out whole time blocks or whole sessions:
//! neighbouring frames are nearly identical, so a random frame split would leak
//! and overstate accuracy.

use crate::track::Detection;
use crate::{Annotation, EventMatch, FrameAgreement, Review, Source};
use serde::{Deserialize, Serialize};

pub const FEATURE_VERSION: u32 = 1;

/// Frames × features (row-major), with column names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Features {
    pub names: Vec<String>,
    pub rows: Vec<Vec<f32>>,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let k = v.len() / 2;
    *v.select_nth_unstable_by(k, f64::total_cmp).1
}

/// Centered rolling mean and std over `half` frames each side, ignoring NaN.
fn rolling(x: &[f64], half: usize) -> (Vec<f64>, Vec<f64>) {
    let n = x.len();
    let (mut s, mut s2, mut c) = (vec![0.0; n + 1], vec![0.0; n + 1], vec![0.0; n + 1]);
    for i in 0..n {
        let (v, ok) = if x[i].is_finite() { (x[i], 1.0) } else { (0.0, 0.0) };
        s[i + 1] = s[i] + v;
        s2[i + 1] = s2[i] + v * v;
        c[i + 1] = c[i] + ok;
    }
    (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(n));
            let k = c[b] - c[a];
            if k == 0.0 {
                return (f64::NAN, f64::NAN);
            }
            let m = (s[b] - s[a]) / k;
            (m, ((s2[b] - s2[a]) / k - m * m).max(0.0).sqrt())
        })
        .unzip()
}

/// Kinematic features from tracking (scale-free: normalized by the arena), plus any
/// `extra` per-frame columns (e.g. pose-derived), each with 0.5 s and 2 s windows.
pub fn frame_features(dets: &[Detection], fps: f64, roi: [usize; 4], extra: &[(String, Vec<f64>)]) -> Result<Features, String> {
    let n = dets.len();
    if n < 3 || !fps.is_finite() || fps <= 0.0 {
        return Err("need at least 3 frames and a frame rate".into());
    }
    if let Some((name, _)) = extra.iter().find(|(_, v)| v.len() != n) {
        return Err(format!("extra feature '{name}' has the wrong length"));
    }
    let (rw, rh) = ((roi[2] - roi[0]) as f64, (roi[3] - roi[1]) as f64);
    let (cx, cy, diag) = (roi[0] as f64 + rw / 2.0, roi[1] as f64 + rh / 2.0, rw.hypot(rh));
    let mut areas: Vec<f64> = dets.iter().map(|d| d.area).filter(|a| *a > 0.0).collect();
    let med_area = median(&mut areas).max(1.0);
    let present = |d: &Detection| d.x.is_finite();
    let mut base: Vec<(String, Vec<f64>)> = vec![
        ("present".into(), dets.iter().map(|d| present(d) as u8 as f64).collect()),
        (
            "speed".into(),
            (0..n)
                .map(|i| {
                    let (a, b) = (i.saturating_sub(1), (i + 1).min(n - 1));
                    ((dets[b].x - dets[a].x).hypot(dets[b].y - dets[a].y) / diag) * fps / (b - a).max(1) as f64
                })
                .collect(),
        ),
        ("motion".into(), dets.iter().map(|d| if present(d) { d.motion.ln_1p() } else { f64::NAN }).collect()),
        ("area".into(), dets.iter().map(|d| if present(d) { d.area / med_area } else { f64::NAN }).collect()),
        ("elongation".into(), dets.iter().map(|d| d.elongation).collect()),
        (
            "turn".into(),
            (0..n)
                .map(|i| {
                    if i == 0 {
                        return f64::NAN;
                    }
                    // Body axis is undirected: wrap the change into [0, π/2].
                    let d = (dets[i].angle - dets[i - 1].angle).rem_euclid(std::f64::consts::PI);
                    d.min(std::f64::consts::PI - d) * fps
                })
                .collect(),
        ),
        ("center_dist".into(), dets.iter().map(|d| (d.x - cx).hypot(d.y - cy) / (diag / 2.0)).collect()),
        ("edge_dist".into(), dets.iter().map(|d| (d.x - roi[0] as f64).min(roi[2] as f64 - d.x).min(d.y - roi[1] as f64).min(roi[3] as f64 - d.y) / (rw.min(rh) / 2.0)).collect()),
    ];
    base.extend(extra.iter().cloned());
    let mut names = vec![];
    let mut cols: Vec<Vec<f64>> = vec![];
    for (name, v) in &base {
        names.push(name.clone());
        cols.push(v.clone());
        for (tag, secs) in [("0.5s", 0.25), ("2s", 1.0)] {
            let (m, s) = rolling(v, ((secs * fps).round() as usize).max(1));
            names.push(format!("{name}_mean_{tag}"));
            cols.push(m);
            names.push(format!("{name}_std_{tag}"));
            cols.push(s);
        }
    }
    let rows = (0..n).map(|i| cols.iter().map(|c| c[i] as f32).collect()).collect();
    Ok(Features { names, rows })
}

// ---------------------------------------------------------------- boosted trees

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub trees: usize,
    pub depth: usize,
    pub learning_rate: f32,
    pub min_leaf: usize,
    pub bins: usize,
    pub subsample: f32,
    pub l2: f32,
    pub seed: u64,
}

impl Default for Params {
    fn default() -> Self {
        Params { trees: 150, depth: 3, learning_rate: 0.1, min_leaf: 20, bins: 32, subsample: 0.8, l2: 1.0, seed: 1 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Node {
    Split { feature: usize, threshold: f32, nan_left: bool, left: usize, right: usize },
    Leaf(f32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub feature_version: u32,
    pub names: Vec<String>,
    pub behavior: String,
    pub params: Params,
    base: f32,
    trees: Vec<Vec<Node>>,
}

fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

impl Model {
    fn raw(&self, row: &[f32]) -> f32 {
        let mut z = self.base;
        for t in &self.trees {
            let mut i = 0;
            loop {
                match &t[i] {
                    Node::Leaf(v) => {
                        z += v;
                        break;
                    }
                    Node::Split { feature, threshold, nan_left, left, right } => {
                        let x = row[*feature];
                        i = if x.is_nan() { if *nan_left { *left } else { *right } } else if x <= *threshold { *left } else { *right };
                    }
                }
            }
        }
        z
    }

    pub fn predict(&self, rows: &[Vec<f32>]) -> Vec<f32> {
        rows.iter().map(|r| sigmoid(self.raw(r))).collect()
    }
}

/// Train a binary classifier. Positives are up-weighted to balance classes (capped at 20×).
pub fn train(x: &[Vec<f32>], y: &[bool], names: &[String], behavior: &str, p: Params) -> Result<Model, String> {
    let (n, f) = (x.len(), names.len());
    if n != y.len() || n < 2 * p.min_leaf || x.iter().any(|r| r.len() != f) {
        return Err(format!("training data mismatch or too small ({n} frames)"));
    }
    let pos = y.iter().filter(|&&b| b).count();
    if pos < p.min_leaf || n - pos < p.min_leaf {
        return Err(format!("'{behavior}' needs at least {} positive and negative frames (have {pos} / {})", p.min_leaf, n - pos));
    }
    let wpos = ((n - pos) as f32 / pos as f32).clamp(1.0, 20.0);
    let w: Vec<f32> = y.iter().map(|&b| if b { wpos } else { 1.0 }).collect();
    // Quantile bin edges per feature; bin `bins` is NaN.
    let edges: Vec<Vec<f32>> = (0..f)
        .map(|j| {
            let mut v: Vec<f32> = x.iter().map(|r| r[j]).filter(|v| v.is_finite()).collect();
            v.sort_by(f32::total_cmp);
            let mut e: Vec<f32> = (1..p.bins).filter_map(|k| v.get(k * v.len() / p.bins).copied()).collect();
            e.dedup();
            e
        })
        .collect();
    let nan_bin = p.bins;
    let bin: Vec<Vec<u8>> = x
        .iter()
        .map(|r| (0..f).map(|j| if r[j].is_finite() { edges[j].partition_point(|&e| e < r[j]) as u8 } else { nan_bin as u8 }).collect())
        .collect();
    let prior = (pos as f32 * wpos) / (pos as f32 * wpos + (n - pos) as f32);
    let base = (prior / (1.0 - prior)).ln();
    let mut z = vec![base; n];
    let mut trees = vec![];
    let mut rng = crate::rng(p.seed);
    for _ in 0..p.trees {
        let (g, h): (Vec<f32>, Vec<f32>) = (0..n)
            .map(|i| {
                let q = sigmoid(z[i]);
                (w[i] * (q - y[i] as u8 as f32), w[i] * (q * (1.0 - q)).max(1e-6))
            })
            .unzip();
        let rows: Vec<usize> = (0..n).filter(|_| (rng() as f32) < p.subsample).collect();
        let mut nodes = vec![];
        grow(&mut nodes, &rows, &bin, &edges, &g, &h, p, 0, nan_bin);
        for i in 0..n {
            z[i] += leaf_value(&nodes, &x[i]);
        }
        trees.push(nodes);
    }
    Ok(Model { feature_version: FEATURE_VERSION, names: names.to_vec(), behavior: behavior.into(), params: p, base, trees })
}

fn leaf_value(nodes: &[Node], row: &[f32]) -> f32 {
    let mut i = 0;
    loop {
        match &nodes[i] {
            Node::Leaf(v) => return *v,
            Node::Split { feature, threshold, nan_left, left, right } => {
                let x = row[*feature];
                i = if x.is_nan() { if *nan_left { *left } else { *right } } else if x <= *threshold { *left } else { *right };
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn grow(nodes: &mut Vec<Node>, rows: &[usize], bin: &[Vec<u8>], edges: &[Vec<f32>], g: &[f32], h: &[f32], p: Params, depth: usize, nan_bin: usize) -> usize {
    let me = nodes.len();
    let (gs, hs) = rows.iter().fold((0.0f64, 0.0f64), |(a, b), &i| (a + g[i] as f64, b + h[i] as f64));
    let leaf = (-gs / (hs + p.l2 as f64)) as f32 * p.learning_rate;
    nodes.push(Node::Leaf(leaf));
    if depth >= p.depth || rows.len() < 2 * p.min_leaf {
        return me;
    }
    let score = |g: f64, h: f64| g * g / (h + p.l2 as f64);
    let parent = score(gs, hs);
    let mut best: Option<(f64, usize, usize, bool)> = None; // gain, feature, bin, nan_left
    for j in 0..edges.len() {
        let mut hg = vec![0.0f64; nan_bin + 1];
        let mut hh = vec![0.0f64; nan_bin + 1];
        let mut hc = vec![0usize; nan_bin + 1];
        for &i in rows {
            let b = bin[i][j] as usize;
            hg[b] += g[i] as f64;
            hh[b] += h[i] as f64;
            hc[b] += 1;
        }
        let (ng, nh, nc) = (hg[nan_bin], hh[nan_bin], hc[nan_bin]);
        let (mut lg, mut lh, mut lc) = (0.0, 0.0, 0usize);
        for b in 0..edges[j].len() {
            lg += hg[b];
            lh += hh[b];
            lc += hc[b];
            for nan_left in [false, true] {
                let (gl, hl, cl) = if nan_left { (lg + ng, lh + nh, lc + nc) } else { (lg, lh, lc) };
                let (gr, hr, cr) = (gs - gl, hs - hl, rows.len() - cl);
                if cl < p.min_leaf || cr < p.min_leaf {
                    continue;
                }
                let gain = score(gl, hl) + score(gr, hr) - parent;
                if gain > 1e-9 && best.is_none_or(|b| gain > b.0) {
                    best = Some((gain, j, b, nan_left));
                }
            }
        }
    }
    let Some((_, j, b, nan_left)) = best else { return me };
    let (l, r): (Vec<usize>, Vec<usize>) = rows.iter().partition(|&&i| {
        let v = bin[i][j] as usize;
        if v == nan_bin { nan_left } else { v <= b }
    });
    let left = grow(nodes, &l, bin, edges, g, h, p, depth + 1, nan_bin);
    let right = grow(nodes, &r, bin, edges, g, h, p, depth + 1, nan_bin);
    nodes[me] = Node::Split { feature: j, threshold: edges[j][b], nan_left, left, right };
    me
}

// ---------------------------------------------------------------- bouts and evaluation

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decode {
    /// Start a bout when probability reaches `on`, keep it while ≥ `off` (hysteresis).
    pub on: f32,
    pub off: f32,
    pub min_bout_s: f64,
    /// Bouts separated by less than this are merged.
    pub merge_gap_s: f64,
}

impl Default for Decode {
    fn default() -> Self {
        Decode { on: 0.6, off: 0.4, min_bout_s: 1.0, merge_gap_s: 0.5 }
    }
}

impl Decode {
    /// Decoding priors from the human bouts the model learns from: the shortest
    /// acceptable bout is a quarter of the shortest scored bout (≥ 0.5 s), and gaps
    /// shorter than a tenth of the median bout (≤ 3 s) are bridged. Keeps model
    /// bouts on the time scale the scorers actually used.
    pub fn from_labels(t: &[f64], y: &[bool]) -> Decode {
        let truth: Vec<f32> = y.iter().map(|&b| b as u8 as f32).collect();
        let mut d: Vec<f64> = decode_bouts(&truth, t, &Decode { on: 0.5, off: 0.5, min_bout_s: 0.0, merge_gap_s: 0.0 }).iter().map(|b| b.1 - b.0).collect();
        if d.is_empty() {
            return Decode::default();
        }
        d.sort_by(f64::total_cmp);
        Decode { min_bout_s: (0.25 * d[0]).max(0.5), merge_gap_s: (0.1 * d[d.len() / 2]).clamp(0.5, 3.0), ..Decode::default() }
    }
}

/// Bouts `(start_s, end_s, mean probability)` on the frame times `t`.
pub fn decode_bouts(prob: &[f32], t: &[f64], d: &Decode) -> Vec<(f64, f64, f64)> {
    let mut raw: Vec<(usize, usize)> = vec![];
    let mut start = None;
    for i in 0..=prob.len() {
        let p = prob.get(i).copied().unwrap_or(0.0);
        match start {
            None if p >= d.on => start = Some(i),
            Some(s) if p < d.off => {
                raw.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    let end_t = |i: usize| if i < t.len() { t[i] } else { t[t.len() - 1] + (t[t.len() - 1] - t[t.len() - 2]) };
    let mut merged: Vec<(usize, usize)> = vec![];
    for b in raw {
        match merged.last_mut() {
            Some(last) if t[b.0] - end_t(last.1) < d.merge_gap_s => last.1 = b.1,
            _ => merged.push(b),
        }
    }
    merged
        .into_iter()
        .filter(|&(a, b)| end_t(b) - t[a] >= d.min_bout_s)
        .map(|(a, b)| (t[a], end_t(b), prob[a..b].iter().map(|&p| p as f64).sum::<f64>() / (b - a) as f64))
        .collect()
}

/// Model bouts as annotations awaiting review.
pub fn proposals(bouts: &[(f64, f64, f64)], behavior: &str, model: &str) -> Vec<Annotation> {
    bouts
        .iter()
        .map(|&(s, e, c)| Annotation { label: behavior.into(), start_s: s, end_s: Some(e), scorer: model.into(), source: Source::Model, model: Some(model.into()), confidence: Some(c.clamp(0.0, 1.0)), review: Review::Proposed })
        .collect()
}

/// One training/evaluation unit: a session's features, frame times and labels.
pub struct Labeled<'a> {
    pub x: &'a [Vec<f32>],
    pub t: &'a [f64],
    pub y: &'a [bool],
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    /// "leave-one-session-out" or "time blocks (k)".
    pub scheme: String,
    pub folds: usize,
    pub frame: FrameAgreement,
    pub bout: EventMatch,
    pub positive_frames: usize,
    pub frames: usize,
}

/// Cross-validated agreement with the human labels. With ≥ 2 sessions, each session
/// is held out in turn; with one, `k` contiguous time blocks are, each with a 2 s
/// guard band dropped from training on both sides. `d = None` derives decoding
/// priors from each fold's training labels ([`Decode::from_labels`]).
pub fn cross_validate(data: &[Labeled], names: &[String], behavior: &str, p: Params, d: Option<&Decode>, k: usize) -> Result<Evaluation, String> {
    let d = d.copied();
    let mut all_pred = vec![];
    let mut all_true = vec![];
    let (mut ref_bouts, mut pred_bouts) = (vec![], vec![]);
    // Session offsets keep bouts from different sessions apart when pooled.
    let mut offset = 0.0;
    let mut score = |pred: &[f32], s: &Labeled, range: std::ops::Range<usize>, offset: f64, d: &Decode| {
        let t: Vec<f64> = s.t[range.clone()].iter().map(|v| v + offset).collect();
        let truth: Vec<f32> = s.y[range.clone()].iter().map(|&b| b as u8 as f32).collect();
        ref_bouts.extend(decode_bouts(&truth, &t, &Decode { on: 0.5, off: 0.5, min_bout_s: 0.0, merge_gap_s: 0.0 }).into_iter().map(|b| (b.0, b.1)));
        // Frame-level prediction = inside a decoded bout (what a reviewer would see).
        let bouts = decode_bouts(pred, &t, d);
        all_pred.extend(t.iter().map(|&ti| bouts.iter().any(|b| ti >= b.0 && ti < b.1)));
        pred_bouts.extend(bouts.into_iter().map(|b| (b.0, b.1)));
        all_true.extend(s.y[range].iter().copied());
    };
    let (scheme, folds) = if data.len() >= 2 {
        for (i, s) in data.iter().enumerate() {
            let (x, y): (Vec<Vec<f32>>, Vec<bool>) = data.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, o)| o.x.iter().cloned().zip(o.y.iter().copied())).unzip();
            let m = train(&x, &y, names, behavior, p)?;
            let (tt, yy): (Vec<f64>, Vec<bool>) = data.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, o)| o.t.iter().copied().zip(o.y.iter().copied())).unzip();
            let dd = d.unwrap_or_else(|| Decode::from_labels(&tt, &yy));
            score(&m.predict(s.x), s, 0..s.x.len(), offset, &dd);
            offset += s.t.last().copied().unwrap_or(0.0) + 1e4;
        }
        ("leave-one-session-out".to_string(), data.len())
    } else {
        let s = data.first().ok_or("no labeled data")?;
        let n = s.x.len();
        let k = k.clamp(2, 10);
        for f in 0..k {
            let (a, b) = (f * n / k, (f + 1) * n / k);
            let guard = |i: usize| s.t[i] < s.t[a] - 2.0 || s.t[i] > s.t[b - 1] + 2.0;
            let keep: Vec<usize> = (0..n).filter(|&i| guard(i)).collect();
            let (x, y): (Vec<Vec<f32>>, Vec<bool>) = keep.iter().map(|&i| (s.x[i].clone(), s.y[i])).unzip();
            let m = train(&x, &y, names, behavior, p)?;
            // Decoding priors come from the training part only (no peeking at the held-out block).
            let tt: Vec<f64> = keep.iter().map(|&i| s.t[i]).collect();
            let dd = d.unwrap_or_else(|| Decode::from_labels(&tt, &y));
            score(&m.predict(&s.x[a..b]), s, a..b, 0.0, &dd);
        }
        (format!("time blocks ({k})"), k)
    };
    Ok(Evaluation {
        scheme,
        folds,
        frame: crate::frame_agreement(&all_true, &all_pred),
        bout: crate::bout_f1(&ref_bouts, &pred_bouts, 0.5),
        positive_frames: all_true.iter().filter(|&&b| b).count(),
        frames: all_true.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic session: the animal "grooms" (still, hunched) in known bouts and walks otherwise.
    fn session(seed: u64, n: usize) -> (Vec<Detection>, Vec<f64>, Vec<bool>) {
        let mut r = crate::rng(seed);
        let fps = 10.0;
        let mut dets = vec![];
        let mut y = vec![];
        let (mut x, mut yy) = (100.0, 80.0);
        for i in 0..n {
            let groom = (i / 120) % 3 == 1; // 12 s bouts every 36 s
            let (step, elong, motion) = if groom { (0.2, 1.2, 2.5) } else { (3.0, 2.4, 1.5) };
            x = (x + step * (r() - 0.5) * 2.0).clamp(40.0, 260.0);
            yy = (yy + step * (r() - 0.5) * 2.0).clamp(30.0, 200.0);
            dets.push(Detection { x, y: yy, area: 500.0 + 40.0 * r(), angle: r() * 3.0, elongation: elong + 0.2 * r(), motion: motion + r(), motion_frac: 0.0 });
            y.push(groom);
        }
        let t = (0..n).map(|i| i as f64 / fps).collect();
        (dets, t, y)
    }

    #[test]
    fn learns_and_generalizes_across_sessions() {
        let roi = [20, 20, 300, 220];
        let sets: Vec<_> = (0..3).map(|s| session(10 + s, 1800)).collect();
        let feats: Vec<Features> = sets.iter().map(|(d, _, _)| frame_features(d, 10.0, roi, &[]).unwrap()).collect();
        let names = feats[0].names.clone();
        let data: Vec<Labeled> = sets.iter().zip(&feats).map(|((_, t, y), f)| Labeled { x: &f.rows, t, y }).collect();
        let ev = cross_validate(&data, &names, "grooming", Params::default(), None, 5).unwrap();
        println!("{ev:?}");
        assert_eq!(ev.scheme, "leave-one-session-out");
        assert!(ev.frame.kappa > 0.85 && ev.bout.f1 > 0.9, "{ev:?}");
        let one = cross_validate(&data[..1], &names, "grooming", Params::default(), None, 5).unwrap();
        assert!(one.scheme.starts_with("time blocks") && one.frame.kappa > 0.8, "{one:?}");
    }

    #[test]
    fn model_roundtrips_and_rejects_degenerate_labels() {
        let (d, _, y) = session(3, 900);
        let f = frame_features(&d, 10.0, [20, 20, 300, 220], &[]).unwrap();
        let m = train(&f.rows, &y, &f.names, "grooming", Params { trees: 20, ..Default::default() }).unwrap();
        let json = serde_json::to_string(&m).unwrap();
        let back: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(m.predict(&f.rows[..50]), back.predict(&f.rows[..50]));
        assert!(train(&f.rows, &vec![false; f.rows.len()], &f.names, "x", Params::default()).is_err());
    }

    #[test]
    fn hysteresis_merge_and_min_duration() {
        let t: Vec<f64> = (0..100).map(|i| i as f64 * 0.1).collect();
        let mut p = vec![0.1f32; 100];
        p[10..30].iter_mut().for_each(|v| *v = 0.9);
        p[18] = 0.5; // dips above `off`: same bout
        p[32..45].iter_mut().for_each(|v| *v = 0.9); // gap 0.2 s: merged
        p[70..74].iter_mut().for_each(|v| *v = 0.9); // 0.4 s: too short
        let b = decode_bouts(&p, &t, &Decode::default());
        assert_eq!(b.len(), 1);
        assert!((b[0].0 - 1.0).abs() < 1e-9 && (b[0].1 - 4.5).abs() < 1e-9, "{b:?}");
    }
}
