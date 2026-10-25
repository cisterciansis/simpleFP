//! Video behavior features for the CLI and agents: decode with ffmpeg (if
//! installed), track with `simplefp_video::track`, place every frame on the block
//! clock via the camera epoc. The Mac app decodes with AVFoundation instead.

use crate::Recording;
use simplefp_video::track::{self, Detection, Features, TrackParams, Tracker};
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn ffmpeg() -> Result<PathBuf, String> {
    ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .ok_or_else(|| "ffmpeg not found; install it (brew install ffmpeg) to decode video from the command line".into())
}

/// Stream every frame as `w×h` grayscale to `f`.
fn decode(video: &Path, w: usize, h: usize, mut f: impl FnMut(&[u8]) -> Result<(), String>) -> Result<usize, String> {
    let mut child = Command::new(ffmpeg()?)
        .args(["-v", "error", "-i"])
        .arg(video)
        .args(["-map", "0:v:0", "-vf", &format!("scale={w}:{h},format=gray"), "-fps_mode", "passthrough", "-f", "rawvideo", "-"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut out = child.stdout.take().ok_or("ffmpeg produced no output")?;
    let mut buf = vec![0u8; w * h];
    let mut n = 0;
    loop {
        match out.read_exact(&mut buf) {
            Ok(()) => {
                f(&buf)?;
                n += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("reading frames: {e}")),
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("ffmpeg failed on {}", video.display()));
    }
    Ok(n)
}

pub struct TrackResult {
    pub frames: usize,
    pub features: Features,
    pub detections: Vec<Detection>,
    pub warning: Option<String>,
    /// Region tracked (auto-detected arena unless given).
    pub roi: Option<[usize; 4]>,
}

/// Track the first `camera` video of a block. `width` sets the analysis resolution (height keeps 4:3).
pub fn track_block(rec: &Recording, camera: &str, width: usize, params: TrackParams, threshold: Option<f64>, min_immobile_s: f64) -> Result<TrackResult, String> {
    let (_, video) = crate::block_videos(&rec.block.path).into_iter().find(|(c, _)| c == camera).ok_or_else(|| format!("no {camera} video in this block"))?;
    let clock = rec.frame_clock(camera)?;
    let (w, h) = (width.clamp(64, 640), (width.clamp(64, 640) * 3 / 4) & !1);
    // Pass 1: every k-th frame for the median background (~100 samples).
    let total = clock.frames().unwrap_or(0).max(1);
    let k = (total / 100).max(1);
    let mut samples = vec![];
    let mut i = 0;
    decode(&video, w, h, |f| {
        if i % k == 0 {
            samples.push(f.to_vec());
        }
        i += 1;
        Ok(())
    })?;
    let bg = track::median_background(&samples)?;
    let mut params = params;
    let mut warning = None;
    if params.roi.is_none() {
        params.roi = track::auto_arena(&bg, w, h, 2);
        if params.roi.is_none() {
            warning = Some("no enclosed arena in view; tracked the whole frame (fine for side views like tail suspension; pass --roi if other things move in view)".to_string());
        }
    }
    let roi = params.roi;
    let mut tracker = Tracker::new(bg, w, h, params)?;
    // Pass 2: track every frame.
    let mut dets = vec![];
    let frames = decode(&video, w, h, |f| {
        dets.push(tracker.step(f)?);
        Ok(())
    })?;
    let warning = clock.check_frame_count(frames, (clock.fps.round() as usize).max(1))?.or(warning);
    let features = track::features(&clock, &dets, 1.0, threshold, min_immobile_s)?;
    Ok(TrackResult { frames, features, detections: dets, warning, roi })
}

/// `behavior.csv` (per frame) and `behavior_events.csv` (immobility onset/offset proposals).
pub fn export_behavior(r: &TrackResult, out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let f = &r.features;
    let num = |v: f64| if v.is_finite() { format!("{v}") } else { String::new() };
    let mut s = String::from("time_s,x_px,y_px,area_px,speed_px_s,motion,immobile\n");
    for i in 0..f.t.len() {
        let _ = writeln!(s, "{},{},{},{},{},{},{}", f.t[i], num(f.x[i]), num(f.y[i]), r.detections[i].area, num(f.speed[i]), num(f.motion[i]), f.immobile[i] as u8);
    }
    std::fs::write(out.join("behavior.csv"), s).map_err(|e| e.to_string())?;
    let mut s = String::from("event,time_s\n");
    for (a, b) in &f.bouts {
        let _ = writeln!(s, "immobility_onset,{a}\nmobility_onset,{b}");
    }
    std::fs::write(out.join("behavior_events.csv"), s).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- behavior classifiers

use simplefp_video::classify::{self, CurvePoint, Detector, Evaluation, Features as FrameFeatures, Labeled, Params};

/// A trained behavior detector as saved to disk, with its provenance and validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    pub schema: u32,
    pub behavior: String,
    /// `simplefp-bt/<behavior>@<hash>`: written on every proposal it makes.
    pub id: String,
    pub tracking_width: usize,
    pub trained_on: Vec<String>,
    /// Cross-validated agreement of this detector with the human labels.
    pub evaluation: serde_json::Value,
    /// The same folds scored by the v1 single-pass model, for comparison.
    pub baseline: serde_json::Value,
    /// Held-out agreement vs number of training bouts.
    pub learning_curve: Vec<CurvePoint>,
    pub detector: Detector,
}

pub const MODEL_SCHEMA: u32 = 2;

impl ModelFile {
    pub fn parse(json: &str) -> Result<ModelFile, String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if v["schema"].as_u64() != Some(MODEL_SCHEMA as u64) {
            return Err(format!("model file schema {} is not supported (need {MODEL_SCHEMA}); retrain it", v["schema"]));
        }
        serde_json::from_value(v).map_err(|e| e.to_string())
    }
}

/// Features for a tracked session (ROI from tracking, or the whole frame).
pub fn session_features(r: &TrackResult, fps: f64, width: usize) -> Result<FrameFeatures, String> {
    let roi = r.roi.unwrap_or([0, 0, width, width * 3 / 4]);
    classify::frame_features(&r.detections[..r.features.t.len()], fps, roi, &[])
}

/// Frame labels from human bouts `(start_s, end_s)`.
pub fn labels(t: &[f64], bouts: &[(f64, f64)]) -> Vec<bool> {
    t.iter().map(|&ti| bouts.iter().any(|&(a, b)| ti >= a && ti < b)).collect()
}

/// Bouts of `behavior` from a CSV with header `behavior,start_s,end_s`.
pub fn read_bouts_csv(path: &Path, behavior: &str) -> Result<Vec<(f64, f64)>, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = src.lines().filter(|l| !l.trim().is_empty());
    if lines.next().map(|h| h.trim().to_ascii_lowercase()) != Some("behavior,start_s,end_s".into()) {
        return Err(format!("{}: header must be behavior,start_s,end_s", path.display()));
    }
    let mut out = vec![];
    for (i, l) in lines.enumerate() {
        let c: Vec<&str> = l.split(',').map(str::trim).collect();
        let (a, b) = match c.as_slice() {
            [lab, a, b] if *lab == behavior => (a.parse::<f64>(), b.parse::<f64>()),
            [_, _, _] => continue,
            _ => return Err(format!("row {}: expected behavior,start_s,end_s", i + 2)),
        };
        match (a, b) {
            (Ok(a), Ok(b)) if a.is_finite() && b > a => out.push((a, b)),
            _ => return Err(format!("row {}: start and end must be numbers with end > start", i + 2)),
        }
    }
    if out.is_empty() {
        return Err(format!("no '{behavior}' bouts in {}", path.display()));
    }
    Ok(out)
}

