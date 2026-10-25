//! Swift-facing API. Heavy data stays in Rust; Swift asks for render-ready
//! envelopes sized to the view, so a multi-hour recording draws in O(pixels).

use simplefp::core::analysis::Analysis;
use simplefp::core::events::Baseline;
use simplefp::core::preset::{self, Preset};
use simplefp::{EventSource, Recording};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Error)]
pub enum FpError {
    Failed { message: String },
}

impl std::error::Error for FpError {}

impl std::fmt::Display for FpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let FpError::Failed { message } = self;
        f.write_str(message)
    }
}

impl From<String> for FpError {
    fn from(message: String) -> Self {
        FpError::Failed { message }
    }
}

type R<T> = Result<T, FpError>;

#[derive(uniffi::Record)]
pub struct StreamInfo {
    pub name: String,
    pub fs: f64,
    pub channels: u32,
    pub samples: u64,
}

#[derive(uniffi::Record)]
pub struct EpocInfo {
    pub name: String,
    pub count: u32,
}

#[derive(uniffi::Record)]
pub struct BlockInfo {
    pub path: String,
    pub name: String,
    pub start_unix: f64,
    pub duration_s: f64,
    pub signal: String,
    pub control: Option<String>,
    pub streams: Vec<StreamInfo>,
    pub epocs: Vec<EpocInfo>,
}

#[derive(uniffi::Record)]
pub struct PresetInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub sensor: String,
    pub toml: String,
}

#[derive(uniffi::Record)]
pub struct VideoFile {
    pub camera: String,
    pub path: String,
}

#[derive(uniffi::Record)]
pub struct ClockInfo {
    pub times: Vec<u8>,
    pub fps: f64,
    pub jitter_ms: f64,
    pub gaps: u32,
    pub hash: String,
}

/// Set one parameter on a preset (dotted path, TOML value) and return the validated TOML.
#[uniffi::export]
pub fn preset_set(toml_src: String, key: String, value: String) -> R<String> {
    let p = parse_preset(&toml_src)?;
    Ok(preset::with_overrides(&p, &[(key, value)])?.to_toml())
}

/// A preset as JSON (for building controls from its current values).
#[uniffi::export]
pub fn preset_json(toml_src: String) -> R<String> {
    Ok(serde_json::to_string(&parse_preset(&toml_src)?).unwrap_or_default())
}

/// Default ethogram (feeding) as JSON.
#[uniffi::export]
pub fn default_ethogram() -> String {
    serde_json::to_string(&simplefp_video::Ethogram::feeding()).unwrap_or_default()
}

/// Event specs from an annotation set (bout onsets as `label`, offsets as `label_offset`;
/// unreviewed model proposals excluded).
#[uniffi::export]
pub fn annotation_events(json: String) -> R<Vec<EventSpec>> {
    let set: simplefp::AnnotationSet = serde_json::from_str(&json).map_err(|e| FpError::from(e.to_string()))?;
    Ok(simplefp_video::event_sets(&set.annotations).into_iter().map(|(label, times)| EventSpec::Manual { label, times }).collect())
}

#[derive(uniffi::Enum, Clone)]
pub enum EventSpec {
    Epoc { store: String },
    EpocOffset { store: String },
    Manual { label: String, times: Vec<f64> },
}

#[derive(uniffi::Record)]
pub struct Qc {
    pub fs: f64,
    pub decimation: u32,
    pub duration_s: f64,
    pub analyzed_from_s: f64,
    pub analyzed_to_s: f64,
    pub bleach_signal_pct: f64,
    pub bleach_control_pct: Option<f64>,
    pub bleach_fallback: bool,
    pub motion_r: Option<f64>,
    pub motion_slope: Option<f64>,
    pub motion_zero_weight_frac: Option<f64>,
    pub saturation_frac: f64,
    pub noise_dff_pct: f64,
    pub residual_drift_pct_per_10min: f64,
    pub warnings: Vec<String>,
}

#[derive(uniffi::Record)]
pub struct TrialMetrics {
    pub event_time: f64,
    pub mean: f64,
    pub auc: f64,
    pub auc_pos: f64,
    pub auc_neg: f64,
    pub peak: f64,
    pub peak_latency: f64,
    pub trough: f64,
    pub trough_latency: f64,
}

#[derive(uniffi::Record)]
pub struct EventSummary {
    pub label: String,
    pub event_times: Vec<f64>,
    pub dropped: Vec<f64>,
    pub t: Vec<f64>,
    /// Empty when fewer than two trials (no CI).
    pub mean: Vec<f64>,
    pub lo: Vec<f64>,
    pub hi: Vec<f64>,
    pub significant: Vec<bool>,
    /// Control channel (% of its baseline), same epochs; empty without a control.
    pub control_mean: Vec<f64>,
    pub control_lo: Vec<f64>,
    pub control_hi: Vec<f64>,
    pub control_responds: bool,
    pub metrics: Vec<TrialMetrics>,
    /// Trials × samples, row-major little-endian f32.
    pub heatmap: Vec<u8>,
    pub unit: String,
}

#[derive(uniffi::Record)]
pub struct Results {
    pub qc: Qc,
    pub events: Vec<EventSummary>,
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum TraceKind {
    Dff,
    Z,
    Signal,
    SignalBaseline,
    /// Bleaching baseline plus the scaled control: what the signal "should" be without ACh.
    FittedControl,
    Control,
    ControlBaseline,
}

#[uniffi::export]
pub fn presets() -> Vec<PresetInfo> {
    preset::BUILTIN
        .iter()
        .map(|(id, src)| {
            let p = preset::builtin(id).expect("valid builtin");
            PresetInfo { id: id.to_string(), name: p.name, description: p.description, sensor: p.sensor.name, toml: src.to_string() }
        })
        .collect()
}

#[uniffi::export]
pub fn engine_version() -> String {
    simplefp::VERSION.to_string()
}

/// Write a synthetic block with known ground truth (for first-run demo).
#[uniffi::export]
pub fn write_demo(dir: String, seed: u64) -> R<u32> {
    Ok(simplefp::write_demo_block(Path::new(&dir), seed)?.len() as u32)
}

/// Parse a manual events CSV (`event,time_s` header) into event specs.
#[uniffi::export]
pub fn read_events_csv(path: String) -> R<Vec<EventSpec>> {
    Ok(simplefp::read_events_csv(Path::new(&path))?
        .into_iter()
        .filter_map(|e| match e {
            EventSource::Manual { label, times } => Some(EventSpec::Manual { label, times }),
            _ => None,
        })
        .collect())
}

#[uniffi::export]
pub fn is_tdt_block(dir: String) -> bool {
    simplefp::tdt::is_block(Path::new(&dir))
}

fn parse_preset(src: &str) -> R<Preset> {
    Ok(preset::parse(src, &BTreeMap::new())?)
}

#[derive(uniffi::Object)]
pub struct Session {
    rec: Recording,
    state: Mutex<(Preset, Option<Analysis>)>,
}

#[uniffi::export]
impl Session {
    /// Open a TDT block directory using a preset given as TOML source.
    #[uniffi::constructor]
    pub fn open(path: String, preset_toml: String) -> R<Self> {
        let p = parse_preset(&preset_toml)?;
        let rec = simplefp::load(Path::new(&path), &p)?;
        Ok(Session { rec, state: Mutex::new((p, None)) })
    }

