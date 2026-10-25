use clap::{Parser, Subcommand};
use simplefp::core::preset::{self, Preset};
use simplefp::{EventSource, tdt};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
    },
    /// List built-in presets, or print one as TOML.
    Presets { id: Option<String> },
    /// Write a synthetic TDT block with known ground truth.
    Demo {
        dir: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
}

fn load_preset(arg: &str) -> Result<Preset, String> {
    if let Some(p) = preset::builtin(arg) {
        return Ok(p);
    }
    let path = Path::new(arg);
    let src = std::fs::read_to_string(path).map_err(|e| format!("preset '{arg}' is not built in and not readable: {e}"))?;
    // Sibling .toml files form the library that `extends` can name.
    let mut lib = BTreeMap::new();
    if let Some(dir) = path.parent() {
        for f in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")) {
            if let (Some(stem), Ok(s)) = (f.file_stem().and_then(|s| s.to_str()), std::fs::read_to_string(&f)) {
                lib.insert(stem.to_string(), s);
            }
        }
    }
    preset::parse(&src, &lib).map_err(|e| format!("{arg}: {e}"))
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
        Cmd::Run { block, preset, epoc, epoc_offset, events, out } => {
            let p = load_preset(&preset)?;
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
        Cmd::Presets { id: None } => {
            for (id, _) in preset::BUILTIN {
                let p = preset::builtin(id).unwrap();
                println!("{id:<18} {}  — {}", p.name, p.description);
            }
        }
        Cmd::Presets { id: Some(id) } => {
            let src = preset::BUILTIN.iter().find(|(i, _)| *i == id).ok_or_else(|| format!("no built-in preset '{id}'"))?.1;
            print!("{src}");
        }
        Cmd::Demo { dir, seed } => {
            let ev = simplefp::write_demo_block(&dir, seed)?;
            println!("wrote {} with {} events on epoc 'Evt_'", dir.display(), ev.len());
        }
    }
    Ok(())
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