/// Train on labeled sessions, cross-validate against the v1 baseline, measure the
/// learning curve, and package the detector.
pub fn train_model(sessions: &[(String, FrameFeatures, Vec<f64>, Vec<bool>)], behavior: &str, width: usize, params: Params) -> Result<(ModelFile, Evaluation), String> {
    let names = sessions.first().ok_or("no sessions")?.1.names.clone();
    let data: Vec<Labeled> = sessions.iter().map(|(_, f, t, y)| Labeled { x: f.rows.clone(), t: t.clone(), y: y.clone() }).collect();
    let fit = |tr: &[Labeled]| classify::fit_detector(tr, &names, behavior, params);
    let ev = classify::evaluate(&data, 5, fit)?;
    let base = classify::evaluate(&data, 5, |tr| classify::fit_baseline(tr, &names, behavior, params))?;
    // Bouts = rising edges of the labels.
    let total: usize = sessions.iter().map(|(_, _, _, y)| y.first().map_or(0, |&b| b as usize) + y.windows(2).filter(|w| !w[0] && w[1]).count()).sum();
    let steps: Vec<usize> = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89].into_iter().filter(|&m| m < total).chain([total]).collect();
    let learning_curve = classify::learning_curve(&data, 5, &steps, fit)?;
    let detector = fit(&data)?;
    let hash = blake3::hash(serde_json::to_string(&detector).map_err(|e| e.to_string())?.as_bytes()).to_hex()[..10].to_string();
    let file = ModelFile {
        schema: MODEL_SCHEMA,
        behavior: behavior.into(),
        id: format!("simplefp-bt/{behavior}@{hash}"),
        tracking_width: width,
        trained_on: sessions.iter().map(|s| s.0.clone()).collect(),
        evaluation: serde_json::to_value(&ev).map_err(|e| e.to_string())?,
        baseline: serde_json::to_value(&base).map_err(|e| e.to_string())?,
        learning_curve,
        detector,
    };
    Ok((file, ev))
}

/// Proposals for one tracked session (review required before use).
pub fn propose(m: &ModelFile, f: &FrameFeatures, t: &[f64]) -> Result<Vec<simplefp_video::Annotation>, String> {
    if f.names != m.detector.names {
        return Err("feature set differs from the one the model was trained on (re-track with the model's tracking width or retrain)".into());
    }
    let p = m.detector.probabilities(&f.rows);
    Ok(classify::proposals(&classify::decode_bouts(&p, t, &m.detector.decode), &m.behavior, &m.id))
}
