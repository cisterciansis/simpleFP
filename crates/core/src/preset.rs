//! Presets: declarative, versioned analysis recipes stored as TOML.
//!
//! A preset may `extends = "<id>"` another; tables are deep-merged, so a lab
//! preset only states what differs. Unknown keys are rejected so a typo can
//! never silently fall back to a default.

use crate::events::Baseline;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub schema: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    pub sensor: Sensor,
    pub channels: Channels,
    pub preprocess: Preprocess,
    pub bleach: BleachMethod,
    pub motion: MotionMethod,
    pub output: OutputOpts,
    pub events: EventOpts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sensor {
    pub name: String,
    /// Analyte, e.g. "acetylcholine".
    pub analyte: String,
    pub excitation_nm: f64,
    pub control_nm: Option<f64>,
    /// Whether the control wavelength is a true isosbestic point for this sensor.
    /// False for GRAB sensors at 405 nm: the control then carries some ligand-dependent signal.
    pub control_is_isosbestic: bool,
    pub tau_on_s: Option<f64>,
    pub tau_off_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channels {
    /// Candidate store names for the signal, first match wins (e.g. ["465A", "x465A"]).
    pub signal: Vec<String>,
    /// Candidate store names for the control; empty = no control channel.
    #[serde(default)]
    pub control: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preprocess {
    /// Seconds at the start excluded from fitting and statistics (cable/LED settling).
    pub trim_start_s: f64,
    #[serde(default)]
    pub trim_end_s: f64,
    pub lowpass_hz: Option<f64>,
    pub lowpass_order: usize,
    pub target_fs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum BleachMethod {
    DoubleExp { robust: bool },
    /// Robust straight line in time.
    Linear,
    Highpass { cutoff_hz: f64 },
    /// Constant F0 = median of a reference period (block-clock seconds), e.g. the
    /// anesthetized baseline before a state change. Use when the session contains a
    /// tonic change of interest that any detrending would remove (methods-spec §3.4).
    Window { start_s: f64, end_s: f64 },
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum MotionMethod {
    /// Robust (Tukey bisquare) regression of the bleach-detrended control onto
    /// the detrended signal; ΔF/F denominator is the signal's own bleaching fit
    /// (Simpson et al. 2024 order: bleaching first, then motion).
    Irls { tuning: f64 },
    /// OLS variant of `irls` (Lerner et al. 2015; legacy). Biased by large transients.
    Ols,
    /// Robust fit of the raw control to the raw signal; ΔF/F = (S − Ĉ)/Ĉ
    /// (Keevers & Jean-Richard-dit-Bressel 2025; GuPPy). Assumes both channels
    /// bleach proportionally. `bleach` is then used only for QC.
    FitRaw { tuning: f64 },
    /// Bleaching correction only (sensitivity analysis for non-isosbestic controls).
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputOpts {
    /// Session z-score with median/MAD instead of mean/sd.
    pub robust_z: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOpts {
    pub pre_s: f64,
    pub post_s: f64,
    /// Which trace epochs are cut from: "dff" or "z".
    pub trace: Trace,
    pub baseline: Baseline,
    pub baseline_window: [f64; 2],
    /// Window for scalar metrics (mean, AUC, peak, trough).
    pub metric_window: [f64; 2],
    pub n_boot: usize,
    pub ci_level: f64,
    /// Minimum run of consecutive CI-excluding-zero samples, in seconds.
    pub min_consecutive_s: f64,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trace {
    Dff,
    Z,
}

fn in_range(name: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
    if v.is_finite() && v >= lo && v <= hi { Ok(()) } else { Err(format!("{name} = {v} is outside [{lo}, {hi}]")) }
}

impl Preset {
    /// Fully resolved preset (no `extends`), suitable for saving or editing.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("preset serializes")
    }

    /// Bounds checks that serde's types cannot express.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA_VERSION {
            return Err(format!("preset schema {} not supported (expected {SCHEMA_VERSION})", self.schema));
        }
        if self.channels.signal.is_empty() {
            return Err("channels.signal must name at least one store".into());
        }
        let pp = &self.preprocess;
        in_range("preprocess.trim_start_s", pp.trim_start_s, 0.0, 3600.0)?;
        in_range("preprocess.trim_end_s", pp.trim_end_s, 0.0, 3600.0)?;
        in_range("preprocess.target_fs", pp.target_fs, 1.0, 1000.0)?;
        if let Some(f) = pp.lowpass_hz {
            in_range("preprocess.lowpass_hz", f, 0.01, 500.0)?;
            if f >= pp.target_fs / 2.0 {
                return Err(format!("lowpass_hz ({f}) must be below the post-decimation Nyquist ({})", pp.target_fs / 2.0));
            }
        }
        in_range("preprocess.lowpass_order", pp.lowpass_order as f64, 1.0, 8.0)?;
        if let BleachMethod::Highpass { cutoff_hz } = self.bleach {
            in_range("bleach.cutoff_hz", cutoff_hz, 1e-4, 0.1)?;
        }
        if let BleachMethod::Window { start_s, end_s } = self.bleach
            && !(start_s >= 0.0 && end_s - start_s >= 2.0)
        {
            return Err(format!("bleach window [{start_s}, {end_s}] s must start at ≥ 0 and span at least 2 s"));
        }
        if let MotionMethod::Irls { tuning } | MotionMethod::FitRaw { tuning } = self.motion {
            in_range("motion.tuning", tuning, 1.0, 10.0)?;
        }
        let e = &self.events;
        in_range("events.pre_s", e.pre_s, 0.0, 600.0)?;
        in_range("events.post_s", e.post_s, 0.0, 600.0)?;
        in_range("events.ci_level", e.ci_level, 0.5, 0.999)?;
        in_range("events.n_boot", e.n_boot as f64, 100.0, 1e6)?;
        in_range("events.min_consecutive_s", e.min_consecutive_s, 0.0, 60.0)?;
        let [b0, b1] = e.baseline_window;
        if e.baseline != Baseline::None && !(b0 >= -e.pre_s - 1e-9 && b1 <= 1e-9 && b1 - b0 >= 2.0 - 1e-9) {
            return Err(format!("events.baseline_window {:?} must lie in [-{}, 0] and span at least 2 s", e.baseline_window, e.pre_s));
        }
        let [m0, m1] = e.metric_window;
        if !(m0 < m1 && m0 >= -e.pre_s - 1e-9 && m1 <= e.post_s + 1e-9) {
            return Err(format!("events.metric_window {:?} must be increasing and inside [-{}, {}]", e.metric_window, e.pre_s, e.post_s));
        }
        Ok(())
    }
}

/// Built-in presets, embedded at compile time.
pub const BUILTIN: &[(&str, &str)] = &[
    ("grab-ach3.0-tdt", include_str!("../../../presets/grab-ach3.0-tdt.toml")),
    ("gcamp-tdt", include_str!("../../../presets/gcamp-tdt.toml")),
];

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            // A tagged method table (`method = …`) switching method is replaced whole:
            // the parent's method-specific keys would be invalid for the new method.
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) if o.get("method").is_some_and(|m| Some(m) != b.get("method")) => {
                *b = o;
            }
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// Parse a preset, resolving `extends` against built-ins and `library` (id → TOML source).
pub fn parse(src: &str, library: &BTreeMap<String, String>) -> Result<Preset, String> {
    let mut table: toml::Table = src.parse().map_err(|e| format!("{e}"))?;
    let mut seen = vec![];
    while let Some(parent) = table.get("extends").and_then(|v| v.as_str()).map(str::to_owned) {
        if seen.contains(&parent) || seen.len() > 8 {
            return Err(format!("preset inheritance cycle at '{parent}'"));
        }
        let psrc = library
            .get(&parent)
            .map(String::as_str)
            .or_else(|| BUILTIN.iter().find(|(id, _)| *id == parent).map(|(_, s)| *s))
            .ok_or_else(|| format!("unknown parent preset '{parent}'"))?;
        let mut base: toml::Table = psrc.parse().map_err(|e| format!("{parent}: {e}"))?;
        table.remove("extends");
        merge(&mut base, table);
        table = base;
        seen.push(parent);
    }
    let p: Preset = toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| e.to_string())?;
    p.validate()?;
    Ok(p)
}

/// Apply `key = value` overrides (dotted paths, values in TOML syntax; bare words are
/// strings), e.g. `preprocess.lowpass_hz=6`, `bleach.method=window`. The result is
/// re-validated, so an override can never produce an invalid preset. Switching a
/// method drops that table's old method-specific keys.
pub fn with_overrides(p: &Preset, sets: &[(String, String)]) -> Result<Preset, String> {
    let mut table: toml::Table = toml::Value::try_from(p).map_err(|e| e.to_string())?.as_table().cloned().unwrap_or_default();
    for (key, raw) in sets {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.iter().any(|s| s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            return Err(format!("'{key}' is not a valid parameter path"));
        }
        let value: toml::Value = format!("v = {raw}").parse::<toml::Table>().ok().and_then(|mut t| t.remove("v")).unwrap_or_else(|| toml::Value::String(raw.clone()));
        let mut over = toml::Table::new();
        let mut cur = &mut over;
        for part in &parts[..parts.len() - 1] {
            cur = cur.entry(part.to_string()).or_insert_with(|| toml::Value::Table(toml::Table::new())).as_table_mut().unwrap();
        }
        cur.insert(parts[parts.len() - 1].to_string(), value);
        merge(&mut table, over);
    }
    let out: Preset = toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| e.to_string())?;
    out.validate()?;
    Ok(out)
}

pub fn builtin(id: &str) -> Option<Preset> {
    BUILTIN.iter().find(|(i, _)| *i == id).map(|(_, s)| parse(s, &BTreeMap::new()).expect("built-in preset is valid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse() {
        for (id, _) in BUILTIN {
            assert_eq!(builtin(id).unwrap().id, *id);
        }
    }

    #[test]
    fn extends_and_overrides() {
        let src = r#"
            extends = "grab-ach3.0-tdt"
            id = "lab-feeding"
            name = "Lab feeding"
            [events]
            pre_s = 10.0
        "#;
        let p = parse(src, &BTreeMap::new()).unwrap();
        assert_eq!(p.id, "lab-feeding");
        assert_eq!(p.events.pre_s, 10.0);
        assert_eq!(p.sensor.name, builtin("grab-ach3.0-tdt").unwrap().sensor.name);
    }

    #[test]
    fn switching_method_replaces_its_table() {
        let src = "extends = \"grab-ach3.0-tdt\"\nid = \"w\"\nname = \"w\"\n[bleach]\nmethod = \"window\"\nstart_s = 30.0\nend_s = 230.0\n[motion]\nmethod = \"none\"\n";
        let p = parse(src, &BTreeMap::new()).unwrap();
        assert_eq!(p.bleach, BleachMethod::Window { start_s: 30.0, end_s: 230.0 });
        // Same method: keys still merge (override one, inherit the rest).
        let src = "extends = \"grab-ach3.0-tdt\"\nid = \"t\"\nname = \"t\"\n[motion]\nmethod = \"irls\"\ntuning = 3.0\n";
        assert_eq!(parse(src, &BTreeMap::new()).unwrap().motion, MotionMethod::Irls { tuning: 3.0 });
    }

    #[test]
    fn overrides_are_validated() {
        let p = builtin("grab-ach3.0-tdt").unwrap();
        let set = |k: &str, v: &str| with_overrides(&p, &[(k.into(), v.into())]);
        assert_eq!(set("preprocess.lowpass_hz", "6").unwrap().preprocess.lowpass_hz, Some(6.0));
        assert!(set("preprocess.lowpass_hz", "50").is_err(), "above post-decimation Nyquist");
        assert!(set("preprocess.lowpas_hz", "6").is_err(), "typo rejected");
        let w = with_overrides(&p, &[("bleach.method".into(), "window".into()), ("bleach.start_s".into(), "30".into()), ("bleach.end_s".into(), "200".into())]).unwrap();
        assert_eq!(w.bleach, BleachMethod::Window { start_s: 30.0, end_s: 200.0 });
        assert!(set("events.baseline_window", "[-1.0, 0.0]").is_err(), "baseline must span ≥ 2 s");
        assert!(set("a..b", "1").is_err());
    }

    #[test]
    fn rejects_typos_and_bad_windows() {
        let typo = "extends = \"grab-ach3.0-tdt\"\n[events]\npre_sec = 3.0\n";
        assert!(parse(typo, &BTreeMap::new()).unwrap_err().contains("pre_sec"));
        let bad = "extends = \"grab-ach3.0-tdt\"\n[events]\npre_s = 1.0\nbaseline_window = [-10.0, -2.0]\n";
        assert!(parse(bad, &BTreeMap::new()).unwrap_err().contains("baseline_window"));
    }
}
