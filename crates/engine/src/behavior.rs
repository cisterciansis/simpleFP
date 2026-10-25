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
            warning = Some("no bright arena found; tracking the whole frame (draw an ROI for reliable results)".to_string());
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
