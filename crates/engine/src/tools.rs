//! The tool registry: every SimpleFP capability, defined once. A tool is one argument struct, from which the CLI
//! flags, the JSON arguments and the JSON Schema all derive, plus one `run` that returns JSON with a human
//! `summary`. The CLI and the MCP server dispatch through `TOOLS`. Adding a capability is one struct, one `impl`
//! and one line in `TOOLS`.

use crate::{EventSource, behavior, cohort};
use clap::{ArgMatches, Args, Command};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use simplefp_core::preset;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub trait Tool: Args + DeserializeOwned + JsonSchema {
    /// Name on the CLI and over MCP.
    const NAME: &'static str;
    /// Do the work. The result is a JSON object whose `summary` is what the CLI prints.
    fn run(self) -> Result<Value, String>;
}

/// A type-erased tool, so front-ends can list and call tools by name.
pub struct Entry {
    pub name: &'static str,
    /// The CLI subcommand with its flags; `about` is the struct's doc comment.
    pub command: fn() -> Command,
    /// JSON Schema of the arguments; `description` is the struct's doc comment.
    pub schema: fn() -> Value,
    /// Run from JSON arguments (unknown fields are rejected).
    pub call: fn(Value) -> Result<Value, String>,
    /// Run from parsed CLI arguments.
    pub call_cli: fn(&ArgMatches) -> Result<Value, String>,
}

const fn entry<T: Tool>() -> Entry {
    Entry { name: T::NAME, command: command::<T>, schema: schema::<T>, call: call::<T>, call_cli: call_cli::<T> }
}

fn command<T: Tool>() -> Command {
    T::augment_args(Command::new(T::NAME))
}

fn schema<T: Tool>() -> Value {
    let g = schemars::generate::SchemaSettings::draft2020_12()
        .with(|s| {
            s.meta_schema = None;
            s.inline_subschemas = true;
        })
        .into_generator();
    let mut v = g.into_root_schema_for::<T>().to_value();
    if let Some(o) = v.as_object_mut() {
        o.remove("title");
    }
    v
}

fn call<T: Tool>(args: Value) -> Result<Value, String> {
    serde_json::from_value::<T>(args).map_err(|e| format!("invalid arguments: {e}"))?.run()
}

fn call_cli<T: Tool>(m: &ArgMatches) -> Result<Value, String> {
    T::from_arg_matches(m).map_err(|e| e.to_string())?.run()
}

pub const TOOLS: &[Entry] = &[
    entry::<Inspect>(),
    entry::<Presets>(),
    entry::<Run>(),
    entry::<Cohort>(),
    entry::<Track>(),
    entry::<ClassifyTrain>(),
    entry::<ClassifyPredict>(),
    entry::<Demo>(),
    entry::<DemoCohort>(),
];

pub fn find(name: &str) -> Option<&'static Entry> {
    TOOLS.iter().find(|t| t.name == name)
}

// ---- shared argument types ----

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum Scalar {
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
}

/// JSON `{"preprocess.lowpass_hz": 6}` → the CLI's `[("preprocess.lowpass_hz", "6")]`; `preset::with_overrides` validates both.
fn set_map<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
    let m = BTreeMap::<String, Scalar>::deserialize(d)?;
    Ok(m.into_iter()
        .map(|(k, v)| {
            let v = match v {
                Scalar::Bool(b) => b.to_string(),
                Scalar::Num(n) => n.to_string(),
                Scalar::Str(s) => s,
            };
            (k, v)
        })
        .collect())
}

fn parse_set(s: &str) -> Result<(String, String), String> {
    let (k, v) = s.split_once('=').ok_or("expected key=value, e.g. preprocess.lowpass_hz=6")?;
    Ok((k.trim().to_string(), v.trim().to_string()))
}

/// Arena ROI as frame fractions → pixels at the analysis width (height keeps 4:3).
fn roi_px(roi: Option<&[f64]>, width: usize) -> Result<Option<[usize; 4]>, String> {
    let (w, h) = (width.clamp(64, 640) as f64, ((width.clamp(64, 640) * 3 / 4) & !1) as f64);
    match roi {
        Some(&[x0, y0, x1, y1]) if 0.0 <= x0 && x0 < x1 && x1 <= 1.0 && 0.0 <= y0 && y0 < y1 && y1 <= 1.0 => {
            Ok(Some([(x0 * w) as usize, (y0 * h) as usize, (x1 * w).ceil() as usize, (y1 * h).ceil() as usize]))
        }
        Some(_) => Err("roi needs four fractions 0 ≤ x0 < x1 ≤ 1, 0 ≤ y0 < y1 ≤ 1".into()),
        None => Ok(None),
    }
}

