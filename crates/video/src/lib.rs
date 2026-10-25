//! Behavior video foundations, with no decoding: decoding belongs to the platform
//! (AVFoundation on macOS). This crate owns the science around video:
//!
//! - [`FrameClock`]: video frame index ↔ TDT block time, from per-frame camera
//!   epoc timestamps (Synapse camera gizmo / frame TTLs), with drift and
//!   dropped-frame accounting;
//! - [`Annotation`]: point events and bouts with provenance (who/what scored
//!   them, model version, confidence, review status), and conversion to the
//!   event times the photometry engine aligns to;
//! - agreement: frame-level Cohen's κ and tolerance-window event F1, to
//!   validate AI or second-rater scoring against a human reference;
//! - pose: DeepLabCut CSV import and derived kinematics (speed, distance to ROI).

use serde::{Deserialize, Serialize};

/// Maps frames to block time. Built from one timestamp per frame when available
/// (TDT camera epocs), otherwise from a nominal rate plus an offset.
#[derive(Debug, Clone, Serialize)]
pub struct FrameClock {
    /// Block time of each frame (s); empty when `nominal` is used.
    frame_times: Vec<f64>,
    nominal: Option<(f64, f64)>, // (fps, offset_s)
    /// Linear fit of frame time on index: effective fps and the residual jitter.
    pub fps: f64,
    pub jitter_ms: f64,
    /// Intervals longer than 1.5× the median interval (dropped frames or pauses).
    pub gaps: usize,
}

impl FrameClock {
    /// From per-frame timestamps (e.g. the onsets of a TDT `Cam1` epoc).
    pub fn from_timestamps(times: &[f64]) -> Result<FrameClock, String> {
        if times.len() < 2 {
            return Err("need at least two frame timestamps".into());
        }
        if times.windows(2).any(|w| w[1] <= w[0]) {
            return Err("frame timestamps must increase strictly".into());
        }
        let n = times.len() as f64;
        let mx = (n - 1.0) / 2.0;
        let my = times.iter().sum::<f64>() / n;
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (i, &t) in times.iter().enumerate() {
            sxy += (i as f64 - mx) * (t - my);
            sxx += (i as f64 - mx).powi(2);
        }
        let slope = sxy / sxx;
        let resid: Vec<f64> = times.iter().enumerate().map(|(i, &t)| t - (my + slope * (i as f64 - mx))).collect();
        let rms = (resid.iter().map(|r| r * r).sum::<f64>() / n).sqrt();
        let mut d: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).collect();
        d.sort_by(f64::total_cmp);
        let med = d[d.len() / 2];
        let gaps = times.windows(2).filter(|w| w[1] - w[0] > 1.5 * med).count();
        Ok(FrameClock { frame_times: times.to_vec(), nominal: None, fps: 1.0 / slope, jitter_ms: 1e3 * rms, gaps })
    }

    /// From a nominal frame rate: frame `k` is at `offset_s + k / fps`. Use only when
    /// no per-frame timestamps exist; drift is then unknowable and grows with time.
    pub fn nominal(fps: f64, offset_s: f64) -> Result<FrameClock, String> {
        if !(fps.is_finite() && fps > 0.0 && fps <= 10_000.0) {
            return Err(format!("frame rate {fps} is not plausible"));
        }
        Ok(FrameClock { frame_times: vec![], nominal: Some((fps, offset_s)), fps, jitter_ms: f64::NAN, gaps: 0 })
    }

    pub fn frames(&self) -> Option<usize> {
        (!self.frame_times.is_empty()).then_some(self.frame_times.len())
    }

    /// Block time of frame `k`.
    pub fn time(&self, k: usize) -> Option<f64> {
        match self.nominal {
            Some((fps, off)) => Some(off + k as f64 / fps),
            None => self.frame_times.get(k).copied(),
        }
    }

    /// The frame on screen at block time `t`: the last frame starting at or before `t`.
    pub fn frame_at(&self, t: f64) -> Option<usize> {
        match self.nominal {
            Some((fps, off)) => (t >= off).then(|| ((t - off) * fps).floor() as usize),
            None => self.frame_times.partition_point(|&x| x <= t).checked_sub(1),
        }
    }

    /// From a TDT camera epoc: `onset[k]` is frame k's time and `data[k]` its frame
    /// number, which must step by exactly 1 (a jump means the file lost frames, so
    /// pairing frames to strobes would be ambiguous).
    pub fn from_epoc(onset: &[f64], frame_numbers: &[f64]) -> Result<FrameClock, String> {
        if frame_numbers.len() == onset.len()
            && let Some(w) = frame_numbers.windows(2).position(|w| (w[1] - w[0] - 1.0).abs() > 1e-9) {
                return Err(format!("camera frame numbers jump from {} to {} at frame {w}: frames are missing", frame_numbers[w], frame_numbers[w + 1]));
            }
        Self::from_timestamps(onset)
    }

    /// Reconcile with the video's frame count (from the sample table). Extra
    /// trailing video frames are normal (TDT TN0987) and stay unclocked; fewer
    /// video frames than timestamps is an error because the pairing is ambiguous.
    /// Returns a warning when more than `max_trailing` frames are unclocked.
    pub fn check_frame_count(&self, video_frames: usize, max_trailing: usize) -> Result<Option<String>, String> {
        match self.frames() {
            Some(n) if video_frames < n => Err(format!("the video has {video_frames} frames but the camera epoc logged {n}: frames were lost from the file")),
            Some(n) if video_frames - n > max_trailing => Ok(Some(format!("{} trailing video frames have no timestamp and are excluded", video_frames - n))),
            _ => Ok(None),
        }
    }

    /// Frame times as stored (empty for a nominal clock).
    pub fn frame_times(&self) -> &[f64] {
        &self.frame_times
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Manual,
    Model,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    /// Model proposal not yet looked at. Never used for analysis.
    Proposed,
    Accepted,
    Rejected,
    /// A human moved the times or relabeled it.
    Edited,
}

