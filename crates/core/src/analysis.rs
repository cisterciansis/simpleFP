//! One call from raw channels + event times to every result a figure needs.

use crate::events::{self, Epochs, MeanCi, Metrics};
use crate::pipeline::{self, Input, Output};
use crate::preset::{EventOpts, Preset, Trace};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EventResult {
    pub label: String,
    pub epochs: Epochs,
    /// Bootstrap CI across trials; `None` with fewer than 3 trials.
    pub ci: Option<MeanCi>,
    pub metrics: Vec<Metrics>,
    /// Same analysis on the control channel (% of its baseline).
    pub control_ci: Option<MeanCi>,
    /// The control CI excludes 0 inside the response (metric) window. Then part of
    /// the signal response may be shared with the control, and the result needs a
    /// ligand-insensitive control (mutant sensor) before interpretation. Judged only
    /// in the response window: sparse motion artifacts make chance runs elsewhere in
    /// a long epoch common with few trials.
    pub control_responds: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub output: Output,
    pub events: Vec<EventResult>,
}

impl Analysis {
    /// All event times of a label, including events too close to the edges to epoch.
    pub fn event_times(&self, label: &str) -> Option<Vec<f64>> {
        let e = self.events.iter().find(|e| e.label == label)?;
        let mut v: Vec<f64> = e.epochs.event_times.iter().chain(&e.epochs.dropped).copied().collect();
        v.sort_by(f64::total_cmp);
        Some(v)
    }
}

/// Minimum trials for a bootstrap CI (the √(n/(n−1)) expansion is unstable below this).
pub const MIN_TRIALS_FOR_CI: usize = 3;

fn epochs_of(o: &Output, y: &[f64], times: &[f64], e: &EventOpts) -> Result<Epochs, String> {
    // Epochs must not reach into the trimmed (NaN) region.
    let lo = o.time(o.valid.start) + e.pre_s;
    let hi = o.time(o.valid.end - 1) - e.post_s;
    let (keep, mut out_of_range): (Vec<f64>, Vec<f64>) = times.iter().partition(|&&t| t >= lo && t <= hi);
    let mut ep = events::extract(o.t0, o.fs, y, &keep, e.pre_s, e.post_s);
    ep.dropped.append(&mut out_of_range);
    ep.dropped.sort_by(f64::total_cmp);
    events::baseline(&mut ep, e.baseline, e.baseline_window[0], e.baseline_window[1])?;
    Ok(ep)
}

fn ci(ep: &Epochs, e: &EventOpts, min_run: usize) -> Option<MeanCi> {
    (ep.trials.len() >= MIN_TRIALS_FOR_CI).then(|| events::bootstrap_mean_ci(&ep.trials, e.n_boot, e.ci_level, min_run, e.seed))
}

pub fn analyze(input: &Input, p: &Preset, event_sets: &[(String, Vec<f64>)]) -> Result<Analysis, String> {
    let mut output = pipeline::run(input, p)?;
    let e = &p.events;
    let trace = match e.trace {
        Trace::Dff => &output.dff,
        Trace::Z => &output.z,
    };
    let min_run = (e.min_consecutive_s * output.fs).ceil() as usize;
    let mut results = Vec::with_capacity(event_sets.len());
    for (label, times) in event_sets {
        let ep = epochs_of(&output, trace, times, e)?;
        let metrics = ep.trials.iter().map(|r| events::metrics(&ep.t, r, e.metric_window[0], e.metric_window[1])).collect();
        let control_ci = match &output.control_dff {
            Some(c) => ci(&epochs_of(&output, c, times, e)?, e, min_run),
            None => None,
        };
        let w = events::window(&ep.t, e.metric_window[0], e.metric_window[1]);
        let control_responds = control_ci.as_ref().is_some_and(|c| c.significant[w].iter().any(|&s| s));
        results.push(EventResult { label: label.clone(), ci: ci(&ep, e, min_run), epochs: ep, metrics, control_ci, control_responds });
    }
    for r in results.iter().filter(|r| r.control_responds) {
        output.qc.warnings.push(format!(
            "control channel responds to '{}': part of the signal response may be shared with the control; confirm with a ligand-insensitive (mutant) sensor",
            r.label
        ));
    }
    Ok(Analysis { output, events: results })
}