fn track_cam1(block: &Path, width: usize, roi: Option<&[f64]>) -> Result<(crate::Recording, behavior::TrackResult), String> {
    let rec = crate::load(block, &preset::builtin("grab-ach3.0-tdt").unwrap())?;
    let params = simplefp_video::track::TrackParams { roi: roi_px(roi, width)?, ..Default::default() };
    let r = behavior::track_block(&rec, "Cam1", width, params, None, 1.0)?;
    Ok((rec, r))
}

fn name_of(p: &Path) -> String {
    p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())
}

fn write_json(path: &Path, v: &impl serde::Serialize) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- tools ----

/// List the streams and epocs (event stores) of a TDT block folder.
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    /// TDT block folder (the one containing .tsq/.tev).
    pub block: PathBuf,
}

impl Tool for Inspect {
    const NAME: &'static str = "inspect";
    fn run(self) -> Result<Value, String> {
        let b = simplefp_tdt::read_block(&self.block, &simplefp_tdt::ReadOptions::default()).map_err(|e| e.to_string())?;
        let mut s = format!("{}  ({})\n", b.path.display(), b.duration.map_or("unclosed".into(), |d| format!("{d:.1} s")));
        let mut streams = vec![];
        for st in b.streams.values() {
            let n = st.channels.first().map_or(0, Vec::len);
            let _ = writeln!(s, "  stream {:<6} {:>2} ch  {:>10.4} Hz  {:>9} samples  start {:.4} s", st.name, st.channels.len(), st.fs, n, st.start_time);
            streams.push(json!({"name": st.name, "channels": st.channels.len(), "fs": st.fs, "samples": n, "start_s": st.start_time}));
        }
        let mut epocs = vec![];
        for e in b.epocs.values() {
            let first = e.onset.first().map_or(String::new(), |t| format!("first {t:.3} s"));
            let _ = writeln!(s, "  epoc   {:<6} {:>5} events  {first}", e.name, e.onset.len());
            epocs.push(json!({"name": e.name, "events": e.onset.len(), "first_s": e.onset.first()}));
        }
        Ok(json!({"summary": s, "block": b.path, "duration_s": b.duration, "streams": streams, "epocs": epocs}))
    }
}

/// List the built-in analysis presets, or print one as TOML, optionally with overrides applied and validated
/// (use this to check a parameter change before running).
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Presets {
    /// Built-in preset id or path to a .toml preset; omit to list the built-ins.
    pub id: Option<String>,
    /// Preset overrides by dotted key, e.g. preprocess.lowpass_hz=6 (CLI: repeat --set). Validated like Easy mode.
    #[arg(long = "set", value_parser = parse_set)]
    #[serde(default, deserialize_with = "set_map")]
    #[schemars(with = "BTreeMap<String, Scalar>")]
    pub set: Vec<(String, String)>,
}

impl Tool for Presets {
    const NAME: &'static str = "presets";
    fn run(self) -> Result<Value, String> {
        let Some(id) = self.id else {
            let mut s = String::new();
            let mut list = vec![];
            for (id, _) in preset::BUILTIN {
                let p = preset::builtin(id).unwrap();
                let _ = writeln!(s, "{id:<18} {}  — {}", p.name, p.description);
                list.push(json!({"id": id, "name": p.name, "description": p.description}));
            }
            return Ok(json!({"summary": s, "presets": list}));
        };
        // Built-ins print their source (with comments) unless overridden.
        let toml = match preset::BUILTIN.iter().find(|(i, _)| *i == id) {
            Some((_, src)) if self.set.is_empty() => src.to_string(),
            _ => preset::with_overrides(&crate::load_preset(&id, Path::new("."))?, &self.set)?.to_toml(),
        };
        Ok(json!({"summary": toml, "id": id, "toml": toml}))
    }
}