    pub fn info(&self) -> BlockInfo {
        let b = &self.rec.block;
        let s = &b.streams[&self.rec.signal];
        BlockInfo {
            path: b.path.display().to_string(),
            name: b.path.file_name().map_or(String::new(), |n| n.to_string_lossy().into()),
            start_unix: b.start_unix,
            duration_s: b.duration.unwrap_or(s.channels[0].len() as f64 / s.fs),
            signal: self.rec.signal.clone(),
            control: self.rec.control.clone(),
            streams: b.streams.values().map(|s| StreamInfo { name: s.name.clone(), fs: s.fs, channels: s.channels.len() as u32, samples: s.channels[0].len() as u64 }).collect(),
            epocs: b.epocs.values().map(|e| EpocInfo { name: e.name.clone(), count: e.onset.len() as u32 }).collect(),
        }
    }

    pub fn preset_toml(&self) -> String {
        self.state.lock().unwrap().0.to_toml()
    }

    /// Run (or re-run with a new preset) and return everything the UI shows.
    pub fn analyze(&self, preset_toml: Option<String>, events: Vec<EventSpec>) -> R<Results> {
        let mut st = self.state.lock().unwrap();
        if let Some(src) = preset_toml {
            st.0 = parse_preset(&src)?;
        }
        let sources: Vec<EventSource> = events
            .into_iter()
            .map(|e| match e {
                EventSpec::Epoc { store } => EventSource::Epoc(store),
                EventSpec::EpocOffset { store } => EventSource::EpocOffset(store),
                EventSpec::Manual { label, times } => EventSource::Manual { label, times },
            })
            .collect();
        let a = self.rec.analyze(&st.0, &sources)?;
        let results = results_of(&a, &st.0);
        st.1 = Some(a);
        Ok(results)
    }

    /// Min/max envelope of a trace over `[t0, t1]` in `buckets` columns, as
    /// little-endian f32 triplets `(t, lo, hi)`. When zoomed past one sample per
    /// bucket, each sample is returned once with `lo == hi`. NaN regions are skipped.
    pub fn envelope(&self, kind: TraceKind, t0: f64, t1: f64, buckets: u32) -> Vec<u8> {
        let st = self.state.lock().unwrap();
        let Some(a) = &st.1 else { return vec![] };
        let o = &a.output;
        let owned: Vec<f64>;
        let y: &[f64] = match kind {
            TraceKind::Dff => &o.dff,
            TraceKind::Z => &o.z,
            TraceKind::Signal => &o.signal,
            TraceKind::SignalBaseline => &o.signal_baseline,
            TraceKind::FittedControl => match &o.motion_fit {
                Some(m) => {
                    owned = o.signal_baseline.iter().zip(m).map(|(b, m)| b + m).collect();
                    &owned
                }
                None => return vec![],
            },
            TraceKind::Control => match &o.control {
                Some(c) => c,
                None => return vec![],
            },
            TraceKind::ControlBaseline => match &o.control_baseline {
                Some(c) => c,
                None => return vec![],
            },
        };
        envelope(y, o.t0, o.fs, t0, t1, buckets.max(1) as usize)
    }

    /// Camera videos stored in the block (`…Cam1.mp4`).
    pub fn videos(&self) -> Vec<VideoFile> {
        simplefp::block_videos(&self.rec.block.path).into_iter().map(|(camera, p)| VideoFile { camera, path: p.display().to_string() }).collect()
    }

    /// Frame clock of a camera epoc: per-frame block times as little-endian f64 bytes.
    pub fn frame_clock(&self, camera: String) -> R<ClockInfo> {
        let c = self.rec.frame_clock(&camera)?;
        Ok(ClockInfo { times: c.frame_times().iter().flat_map(|t| t.to_le_bytes()).collect(), fps: c.fps, jitter_ms: c.jitter_ms, gaps: c.gaps as u32, hash: simplefp::clock_hash(&camera, &c) })
    }

    /// Reconcile the clock with the video's sample-table frame count. Returns a warning or fails.
    pub fn check_video_frames(&self, camera: String, video_frames: u64) -> R<Option<String>> {
        let c = self.rec.frame_clock(&camera)?;
        Ok(c.check_frame_count(video_frames as usize, (c.fps.round() as usize).max(1))?)
    }

    pub fn load_annotations(&self, scorer: String) -> R<Option<String>> {
        Ok(simplefp::load_annotations(&self.rec.block.path, &scorer)?.map(|s| serde_json::to_string(&s).unwrap_or_default()))
    }

    /// Scorers with saved annotation files.
    pub fn scorers(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(simplefp::annotations_dir(&self.rec.block.path))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(str::to_owned).filter(|_| e.path().extension().is_some_and(|x| x == "json")))
            .collect();
        v.sort();
        v
    }

    pub fn save_annotations(&self, json: String) -> R<String> {
        let set: simplefp::AnnotationSet = serde_json::from_str(&json).map_err(|e| FpError::from(e.to_string()))?;
        Ok(simplefp::save_annotations(&self.rec.block.path, &set)?.display().to_string())
    }

