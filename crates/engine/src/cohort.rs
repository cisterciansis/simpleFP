//! Cohort manifests: many recordings, one preset, group inference with the
//! animal as the unit. The manifest is the pre-registration: sessions, their
//! animal/group/condition, exclusions (with reasons) and contrasts are all
//! declared before results exist.
//!
//! ```toml
//! schema = 1
//! name = "Feeding · ChAT vs WT"
//! preset = "grab-ach3.0-tdt"          # built-in id or a path relative to this file
//! events = [{ epoc = "PC0_" }]         # default for every session
//!
//! [[session]]
//! block = "tanks/A1-250619-103950"     # relative to this file
//! animal = "A1"
//! group = "ChAT"
//! condition = "fasted"
//!
//! [[contrast]]
//! kind = "between"
//! groups = ["ChAT", "WT"]
//! condition = "fasted"
//! ```

use crate::{EventSource, Recording, export, load, load_preset, read_events_csv};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use simplefp_core::analysis::Analysis;
use simplefp_core::group::{self, ContrastSpec, GroupAnalysis, GroupOpts, SessionEvent};
use simplefp_core::preset::Preset;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub preset: String,
    #[serde(default)]
    pub events: Vec<EventRef>,
    #[serde(default, rename = "session")]
    pub sessions: Vec<SessionEntry>,
    #[serde(default, rename = "contrast")]
    pub contrasts: Vec<ContrastSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum EventRef {
    Epoc { epoc: String },
    EpocOffset { epoc_offset: String },
    /// `event,time_s` CSV, path relative to the manifest.
    Csv { csv: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEntry {
    pub block: String,
    pub animal: String,
    #[serde(default = "default_group")]
    pub group: String,
    #[serde(default = "default_condition")]
    pub condition: String,
    /// Replaces the manifest-level `events` for this session.
    #[serde(default)]
    pub events: Option<Vec<EventRef>>,
    #[serde(default)]
    pub exclude: bool,
    #[serde(default)]
    pub exclude_reason: Option<String>,
}

fn default_group() -> String {
    "all".into()
}
fn default_condition() -> String {
    "session".into()
}

fn check_label(what: &str, s: &str) -> Result<(), String> {
    let ok = !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_alphanumeric() || " _-.+".contains(c));
    if ok { Ok(()) } else { Err(format!("{what} '{s}' must be 1–64 characters of letters, digits, space or _-.+")) }
}

impl Manifest {
    pub fn parse(src: &str) -> Result<Self, String> {
        let m: Manifest = toml::from_str(src).map_err(|e| e.to_string())?;
        if m.schema != 1 {
            return Err(format!("cohort schema {} not supported (expected 1)", m.schema));
        }
        if m.sessions.is_empty() {
            return Err("a cohort needs at least one [[session]]".into());
        }
        let mut blocks = BTreeSet::new();
        for s in &m.sessions {
            check_label("animal", &s.animal)?;
            check_label("group", &s.group)?;
            check_label("condition", &s.condition)?;
            if !blocks.insert(&s.block) {
                return Err(format!("block '{}' is listed twice", s.block));
            }
            if s.exclude && s.exclude_reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
                return Err(format!("session '{}' is excluded without an exclude_reason", s.block));
            }
            if s.events.as_ref().unwrap_or(&m.events).is_empty() {
                return Err(format!("session '{}' has no events (set `events` at the top or on the session)", s.block));
            }
        }
        Ok(m)
    }
}

/// Per-session outcome kept for the report.
#[derive(Debug, Serialize)]
pub struct SessionReport {
    pub block: String,
    pub animal: String,
    pub group: String,
    pub condition: String,
    pub status: String,
    pub qc: Option<simplefp_core::pipeline::Qc>,
}

pub struct CohortRun {
    pub manifest: Manifest,
    pub preset: Preset,
    pub reports: Vec<SessionReport>,
    pub group: GroupAnalysis,
    analyses: Vec<(usize, Recording, Analysis)>,
}

type SessionResult = Result<(Recording, Analysis), String>;