/// Preprocess a TDT block (filtering, bleaching and motion correction, ΔF/F) and run peri-event analysis with
/// bootstrap CIs. Writes CSV/JSON results and run.json provenance to `out` and returns the QC.
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// TDT block folder.
    pub block: PathBuf,
    /// Built-in preset id or path to a .toml preset (default grab-ach3.0-tdt).
    #[arg(short, long)]
    pub preset: Option<String>,
    /// Epoc stores whose onsets are events, e.g. PC0_ (CLI: repeatable).
    #[arg(long)]
    #[serde(default)]
    pub epoc: Vec<String>,
    /// Epoc stores whose offsets are events (CLI: repeatable).
    #[arg(long)]
    #[serde(default)]
    pub epoc_offset: Vec<String>,
    /// CSV of manual events with header `event,time_s` (seconds on the block clock).
    #[arg(long)]
    pub events: Option<PathBuf>,
    /// Output directory (default <block>/simplefp).
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Preset overrides by dotted key, e.g. preprocess.lowpass_hz=6 (CLI: repeat --set). Validated like Easy mode.
    #[arg(long = "set", value_parser = parse_set)]
    #[serde(default, deserialize_with = "set_map")]
    #[schemars(with = "BTreeMap<String, Scalar>")]
    pub set: Vec<(String, String)>,
}

impl Tool for Run {
    const NAME: &'static str = "run";
    fn run(self) -> Result<Value, String> {
        let p = preset::with_overrides(&crate::load_preset(self.preset.as_deref().unwrap_or("grab-ach3.0-tdt"), Path::new("."))?, &self.set)?;
        let rec = crate::load(&self.block, &p)?;
        let mut sources: Vec<EventSource> = self.epoc.into_iter().map(EventSource::Epoc).collect();
        sources.extend(self.epoc_offset.into_iter().map(EventSource::EpocOffset));
        if let Some(f) = &self.events {
            sources.extend(crate::read_events_csv(f)?);
        }
        let a = rec.analyze(&p, &sources)?;
        let out = self.out.unwrap_or_else(|| self.block.join("simplefp"));
        crate::export(&rec, &p, &a, &out)?;
        let q = &a.output.qc;
        let mut s = format!("{} · {} vs {}\n", p.name, rec.signal, rec.control.as_deref().unwrap_or("no control"));
        let _ = writeln!(s, "  {:.1} s at {:.3} Hz (÷{}), bleaching {:.1} % signal / {} control", q.duration_s, q.fs, q.decimation, q.bleach_signal_pct, q.bleach_control_pct.map_or("–".into(), |v| format!("{v:.1} %")));
        if let (Some(r), Some(m)) = (q.motion_r, q.motion_slope) {
            let _ = writeln!(s, "  control fit r = {r:.3}, slope = {m:.3}; noise ≈ {:.3} % ΔF/F", q.noise_dff_pct);
        }
        for w in &q.warnings {
            let _ = writeln!(s, "  warning: {w}");
        }
        let mut events = vec![];
        for ev in &a.events {
            let sig_s = ev.ci.as_ref().map(|c| c.significant.iter().filter(|&&x| x).count() as f64 / q.fs);
            let sig = match sig_s {
                Some(v) => format!("{v:.2} s with CI excluding 0"),
                None => format!("no CI (< {} trials)", simplefp_core::analysis::MIN_TRIALS_FOR_CI),
            };
            let _ = writeln!(s, "  {:<12} {:>3} trials ({} dropped), {sig}", ev.label, ev.epochs.trials.len(), ev.epochs.dropped.len());
            events.push(json!({"label": ev.label, "trials": ev.epochs.trials.len(), "dropped": ev.epochs.dropped.len(), "ci_excludes_zero_s": sig_s}));
        }
        let _ = writeln!(s, "  → {}", out.display());
        Ok(json!({"summary": s, "preset": p.name, "signal": rec.signal, "control": rec.control, "qc": q, "events": events, "out": out}))
    }
}