    /// Agreement of two scorers per behavior on a 1/fps frame grid (frame κ, PABAK, bout/event F1) as JSON.
    pub fn agreement(&self, scorer_a: String, scorer_b: String, fps: f64) -> R<String> {
        use simplefp_video as v;
        let load = |s: &str| simplefp::load_annotations(&self.rec.block.path, s)?.ok_or_else(|| format!("no annotations by '{s}'"));
        let (a, b) = (load(&scorer_a)?, load(&scorer_b)?);
        if a.clock_hash != b.clock_hash {
            return Err(FpError::from("the two scorers used different frame clocks".to_string()));
        }
        let dur = self.info().duration_s;
        let n = (dur * fps).ceil() as usize;
        let mut out = vec![];
        for beh in &a.ethogram.behaviors {
            let pick = |set: &simplefp::AnnotationSet| set.annotations.iter().filter(|x| x.label == beh.label && x.usable()).cloned().collect::<Vec<_>>();
            let (pa, pb) = (pick(&a), pick(&b));
            let row = match beh.kind {
                v::BehaviorKind::State => {
                    let fa = v::frame_agreement(&v::raster(&pa, &beh.label, 0.0, 1.0 / fps, n), &v::raster(&pb, &beh.label, 0.0, 1.0 / fps, n));
                    let bouts = |x: &[v::Annotation]| x.iter().filter_map(|y| y.end_s.map(|e| (y.start_s, e))).collect::<Vec<_>>();
                    serde_json::json!({"label": beh.label, "kind": "state", "frame": fa, "bout": v::bout_f1(&bouts(&pa), &bouts(&pb), 0.5)})
                }
                v::BehaviorKind::Point => {
                    let t = |x: &[v::Annotation]| x.iter().map(|y| y.start_s).collect::<Vec<_>>();
                    serde_json::json!({"label": beh.label, "kind": "point", "event": v::event_f1(&t(&pa), &t(&pb), 0.5)})
                }
            };
            out.push(row);
        }
        Ok(serde_json::to_string(&out).unwrap_or_default())
    }

    /// Export CSVs + run.json for the current analysis.
    pub fn export(&self, dir: String) -> R<u32> {
        let st = self.state.lock().unwrap();
        let a = st.1.as_ref().ok_or_else(|| FpError::from("run analyze() first".to_string()))?;
        Ok(simplefp::export(&self.rec, &st.0, a, Path::new(&dir))?.len() as u32)
    }
}

#[derive(uniffi::Record)]
pub struct CurveBand {
    pub event: String,
    /// "group · condition" for group curves, contrast label for contrasts.
    pub label: String,
    pub group: String,
    pub condition: String,
    pub n: Vec<u32>,
    pub mean: Vec<f64>,
    pub lo: Vec<f64>,
    pub hi: Vec<f64>,
    pub significant: Vec<bool>,
}

#[derive(uniffi::Record)]
pub struct AnimalPoint {
    pub event: String,
    pub animal: String,
    pub group: String,
    pub condition: String,
    pub n_trials: u32,
    pub mean: f64,
    pub auc: f64,
    pub peak: f64,
    pub trough: f64,
}

#[derive(uniffi::Record)]
pub struct MetricTestRow {
    pub event: String,
    pub contrast: String,
    pub metric: String,
    pub n_a: u32,
    pub n_b: u32,
    pub estimate: f64,
    pub lo: f64,
    pub hi: f64,
    pub p: f64,
    pub exact: bool,
    pub q: f64,
}

#[derive(uniffi::Record)]
pub struct MetricSummaryRow {
    pub event: String,
    pub group: String,
    pub condition: String,
    pub metric: String,
    pub n_animals: u32,
    pub mean: f64,
    pub lo: f64,
    pub hi: f64,
}

#[derive(uniffi::Record)]
pub struct SessionRow {
    pub block: String,
    pub animal: String,
    pub group: String,
    pub condition: String,
    pub status: String,
}

#[derive(uniffi::Record)]
pub struct CohortResults {
    pub name: String,
    pub description: String,
    pub preset_name: String,
    pub unit: String,
    pub t: Vec<f64>,
    pub groups: Vec<CurveBand>,
    pub contrasts: Vec<CurveBand>,
    pub animals: Vec<AnimalPoint>,
    pub summaries: Vec<MetricSummaryRow>,
    pub tests: Vec<MetricTestRow>,
    pub sessions: Vec<SessionRow>,
    pub warnings: Vec<String>,
}

#[derive(uniffi::Object)]
pub struct Cohort {
    run: simplefp::cohort::CohortRun,
}

#[uniffi::export]
impl Cohort {
    /// Load and analyze a cohort manifest (`cohort.toml`).
    #[uniffi::constructor]
    pub fn open(manifest_path: String) -> R<Self> {
        Ok(Cohort { run: simplefp::cohort::run(Path::new(&manifest_path))? })
    }

    pub fn results(&self) -> CohortResults {
        let r = &self.run;
        let g = &r.group;
        let unit = if r.preset.events.baseline == Baseline::None { "ΔF/F (%)" } else { "ΔF/F (%), baseline-subtracted" };
        CohortResults {
            name: r.manifest.name.clone(),
            description: r.manifest.description.clone(),
            preset_name: r.preset.name.clone(),
            unit: unit.into(),
            t: g.t.clone(),
            groups: g
                .groups
                .iter()
                .map(|c| {
                    let (lo, hi, significant) = c.ci.as_ref().map_or((vec![], vec![], vec![]), |ci| (ci.lo.clone(), ci.hi.clone(), ci.significant.clone()));
                    CurveBand { event: c.event.clone(), label: format!("{} · {}", c.group, c.condition), group: c.group.clone(), condition: c.condition.clone(), n: vec![c.n_animals as u32], mean: c.mean.clone(), lo, hi, significant }
                })
                .collect(),
            contrasts: g
                .contrasts
                .iter()
                .map(|c| CurveBand { event: c.event.clone(), label: c.label.clone(), group: String::new(), condition: String::new(), n: c.n.iter().map(|&v| v as u32).collect(), mean: c.diff.clone(), lo: c.lo.clone(), hi: c.hi.clone(), significant: c.significant.clone() })
                .collect(),
            animals: g
                .animals
                .iter()
                .map(|a| AnimalPoint { event: a.event.clone(), animal: a.animal.clone(), group: a.group.clone(), condition: a.condition.clone(), n_trials: a.n_trials as u32, mean: a.metrics.mean, auc: a.metrics.auc, peak: a.metrics.peak, trough: a.metrics.trough })
                .collect(),
            summaries: g
                .metric_summaries
                .iter()
                .map(|m| MetricSummaryRow { event: m.event.clone(), group: m.group.clone(), condition: m.condition.clone(), metric: m.metric.clone(), n_animals: m.n_animals as u32, mean: m.mean, lo: m.lo, hi: m.hi })
                .collect(),
            tests: g
                .metric_tests
                .iter()
                .map(|t| MetricTestRow { event: t.event.clone(), contrast: t.contrast.clone(), metric: t.metric.clone(), n_a: t.n[0] as u32, n_b: t.n[1] as u32, estimate: t.estimate, lo: t.lo, hi: t.hi, p: t.p, exact: t.exact, q: t.q })
                .collect(),
            sessions: r.reports.iter().map(|s| SessionRow { block: s.block.clone(), animal: s.animal.clone(), group: s.group.clone(), condition: s.condition.clone(), status: s.status.clone() }).collect(),
            warnings: g.warnings.clone(),
        }
    }

