//! SimpleFP engine: the I/O layer shared by the CLI, the macOS app and future
//! front-ends. Loads a TDT block, picks channels by preset, runs the analysis,
//! and writes results with a provenance manifest.

use serde::Serialize;
use simplefp_core::analysis::{self, Analysis};
use simplefp_core::pipeline::Input;
use simplefp_core::preset::Preset;
use simplefp_tdt::{Block, ReadOptions};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

pub mod cohort;

pub use simplefp_core as core;
pub use simplefp_tdt as tdt;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where event times come from.
#[derive(Debug, Clone)]
pub enum EventSource {
    /// Onsets of a TDT epoc store (e.g. `PC0_`).
    Epoc(String),
    /// Offsets of a TDT epoc store.
    EpocOffset(String),
    /// Manually supplied times (s, block clock), e.g. from video scoring.
    Manual { label: String, times: Vec<f64> },
}

/// Resolve a preset: a built-in id, or a `.toml` path (relative to `base`).
/// Sibling `.toml` files form the library that `extends` can name.
pub fn load_preset(arg: &str, base: &Path) -> Result<Preset, String> {
    if let Some(p) = simplefp_core::preset::builtin(arg) {
        return Ok(p);
    }
    let path = if Path::new(arg).is_absolute() { PathBuf::from(arg) } else { base.join(arg) };
    let src = fs::read_to_string(&path).map_err(|e| format!("preset '{arg}' is not built in and not readable: {e}"))?;
    let mut lib = std::collections::BTreeMap::new();
    if let Some(dir) = path.parent() {
        for f in fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")) {
            if let (Some(stem), Ok(s)) = (f.file_stem().and_then(|s| s.to_str()), fs::read_to_string(&f)) {
                lib.insert(stem.to_string(), s);
            }
        }
    }
    simplefp_core::preset::parse(&src, &lib).map_err(|e| format!("{arg}: {e}"))
}

/// A loaded recording with the channels chosen by the preset.
pub struct Recording {
    pub block: Block,
    pub signal: String,
    pub control: Option<String>,
}

pub fn load(path: &Path, preset: &Preset) -> Result<Recording, String> {
    let block = simplefp_tdt::read_block(path, &ReadOptions::default()).map_err(|e| e.to_string())?;
    let pick = |names: &[String]| names.iter().find(|n| block.streams.contains_key(n.as_str())).cloned();
    let signal = pick(&preset.channels.signal).ok_or_else(|| {
        format!("none of the signal stores {:?} found; block has {:?}", preset.channels.signal, block.streams.keys().collect::<Vec<_>>())
    })?;
    let control = pick(&preset.channels.control);
    if !preset.channels.control.is_empty() && control.is_none() {
        return Err(format!("none of the control stores {:?} found", preset.channels.control));
    }
    Ok(Recording { block, signal, control })
}

impl Recording {
    pub fn event_sets(&self, sources: &[EventSource]) -> Result<Vec<(String, Vec<f64>)>, String> {
        sources
            .iter()
            .map(|s| match s {
                EventSource::Epoc(n) | EventSource::EpocOffset(n) => {
                    let e = self.block.epocs.get(n).ok_or_else(|| format!("epoc store '{n}' not found; block has {:?}", self.block.epocs.keys().collect::<Vec<_>>()))?;
                    Ok(match s {
                        EventSource::Epoc(_) => (n.clone(), e.onset.clone()),
                        _ => (format!("{n}_offset"), e.offset.iter().copied().filter(|t| t.is_finite()).collect()),
                    })
                }
                EventSource::Manual { label, times } => Ok((label.clone(), times.clone())),
            })
            .collect()
    }

    pub fn analyze(&self, preset: &Preset, sources: &[EventSource]) -> Result<Analysis, String> {
        let s = &self.block.streams[&self.signal];
        let to64 = |v: &[f32]| v.iter().map(|&x| x as f64).collect::<Vec<f64>>();
        let sig = to64(&s.channels[0]);
        let ctl = self.control.as_ref().map(|c| to64(&self.block.streams[c].channels[0]));
        if let Some(c) = &self.control {
            let cs = &self.block.streams[c];
            if (cs.fs - s.fs).abs() > 1e-6 || (cs.start_time - s.start_time).abs() > 1.0 / s.fs {
                return Err(format!("{} and {c} are not sampled on the same clock", self.signal));
            }
        }
        let n = sig.len().min(ctl.as_ref().map_or(usize::MAX, Vec::len));
        let input = Input { fs: s.fs, t0: s.start_time, signal: &sig[..n], control: ctl.as_deref().map(|c| &c[..n]) };
        analysis::analyze(&input, preset, &self.event_sets(sources)?)
    }
}

