use clap::{Parser, Subcommand};
use simplefp::core::preset;
use simplefp::{EventSource, tdt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod mcp;

/// Fiber photometry analysis for TDT recordings.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the streams and epocs in a TDT block.
    Inspect { block: PathBuf },
    /// Preprocess a block and run peri-event analysis.
    Run {
        block: PathBuf,
        /// Built-in preset id or path to a .toml preset.
        #[arg(short, long, default_value = "grab-ach3.0-tdt")]
        preset: String,
        /// Epoc store whose onsets are events (repeatable), e.g. --epoc PC0_
        #[arg(long)]
        epoc: Vec<String>,
        /// Epoc store whose offsets are events (repeatable).
        #[arg(long)]
        epoc_offset: Vec<String>,
        /// CSV of manual events with header `event,time_s` (block clock, seconds).
        #[arg(long)]
        events: Option<PathBuf>,
        /// Output directory (default: <block>/simplefp).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Override a preset parameter (repeatable), e.g. --set preprocess.lowpass_hz=6
        #[arg(long = "set", value_parser = parse_set)]
        sets: Vec<(String, String)>,
    },
    /// Group analysis of many recordings from a cohort manifest (animal = unit).
    Cohort {
        manifest: PathBuf,
        /// Output directory (default: <manifest dir>/simplefp-cohort).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Track the animal in a block's camera video (classical, no model; needs ffmpeg).
    Track {
        block: PathBuf,
        #[arg(long, default_value = "Cam1")]
        camera: String,
        /// Analysis width in pixels (height keeps 4:3).
        #[arg(long, default_value_t = 160)]
        width: usize,
        /// Immobility threshold on motion energy (gray levels/frame); default: suggested from the data.
        #[arg(long)]
        immobility_threshold: Option<f64>,
        /// Minimum immobility bout (s).
        #[arg(long, default_value_t = 1.0)]
        min_immobile_s: f64,
        /// The animal is lighter than the background.
        #[arg(long)]
        light_animal: bool,
        /// Arena region as fractions of the frame: x0,y0,x1,y1 (e.g. 0.42,0.42,0.78,0.92). Default: auto-detect.
        #[arg(long, value_delimiter = ',')]
        roi: Option<Vec<f64>>,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Train a behavior detector from human-scored bouts and report cross-validated agreement.
    ClassifyTrain {
        /// Blocks to learn from (each needs bouts in --labels).
        #[arg(required = true)]
        blocks: Vec<PathBuf>,
        /// CSV `behavior,start_s,end_s` per block, in the same order as the blocks.
        #[arg(long, required = true, num_args = 1..)]
        labels: Vec<PathBuf>,
        #[arg(long)]
        behavior: String,
        #[arg(long, default_value_t = 320)]
        width: usize,
        /// Arena ROI fractions x0,y0,x1,y1 (applied to every block); default auto.
        #[arg(long, value_delimiter = ',')]
        roi: Option<Vec<f64>>,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Propose bouts in a block with a trained model (written for review, never auto-accepted).
    ClassifyPredict {
        block: PathBuf,
        #[arg(long)]
        model: PathBuf,
        #[arg(long, value_delimiter = ',')]
        roi: Option<Vec<f64>>,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Write a synthetic two-group, two-condition cohort with known effects.
    DemoCohort { dir: PathBuf },
    /// List built-in presets, or print one as TOML.
    Presets {
        id: Option<String>,
        /// Override a parameter and print the resolved, validated preset (repeatable).
        #[arg(long = "set", value_parser = parse_set)]
        sets: Vec<(String, String)>,
    },
    /// Serve SimpleFP's tools to AI agents over the Model Context Protocol (stdio).
    Mcp,
    /// Write a synthetic TDT block with known ground truth.
    Demo {
        dir: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Inspect { block } => {
            let b = tdt::read_block(&block, &tdt::ReadOptions::default()).map_err(|e| e.to_string())?;
            println!("{}  ({})", b.path.display(), b.duration.map_or("unclosed".into(), |d| format!("{d:.1} s")));
            for s in b.streams.values() {
                let n = s.channels.first().map_or(0, Vec::len);
                println!("  stream {:<6} {:>2} ch  {:>10.4} Hz  {:>9} samples  start {:.4} s", s.name, s.channels.len(), s.fs, n, s.start_time);
            }
            for e in b.epocs.values() {
                let first = e.onset.first().map_or(String::new(), |t| format!("first {t:.3} s"));
                println!("  epoc   {:<6} {:>5} events  {first}", e.name, e.onset.len());
            }
        }
        Cmd::Run { block, preset, epoc, epoc_offset, events, out, sets } => {
            let p = preset::with_overrides(&simplefp::load_preset(&preset, Path::new("."))?, &sets)?;
            let rec = simplefp::load(&block, &p)?;
            let mut sources: Vec<EventSource> = epoc.into_iter().map(EventSource::Epoc).collect();
            sources.extend(epoc_offset.into_iter().map(EventSource::EpocOffset));
            if let Some(f) = events {
                sources.extend(simplefp::read_events_csv(&f)?);
            }
            let a = rec.analyze(&p, &sources)?;
            let out = out.unwrap_or_else(|| block.join("simplefp"));
            simplefp::export(&rec, &p, &a, &out)?;
            let q = &a.output.qc;
            println!("{} · {} vs {}", p.name, rec.signal, rec.control.as_deref().unwrap_or("no control"));
            println!("  {:.1} s at {:.3} Hz (÷{}), bleaching {:.1} % signal / {} control", q.duration_s, q.fs, q.decimation, q.bleach_signal_pct, q.bleach_control_pct.map_or("–".into(), |v| format!("{v:.1} %")));
            if let (Some(r), Some(m)) = (q.motion_r, q.motion_slope) {
                println!("  control fit r = {r:.3}, slope = {m:.3}; noise ≈ {:.3} % ΔF/F", q.noise_dff_pct);
            }
            for w in &q.warnings {
                println!("  warning: {w}");
            }
            for ev in &a.events {
                let sig = match &ev.ci {
                    Some(c) => format!("{:.2} s with CI excluding 0", c.significant.iter().filter(|&&s| s).count() as f64 / q.fs),
                    None => format!("no CI (< {} trials)", simplefp::core::analysis::MIN_TRIALS_FOR_CI),
                };
                println!("  {:<12} {:>3} trials ({} dropped), {sig}", ev.label, ev.epochs.trials.len(), ev.epochs.dropped.len());
            }
            println!("  → {}", out.display());
        }
        Cmd::Cohort { manifest, out } => {
            let run = simplefp::cohort::run(&manifest)?;
            let out = out.unwrap_or_else(|| manifest.parent().unwrap_or(Path::new(".")).join("simplefp-cohort"));
            simplefp::cohort::export_cohort(&run, &out)?;
            let g = &run.group;
            let ok = run.reports.iter().filter(|r| r.status == "ok").count();
            println!("{} · {} · {ok}/{} sessions analyzed", run.manifest.name, run.preset.name, run.reports.len());
            for r in run.reports.iter().filter(|r| r.status != "ok") {
                println!("  {}: {}", r.block, r.status);
            }
            for c in &g.groups {
                println!("  {:<10} {:<10} {:<10} n = {} animals", c.event, c.group, c.condition, c.n_animals);
            }
            for t in g.metric_tests.iter().filter(|t| t.metric == "mean" || t.metric == "window_mean") {
                println!(
                    "  {:<10} {:<28} Δ mean = {:+.3} [{:+.3}, {:+.3}]  p = {:.4}{}  q = {:.4}  (n = {}/{})",
                    t.event, t.contrast, t.estimate, t.lo, t.hi, t.p, if t.exact { " exact" } else { "" }, t.q, t.n[0], t.n[1]
                );
            }
            for w in &g.warnings {
                println!("  warning: {w}");
            }
            println!("  → {}", out.display());
        }
        Cmd::Track { block, camera, width, immobility_threshold, min_immobile_s, light_animal, roi, out } => {
            let p = simplefp::core::preset::builtin("grab-ach3.0-tdt").unwrap();
            let rec = simplefp::load(&block, &p)?;
            let (w, h) = (width.clamp(64, 640), (width.clamp(64, 640) * 3 / 4) & !1);
            let roi = match roi.as_deref() {
                Some(&[x0, y0, x1, y1]) if (0.0..=1.0).contains(&x0) && (0.0..=1.0).contains(&y1) && x0 < x1 && y0 < y1 && x1 <= 1.0 && y0 >= 0.0 => {
                    Some([(x0 * w as f64) as usize, (y0 * h as f64) as usize, (x1 * w as f64).ceil() as usize, (y1 * h as f64).ceil() as usize])
                }
                Some(_) => return Err("--roi needs four fractions 0 ≤ x0 < x1 ≤ 1, 0 ≤ y0 < y1 ≤ 1".into()),
                None => None,
            };
            let params = simplefp::video::track::TrackParams { dark_animal: !light_animal, roi, ..Default::default() };
            let r = simplefp::behavior::track_block(&rec, &camera, width, params, immobility_threshold, min_immobile_s)?;
            let out = out.unwrap_or_else(|| block.join("simplefp").join("behavior"));
            simplefp::behavior::export_behavior(&r, &out)?;
            let f = &r.features;
            let imm = f.immobile.iter().filter(|&&b| b).count() as f64 / f.t.len().max(1) as f64;
            println!("arena ROI {:?} at {width} px wide", r.roi);
            println!("{} frames · {:.1} % lost · immobile {:.1} % of the time in {} bouts (threshold {:.2})", r.frames, 100.0 * f.lost_frac, 100.0 * imm, f.bouts.len(), f.immobility_threshold);
            if let Some(w) = r.warning {
                println!("  warning: {w}");
            }
            println!("  → {}", out.display());
        }
        Cmd::ClassifyTrain { blocks, labels, behavior, width, roi, out } => {
            if blocks.len() != labels.len() {
                return Err("give one --labels file per block".into());
            }
            let mut sessions = vec![];
            for (b, l) in blocks.iter().zip(&labels) {
                let (rec, r) = track(b, width, roi.as_deref())?;
                let fps = rec.frame_clock("Cam1")?.fps;
                let f = simplefp::behavior::session_features(&r, fps, width)?;
                let y = simplefp::behavior::labels(&r.features.t, &simplefp::behavior::read_bouts_csv(l, &behavior)?);
                sessions.push((b.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()), f, r.features.t.clone(), y));
            }
            let (file, ev) = simplefp::behavior::train_model(&sessions, &behavior, width, Default::default())?;
            std::fs::write(&out, serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", out.display()))?;
            println!("{} · trained on {} session(s), {} frames ({} positive)", file.id, sessions.len(), ev.frames, ev.positive_frames);
            println!("  cross-validation ({}, {} folds): frame κ {:.3} · PABAK {:.3} · frame F1 {:.3} · bout F1 {:.3} (tIoU ≥ 0.5; {} TP / {} FP / {} FN) · median onset error {:.2} s",
                ev.scheme, ev.folds, ev.frame.kappa, ev.frame.pabak, ev.frame.f1, ev.bout.f1, ev.bout.true_positive, ev.bout.false_positive, ev.bout.false_negative, ev.bout.median_offset_s);
            let b = &file.baseline;
            println!("  v1 baseline, same folds: frame κ {:.3} · bout F1 {:.3}", b["frame"]["kappa"].as_f64().unwrap_or(f64::NAN), b["bout"]["f1"].as_f64().unwrap_or(f64::NAN));
            println!("  learning curve (held-out): {}", file.learning_curve.iter().map(|c| format!("{} bouts κ {:.2} / F1 {:.2}", c.bouts, c.kappa, c.bout_f1)).collect::<Vec<_>>().join(" · "));
            println!("  → {}", out.display());
        }
        Cmd::ClassifyPredict { block, model, roi, out } => {
            let file = simplefp::behavior::ModelFile::parse(&std::fs::read_to_string(&model).map_err(|e| format!("{}: {e}", model.display()))?).map_err(|e| format!("{}: {e}", model.display()))?;
            let (rec, r) = track(&block, file.tracking_width, roi.as_deref())?;
            let f = simplefp::behavior::session_features(&r, rec.frame_clock("Cam1")?.fps, file.tracking_width)?;
            let props = simplefp::behavior::propose(&file, &f, &r.features.t)?;
            let out = out.unwrap_or_else(|| block.join("simplefp").join("proposals.json"));
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(&out, serde_json::to_string_pretty(&props).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            for a in &props {
                println!("  {} {:.1}–{:.1} s  (confidence {:.2}, proposed)", a.label, a.start_s, a.end_s.unwrap_or(a.start_s), a.confidence.unwrap_or(0.0));
            }
            println!("{} proposals by {} → {}", props.len(), file.id, out.display());
        }
        Cmd::DemoCohort { dir } => {
            let m = simplefp::write_demo_cohort(&dir)?;
            println!("wrote 16 blocks and {}", m.display());
        }
        Cmd::Presets { id: Some(id), sets } if !sets.is_empty() => {
            print!("{}", preset::with_overrides(&simplefp::load_preset(&id, Path::new("."))?, &sets)?.to_toml());
        }
        Cmd::Presets { id: None, .. } => {
            for (id, _) in preset::BUILTIN {
                let p = preset::builtin(id).unwrap();
                println!("{id:<18} {}  — {}", p.name, p.description);
            }
        }
        Cmd::Presets { id: Some(id), .. } => {
            let src = preset::BUILTIN.iter().find(|(i, _)| *i == id).ok_or_else(|| format!("no built-in preset '{id}'"))?.1;
            print!("{src}");
        }
        Cmd::Mcp => mcp::serve()?,
        Cmd::Demo { dir, seed } => {
            let ev = simplefp::write_demo_block(&dir, seed)?;
            println!("wrote {} with {} events on epoc 'Evt_'", dir.display(), ev.len());
        }
    }
    Ok(())
}

/// Track a block's Cam1 video (ROI as frame fractions, or auto).
fn track(block: &Path, width: usize, roi: Option<&[f64]>) -> Result<(simplefp::Recording, simplefp::behavior::TrackResult), String> {
    let p = preset::builtin("grab-ach3.0-tdt").unwrap();
    let rec = simplefp::load(block, &p)?;
    let (w, h) = (width.clamp(64, 640), (width.clamp(64, 640) * 3 / 4) & !1);
    let roi = match roi {
        Some(&[x0, y0, x1, y1]) if (0.0..1.0).contains(&x0) && x0 < x1 && x1 <= 1.0 && (0.0..1.0).contains(&y0) && y0 < y1 && y1 <= 1.0 => {
            Some([(x0 * w as f64) as usize, (y0 * h as f64) as usize, (x1 * w as f64).ceil() as usize, (y1 * h as f64).ceil() as usize])
        }
        Some(_) => return Err("--roi needs four fractions 0 ≤ x0 < x1 ≤ 1, 0 ≤ y0 < y1 ≤ 1".into()),
        None => None,
    };
    let params = simplefp::video::track::TrackParams { roi, ..Default::default() };
    let r = simplefp::behavior::track_block(&rec, "Cam1", width, params, None, 1.0)?;
    Ok((rec, r))
}

fn parse_set(s: &str) -> Result<(String, String), String> {
    let (k, v) = s.split_once('=').ok_or("expected key=value, e.g. preprocess.lowpass_hz=6")?;
    Ok((k.trim().to_string(), v.trim().to_string()))
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