    pub fn export(&self, dir: String) -> R<u32> {
        Ok(simplefp::cohort::export_cohort(&self.run, Path::new(&dir))?.len() as u32)
    }
}

/// Write a synthetic two-group cohort (16 blocks + cohort.toml); returns the manifest path.
#[uniffi::export]
pub fn write_demo_cohort(dir: String) -> R<String> {
    Ok(simplefp::write_demo_cohort(Path::new(&dir))?.display().to_string())
}

/// Write a cohort manifest from sessions assigned in the UI. Returns the TOML written.
#[uniffi::export]
pub fn write_cohort_manifest(path: String, name: String, preset_id: String, epocs: Vec<String>, sessions: Vec<SessionRow>) -> R<String> {
    let manifest = simplefp::cohort::Manifest {
        schema: 1,
        name,
        description: String::new(),
        preset: preset_id,
        events: epocs.into_iter().map(|epoc| simplefp::cohort::EventRef::Epoc { epoc }).collect(),
        sessions: sessions
            .into_iter()
            .map(|s| simplefp::cohort::SessionEntry { block: s.block, animal: s.animal, group: s.group, condition: s.condition, events: None, exclude: false, exclude_reason: None })
            .collect(),
        contrasts: vec![],
        window_contrasts: vec![],
    };
    let src = toml::to_string_pretty(&manifest).map_err(|e| FpError::from(e.to_string()))?;
    simplefp::cohort::Manifest::parse(&src)?;
    std::fs::write(&path, &src).map_err(|e| FpError::from(format!("{path}: {e}")))?;
    Ok(src)
}

// MARK: lab store (JSON at the boundary: rows are objects keyed by column name)

#[derive(uniffi::Object)]
pub struct LabStore {
    lab: Mutex<simplefp_lab::Lab>,
}

fn obj(json: &str) -> R<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str(json) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        _ => Err(FpError::from("expected a JSON object".to_string())),
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

#[uniffi::export]
impl LabStore {
    /// Open or create a lab store. `actor` (user initials) is written to every audit row.
    #[uniffi::constructor]
    pub fn open(path: String, actor: String) -> R<Self> {
        Ok(LabStore { lab: Mutex::new(simplefp_lab::Lab::open(Path::new(&path), &actor)?) })
    }

    pub fn list(&self, table: String, filter_column: Option<String>, filter_value: Option<String>) -> R<String> {
        let lab = self.lab.lock().unwrap();
        let filter = filter_column.as_deref().zip(filter_value.as_deref());
        Ok(to_json(&lab.list(&table, filter)?))
    }

    pub fn insert(&self, table: String, json: String) -> R<String> {
        Ok(self.lab.lock().unwrap().insert(&table, obj(&json)?)?)
    }

    pub fn update(&self, table: String, id: String, json: String, reason: Option<String>) -> R<()> {
        Ok(self.lab.lock().unwrap().update(&table, &id, obj(&json)?, reason.as_deref())?)
    }

    /// Returns warnings (capacity, sex mixing); errors block the move.
    pub fn move_animal(&self, animal_id: String, cage_id: String, reason: Option<String>) -> R<Vec<String>> {
        Ok(self.lab.lock().unwrap().move_animal(&animal_id, &cage_id, reason.as_deref())?)
    }

    /// `pups_json`: `[{"label","sex","ear_tag"?,"cage_id"}]`. Returns the new animal ids.
    pub fn wean(&self, litter_id: String, weaned_on: String, pups_json: String) -> R<Vec<String>> {
        let pups: Vec<simplefp_lab::Pup> = serde_json::from_str(&pups_json).map_err(|e| FpError::from(e.to_string()))?;
        Ok(self.lab.lock().unwrap().wean(&litter_id, &weaned_on, &pups)?)
    }

    /// `json`: `{"procedure":{…},"frame":{…},"injections":[…],"implants":[…]}`.
    pub fn record_surgery(&self, json: String) -> R<String> {
        let s: simplefp_lab::Surgery = serde_json::from_str(&json).map_err(|e| FpError::from(e.to_string()))?;
        Ok(self.lab.lock().unwrap().record_surgery(s)?)
    }

    pub fn exclude(&self, scope: String, target_id: String, cohort_id: Option<String>, reason: String, detail: Option<String>) -> R<String> {
        Ok(self.lab.lock().unwrap().exclude(&scope, &target_id, cohort_id.as_deref(), &reason, detail.as_deref())?)
    }

    pub fn add_cohort_member(&self, cohort_id: String, animal_id: String, group: String, allocation: String, blind_code: Option<String>) -> R<()> {
        Ok(self.lab.lock().unwrap().add_cohort_member(&cohort_id, &animal_id, &group, &allocation, blind_code.as_deref())?)
    }

    pub fn add_cohort_session(&self, cohort_id: String, session_id: String, condition: String) -> R<()> {
        Ok(self.lab.lock().unwrap().add_cohort_session(&cohort_id, &session_id, &condition)?)
    }

    pub fn freeze_cohort(&self, cohort_id: String) -> R<String> {
        Ok(self.lab.lock().unwrap().freeze_cohort(&cohort_id)?)
    }

    pub fn unblind(&self, cohort_id: String, reason: String) -> R<()> {
        Ok(self.lab.lock().unwrap().unblind(&cohort_id, &reason)?)
    }

    /// Write the cohort as an analysis manifest (`cohort.toml`) and return its path.
    pub fn write_cohort_manifest(&self, cohort_id: String, data_root: String, path: String) -> R<String> {
        let src = self.lab.lock().unwrap().cohort_manifest(&cohort_id, Path::new(&data_root))?;
        simplefp::cohort::Manifest::parse(&src)?;
        std::fs::write(&path, src).map_err(|e| FpError::from(format!("{path}: {e}")))?;
        Ok(path)
    }