/// Fields for a lab `session` row from a block on disk: identity read from the
/// TSQ index (no sample data), channels chosen by the preset, and the block path
/// stored relative to `data_root` so the store survives volume renames.
pub fn session_fields(block: &Path, data_root: Option<&Path>, preset: &Preset) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let info = simplefp_tdt::block_info(block).map_err(|e| e.to_string())?;
    let pick = |names: &[String]| names.iter().find(|n| info.streams.contains(n)).cloned();
    let signal = pick(&preset.channels.signal).ok_or_else(|| format!("{}: none of the signal stores {:?} found", info.name, preset.channels.signal))?;
    let tsq = fs::read(&info.tsq_path).map_err(|e| format!("{}: {e}", info.tsq_path.display()))?;
    let abs = block.canonicalize().map_err(|e| format!("{}: {e}", block.display()))?;
    let rel = data_root.and_then(|r| r.canonicalize().ok()).and_then(|r| abs.strip_prefix(r).ok().map(Path::to_path_buf)).unwrap_or(abs);
    let mut m = serde_json::Map::new();
    m.insert("block_path".into(), rel.display().to_string().into());
    m.insert("block_digest".into(), blake3::hash(&tsq).to_hex().to_string().into());
    m.insert("block_name".into(), info.name.into());
    m.insert("started_unix".into(), info.start_unix.into());
    m.insert("signal_store".into(), signal.into());
    m.insert("control_store".into(), pick(&preset.channels.control).map_or(serde_json::Value::Null, Into::into));
    Ok(m)
}

/// Parse a manual events CSV: header `event,time_s`, one event per row.
pub fn read_events_csv(path: &Path) -> Result<Vec<EventSource>, String> {
    let src = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = src.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'));
    match lines.next() {
        Some((_, h)) if h.trim().eq_ignore_ascii_case("event,time_s") => {}
        _ => return Err(format!("{}: first line must be the header 'event,time_s'", path.display())),
    }
    let mut sets: Vec<(String, Vec<f64>)> = vec![];
    for (i, line) in lines {
        let (label, t) = line.split_once(',').ok_or_else(|| format!("line {}: expected 'event,time_s'", i + 1))?;
        let label = label.trim();
        if label.is_empty() || label.len() > 64 || !label.chars().all(|c| c.is_ascii_alphanumeric() || "_-. ".contains(c)) {
            return Err(format!("line {}: event label must be 1-64 chars of [A-Za-z0-9_-. ]", i + 1));
        }
        let t: f64 = t.trim().parse().map_err(|_| format!("line {}: '{}' is not a number", i + 1, t.trim()))?;
        if !t.is_finite() || t < 0.0 {
            return Err(format!("line {}: time must be a finite, non-negative number of seconds", i + 1));
        }
        match sets.iter_mut().find(|(l, _)| l == label) {
            Some((_, v)) => v.push(t),
            None => sets.push((label.to_string(), vec![t])),
        }
    }
    Ok(sets.into_iter().map(|(label, times)| EventSource::Manual { label, times }).collect())
}

#[derive(Serialize)]
struct FileDigest {
    name: String,
    bytes: u64,
    blake3: String,
}

#[derive(Serialize)]
struct Manifest<'a> {
    engine: &'static str,
    engine_version: &'static str,
    block: String,
    block_start_unix: f64,
    files: Vec<FileDigest>,
    preset_blake3: String,
    signal: &'a str,
    control: Option<&'a str>,
    preset: &'a Preset,
    qc: &'a simplefp_core::pipeline::Qc,
    events: Vec<serde_json::Value>,
}