/// A scored behavior: a point event (`end_s == None`) or a bout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub label: String,
    pub start_s: f64,
    #[serde(default)]
    pub end_s: Option<f64>,
    pub scorer: String,
    pub source: Source,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    pub review: Review,
}

impl Annotation {
    pub fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty() || self.label.len() > 64 {
            return Err("label must be 1–64 characters".into());
        }
        if !self.start_s.is_finite() || self.start_s < 0.0 {
            return Err("start must be a non-negative time".into());
        }
        if let Some(e) = self.end_s
            && !(e.is_finite() && e > self.start_s) {
                return Err("bout end must be after its start".into());
            }
        if self.confidence.is_some_and(|c| !(0.0..=1.0).contains(&c)) {
            return Err("confidence must be within 0–1".into());
        }
        if self.source == Source::Model && self.model.is_none() {
            return Err("model annotations must name the model and version".into());
        }
        Ok(())
    }

    /// Usable for analysis: human-made, or a model proposal a human accepted or edited.
    pub fn usable(&self) -> bool {
        self.review != Review::Rejected && (self.source != Source::Model || matches!(self.review, Review::Accepted | Review::Edited))
    }
}

/// Event times per label for the photometry engine: bout onsets as `label`,
/// bout offsets as `label_offset`. Unreviewed model proposals are excluded.
pub fn event_sets(annotations: &[Annotation]) -> Vec<(String, Vec<f64>)> {
    let mut map: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for a in annotations.iter().filter(|a| a.usable()) {
        map.entry(a.label.clone()).or_default().push(a.start_s);
        if let Some(e) = a.end_s {
            map.entry(format!("{}_offset", a.label)).or_default().push(e);
        }
    }
    map.into_iter()
        .map(|(k, mut v)| {
            v.sort_by(f64::total_cmp);
            (k, v)
        })
        .collect()
}

/// Rasterize bouts of `label` onto a frame grid (`true` where the behavior occurs).
pub fn raster(annotations: &[Annotation], label: &str, t0: f64, dt: f64, n: usize) -> Vec<bool> {
    let mut v = vec![false; n];
    for a in annotations.iter().filter(|a| a.label == label && a.usable()) {
        let end = a.end_s.unwrap_or(a.start_s + dt);
        let i0 = ((a.start_s - t0) / dt).floor().max(0.0) as usize;
        let i1 = (((end - t0) / dt).ceil().max(0.0) as usize).min(n);
        v[i0.min(n)..i1].iter_mut().for_each(|x| *x = true);
    }
    v
}