    pub fn tasks(&self, today: String, wean_age_days: i64) -> R<String> {
        Ok(to_json(&self.lab.lock().unwrap().tasks(&today, wean_age_days)?))
    }

    pub fn timeline(&self, animal_id: String) -> R<String> {
        Ok(to_json(&self.lab.lock().unwrap().timeline(&animal_id)?))
    }

    /// Register a TDT block as a session of `animal_id` (identity from the TSQ index, channels from the preset).
    pub fn intake_session(&self, block_path: String, data_root: Option<String>, preset_id: String, animal_id: String, paradigm: String, condition: Option<String>) -> R<String> {
        let preset = simplefp::load_preset(&preset_id, Path::new("."))?;
        let mut f = simplefp::session_fields(Path::new(&block_path), data_root.as_deref().map(Path::new), &preset)?;
        f.insert("animal_id".into(), animal_id.into());
        f.insert("paradigm".into(), paradigm.into());
        f.insert("condition".into(), condition.map_or(serde_json::Value::Null, Into::into));
        Ok(self.lab.lock().unwrap().insert("session", f)?)
    }

    /// Seed a realistic demo colony whose animals own the demo-cohort recordings in `dir`.
    pub fn seed_demo(&self, dir: String, today: String) -> R<()> {
        use serde_json::json;
        let manifest = simplefp::write_demo_cohort(Path::new(&dir))?;
        let m = simplefp::cohort::Manifest::parse(&std::fs::read_to_string(&manifest).map_err(|e| FpError::from(e.to_string()))?)?;
        let preset = simplefp::load_preset(&m.preset, Path::new("."))?;
        let mut lab = self.lab.lock().unwrap();
        let o = |v: serde_json::Value| v.as_object().cloned().unwrap_or_default();
        let t = simplefp_lab::days(&today).ok_or_else(|| FpError::from("today must be YYYY-MM-DD".to_string()))?;
        let d = |off: i64| simplefp_lab::date(t + off);
        let proto = lab.insert("protocol", o(json!({"number": "DEMO-2026-01", "title": "Cholinergic signalling in feeding (demo)", "approved_from": d(-300), "expires_on": d(20), "species": "Mus musculus", "approved_count": 120})))?;
        let chat = lab.insert("line", o(json!({"name": "ChAT-Cre", "official_symbol": "Chat<tm2(cre)Lowl>", "background": "C57BL/6J", "source": "JAX 006410"})))?;
        let wt = lab.insert("line", o(json!({"name": "C57BL/6J", "background": "C57BL/6J", "source": "JAX 000664"})))?;
        let virus = lab.insert("virus_lot", o(json!({"construct": "AAV9-hSyn-GRAB-ACh3.0", "serotype": "AAV9", "titer_vg_per_ml": 2.1e13, "source": "WZ Biosciences", "lot": "DEMO-17"})))?;
        let cage = |lab: &mut simplefp_lab::Lab, label: &str, kind: &str, pos: i64| lab.insert("cage", o(json!({"label": label, "kind": kind, "room": "B214", "rack": "R1", "position": pos, "protocol_id": proto, "opened_on": d(-120)})));
        let mut cages = std::collections::BTreeMap::new();
        for (i, (label, kind)) in [("E-01", "experimental"), ("E-02", "experimental"), ("E-03", "experimental"), ("E-04", "experimental"), ("B-01", "breeding"), ("H-01", "holding"), ("H-02", "holding")].iter().enumerate() {
            cages.insert(*label, cage(&mut lab, label, kind, i as i64 + 1)?);
        }
        let cohort = lab.insert("cohort", o(json!({"name": "Demo · refeed", "question": "Does ChAT-Cre cholinergic tone at food contact exceed WT, and does fasting raise it?", "preset": m.preset, "events": "[\"Evt_\"]", "blinding": "none", "created_by": "demo"})))?;
        let mut animals = std::collections::BTreeMap::new();
        for s in &m.sessions {
            if !animals.contains_key(&s.animal) {
                let line = if s.group == "ChAT" { &chat } else { &wt };
                let cage_label = format!("E-0{}", s.animal[1..].parse::<usize>().unwrap_or(1));
                let a = lab.insert("animal", o(json!({"label": s.animal, "sex": "M", "line_id": line, "dob": d(-110), "source": "vendor", "vendor": "JAX", "arrived_on": d(-90), "protocol_id": proto, "owner": "demo"})))?;
                lab.move_animal(&a, &cages[cage_label.as_str()], Some("arrival"))?;
                lab.record_surgery(serde_json::from_value(json!({
                    "procedure": {"animal_id": a, "performed_on": d(-45), "operators": "demo", "protocol_id": proto, "anesthesia": "isoflurane 1.5–2 %", "analgesia": "meloxicam 5 mg/kg", "weight_pre_g": 26.5, "outcome": "recovered"},
                    "frame": {"reference": "bregma", "skull_flat": 1, "atlas": "Paxinos & Franklin 2019"},
                    "injections": [{"virus_lot_id": virus, "target_region": "NAc core", "hemisphere": "L", "ap_mm": 1.3, "ml_mm": -1.0, "dv_mm": 4.2, "dv_reference": "skull", "volume_nl": 300.0, "rate_nl_per_min": 50.0}],
                    "implants": [{"kind": "optic_fiber", "target_region": "NAc core", "hemisphere": "L", "ap_mm": 1.3, "ml_mm": -1.0, "dv_mm": 4.1, "dv_reference": "skull", "core_um": 400.0, "na": 0.5}]
                })).map_err(|e| FpError::from(e.to_string()))?)?;
                lab.add_cohort_member(&cohort, &a, &s.group, "genotype", None)?;
                animals.insert(s.animal.clone(), a);
            }
            let mut f = simplefp::session_fields(&Path::new(&dir).join(&s.block), Some(Path::new(&dir)), &preset)?;
            f.insert("animal_id".into(), animals[&s.animal].clone().into());
            f.insert("paradigm".into(), "refeed".into());
            f.insert("condition".into(), s.condition.clone().into());
            let sid = lab.insert("session", f)?;
            lab.add_cohort_session(&cohort, &sid, &s.condition)?;
        }
        // A breeding pair with a litter due to wean, and a recent surgery needing post-op checks.
        let sire = lab.insert("animal", o(json!({"label": "S-01", "sex": "M", "line_id": chat, "dob": d(-150), "source": "vendor", "vendor": "JAX", "arrived_on": d(-130), "protocol_id": proto})))?;
        let dam = lab.insert("animal", o(json!({"label": "D-01", "sex": "F", "line_id": wt, "dob": d(-140), "source": "vendor", "vendor": "JAX", "arrived_on": d(-130), "protocol_id": proto})))?;
        lab.move_animal(&sire, &cages["B-01"], Some("mating"))?;
        lab.move_animal(&dam, &cages["B-01"], Some("mating"))?;
        let pair = lab.insert("breeding_pair", o(json!({"cage_id": cages["B-01"], "kind": "pair", "male_id": sire, "set_up_on": d(-60), "goal": "ChAT-Cre het"})))?;
        lab.insert("litter", o(json!({"breeding_pair_id": pair, "dam_id": dam, "born_on": d(-20), "born_count": 7})))?;
        let fresh = lab.insert("animal", o(json!({"label": "A-09", "sex": "F", "line_id": chat, "dob": d(-80), "source": "vendor", "vendor": "JAX", "arrived_on": d(-60), "protocol_id": proto})))?;
        lab.move_animal(&fresh, &cages["H-01"], Some("arrival"))?;
        lab.record_surgery(serde_json::from_value(json!({
            "procedure": {"animal_id": fresh, "performed_on": d(-1), "operators": "demo", "protocol_id": proto, "weight_pre_g": 22.1, "outcome": "recovered"},
            "frame": {"reference": "bregma", "skull_flat": 1},
            "injections": [{"virus_lot_id": virus, "target_region": "BLA", "hemisphere": "R", "ap_mm": -1.4, "ml_mm": 3.3, "dv_mm": 4.9, "dv_reference": "skull", "volume_nl": 250.0}],
            "implants": [{"kind": "optic_fiber", "target_region": "BLA", "hemisphere": "R", "ap_mm": -1.4, "ml_mm": 3.3, "dv_mm": 4.8, "dv_reference": "skull", "core_um": 400.0, "na": 0.5}]
        })).map_err(|e| FpError::from(e.to_string()))?)?;
        lab.insert("note", o(json!({"author": "demo", "body_md": "Fiber patency good after surgery; mild swelling at incision.", "entity": "animal", "entity_id": fresh})))?;
        Ok(())
    }

