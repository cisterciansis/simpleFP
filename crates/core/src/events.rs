//! Peri-event analysis: epoch extraction, per-trial baselining, response
//! metrics, and bootstrap confidence intervals with a consecutive-sample
//! significance criterion (Jean-Richard-dit-Bressel, Clifford & McNally 2020,
//! Front. Mol. Neurosci. 13:14, doi:10.3389/fnmol.2020.00014).

use crate::stats::{self, Rng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Trials × samples on a shared relative-time grid.
#[derive(Debug, Clone, Serialize)]
pub struct Epochs {
    /// Relative time of each column (s), 0 = event.
    pub t: Vec<f64>,
    /// One row per kept event.
    pub trials: Vec<Vec<f64>>,
    /// Event times (s) of the kept rows, same order as `trials`.
    pub event_times: Vec<f64>,
    /// Event times dropped because the window ran past the recording.
    pub dropped: Vec<f64>,
}

/// Cut `[event − pre, event + post]` windows from a uniformly sampled trace.
///
/// The grid is `k / fs` relative to each event, and values are linearly
/// interpolated, so sub-sample event times (TDT epocs are 2.56 µs resolution)
/// do not jitter the average by up to half a sample.
pub fn extract(t0: f64, fs: f64, y: &[f64], events: &[f64], pre: f64, post: f64) -> Epochs {
    let k0 = -(pre * fs).round() as i64;
    let k1 = (post * fs).round() as i64;
    let t: Vec<f64> = (k0..=k1).map(|k| k as f64 / fs).collect();
    let mut out = Epochs { t, trials: vec![], event_times: vec![], dropped: vec![] };
    let n = y.len();
    for &e in events {
        let first = (e - t0) * fs + k0 as f64;
        let last = (e - t0) * fs + k1 as f64;
        if !e.is_finite() || first < 0.0 || last > (n - 1) as f64 {
            out.dropped.push(e);
            continue;
        }
        let row = out
            .t
            .iter()
            .map(|rt| {
                let x = (e + rt - t0) * fs;
                let i = (x.floor() as usize).min(n - 2);
                let f = x - i as f64;
                y[i] * (1.0 - f) + y[i + 1] * f
            })
            .collect();
        out.trials.push(row);
        out.event_times.push(e);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Baseline {
    /// Leave epochs on the session-normalized scale.
    None,
    /// Subtract the baseline-window mean.
    Subtract,
    /// (x − mean) / sd of the baseline window.
    Zscore,
    /// (x − median) / (1.4826 · MAD) of the baseline window; resistant to transients.
    RobustZscore,
}

/// Columns of `t` inside the closed window `[a, b]`.
pub fn window(t: &[f64], a: f64, b: f64) -> std::ops::Range<usize> {
    let eps = 1e-9;
    let i = t.partition_point(|&v| v < a - eps);
    let j = t.partition_point(|&v| v <= b + eps);
    i..j.max(i)
}

/// Per-trial baseline correction using the `[a, b]` window (relative s).
/// Returns an error if the window has fewer than 2 samples in the epoch grid,
/// rather than silently skipping normalization.
pub fn baseline(ep: &mut Epochs, mode: Baseline, a: f64, b: f64) -> Result<(), String> {
    if mode == Baseline::None {
        return Ok(());
    }
    let w = window(&ep.t, a, b);
    if w.len() < 2 {
        return Err(format!(
            "baseline window [{a}, {b}] s has {} samples inside the epoch [{:.3}, {:.3}] s",
            w.len(),
            ep.t[0],
            ep.t[ep.t.len() - 1]
        ));
    }
    for row in &mut ep.trials {
        let base = &row[w.clone()];
        let (center, scale) = match mode {
            Baseline::Subtract => (stats::mean(base), 1.0),
            Baseline::Zscore => (stats::mean(base), stats::std(base)),
            Baseline::RobustZscore => (stats::median(base), 1.4826 * stats::mad(base)),
            Baseline::None => unreachable!(),
        };
        let scale = if scale > f64::EPSILON { scale } else { f64::NAN };
        row.iter_mut().for_each(|v| *v = (*v - center) / scale);
    }
    Ok(())
}

/// Scalar response measures of one trial in a window.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Metrics {
    pub mean: f64,
    /// Trapezoidal area (signal units × s). Signed: dips count negative.
    pub auc: f64,
    /// Area of the positive and negative lobes separately (they cancel in `auc`).
    pub auc_pos: f64,
    pub auc_neg: f64,
    pub peak: f64,
    pub peak_latency: f64,
    pub trough: f64,
    pub trough_latency: f64,
}

pub fn metrics(t: &[f64], row: &[f64], a: f64, b: f64) -> Metrics {
    let w = window(t, a, b);
    let (tt, y) = (&t[w.clone()], &row[w]);
    let trap = |f: fn(f64) -> f64| tt.windows(2).zip(y.windows(2)).map(|(t, y)| 0.5 * (t[1] - t[0]) * (f(y[0]) + f(y[1]))).sum::<f64>();
    let (auc, auc_pos, auc_neg) = (trap(|v| v), trap(|v| v.max(0.0)), trap(|v| v.min(0.0)));
    let (mut pi, mut ti) = (0, 0);
    for i in 0..y.len() {
        if y[i] > y[pi] {
            pi = i;
        }
        if y[i] < y[ti] {
            ti = i;
        }
    }
    Metrics { mean: stats::mean(y), auc, auc_pos, auc_neg, peak: y[pi], peak_latency: tt[pi], trough: y[ti], trough_latency: tt[ti] }
}

/// Column-wise mean and bootstrap percentile CI of the mean waveform.
#[derive(Debug, Clone, Serialize)]
pub struct MeanCi {
    pub mean: Vec<f64>,
    pub sem: Vec<f64>,
    pub lo: Vec<f64>,
    pub hi: Vec<f64>,
    /// Samples where the CI excludes 0 for at least `min_run` consecutive samples.
    pub significant: Vec<bool>,
}

/// Bootstrap over rows (resample with replacement), percentile interval expanded
/// by √(n/(n−1)) to correct small-sample narrowness (Jean-Richard-dit-Bressel
/// et al. 2020, `CIadjust.m`; Hesterberg 2015). Requires n ≥ 2.
///
/// The unit of resampling is whatever rows you pass: trials of one animal for
/// a single-subject figure, or per-animal mean waveforms for group inference
/// (trials within an animal are not independent; pooling them inflates n).
pub fn bootstrap_mean_ci(rows: &[Vec<f64>], n_boot: usize, level: f64, min_run: usize, seed: u64) -> MeanCi {
    let n = rows.len();
    let m = rows[0].len();
    let col_mean = |idx: &mut dyn Iterator<Item = usize>| {
        let mut acc = vec![0.0; m];
        for r in idx {
            acc.iter_mut().zip(&rows[r]).for_each(|(a, v)| *a += v);
        }
        acc.iter_mut().for_each(|a| *a /= n as f64);
        acc
    };
    let mean = col_mean(&mut (0..n));
    let sem = (0..m)
        .map(|j| {
            let col: Vec<f64> = rows.iter().map(|r| r[j]).collect();
            if n > 1 { stats::std(&col) / (n as f64).sqrt() } else { f64::NAN }
        })
        .collect();
    // Each replicate gets its own deterministic stream so results are seed-reproducible regardless of thread count.
    let boots: Vec<Vec<f64>> = (0..n_boot)
        .into_par_iter()
        .map(|b| {
            let mut rng = Rng::new(seed ^ (b as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            col_mean(&mut (0..n).map(|_| rng.below(n)))
        })
        .collect();
    let alpha = (1.0 - level) / 2.0;
    let expand = (n as f64 / (n as f64 - 1.0)).sqrt();
    let (lo, hi): (Vec<f64>, Vec<f64>) = (0..m)
        .into_par_iter()
        .map(|j| {
            let mut col: Vec<f64> = boots.iter().map(|b| b[j]).collect();
            let (l, u) = (stats::quantile_mut(&mut col, alpha), stats::quantile_mut(&mut col, 1.0 - alpha));
            let (mid, half) = ((u + l) / 2.0, (u - l) / 2.0 * expand);
            (mid - half, mid + half)
        })
        .unzip();
    let raw: Vec<bool> = lo.iter().zip(&hi).map(|(l, h)| *l > 0.0 || *h < 0.0).collect();
    MeanCi { mean, sem, lo, hi, significant: min_consecutive(&raw, min_run) }
}

/// Keep only runs of `true` at least `min_run` long.
pub fn min_consecutive(mask: &[bool], min_run: usize) -> Vec<bool> {
    let mut out = vec![false; mask.len()];
    let mut i = 0;
    while i < mask.len() {
        if !mask[i] {
            i += 1;
            continue;
        }
        let j = (i..mask.len()).find(|&j| !mask[j]).unwrap_or(mask.len());
        if j - i >= min_run.max(1) {
            out[i..j].iter_mut().for_each(|v| *v = true);
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_aligns_subsample_events() {
        let fs = 10.0;
        let y: Vec<f64> = (0..200).map(|i| i as f64 / fs).collect(); // y = t
        let ep = extract(0.0, fs, &y, &[5.03, 0.1, 19.95], 1.0, 2.0);
        assert_eq!(ep.trials.len(), 1);
        assert_eq!(ep.dropped, vec![0.1, 19.95]);
        assert_eq!(ep.t.len(), 31);
        for (rt, v) in ep.t.iter().zip(&ep.trials[0]) {
            assert!((v - (5.03 + rt)).abs() < 1e-12);
        }
    }

    #[test]
    fn baseline_window_outside_epoch_is_an_error() {
        let mut ep = extract(0.0, 10.0, &vec![1.0; 200], &[10.0], 4.0, 4.0);
        assert!(baseline(&mut ep, Baseline::Zscore, -10.0, -2.0).is_ok());
        let mut ep = extract(0.0, 10.0, &vec![1.0; 200], &[10.0], 1.0, 4.0);
        assert!(baseline(&mut ep, Baseline::Zscore, -10.0, -2.0).is_err());
    }

    #[test]
    fn metrics_signed_auc_and_trough() {
        let t: Vec<f64> = (0..=10).map(|i| i as f64 * 0.1).collect();
        let y: Vec<f64> = t.iter().map(|_| -1.0).collect();
        let m = metrics(&t, &y, 0.0, 1.0);
        assert!((m.auc + 1.0).abs() < 1e-12 && m.trough == -1.0);
        assert!((m.auc_neg + 1.0).abs() < 1e-12 && m.auc_pos == 0.0);
    }

    #[test]
    fn bootstrap_flags_real_response_only() {
        let mut rng = Rng::new(3);
        let rows: Vec<Vec<f64>> = (0..30)
            .map(|_| (0..100).map(|j| if (40..70).contains(&j) { 2.0 } else { 0.0 } + rng.below(1000) as f64 / 500.0 - 1.0).collect())
            .collect();
        let ci = bootstrap_mean_ci(&rows, 2000, 0.95, 5, 42);
        assert!(ci.significant[45..65].iter().all(|&s| s));
        assert!(ci.significant[..30].iter().filter(|&&s| s).count() <= 5);
        let again = bootstrap_mean_ci(&rows, 2000, 0.95, 5, 42);
        assert_eq!(ci.lo, again.lo);
    }

    #[test]
    fn consecutive_filter() {
        let m = [true, true, false, true, true, true, false];
        assert_eq!(min_consecutive(&m, 3), [false, false, false, true, true, true, false]);
    }
}