fn resolve(base: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

fn sources(base: &Path, refs: &[EventRef]) -> Result<Vec<EventSource>, String> {
    let mut out = vec![];
    for r in refs {
        match r {
            EventRef::Epoc { epoc } => out.push(EventSource::Epoc(epoc.clone())),
            EventRef::EpocOffset { epoc_offset } => out.push(EventSource::EpocOffset(epoc_offset.clone())),
            EventRef::Csv { csv } => out.extend(read_events_csv(&resolve(base, csv))?),
        }
    }
    Ok(out)
}

/// Load and analyze every included session in parallel, then run group inference.
/// A session that fails to load is reported, not fatal; the run fails only if none succeed.
pub fn run(manifest_path: &Path) -> Result<CohortRun, String> {
    let src = fs::read_to_string(manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest = Manifest::parse(&src).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let base = manifest_path.parent().unwrap_or(Path::new("."));
    let preset = load_preset(&manifest.preset, base)?;

    let results: Vec<(usize, SessionResult)> = manifest
        .sessions
        .par_iter()
        .enumerate()
        .filter(|(_, s)| !s.exclude)
        .map(|(i, s)| {
            let r = (|| {
                let rec = load(&resolve(base, &s.block), &preset)?;
                let src = sources(base, s.events.as_ref().unwrap_or(&manifest.events))?;
                let a = rec.analyze(&preset, &src)?;
                Ok((rec, a))
            })();
            (i, r)
        })
        .collect();

    let mut reports: Vec<SessionReport> = manifest
        .sessions
        .iter()
        .map(|s| SessionReport {
            block: s.block.clone(),
            animal: s.animal.clone(),
            group: s.group.clone(),
            condition: s.condition.clone(),
            status: if s.exclude { format!("excluded: {}", s.exclude_reason.as_deref().unwrap_or("")) } else { String::new() },
            qc: None,
        })
        .collect();
    let mut analyses = vec![];
    for (i, r) in results {
        match r {
            Ok((rec, a)) => {
                reports[i].status = "ok".into();
                reports[i].qc = Some(a.output.qc.clone());
                analyses.push((i, rec, a));
            }
            Err(e) => reports[i].status = format!("failed: {e}"),
        }
    }
    if analyses.is_empty() {
        return Err(format!("no session could be analyzed: {}", reports.iter().map(|r| format!("{} ({})", r.block, r.status)).collect::<Vec<_>>().join("; ")));
    }
    let session_events: Vec<SessionEvent> = analyses
        .iter()
        .flat_map(|(i, _, a)| {
            let s = &manifest.sessions[*i];
            a.events.iter().map(move |e| SessionEvent {
                session: &s.block,
                animal: &s.animal,
                group: &s.group,
                condition: &s.condition,
                event: &e.label,
                t: &e.epochs.t,
                trials: &e.epochs.trials,
                metrics: &e.metrics,
            })
        })
        .collect();
    let e = &preset.events;
    let fs = analyses[0].2.output.fs;
    let opts = GroupOpts { n_boot: e.n_boot, ci_level: e.ci_level, min_run: (e.min_consecutive_s * fs).ceil() as usize, seed: e.seed };
    let mut group = group::analyze(&session_events, &manifest.contrasts, &opts)?;
    for r in reports.iter().filter(|r| r.status.starts_with("failed")) {
        group.warnings.insert(0, format!("{}: {}", r.block, r.status));
    }
    Ok(CohortRun { manifest, preset, reports, group, analyses })
}

fn num(v: f64) -> String {
    if v.is_nan() { String::new() } else { format!("{v}") }
}

fn q(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// Write cohort tables, per-session exports and the cohort manifest under `out`.
pub fn export_cohort(run: &CohortRun, out: &Path) -> Result<Vec<PathBuf>, String> {
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    fs::create_dir_all(out).map_err(|e| io(out, e))?;
    let mut written = vec![];
    let mut put = |name: &str, s: String| -> Result<(), String> {
        let p = out.join(name);
        fs::write(&p, s).map_err(|e| io(&p, e))?;
        written.push(p);
        Ok(())
    };
    let g = &run.group;

    let mut s = String::from("event,animal,group,condition,n_sessions,n_trials,mean,auc,peak,trough\n");
    for a in &g.animals {
        let m = a.metrics;
        let _ = writeln!(s, "{},{},{},{},{},{},{},{},{},{}", q(&a.event), q(&a.animal), q(&a.group), q(&a.condition), a.n_sessions, a.n_trials, num(m.mean), num(m.auc), num(m.peak), num(m.trough));
    }
    put("animals.csv", s)?;

    let mut s = String::from("event,animal,group,condition,t,mean\n");
    for a in &g.animals {
        for (t, v) in g.t.iter().zip(&a.mean) {
            let _ = writeln!(s, "{},{},{},{},{t},{}", q(&a.event), q(&a.animal), q(&a.group), q(&a.condition), num(*v));
        }
    }
    put("animal_curves.csv", s)?;

    let mut s = String::from("event,group,condition,n_animals,t,mean,ci_lo,ci_hi,significant\n");
    for c in &g.groups {
        for (j, t) in g.t.iter().enumerate() {
            let (lo, hi, sig) = c.ci.as_ref().map_or((f64::NAN, f64::NAN, false), |ci| (ci.lo[j], ci.hi[j], ci.significant[j]));
            let _ = writeln!(s, "{},{},{},{},{t},{},{},{},{}", q(&c.event), q(&c.group), q(&c.condition), c.n_animals, num(c.mean[j]), num(lo), num(hi), sig as u8);
        }
    }
    put("group_curves.csv", s)?;

    let mut s = String::from("event,contrast,n_a,n_b,t,diff,ci_lo,ci_hi,significant\n");
    for c in &g.contrasts {
        for (j, t) in g.t.iter().enumerate() {
            let _ = writeln!(s, "{},{},{},{},{t},{},{},{},{}", q(&c.event), q(&c.label), c.n[0], c.n[1], num(c.diff[j]), num(c.lo[j]), num(c.hi[j]), c.significant[j] as u8);
        }
    }
    put("contrast_curves.csv", s)?;

    let mut s = String::from("event,group,condition,metric,n_animals,mean,ci_lo,ci_hi\n");
    for m in &g.metric_summaries {
        let _ = writeln!(s, "{},{},{},{},{},{},{},{}", q(&m.event), q(&m.group), q(&m.condition), m.metric, m.n_animals, num(m.mean), num(m.lo), num(m.hi));
    }
    put("metric_summaries.csv", s)?;

    let mut s = String::from("event,contrast,metric,n_a,n_b,estimate,ci_lo,ci_hi,p,exact,q_bh\n");
    for m in &g.metric_tests {
        let _ = writeln!(s, "{},{},{},{},{},{},{},{},{},{},{}", q(&m.event), q(&m.contrast), m.metric, m.n[0], m.n[1], num(m.estimate), num(m.lo), num(m.hi), m.p, m.exact as u8, m.q);
    }
    put("metric_tests.csv", s)?;

    // Trial-level long format with the nesting columns fastFMM needs: fui(dff ~ group + (1 | animal/session)).
    let mut s = String::from("animal,group,condition,session,event,trial,event_time_s,t,dff_baseline_sub\n");
    for (i, _, a) in &run.analyses {
        let ses = &run.manifest.sessions[*i];
        for e in &a.events {
            for (k, (row, et)) in e.epochs.trials.iter().zip(&e.epochs.event_times).enumerate() {
                for (t, v) in e.epochs.t.iter().zip(row) {
                    let _ = writeln!(s, "{},{},{},{},{},{},{et},{t},{}", q(&ses.animal), q(&ses.group), q(&ses.condition), q(&ses.block), q(&e.label), k + 1, num(*v));
                }
            }
        }
    }
    put("flmm_long.csv", s)?;

    let manifest = serde_json::json!({
        "engine": "simplefp",
        "engine_version": crate::VERSION,
        "cohort": run.manifest,
        "preset": run.preset,
        "preset_blake3": blake3::hash(run.preset.to_toml().as_bytes()).to_hex().to_string(),
        "inference": "two-stage: trials averaged within animal × condition × event; group statistics across animals",
        "sessions": run.reports,
        "warnings": g.warnings,
    });
    put("cohort.json", serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?)?;

    for (i, rec, a) in &run.analyses {
        let name: String = run.manifest.sessions[*i].block.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        written.extend(export(rec, &run.preset, a, &out.join("sessions").join(name))?);
    }
    Ok(written)
}