fn digests(dir: &Path) -> Vec<FileDigest> {
    let mut v: Vec<FileDigest> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| ["tsq", "tev", "sev", "tbk", "tdx", "tin"].iter().any(|k| x.eq_ignore_ascii_case(k))))
        .filter_map(|e| {
            let bytes = fs::read(e.path()).ok()?;
            Some(FileDigest { name: e.file_name().to_string_lossy().into(), bytes: bytes.len() as u64, blake3: blake3::hash(&bytes).to_hex().to_string() })
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Quote a CSV field if it contains a delimiter, quote or newline.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn num(v: f64) -> String {
    if v.is_nan() { String::new() } else { format!("{v}") }
}

/// Write all results under `out` (created if missing). Returns the files written.
pub fn export(rec: &Recording, preset: &Preset, a: &Analysis, out: &Path) -> Result<Vec<PathBuf>, String> {
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    fs::create_dir_all(out).map_err(|e| io(out, e))?;
    let mut written = vec![];
    let mut put = |p: PathBuf, s: String| -> Result<(), String> {
        fs::write(&p, s).map_err(|e| io(&p, e))?;
        written.push(p);
        Ok(())
    };

    let o = &a.output;
    let mut s = String::from("time_s,signal,control,signal_baseline,control_baseline,motion_fit,dff_pct,z,valid\n");
    let opt = |v: &Option<Vec<f64>>, i: usize| v.as_ref().map_or(String::new(), |v| num(v[i]));
    for i in 0..o.dff.len() {
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{},{},{}",
            o.time(i),
            o.signal[i],
            opt(&o.control, i),
            o.signal_baseline[i],
            opt(&o.control_baseline, i),
            opt(&o.motion_fit, i),
            num(o.dff[i]),
            num(o.z[i]),
            o.valid.contains(&i) as u8
        );
    }
    put(out.join("trace.csv"), s)?;

    let mut ev_meta = vec![];
    for ev in &a.events {
        let dir = out.join("events").join(safe_name(&ev.label));
        fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let ep = &ev.epochs;
        let mut s = String::from("time_s");
        ep.event_times.iter().for_each(|t| _ = write!(s, ",trial@{t:.3}"));
        s.push('\n');
        for (j, t) in ep.t.iter().enumerate() {
            let _ = write!(s, "{t}");
            ep.trials.iter().for_each(|r| _ = write!(s, ",{}", num(r[j])));
            s.push('\n');
        }
        put(dir.join("epochs.csv"), s)?;
        let summary = |ci: &simplefp_core::events::MeanCi| {
            let mut s = String::from("time_s,mean,sem,ci_lo,ci_hi,significant\n");
            for j in 0..ep.t.len() {
                let _ = writeln!(s, "{},{},{},{},{},{}", ep.t[j], ci.mean[j], num(ci.sem[j]), ci.lo[j], ci.hi[j], ci.significant[j] as u8);
            }
            s
        };
        if let Some(ci) = &ev.ci {
            put(dir.join("summary.csv"), summary(ci))?;
        }
        if let Some(ci) = &ev.control_ci {
            put(dir.join("control_summary.csv"), summary(ci))?;
        }
        let mut s = String::from("trial,event_time_s,mean,auc,auc_pos,auc_neg,peak,peak_latency_s,trough,trough_latency_s\n");
        for (k, (m, t)) in ev.metrics.iter().zip(&ep.event_times).enumerate() {
            let _ = writeln!(s, "{},{t},{},{},{},{},{},{},{},{}", k + 1, num(m.mean), num(m.auc), num(m.auc_pos), num(m.auc_neg), num(m.peak), m.peak_latency, num(m.trough), m.trough_latency);
        }
        put(dir.join("metrics.csv"), s)?;
        ev_meta.push(serde_json::json!({
            "label": ev.label,
            "n_trials": ep.trials.len(),
            "dropped": ep.dropped,
            "control_responds_in_response_window": ev.control_responds,
            "inference_unit": "trials of one recording (descriptive); average per animal before group inference",
        }));
    }

    // Long format for functional mixed models (fastFMM: `fui(dff ~ ... + (1 | session))`).
    let session = rec.block.path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let mut s = String::from("session,event,trial,event_time_s,t,dff_baseline_sub\n");
    for ev in &a.events {
        for (k, (row, et)) in ev.epochs.trials.iter().zip(&ev.epochs.event_times).enumerate() {
            for (t, v) in ev.epochs.t.iter().zip(row) {
                let _ = writeln!(s, "{},{},{},{et},{t},{}", csv_field(&session), csv_field(&ev.label), k + 1, num(*v));
            }
        }
    }
    put(out.join("flmm_long.csv"), s)?;

    let manifest = Manifest {
        engine: "simplefp",
        engine_version: VERSION,
        block: rec.block.path.display().to_string(),
        block_start_unix: rec.block.start_unix,
        files: digests(&rec.block.path),
        preset_blake3: blake3::hash(preset.to_toml().as_bytes()).to_hex().to_string(),
        signal: &rec.signal,
        control: rec.control.as_deref(),
        preset,
        qc: &o.qc,
        events: ev_meta,
    };
    put(out.join("run.json"), serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?)?;
    Ok(written)
}

/// Write a demo cohort: two groups × 4 animals × fed/fasted. Group "ChAT" has a
/// larger evoked response (4 % vs 2 % ΔF/F) and fasting adds 1 %, so both a
/// between-group and a within-subject effect exist. Returns the manifest path.
pub fn write_demo_cohort(dir: &Path) -> Result<PathBuf, String> {
    let mut toml = String::from("schema = 1\nname = \"Demo cohort\"\ndescription = \"Synthetic: ChAT 4 % vs WT 2 % evoked ΔF/F; fasting +1 %.\"\npreset = \"grab-ach3.0-tdt\"\nevents = [{ epoc = \"Evt_\" }]\n");
    let mut seed = 10;
    for (group, amp) in [("ChAT", 0.04), ("WT", 0.02)] {
        for k in 1..=4 {
            let animal = format!("{}{k}", &group[..1]);
            for (cond, extra) in [("fed", 0.0), ("fasted", 0.01)] {
                seed += 1;
                // Animals differ in response size, as real ones do.
                let a = (amp + extra) * (0.8 + 0.1 * k as f64);
                let name = format!("{animal}-{cond}");
                write_demo_block_with(&dir.join(&name), &simplefp_core::sim::SimParams { amplitude: a, seed, ..Default::default() })?;
                let _ = write!(toml, "\n[[session]]\nblock = \"{name}\"\nanimal = \"{animal}\"\ngroup = \"{group}\"\ncondition = \"{cond}\"\n");
            }
        }
    }
    toml.push_str("\n[[contrast]]\nkind = \"between\"\ngroups = [\"ChAT\", \"WT\"]\ncondition = \"fed\"\n\n[[contrast]]\nkind = \"within\"\nconditions = [\"fasted\", \"fed\"]\ngroup = \"ChAT\"\n");
    let path = dir.join("cohort.toml");
    fs::write(&path, toml).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Write a realistic synthetic TDT block with an `Evt_` epoc at the evoked events.
pub fn write_demo_block(dir: &Path, seed: u64) -> Result<Vec<f64>, String> {
    write_demo_block_with(dir, &simplefp_core::sim::SimParams { seed, ..Default::default() })
}

pub fn write_demo_block_with(dir: &Path, params: &simplefp_core::sim::SimParams) -> Result<Vec<f64>, String> {
    use simplefp_core::sim;
    use simplefp_tdt::synth::*;
    let s = sim::simulate(params);
    let f32s = |v: &[f64]| v.iter().map(|&x| x as f32).collect::<Vec<f32>>();
    let (sig, ctl) = (f32s(&s.signal), f32s(&s.control));
    write_block(
        dir,
        dir.file_name().and_then(|n| n.to_str()).unwrap_or("Demo"),
        1.75e9,
        &[SynthStream { name: "465A", fs: s.fs as f32, channels: &[&sig] }, SynthStream { name: "405A", fs: s.fs as f32, channels: &[&ctl] }],
        &[SynthEpoc { name: "Evt_", onsets: &s.events, offsets: None }],
    )
    .map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(s.events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_roundtrip_through_tdt_files() {
        let dir = std::env::temp_dir().join(format!("simplefp-demo-{}", std::process::id()));
        let truth = write_demo_block(&dir, 3).unwrap();
        let p = simplefp_core::preset::builtin("grab-ach3.0-tdt").unwrap();
        let rec = load(&dir, &p).unwrap();
        assert_eq!((rec.signal.as_str(), rec.control.as_deref()), ("465A", Some("405A")));
        let a = rec.analyze(&p, &[EventSource::Epoc("Evt_".into())]).unwrap();
        assert_eq!(a.events[0].epochs.trials.len() + a.events[0].epochs.dropped.len(), truth.len());
        let out = dir.join("out");
        let files = export(&rec, &p, &a, &out).unwrap();
        assert!(files.iter().any(|f| f.ends_with("run.json")));
        let manifest = fs::read_to_string(out.join("run.json")).unwrap();
        assert!(manifest.contains("blake3"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn demo_cohort_detects_designed_effects() {
        let dir = std::env::temp_dir().join(format!("simplefp-cohort-{}", std::process::id()));
        let manifest = write_demo_cohort(&dir).unwrap();
        let run = cohort::run(&manifest).unwrap();
        let g = &run.group;
        assert_eq!(g.animals.len(), 16); // 8 animals × 2 conditions, one event
        let test = |contrast: &str| g.metric_tests.iter().find(|t| t.contrast.starts_with(contrast) && t.metric == "mean").unwrap();
        let between = test("ChAT − WT");
        let within = test("fasted − fed");
        assert!(between.estimate > 0.0 && between.p < 0.05 && between.exact, "{between:?}");
        assert!(within.estimate > 0.0 && within.p <= 0.125, "{within:?}"); // 4 pairs: exact sign-flip floor is 2/16
        let curve = g.contrasts.iter().find(|c| c.label.starts_with("ChAT − WT")).unwrap();
        assert!(curve.significant.iter().any(|&s| s));
        let files = cohort::export_cohort(&run, &dir.join("out")).unwrap();
        assert!(files.iter().any(|f| f.ends_with("metric_tests.csv")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn events_csv_validation() {
        let dir = std::env::temp_dir().join(format!("simplefp-ev-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("e.csv");
        fs::write(&p, "event,time_s\nbite,12.5\nbite,40\napproach,20\n").unwrap();
        let ev = read_events_csv(&p).unwrap();
        assert_eq!(ev.len(), 2);
        fs::write(&p, "event,time_s\nbite,-1\n").unwrap();
        assert!(read_events_csv(&p).is_err());
        fs::write(&p, "label,t\nbite,1\n").unwrap();
        assert!(read_events_csv(&p).is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