/// Group analysis of many recordings from a cohort manifest, with the animal as the unit of inference
/// (bootstrap CIs, permutation tests, Benjamini–Hochberg).
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Cohort {
    /// Path to cohort.toml.
    pub manifest: PathBuf,
    /// Output directory (default <manifest dir>/simplefp-cohort).
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

impl Tool for Cohort {
    const NAME: &'static str = "cohort";
    fn run(self) -> Result<Value, String> {
        let run = cohort::run(&self.manifest)?;
        let out = self.out.unwrap_or_else(|| self.manifest.parent().unwrap_or(Path::new(".")).join("simplefp-cohort"));
        cohort::export_cohort(&run, &out)?;
        let g = &run.group;
        let ok = run.reports.iter().filter(|r| r.status == "ok").count();
        let mut s = format!("{} · {} · {ok}/{} sessions analyzed\n", run.manifest.name, run.preset.name, run.reports.len());
        for r in run.reports.iter().filter(|r| r.status != "ok") {
            let _ = writeln!(s, "  {}: {}", r.block, r.status);
        }
        let mut groups = vec![];
        for c in &g.groups {
            let _ = writeln!(s, "  {:<10} {:<10} {:<10} n = {} animals", c.event, c.group, c.condition, c.n_animals);
            groups.push(json!({"event": c.event, "group": c.group, "condition": c.condition, "n_animals": c.n_animals}));
        }
        for t in g.metric_tests.iter().filter(|t| t.metric == "mean" || t.metric == "window_mean") {
            let _ = writeln!(
                s,
                "  {:<10} {:<28} Δ mean = {:+.3} [{:+.3}, {:+.3}]  p = {:.4}{}  q = {:.4}  (n = {}/{})",
                t.event, t.contrast, t.estimate, t.lo, t.hi, t.p, if t.exact { " exact" } else { "" }, t.q, t.n[0], t.n[1]
            );
        }
        for w in &g.warnings {
            let _ = writeln!(s, "  warning: {w}");
        }
        let _ = writeln!(s, "  → {}", out.display());
        Ok(json!({"summary": s, "name": run.manifest.name, "preset": run.preset.name, "sessions": run.reports, "groups": groups, "metric_tests": g.metric_tests, "warnings": g.warnings, "out": out}))
    }
}

/// Track the animal in a block's camera video (classical, no model; needs ffmpeg): centroid, speed and immobility
/// on the TDT clock.
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Track {
    /// TDT block folder.
    pub block: PathBuf,
    /// Camera epoc and video (default Cam1).
    #[arg(long)]
    pub camera: Option<String>,
    /// Analysis width in pixels, 64–640; height keeps 4:3 (default 160).
    #[arg(long)]
    pub width: Option<usize>,
    /// Immobility threshold on motion energy (gray levels/frame); default suggested from the data.
    #[arg(long)]
    pub immobility_threshold: Option<f64>,
    /// Minimum immobility bout in seconds (default 1).
    #[arg(long)]
    pub min_immobile_s: Option<f64>,
    /// The animal is lighter than the background.
    #[arg(long)]
    #[serde(default)]
    pub light_animal: bool,
    /// Arena as frame fractions x0,y0,x1,y1, e.g. 0.42,0.42,0.78,0.92 (default auto-detect).
    #[arg(long, value_delimiter = ',')]
    pub roi: Option<Vec<f64>>,
    /// Output directory (default <block>/simplefp/behavior).
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

impl Tool for Track {
    const NAME: &'static str = "track";
    fn run(self) -> Result<Value, String> {
        let width = self.width.unwrap_or(160);
        let rec = crate::load(&self.block, &preset::builtin("grab-ach3.0-tdt").unwrap())?;
        let params = simplefp_video::track::TrackParams { dark_animal: !self.light_animal, roi: roi_px(self.roi.as_deref(), width)?, ..Default::default() };
        let r = behavior::track_block(&rec, self.camera.as_deref().unwrap_or("Cam1"), width, params, self.immobility_threshold, self.min_immobile_s.unwrap_or(1.0))?;
        let out = self.out.unwrap_or_else(|| self.block.join("simplefp").join("behavior"));
        behavior::export_behavior(&r, &out)?;
        let f = &r.features;
        let imm = f.immobile.iter().filter(|&&b| b).count() as f64 / f.t.len().max(1) as f64;
        let mut s = format!("arena ROI {:?} at {width} px wide\n", r.roi);
        let _ = writeln!(s, "{} frames · {:.1} % lost · immobile {:.1} % of the time in {} bouts (threshold {:.2})", r.frames, 100.0 * f.lost_frac, 100.0 * imm, f.bouts.len(), f.immobility_threshold);
        if let Some(w) = &r.warning {
            let _ = writeln!(s, "  warning: {w}");
        }
        let _ = writeln!(s, "  → {}", out.display());
        Ok(json!({"summary": s, "roi_px": r.roi, "width": width, "frames": r.frames, "lost_frac": f.lost_frac, "immobile_frac": imm, "immobile_bouts": f.bouts.len(), "immobility_threshold": f.immobility_threshold, "warning": r.warning, "out": out}))
    }
}

/// Train a behavior detector from human-scored bouts and report cross-validated agreement, the v1 baseline and a
/// learning curve. Slow: tens of seconds per session.
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClassifyTrain {
    /// Blocks to learn from; each needs bouts in `labels`.
    #[arg(required = true)]
    pub blocks: Vec<PathBuf>,
    /// One CSV `behavior,start_s,end_s` per block, in the same order as the blocks.
    #[arg(long, required = true, num_args = 1..)]
    pub labels: Vec<PathBuf>,
    /// Behavior to learn (rows of the labels CSVs with this name).
    #[arg(long)]
    pub behavior: String,
    /// Tracking width in pixels (default 320).
    #[arg(long)]
    pub width: Option<usize>,
    /// Arena as frame fractions x0,y0,x1,y1, applied to every block (default auto-detect).
    #[arg(long, value_delimiter = ',')]
    pub roi: Option<Vec<f64>>,
    /// Model file to write (.json).
    #[arg(short, long)]
    pub out: PathBuf,
}

impl Tool for ClassifyTrain {
    const NAME: &'static str = "classify-train";
    fn run(self) -> Result<Value, String> {
        if self.blocks.is_empty() || self.blocks.len() != self.labels.len() {
            return Err("give one labels file per block".into());
        }
        let width = self.width.unwrap_or(320);
        let mut sessions = vec![];
        for (b, l) in self.blocks.iter().zip(&self.labels) {
            let (rec, r) = track_cam1(b, width, self.roi.as_deref())?;
            let f = behavior::session_features(&r, rec.frame_clock("Cam1")?.fps, width)?;
            let y = behavior::labels(&r.features.t, &behavior::read_bouts_csv(l, &self.behavior)?);
            sessions.push((name_of(b), f, r.features.t.clone(), y));
        }
        let (file, ev) = behavior::train_model(&sessions, &self.behavior, width, Default::default())?;
        write_json(&self.out, &file)?;
        let mut s = format!("{} · trained on {} session(s), {} frames ({} positive)\n", file.id, sessions.len(), ev.frames, ev.positive_frames);
        let _ = writeln!(
            s,
            "  cross-validation ({}, {} folds): frame κ {:.3} · PABAK {:.3} · frame F1 {:.3} · bout F1 {:.3} (tIoU ≥ 0.5; {} TP / {} FP / {} FN) · median onset error {:.2} s",
            ev.scheme, ev.folds, ev.frame.kappa, ev.frame.pabak, ev.frame.f1, ev.bout.f1, ev.bout.true_positive, ev.bout.false_positive, ev.bout.false_negative, ev.bout.median_offset_s
        );
        let b = &file.baseline;
        let _ = writeln!(s, "  v1 baseline, same folds: frame κ {:.3} · bout F1 {:.3}", b["frame"]["kappa"].as_f64().unwrap_or(f64::NAN), b["bout"]["f1"].as_f64().unwrap_or(f64::NAN));
        let _ = writeln!(s, "  learning curve (held-out): {}", file.learning_curve.iter().map(|c| format!("{} bouts κ {:.2} / F1 {:.2}", c.bouts, c.kappa, c.bout_f1)).collect::<Vec<_>>().join(" · "));
        let _ = writeln!(s, "  → {}", self.out.display());
        Ok(json!({"summary": s, "model": file.id, "sessions": sessions.len(), "evaluation": ev, "baseline": file.baseline, "learning_curve": file.learning_curve, "out": self.out}))
    }
}

/// Propose behavior bouts in a block with a trained detector. Proposals are written for human review and never
/// count as scored data until accepted in the app.
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClassifyPredict {
    /// TDT block folder.
    pub block: PathBuf,
    /// Model file from classify-train.
    #[arg(long)]
    pub model: PathBuf,
    /// Arena as frame fractions x0,y0,x1,y1 (default auto-detect).
    #[arg(long, value_delimiter = ',')]
    pub roi: Option<Vec<f64>>,
    /// Proposals file (default <block>/simplefp/proposals.json).
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

impl Tool for ClassifyPredict {
    const NAME: &'static str = "classify-predict";
    fn run(self) -> Result<Value, String> {
        let m = &self.model;
        let file = behavior::ModelFile::parse(&std::fs::read_to_string(m).map_err(|e| format!("{}: {e}", m.display()))?).map_err(|e| format!("{}: {e}", m.display()))?;
        let (rec, r) = track_cam1(&self.block, file.tracking_width, self.roi.as_deref())?;
        let f = behavior::session_features(&r, rec.frame_clock("Cam1")?.fps, file.tracking_width)?;
        let props = behavior::propose(&file, &f, &r.features.t)?;
        let out = self.out.unwrap_or_else(|| self.block.join("simplefp").join("proposals.json"));
        write_json(&out, &props)?;
        let mut s = String::new();
        for a in &props {
            let _ = writeln!(s, "  {} {:.1}–{:.1} s  (confidence {:.2}, proposed)", a.label, a.start_s, a.end_s.unwrap_or(a.start_s), a.confidence.unwrap_or(0.0));
        }
        let _ = writeln!(s, "{} proposals by {} → {}", props.len(), file.id, out.display());
        Ok(json!({"summary": s, "model": file.id, "proposals": props, "out": out}))
    }
}

/// Write a synthetic TDT block with known ground truth (events on epoc `Evt_`).
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Demo {
    /// Directory to create.
    pub dir: PathBuf,
    /// Random seed (default 1).
    #[arg(long)]
    pub seed: Option<u64>,
}

impl Tool for Demo {
    const NAME: &'static str = "demo";
    fn run(self) -> Result<Value, String> {
        let ev = crate::write_demo_block(&self.dir, self.seed.unwrap_or(1))?;
        let s = format!("wrote {} with {} events on epoc 'Evt_'\n", self.dir.display(), ev.len());
        Ok(json!({"summary": s, "block": self.dir, "epoc": "Evt_", "events": ev.len()}))
    }
}

/// Write a synthetic two-group, two-condition cohort with known effects.
#[derive(Args, Deserialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DemoCohort {
    /// Directory to create.
    pub dir: PathBuf,
}

impl Tool for DemoCohort {
    const NAME: &'static str = "demo-cohort";
    fn run(self) -> Result<Value, String> {
        let m = crate::write_demo_cohort(&self.dir)?;
        Ok(json!({"summary": format!("wrote 16 blocks and {}\n", m.display()), "manifest": m}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli<T: Tool>(argv: &[&str]) -> T {
        let m = command::<T>().no_binary_name(true).try_get_matches_from(argv).unwrap();
        T::from_arg_matches(&m).unwrap()
    }

    #[test]
    fn cli_and_json_parse_to_the_same_arguments() {
        let a: Run = cli(&["/b", "--epoc", "PC0_", "-p", "x", "--set", "preprocess.lowpass_hz=6", "--set", "bleach.method=fit_raw"]);
        let b: Run = serde_json::from_value(json!({"block": "/b", "epoc": ["PC0_"], "preset": "x", "set": {"preprocess.lowpass_hz": 6, "bleach.method": "fit_raw"}})).unwrap();
        assert_eq!(a.set.iter().find(|s| s.0 == "preprocess.lowpass_hz"), b.set.iter().find(|s| s.0 == "preprocess.lowpass_hz"));
        assert_eq!(a.set.len(), b.set.len());
        assert_eq!((a.block, a.epoc, a.preset), (b.block, b.epoc, b.preset));
        let a: Track = cli(&["/b", "--roi", "0.1,0.2,0.8,0.9", "--light-animal"]);
        let b: Track = serde_json::from_value(json!({"block": "/b", "roi": [0.1, 0.2, 0.8, 0.9], "light_animal": true})).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn schemas_are_closed_objects_and_names_unique() {
        for (i, t) in TOOLS.iter().enumerate() {
            let s = (t.schema)();
            assert_eq!(s["type"], "object", "{}", t.name);
            assert_eq!(s["additionalProperties"], false, "{}", t.name);
            assert!(s["description"].as_str().is_some_and(|d| d.len() > 20), "{} needs a doc comment", t.name);
            assert!(!TOOLS[..i].iter().any(|u| u.name == t.name));
            (t.command)().debug_assert();
        }
        assert!((find("run").unwrap().call)(json!({"block": "/b", "shell": "rm"})).unwrap_err().contains("unknown field"));
        assert!(roi_px(Some(&[0.5, 0.0, 0.4, 1.0]), 160).is_err());
    }

    #[test]
    fn demo_then_run_through_the_registry() {
        let dir = std::env::temp_dir().join(format!("simplefp-tools-{}", std::process::id()));
        let block = dir.join("Demo-1");
        (find("demo").unwrap().call)(json!({"dir": block})).unwrap();
        let r = (find("run").unwrap().call)(json!({"block": block, "epoc": ["Evt_"], "set": {"preprocess.lowpass_hz": 6}})).unwrap();
        assert!(r["events"][0]["trials"].as_u64().unwrap() > 10);
        assert!(r["summary"].as_str().unwrap().contains("Evt_"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