    pub fn verify_audit(&self) -> R<u32> {
        Ok(self.lab.lock().unwrap().verify_audit()? as u32)
    }
}

// MARK: video tracking (frames decoded by AVFoundation, luma only)

#[derive(uniffi::Record)]
pub struct TrackSummary {
    /// Block-clock time per frame, then per-frame x, y, speed (little-endian f64 bytes each).
    pub t: Vec<u8>,
    pub x: Vec<u8>,
    pub y: Vec<u8>,
    pub speed: Vec<u8>,
    pub immobile_frac: f64,
    pub lost_frac: f64,
    pub threshold: f64,
    /// Immobility bouts as flattened (start, end) block times.
    pub bouts: Vec<f64>,
    /// ROI used, in tracking-frame pixels [x0, y0, x1, y1].
    pub roi: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

#[derive(uniffi::Object)]
pub struct VideoTracker {
    w: usize,
    h: usize,
    dark: bool,
    state: Mutex<TrackState>,
}

#[derive(Default)]
struct TrackState {
    samples: Vec<Vec<u8>>,
    tracker: Option<simplefp::video::track::Tracker>,
    dets: Vec<simplefp::video::track::Detection>,
    roi: Option<[usize; 4]>,
    /// Keypoints in tracking-frame pixels, if a pose model has run.
    pose: Option<simplefp::video::Pose>,
}

fn f64_bytes(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[uniffi::export]
impl VideoTracker {
    #[uniffi::constructor]
    pub fn new(width: u32, height: u32, dark_animal: bool) -> R<Self> {
        if !(16..=2048).contains(&width) || !(16..=2048).contains(&height) {
            return Err(FpError::from("tracking frame size out of range".to_string()));
        }
        Ok(VideoTracker { w: width as usize, h: height as usize, dark: dark_animal, state: Mutex::new(TrackState::default()) })
    }

    /// Pass 1: a frame sampled for the median background.
    pub fn add_sample(&self, frame: Vec<u8>) -> R<()> {
        if frame.len() != self.w * self.h {
            return Err(FpError::from("frame size mismatch".to_string()));
        }
        self.state.lock().unwrap().samples.push(frame);
        Ok(())
    }

    /// Build the background and start tracking. `roi` = [x0, y0, x1, y1] fractions of the frame, or empty for auto-detect.
    /// Returns the ROI in pixels (empty if none was found and the whole frame is used).
    pub fn begin(&self, roi: Vec<f64>) -> R<Vec<u32>> {
        use simplefp::video::track;
        let mut st = self.state.lock().unwrap();
        let bg = track::median_background(&st.samples)?;
        let roi_px = match roi.as_slice() {
            [x0, y0, x1, y1] if (0.0..1.0).contains(x0) && x0 < x1 && *x1 <= 1.0 && (0.0..1.0).contains(y0) && y0 < y1 && *y1 <= 1.0 => {
                Some([(x0 * self.w as f64) as usize, (y0 * self.h as f64) as usize, (x1 * self.w as f64).ceil() as usize, (y1 * self.h as f64).ceil() as usize])
            }
            [] => track::auto_arena(&bg, self.w, self.h, 2),
            _ => return Err(FpError::from("ROI must be four fractions x0 < x1, y0 < y1 within 0…1".to_string())),
        };
        let p = track::TrackParams { dark_animal: self.dark, roi: roi_px, ..Default::default() };
        st.tracker = Some(track::Tracker::new(bg, self.w, self.h, p)?);
        st.samples.clear();
        st.dets.clear();
        st.roi = roi_px;
        Ok(roi_px.map_or(vec![], |r| r.iter().map(|&v| v as u32).collect()))
    }

    /// Pass 2: track one frame (in order).
    pub fn step(&self, frame: Vec<u8>) -> R<()> {
        let mut st = self.state.lock().unwrap();
        let d = st.tracker.as_mut().ok_or_else(|| FpError::from("call begin() first".to_string()))?.step(&frame)?;
        st.dets.push(d);
        Ok(())
    }

    /// Attach pose keypoints (source-video pixels, flattened x, y, confidence per frame) so
    /// detectors trained from now on also use pose features.
    pub fn set_pose(&self, bodyparts: Vec<String>, xyc: Vec<f64>, source_width: u32, source_height: u32) -> R<()> {
        let k = bodyparts.len();
        if k == 0 || !xyc.len().is_multiple_of(3 * k) || source_width == 0 || source_height == 0 {
            return Err(FpError::from("keypoints do not match the bodyparts".to_string()));
        }
        let (sx, sy) = (self.w as f64 / source_width as f64, self.h as f64 / source_height as f64);
        let n = xyc.len() / (3 * k);
        let col = |b: usize, o: usize, s: f64| (0..n).map(|i| xyc[(i * k + b) * 3 + o] * s).collect::<Vec<f64>>();
        self.state.lock().unwrap().pose = Some(simplefp::video::Pose { x: (0..k).map(|b| col(b, 0, sx)).collect(), y: (0..k).map(|b| col(b, 1, sy)).collect(), likelihood: (0..k).map(|b| col(b, 2, 1.0)).collect(), bodyparts });
        Ok(())
    }

    /// Features on the block clock of `camera` in `session`.
    pub fn finish(&self, session: &Session, camera: String, min_immobile_s: f64) -> R<TrackSummary> {
        let st = self.state.lock().unwrap();
        let clock = session.rec.frame_clock(&camera)?;
        let f = simplefp::video::track::features(&clock, &st.dets, 1.0, None, min_immobile_s)?;
        Ok(TrackSummary {
            t: f64_bytes(&f.t),
            x: f64_bytes(&f.x),
            y: f64_bytes(&f.y),
            speed: f64_bytes(&f.speed),
            immobile_frac: f.immobile.iter().filter(|&&b| b).count() as f64 / f.t.len().max(1) as f64,
            lost_frac: f.lost_frac,
            threshold: f.immobility_threshold,
            bouts: f.bouts.iter().flat_map(|&(a, b)| [a, b]).collect(),
            roi: st.roi.map_or(vec![], |r| r.iter().map(|&v| v as u32).collect()),
            width: self.w as u32,
            height: self.h as u32,
        })
    }
}

impl VideoTracker {
    fn frame_features(&self, session: &Session, camera: &str) -> R<(simplefp::video::classify::Features, Vec<f64>)> {
        let st = self.state.lock().unwrap();
        let clock = session.rec.frame_clock(camera)?;
        let n = clock.frames().map_or(st.dets.len(), |f| f.min(st.dets.len()));
        let t: Vec<f64> = (0..n).map(|k| clock.time(k).unwrap_or(f64::NAN)).collect();
        let roi = st.roi.unwrap_or([0, 0, self.w, self.h]);
        let diag = ((roi[2] - roi[0]) as f64).hypot((roi[3] - roi[1]) as f64);
        let extra = st.pose.as_ref().map(|p| simplefp::video::pose::pose_features(p, clock.fps, diag, 0.2)).unwrap_or_default();
        let extra: Vec<(String, Vec<f64>)> = extra.into_iter().map(|(k, mut v)| { v.resize(n, f64::NAN); (k, v) }).collect();
        let f = simplefp::video::classify::frame_features(&st.dets[..n], clock.fps, roi, &extra)?;
        Ok((f, t))
    }

}

/// Decode one frame's heatmaps (little-endian f32 bytes, `[K,H,W]` or `[H,W,K]`) to
/// keypoints in frame pixels via the crop `[x0, y0, x1, y1]`. Returns flattened (x, y, confidence).
#[uniffi::export]
pub fn decode_pose_heatmaps(data: Vec<u8>, k: u32, h: u32, w: u32, channels_last: bool, crop: Vec<f64>) -> R<Vec<f64>> {
    let f: Vec<f32> = data.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    let crop: [f64; 4] = crop.try_into().map_err(|_| FpError::from("crop must have 4 values".to_string()))?;
    let kp = simplefp::video::pose::decode_heatmaps(&f, k as usize, h as usize, w as usize, channels_last)?;
    Ok(kp.into_iter().flat_map(|(x, y, c)| {
        let (fx, fy) = simplefp::video::pose::to_frame((x, y), crop, w as usize, h as usize);
        [fx, fy, c]
    }).collect())
}

/// DeepLabCut-style CSV for keypoints: `xyc` = per frame, per bodypart (x, y, confidence).
#[uniffi::export]
pub fn pose_csv(bodyparts: Vec<String>, xyc: Vec<f64>, scorer: String) -> R<String> {
    let k = bodyparts.len();
    if k == 0 || !xyc.len().is_multiple_of(3 * k) {
        return Err(FpError::from("keypoint array does not match the bodyparts".to_string()));
    }
    let n = xyc.len() / (3 * k);
    let col = |b: usize, o: usize| (0..n).map(|i| xyc[(i * k + b) * 3 + o]).collect::<Vec<f64>>();
    let p = simplefp::video::Pose { bodyparts, x: (0..k).map(|b| col(b, 0)).collect(), y: (0..k).map(|b| col(b, 1)).collect(), likelihood: (0..k).map(|b| col(b, 2)).collect() };
    Ok(simplefp::video::pose::write_dlc_csv(&p, &scorer))
}

/// One tracked, scored session for training: bouts as flattened (start, end) pairs.
#[derive(uniffi::Record)]
pub struct TrainingSession {
    pub name: String,
    pub session: Arc<Session>,
    pub tracker: Arc<VideoTracker>,
    pub camera: String,
    pub bouts: Vec<f64>,
}

/// Train a behavior detector; returns the model file JSON (with its cross-validated evaluation).
#[uniffi::export]
pub fn train_behavior(sessions: Vec<TrainingSession>, behavior: String) -> R<String> {
    let mut data = vec![];
    let mut width = 0;
    for s in &sessions {
        let (f, t) = s.tracker.frame_features(&s.session, &s.camera)?;
        let bouts: Vec<(f64, f64)> = s.bouts.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect();
        width = s.tracker.w;
        data.push((s.name.clone(), f, t.clone(), simplefp::behavior::labels(&t, &bouts)));
    }
    let (file, _) = simplefp::behavior::train_model(&data, &behavior, width, Default::default())?;
    Ok(serde_json::to_string_pretty(&file).unwrap_or_default())
}

/// Proposals (annotation JSON array, `review: proposed`) from a model for one tracked session.
#[uniffi::export]
pub fn propose_behavior(model_json: String, session: Arc<Session>, tracker: Arc<VideoTracker>, camera: String) -> R<String> {
    let m: simplefp::behavior::ModelFile = serde_json::from_str(&model_json).map_err(|e| FpError::from(format!("model file: {e}")))?;
    if m.tracking_width != tracker.w {
        return Err(FpError::from(format!("model was trained at {} px tracking width; this video was tracked at {} px", m.tracking_width, tracker.w)));
    }
    let (f, t) = tracker.frame_features(&session, &camera)?;
    Ok(serde_json::to_string(&simplefp::behavior::propose(&m, &f, &t)?).unwrap_or_default())
}

fn envelope(y: &[f64], t0: f64, fs: f64, a: f64, b: f64, buckets: usize) -> Vec<u8> {
    let n = y.len();
    if n == 0 || b <= a {
        return vec![];
    }
    let i0 = (((a - t0) * fs).floor().max(0.0) as usize).min(n);
    let i1 = (((b - t0) * fs).ceil().max(0.0) as usize + 1).min(n);
    let mut out = Vec::with_capacity(buckets * 12);
    let mut push = |t: f64, lo: f64, hi: f64| {
        out.extend((t as f32).to_le_bytes());
        out.extend((lo as f32).to_le_bytes());
        out.extend((hi as f32).to_le_bytes());
    };
    let count = i1.saturating_sub(i0);
    if count <= buckets {
        for (i, &v) in y.iter().enumerate().take(i1).skip(i0) {
            if v.is_finite() {
                push(t0 + i as f64 / fs, v, v);
            }
        }
    } else {
        for k in 0..buckets {
            let s = i0 + k * count / buckets;
            let e = i0 + (k + 1) * count / buckets;
            let (lo, hi) = y[s..e].iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &v| (l.min(v), h.max(v)));
            if lo.is_finite() {
                push(t0 + (s + e) as f64 / 2.0 / fs, lo, hi);
            }
        }
    }
    out
}

fn results_of(a: &Analysis, p: &Preset) -> Results {
    let q = &a.output.qc;
    let o = &a.output;
    let unit = match p.events.baseline {
        Baseline::Zscore | Baseline::RobustZscore => "z (baseline)",
        _ => match p.events.trace {
            preset::Trace::Dff => "ΔF/F (%)",
            preset::Trace::Z => "z (session)",
        },
    };
    Results {
        qc: Qc {
            fs: q.fs,
            decimation: q.decimation as u32,
            duration_s: q.duration_s,
            analyzed_from_s: o.time(o.valid.start),
            analyzed_to_s: o.time(o.valid.end - 1),
            bleach_signal_pct: q.bleach_signal_pct,
            bleach_control_pct: q.bleach_control_pct,
            bleach_fallback: q.bleach_fallback,
            motion_r: q.motion_r,
            motion_slope: q.motion_slope,
            motion_zero_weight_frac: q.motion_zero_weight_frac,
            saturation_frac: q.saturation_frac,
            noise_dff_pct: q.noise_dff_pct,
            residual_drift_pct_per_10min: q.residual_drift_pct_per_10min,
            warnings: q.warnings.clone(),
        },
        events: a
            .events
            .iter()
            .map(|e| {
                let ep = &e.epochs;
                let (mean, lo, hi, significant) = match &e.ci {
                    Some(c) => (c.mean.clone(), c.lo.clone(), c.hi.clone(), c.significant.clone()),
                    None => (ep.trials.first().cloned().unwrap_or_default(), vec![], vec![], vec![]),
                };
                let (control_mean, control_lo, control_hi) = match &e.control_ci {
                    Some(c) => (c.mean.clone(), c.lo.clone(), c.hi.clone()),
                    None => (vec![], vec![], vec![]),
                };
                EventSummary {
                    label: e.label.clone(),
                    event_times: ep.event_times.clone(),
                    dropped: ep.dropped.clone(),
                    t: ep.t.clone(),
                    mean,
                    lo,
                    hi,
                    significant,
                    control_mean,
                    control_lo,
                    control_hi,
                    control_responds: e.control_responds,
                    metrics: e
                        .metrics
                        .iter()
                        .zip(&ep.event_times)
                        .map(|(m, &t)| TrialMetrics { event_time: t, mean: m.mean, auc: m.auc, auc_pos: m.auc_pos, auc_neg: m.auc_neg, peak: m.peak, peak_latency: m.peak_latency, trough: m.trough, trough_latency: m.trough_latency })
                        .collect(),
                    heatmap: ep.trials.iter().flatten().flat_map(|&v| (v as f32).to_le_bytes()).collect(),
                    unit: unit.to_string(),
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_lab_feeds_cohort_analysis() {
        let dir = std::env::temp_dir().join(format!("simplefp-demolab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = LabStore::open(dir.join("lab.sqlite").display().to_string(), "T".into()).unwrap();
        store.seed_demo(dir.join("data").display().to_string(), "2026-10-25".into()).unwrap();
        let tasks = store.tasks("2026-10-25".into(), 21).unwrap();
        for kind in ["wean", "post_op", "protocol"] {
            assert!(tasks.contains(&format!("\"kind\":\"{kind}\"")), "missing {kind} task: {tasks}");
        }
        let cohorts: Vec<serde_json::Value> = serde_json::from_str(&store.list("cohort".into(), None, None).unwrap()).unwrap();
        let id = cohorts[0]["id"].as_str().unwrap().to_string();
        store.freeze_cohort(id.clone()).unwrap();
        let path = store.write_cohort_manifest(id, dir.join("data").display().to_string(), dir.join("cohort.toml").display().to_string()).unwrap();
        let c = Cohort::open(path).unwrap().results();
        assert_eq!(c.sessions.iter().filter(|s| s.status == "ok").count(), 16);
        assert!(c.tests.iter().any(|t| t.contrast.starts_with("ChAT − WT") && t.metric == "mean" && t.p < 0.05));
        assert!(store.verify_audit().unwrap() > 50);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn envelope_bounds_and_lod() {
        let y: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let f = |b: &[u8]| b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect::<Vec<f32>>();
        let coarse = f(&envelope(&y, 0.0, 10.0, 0.0, 100.0, 10));
        assert_eq!(coarse.len(), 30);
        assert_eq!((coarse[1], coarse[2]), (0.0, 99.0));
        let fine = f(&envelope(&y, 0.0, 10.0, 10.0, 11.0, 100));
        assert_eq!(fine.len() / 3, 11); // samples 100..=110: both window edges included
        assert!(fine.chunks(3).all(|c| c[1] == c[2]));
    }
}