/// Frame-level agreement between two binary rasters. κ alone collapses for rare
/// behaviors (the "kappa paradox"), so prevalence and PABAK (= 2·p_o − 1) are reported with it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FrameAgreement {
    pub kappa: f64,
    pub pabak: f64,
    pub observed: f64,
    pub prevalence: f64,
    pub f1: f64,
}

pub fn frame_agreement(a: &[bool], b: &[bool]) -> FrameAgreement {
    let n = a.len() as f64;
    let tp = a.iter().zip(b).filter(|(x, y)| **x && **y).count() as f64;
    let (na, nb) = (a.iter().filter(|x| **x).count() as f64, b.iter().filter(|x| **x).count() as f64);
    let observed = a.iter().zip(b).filter(|(x, y)| x == y).count() as f64 / n;
    FrameAgreement { kappa: cohen_kappa(a, b), pabak: 2.0 * observed - 1.0, observed, prevalence: (na + nb) / (2.0 * n), f1: if na + nb == 0.0 { f64::NAN } else { 2.0 * tp / (na + nb) } }
}

/// Cohen's κ for two binary frame rasters (agreement beyond chance).
pub fn cohen_kappa(a: &[bool], b: &[bool]) -> f64 {
    assert_eq!(a.len(), b.len());
    let n = a.len() as f64;
    let (mut pa, mut pb) = (0.0, 0.0);
    let mut agree = 0.0;
    for (x, y) in a.iter().zip(b) {
        agree += (x == y) as u8 as f64;
        pa += *x as u8 as f64;
        pb += *y as u8 as f64;
    }
    let po = agree / n;
    let (pa, pb) = (pa / n, pb / n);
    let pe = pa * pb + (1.0 - pa) * (1.0 - pb);
    if (1.0 - pe).abs() < 1e-15 { if po == 1.0 { 1.0 } else { 0.0 } } else { (po - pe) / (1.0 - pe) }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct EventMatch {
    pub true_positive: usize,
    pub false_positive: usize,
    pub false_negative: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    /// Median |Δt| of matched pairs (s): systematic scoring lag shows up here.
    pub median_offset_s: f64,
}

/// One-to-one greedy matching of predicted to reference event times within `tol_s`
/// (closest pairs first). F1 = 2PR/(P+R).
pub fn event_f1(reference: &[f64], predicted: &[f64], tol_s: f64) -> EventMatch {
    let mut pairs: Vec<(f64, usize, usize)> = vec![];
    for (i, r) in reference.iter().enumerate() {
        for (j, p) in predicted.iter().enumerate() {
            let d = (p - r).abs();
            if d <= tol_s {
                pairs.push((d, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut used_r, mut used_p) = (vec![false; reference.len()], vec![false; predicted.len()]);
    let mut offsets = vec![];
    for (d, i, j) in pairs {
        if !used_r[i] && !used_p[j] {
            used_r[i] = true;
            used_p[j] = true;
            offsets.push(d);
        }
    }
    let tp = offsets.len();
    let (fp, fn_) = (predicted.len() - tp, reference.len() - tp);
    let precision = if predicted.is_empty() { f64::NAN } else { tp as f64 / predicted.len() as f64 };
    let recall = if reference.is_empty() { f64::NAN } else { tp as f64 / reference.len() as f64 };
    let f1 = if tp == 0 { 0.0 } else { 2.0 * precision * recall / (precision + recall) };
    offsets.sort_by(f64::total_cmp);
    let median_offset_s = if offsets.is_empty() { f64::NAN } else { offsets[offsets.len() / 2] };
    EventMatch { true_positive: tp, false_positive: fp, false_negative: fn_, precision, recall, f1, median_offset_s }
}

/// Bout matching at a temporal-IoU threshold (one-to-one, best IoU first).
pub fn bout_f1(reference: &[(f64, f64)], predicted: &[(f64, f64)], min_iou: f64) -> EventMatch {
    let iou = |a: (f64, f64), b: (f64, f64)| {
        let inter = (a.1.min(b.1) - a.0.max(b.0)).max(0.0);
        inter / ((a.1 - a.0) + (b.1 - b.0) - inter)
    };
    let mut pairs: Vec<(f64, usize, usize)> = vec![];
    for (i, r) in reference.iter().enumerate() {
        for (j, p) in predicted.iter().enumerate() {
            let v = iou(*r, *p);
            if v >= min_iou {
                pairs.push((v, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (mut ur, mut up) = (vec![false; reference.len()], vec![false; predicted.len()]);
    let mut onset = vec![];
    for (_, i, j) in pairs {
        if !ur[i] && !up[j] {
            ur[i] = true;
            up[j] = true;
            onset.push((predicted[j].0 - reference[i].0).abs());
        }
    }
    let tp = onset.len();
    let precision = if predicted.is_empty() { f64::NAN } else { tp as f64 / predicted.len() as f64 };
    let recall = if reference.is_empty() { f64::NAN } else { tp as f64 / reference.len() as f64 };
    onset.sort_by(f64::total_cmp);
    EventMatch {
        true_positive: tp,
        false_positive: predicted.len() - tp,
        false_negative: reference.len() - tp,
        precision,
        recall,
        f1: if tp == 0 { 0.0 } else { 2.0 * precision * recall / (precision + recall) },
        median_offset_s: onset.get(onset.len() / 2).copied().unwrap_or(f64::NAN),
    }
}

/// An ethogram: the behaviors a scorer can mark, with hotkeys. `state` behaviors
/// are bouts (key toggles start/stop); `point` behaviors are instants. Behaviors
/// sharing an `exclusive` group cannot overlap (starting one stops the other).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ethogram {
    pub name: String,
    #[serde(rename = "behavior")]
    pub behaviors: Vec<Behavior>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Behavior {
    pub label: String,
    pub key: String,
    pub kind: BehaviorKind,
    #[serde(default)]
    pub exclusive: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorKind {
    Point,
    State,
}

impl Ethogram {
    pub fn validate(&self) -> Result<(), String> {
        let mut keys = std::collections::BTreeSet::new();
        let mut labels = std::collections::BTreeSet::new();
        for b in &self.behaviors {
            if b.key.chars().count() != 1 || !b.key.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(format!("{}: hotkey must be one letter or digit", b.label));
            }
            if !keys.insert(b.key.to_ascii_lowercase()) {
                return Err(format!("hotkey '{}' is used twice", b.key));
            }
            if b.label.trim().is_empty() || !labels.insert(b.label.clone()) {
                return Err(format!("behavior label '{}' is empty or duplicated", b.label));
            }
        }
        Ok(())
    }

    /// Default feeding ethogram for the refeeding paradigm.
    pub fn feeding() -> Ethogram {
        let b = |label: &str, key: &str, kind, exclusive: Option<&str>| Behavior { label: label.into(), key: key.into(), kind, exclusive: exclusive.map(Into::into) };
        Ethogram {
            name: "Feeding".into(),
            behaviors: vec![
                b("approach", "a", BehaviorKind::Point, None),
                b("food_contact", "c", BehaviorKind::Point, None),
                b("eating", "e", BehaviorKind::State, Some("activity")),
                b("grooming", "g", BehaviorKind::State, Some("activity")),
                b("rearing", "r", BehaviorKind::State, Some("activity")),
                b("drinking", "d", BehaviorKind::State, Some("activity")),
            ],
        }
    }
}

/// Keypoint tracks: `x[bodypart][frame]`, `y`, `likelihood` (NaN where missing).
#[derive(Debug, Clone, Serialize)]
pub struct Pose {
    pub bodyparts: Vec<String>,
    pub x: Vec<Vec<f64>>,
    pub y: Vec<Vec<f64>>,
    pub likelihood: Vec<Vec<f64>>,
}

/// Parse a single-animal DeepLabCut CSV (`scorer` / `bodyparts` / `coords` header rows,
/// then `frame, x, y, likelihood, …`). Multi-animal files (an `individuals` row) are rejected.
pub fn read_dlc_csv(src: &str) -> Result<Pose, String> {
    let mut lines = src.lines();
    let mut header = |name: &str| -> Result<Vec<String>, String> {
        let l = lines.next().ok_or_else(|| format!("missing '{name}' header row"))?;
        let cells: Vec<String> = l.split(',').map(|c| c.trim().to_string()).collect();
        if !cells[0].eq_ignore_ascii_case(name) {
            return Err(if cells[0].eq_ignore_ascii_case("individuals") {
                "multi-animal DeepLabCut files are not supported yet".into()
            } else {
                format!("expected '{name}' header row, found '{}'", cells[0])
            });
        }
        Ok(cells)
    };
    header("scorer")?;
    let parts = header("bodyparts")?;
    let coords = header("coords")?;
    if parts.len() != coords.len() || (parts.len() - 1) % 3 != 0 {
        return Err("header rows have inconsistent column counts".into());
    }
    let mut bodyparts = vec![];
    for k in (1..parts.len()).step_by(3) {
        if coords[k] != "x" || coords[k + 1] != "y" || coords[k + 2] != "likelihood" {
            return Err(format!("columns for '{}' must be x, y, likelihood", parts[k]));
        }
        bodyparts.push(parts[k].clone());
    }
    let nb = bodyparts.len();
    let (mut x, mut y, mut lk) = (vec![vec![]; nb], vec![vec![]; nb], vec![vec![]; nb]);
    for (row, l) in lines.enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let cells: Vec<&str> = l.split(',').collect();
        if cells.len() != parts.len() {
            return Err(format!("data row {} has {} columns, expected {}", row + 1, cells.len(), parts.len()));
        }
        let num = |s: &str| if s.trim().is_empty() { Ok(f64::NAN) } else { s.trim().parse::<f64>().map_err(|_| format!("data row {}: '{s}' is not a number", row + 1)) };
        for b in 0..nb {
            x[b].push(num(cells[1 + 3 * b])?);
            y[b].push(num(cells[2 + 3 * b])?);
            lk[b].push(num(cells[3 + 3 * b])?);
        }
    }
    Ok(Pose { bodyparts, x, y, likelihood: lk })
}

impl Pose {
    pub fn frames(&self) -> usize {
        self.x.first().map_or(0, Vec::len)
    }

    fn part(&self, name: &str) -> Result<usize, String> {
        self.bodyparts.iter().position(|b| b == name).ok_or_else(|| format!("no bodypart '{name}' (have {:?})", self.bodyparts))
    }

    /// Positions of `bodypart` with low-likelihood points set to NaN.
    pub fn track(&self, bodypart: &str, min_likelihood: f64) -> Result<(Vec<f64>, Vec<f64>), String> {
        let b = self.part(bodypart)?;
        let keep = |i: usize| self.likelihood[b][i] >= min_likelihood;
        Ok((
            (0..self.frames()).map(|i| if keep(i) { self.x[b][i] } else { f64::NAN }).collect(),
            (0..self.frames()).map(|i| if keep(i) { self.y[b][i] } else { f64::NAN }).collect(),
        ))
    }
}

/// Speed (units/s) from positions sampled at `clock` frame times; central differences, NaN across gaps.
pub fn speed(x: &[f64], y: &[f64], clock: &FrameClock, scale: f64) -> Vec<f64> {
    let n = x.len();
    (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(n - 1));
            match (clock.time(a), clock.time(b)) {
                (Some(ta), Some(tb)) if b > a && tb > ta => scale * (x[b] - x[a]).hypot(y[b] - y[a]) / (tb - ta),
                _ => f64::NAN,
            }
        })
        .collect()
}

/// Distance from each point to a circular ROI's edge (negative = inside).
pub fn distance_to_roi(x: &[f64], y: &[f64], cx: f64, cy: f64, r: f64, scale: f64) -> Vec<f64> {
    x.iter().zip(y).map(|(a, b)| scale * ((a - cx).hypot(b - cy) - r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_clock_fits_drift_and_counts_gaps() {
        // 30 fps camera running 0.1 % slow, one dropped frame at 100, small jitter.
        let mut t: Vec<f64> = (0..300).map(|k| 2.0 + k as f64 / 29.97 + if k % 2 == 0 { 0.0005 } else { -0.0005 }).collect();
        t.remove(100);
        let c = FrameClock::from_timestamps(&t).unwrap();
        assert!((c.fps - 29.97).abs() < 0.2, "{}", c.fps);
        assert_eq!(c.gaps, 1);
        assert_eq!(c.frame_at(1.0), None);
        assert_eq!(c.frame_at(t[50] + 1e-6), Some(50));
        assert_eq!(c.time(299), None);
        // 299 stamps: a 300-frame video has one trailing frame (normal); 298 frames lost one.
        assert_eq!(c.check_frame_count(300, 30).unwrap(), None);
        assert!(c.check_frame_count(400, 30).unwrap().is_some());
        assert!(c.check_frame_count(298, 30).is_err());
        let nums: Vec<f64> = (0..299).map(|k| if k < 100 { k as f64 } else { k as f64 + 1.0 }).collect();
        assert!(FrameClock::from_epoc(&t, &nums).unwrap_err().contains("missing"));
        assert!(FrameClock::from_timestamps(&[1.0, 1.0]).is_err());
        let n = FrameClock::nominal(30.0, 1.5).unwrap();
        assert_eq!(n.frame_at(1.5 + 2.0 / 30.0 + 1e-9), Some(2));
    }

    #[test]
    fn model_proposals_need_review() {
        let a = |src, review, label: &str, s, e| Annotation { label: label.into(), start_s: s, end_s: e, scorer: "x".into(), source: src, model: Some("m@1".into()), confidence: Some(0.9), review };
        let anns = vec![
            a(Source::Manual, Review::Accepted, "bite", 10.0, None),
            a(Source::Model, Review::Proposed, "bite", 20.0, None),
            a(Source::Model, Review::Accepted, "bite", 30.0, None),
            a(Source::Model, Review::Rejected, "bite", 40.0, None),
            a(Source::Manual, Review::Accepted, "groom", 50.0, Some(55.0)),
        ];
        let ev = event_sets(&anns);
        assert_eq!(ev, vec![("bite".into(), vec![10.0, 30.0]), ("groom".into(), vec![50.0]), ("groom_offset".into(), vec![55.0])]);
        let bad = Annotation { source: Source::Model, model: None, ..anns[0].clone() };
        assert!(bad.validate().is_err());
        assert!(Annotation { end_s: Some(5.0), ..anns[0].clone() }.validate().is_err());
    }

    #[test]
    fn agreement_metrics() {
        assert_eq!(cohen_kappa(&[true, false, true, false], &[true, false, true, false]), 1.0);
        // Classic example: po = 0.7, pe = 0.5 → κ = 0.4.
        let a = [true, true, true, true, true, false, false, false, false, false];
        let b = [true, true, true, true, false, true, false, false, false, true];
        assert!((cohen_kappa(&a, &b) - 0.4).abs() < 1e-12, "{}", cohen_kappa(&a, &b));
        let m = event_f1(&[1.0, 5.0, 9.0], &[1.1, 5.4, 20.0], 0.5);
        assert_eq!((m.true_positive, m.false_positive, m.false_negative), (2, 1, 1));
        assert!((m.f1 - 2.0 / 3.0).abs() < 1e-12);
        // One prediction cannot match two references.
        assert_eq!(event_f1(&[1.0, 1.2], &[1.1], 0.5).true_positive, 1);
        // Rare behavior: high raw agreement, κ low, PABAK high — why both are reported.
        let mut ra = vec![false; 100];
        let mut rb = vec![false; 100];
        ra[0] = true;
        rb[1] = true;
        let fa = frame_agreement(&ra, &rb);
        assert!(fa.kappa < 0.0 && (fa.pabak - 0.96).abs() < 1e-12 && fa.f1 == 0.0, "{fa:?}");
        let m = bout_f1(&[(0.0, 10.0), (20.0, 30.0)], &[(1.0, 10.0), (26.0, 40.0)], 0.5);
        assert_eq!((m.true_positive, m.false_positive), (1, 1));
        assert!(Ethogram::feeding().validate().is_ok());
    }

    #[test]
    fn dlc_csv_and_kinematics() {
        let src = "scorer,DLC_resnet50,DLC_resnet50,DLC_resnet50\nbodyparts,nose,nose,nose\ncoords,x,y,likelihood\n0,0,0,0.99\n1,3,4,0.99\n2,6,8,0.2\n";
        let p = read_dlc_csv(src).unwrap();
        assert_eq!((p.bodyparts.clone(), p.frames()), (vec!["nose".to_string()], 3));
        let (x, y) = p.track("nose", 0.9).unwrap();
        assert!(x[2].is_nan());
        let c = FrameClock::nominal(10.0, 0.0).unwrap();
        let v = speed(&x, &y, &c, 1.0);
        assert!((v[0] - 50.0).abs() < 1e-9, "{v:?}"); // 5 px in 0.1 s
        assert!(read_dlc_csv("scorer,a\nindividuals,a\n").unwrap_err().contains("multi-animal"));
        let d = distance_to_roi(&[0.0, 10.0], &[0.0, 0.0], 0.0, 0.0, 2.0, 1.0);
        assert_eq!(d, vec![-2.0, 8.0]);
    }
}
