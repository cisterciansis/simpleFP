//! The preprocessing pipeline, driven entirely by a [`Preset`].
//!
//! Order of operations (docs/science/methods-spec.md §0):
//! 1. low-pass both channels at the raw rate (zero-phase Butterworth; anti-alias),
//! 2. decimate by block mean with an integer factor (no interpolation),
//! 3. exclude the start/end trim from all fitting (the clock is never shifted),
//! 4. fit photobleaching per channel and detrend,
//! 5. regress detrended control onto detrended signal (robust IRLS) and subtract,
//! 6. ΔF/F = corrected / signal bleaching baseline; plus session z-scores.

use crate::dsp::{self, Band};
use crate::fit;
use crate::preset::{BleachMethod, MotionMethod, Preset};
use crate::stats;
use serde::Serialize;
use std::ops::Range;

/// Raw input: two channels on one clock (block-relative seconds).
pub struct Input<'a> {
    pub fs: f64,
    pub t0: f64,
    pub signal: &'a [f64],
    pub control: Option<&'a [f64]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Qc {
    pub fs: f64,
    pub decimation: usize,
    pub duration_s: f64,
    /// Fitted baseline drop over the analyzed span, % of initial.
    pub bleach_signal_pct: f64,
    pub bleach_control_pct: Option<f64>,
    /// The double-exponential fit failed or was implausible; high-pass fallback used.
    pub bleach_fallback: bool,
    /// Pearson r between detrended control and detrended signal (shared artifact variance).
    pub motion_r: Option<f64>,
    pub motion_slope: Option<f64>,
    /// Fraction of samples the robust fit rejected (weight 0).
    pub motion_zero_weight_frac: Option<f64>,
    /// Fraction of raw samples within 0.1% of the channel's extreme values (clipping proxy).
    pub saturation_frac: f64,
    /// Robust white-noise estimate of ΔF/F (%): 1.4826·MAD(diff)/√2.
    pub noise_dff_pct: f64,
    /// Linear trend left in ΔF/F, % per 10 min.
    pub residual_drift_pct_per_10min: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    /// Time of sample 0 (block clock); sample i is at `t0 + i / fs`.
    pub t0: f64,
    pub fs: f64,
    /// Analyzed range (sample indices) after trimming; derived traces are NaN outside.
    pub valid: Range<usize>,
    pub signal: Vec<f64>,
    pub control: Option<Vec<f64>>,
    /// ΔF/F denominator F0(t): the signal's bleaching fit (or Ĉ for `fit_raw`).
    pub signal_baseline: Vec<f64>,
    pub control_baseline: Option<Vec<f64>>,
    /// Artifact estimate subtracted from the signal, in signal units.
    pub motion_fit: Option<Vec<f64>>,
    /// ΔF/F in percent.
    pub dff: Vec<f64>,
    /// Control channel as % of its own bleaching baseline (for control-channel event QC).
    pub control_dff: Option<Vec<f64>>,
    /// Session z-score of ΔF/F over the valid range (mean/sd or median/MAD per preset).
    pub z: Vec<f64>,
    pub qc: Qc,
}

impl Output {
    pub fn time(&self, i: usize) -> f64 {
        self.t0 + i as f64 / self.fs
    }
}

fn condition(x: &[f64], p: &Preset, fs: f64) -> Vec<f64> {
    match p.preprocess.lowpass_hz {
        Some(fc) => dsp::sosfiltfilt(&dsp::butter(p.preprocess.lowpass_order, fc, fs, Band::Low), x),
        None => x.to_vec(),
    }
}

/// Slow component removed by a zero-phase 0.001 Hz high-pass (Akam & Burgeno 2023).
fn highpass_baseline(y: &[f64], cutoff_hz: f64, fs: f64) -> Vec<f64> {
    let hp = dsp::sosfiltfilt(&dsp::butter(2, cutoff_hz, fs, Band::High), y);
    y.iter().zip(hp).map(|(a, b)| a - b).collect()
}

/// Fit the bleaching baseline on `range`, evaluate it over the whole trace.
/// Returns `(baseline, used_fallback)`.
fn bleach(y: &[f64], t: &[f64], range: &Range<usize>, m: &BleachMethod, fs: f64) -> (Vec<f64>, bool) {
    match m {
        BleachMethod::DoubleExp { robust } => {
            let f = fit::fit_biexp(&t[range.clone()], &y[range.clone()], *robust);
            let base: Vec<f64> = t.iter().map(|&ti| f.eval(ti)).collect();
            let span = t[range.end - 1] - t[range.start];
            // Implausible fits: non-finite, non-positive, or a "slow" constant far beyond any session.
            let ok = base[range.clone()].iter().all(|b| b.is_finite() && *b > 0.0) && f.tau1 > 0.1 && f.tau2 < 1e3 * span.max(1.0);
            if ok { (base, false) } else { (highpass_baseline(y, 0.001, fs), true) }
        }
        BleachMethod::Highpass { cutoff_hz } => (highpass_baseline(y, *cutoff_hz, fs), false),
        BleachMethod::None => (vec![stats::mean(&y[range.clone()]); y.len()], false),
    }
}

pub fn run(input: &Input, p: &Preset) -> Result<Output, String> {
    p.validate()?;
    let raw_fs = input.fs;
    if let Some(c) = input.control
        && c.len() != input.signal.len() {
            return Err(format!("signal ({}) and control ({}) lengths differ", input.signal.len(), c.len()));
        }
    if let Some(fc) = p.preprocess.lowpass_hz
        && fc >= raw_fs / 2.0 {
            return Err(format!("lowpass {fc} Hz must be below Nyquist ({:.1} Hz)", raw_fs / 2.0));
        }
    let needs_control = !matches!(p.motion, MotionMethod::None);
    if needs_control && input.control.is_none() {
        return Err("the preset's motion correction needs a control channel".into());
    }
    // floor keeps fs_out ≥ target_fs, so the anti-alias margin the preset was validated for holds.
    let factor = ((raw_fs / p.preprocess.target_fs).floor() as usize).max(1);
    let fs = raw_fs / factor as f64;
    // Block k averages raw samples [k·N, (k+1)·N); its timestamp is the block centre.
    let t0 = input.t0 + (factor as f64 - 1.0) / (2.0 * raw_fs);

    let sat = saturation(input.signal).max(input.control.map_or(0.0, saturation));
    let sig = dsp::block_mean(&condition(input.signal, p, raw_fs), factor);
    let ctl = input.control.map(|c| dsp::block_mean(&condition(c, p, raw_fs), factor));
    let n = sig.len();
    let t: Vec<f64> = (0..n).map(|i| t0 + i as f64 / fs).collect();

    let skip = (p.preprocess.trim_start_s * fs).ceil() as usize;
    let tail = (p.preprocess.trim_end_s * fs).ceil() as usize;
    if skip + tail + (10.0 * fs) as usize > n {
        return Err(format!("recording too short: {:.1} s after trimming", n.saturating_sub(skip + tail) as f64 / fs));
    }
    let valid = skip..n - tail;
    let mut warnings = vec![];

    let (sig_base, sig_fb) = bleach(&sig, &t, &valid, &p.bleach, fs);
    if sig_base[valid.clone()].iter().any(|&b| b <= 0.0) {
        return Err("signal baseline is not positive; is the stream dark or background-subtracted?".into());
    }
    let sig_det: Vec<f64> = sig.iter().zip(&sig_base).map(|(a, b)| a - b).collect();
    let ctl_fit = ctl.as_ref().map(|c| bleach(c, &t, &valid, &p.bleach, fs));
    let fallback = sig_fb || ctl_fit.as_ref().is_some_and(|f| f.1);
    if fallback {
        warnings.push("double-exponential bleaching fit implausible; used 0.001 Hz high-pass baseline instead".into());
    }
    let ctl_base = ctl_fit.map(|f| f.0);
    let ctl_det: Option<Vec<f64>> = ctl.as_ref().zip(ctl_base.as_ref()).map(|(c, b)| c.iter().zip(b).map(|(a, b)| a - b).collect());

    let mut dff = vec![f64::NAN; n];
    let (mut f0, mut motion_fit, mut motion_slope, mut zero_frac) = (sig_base.clone(), None, None, None);
    let motion_r = ctl_det.as_ref().map(|c| stats::pearson(&c[valid.clone()], &sig_det[valid.clone()]));

    match (&p.motion, ctl.as_ref(), ctl_det.as_ref()) {
        (MotionMethod::Irls { .. } | MotionMethod::Ols, Some(_), Some(cd)) => {
            let (x, y) = (&cd[valid.clone()], &sig_det[valid.clone()]);
            let f = match p.motion {
                MotionMethod::Irls { tuning } => fit::robust_line(x, y, tuning, 50),
                _ => {
                    let (slope, intercept) = fit::line(x, y, None);
                    fit::LineFit { slope, intercept, zero_weight_frac: 0.0 }
                }
            };
            motion_slope = Some(f.slope);
            zero_frac = Some(f.zero_weight_frac);
            if f.slope <= 0.0 {
                // Subtracting a negatively scaled control would add artifact (spec §4).
                warnings.push(format!("control→signal slope {:.3} ≤ 0: motion correction disabled; the control may carry ligand-dependent signal", f.slope));
            } else {
                motion_fit = Some(cd.iter().map(|v| f.slope * v + f.intercept).collect::<Vec<f64>>());
            }
            if f.zero_weight_frac > 0.3 {
                warnings.push(format!("robust fit rejected {:.0} % of samples", 100.0 * f.zero_weight_frac));
            }
            for i in valid.clone() {
                dff[i] = 100.0 * (sig_det[i] - motion_fit.as_ref().map_or(0.0, |m| m[i])) / sig_base[i];
            }
        }
        (MotionMethod::FitRaw { tuning }, Some(c), _) => {
            let f = fit::robust_line(&c[valid.clone()], &sig[valid.clone()], *tuning, 50);
            if f.slope <= 0.0 {
                return Err(format!("fit_raw: control does not track the signal (slope {:.3} ≤ 0); use method = \"irls\" or \"none\"", f.slope));
            }
            let fitted: Vec<f64> = c.iter().map(|v| f.slope * v + f.intercept).collect();
            if fitted[valid.clone()].iter().any(|&v| v <= 0.0) {
                return Err("fit_raw: fitted control is not positive; cannot use it as F0".into());
            }
            (motion_slope, zero_frac) = (Some(f.slope), Some(f.zero_weight_frac));
            for i in valid.clone() {
                dff[i] = 100.0 * (sig[i] - fitted[i]) / fitted[i];
            }
            motion_fit = Some(fitted.iter().zip(&sig_base).map(|(c, b)| c - b).collect());
            f0 = fitted;
        }
        _ => {
            for i in valid.clone() {
                dff[i] = 100.0 * sig_det[i] / sig_base[i];
            }
        }
    }

    let control_dff = ctl_det.as_ref().zip(ctl_base.as_ref()).map(|(d, b)| {
        let mut v = vec![f64::NAN; n];
        for i in valid.clone() {
            v[i] = 100.0 * d[i] / b[i];
        }
        v
    });

    let v = &dff[valid.clone()];
    let (center, scale) = if p.output.robust_z { (stats::median(v), 1.4826 * stats::mad(v)) } else { (stats::mean(v), stats::std(v)) };
    let z = dff.iter().map(|d| (d - center) / scale).collect();

    let pct = |b: &[f64]| 100.0 * (1.0 - b[valid.end - 1] / b[valid.start]);
    let diffs: Vec<f64> = v.windows(2).map(|w| w[1] - w[0]).collect();
    let noise = 1.4826 * stats::mad(&diffs) / 2f64.sqrt();
    let drift = 600.0 * fit::line(&t[valid.clone()], v, None).0;
    if drift.abs() > 2.0 * noise {
        warnings.push(format!("residual drift {drift:+.2} % ΔF/F per 10 min after bleaching correction"));
    }
    let bleach_signal_pct = pct(&sig_base);
    if bleach_signal_pct > 50.0 {
        warnings.push(format!("signal bleached {bleach_signal_pct:.0} % over the session"));
    }
    if sat > 0.0 {
        warnings.push(format!("{:.2} % of raw samples sit at the channel extremes (possible clipping)", 100.0 * sat));
    }
    let qc = Qc {
        fs,
        decimation: factor,
        duration_s: n as f64 / fs,
        bleach_signal_pct,
        bleach_control_pct: ctl_base.as_deref().map(pct),
        bleach_fallback: fallback,
        motion_r,
        motion_slope,
        motion_zero_weight_frac: zero_frac,
        saturation_frac: sat,
        noise_dff_pct: noise,
        residual_drift_pct_per_10min: drift,
        warnings,
    };
    Ok(Output { t0, fs, valid, signal: sig, control: ctl, signal_baseline: f0, control_baseline: ctl_base, motion_fit, dff, control_dff, z, qc })
}

/// Fraction of samples in flat runs (≥ 10 consecutive) at the channel's max or min.
/// Without the ADC range this is a proxy; true clipping shows as plateaus at an extreme.
fn saturation(x: &[f64]) -> f64 {
    let (lo, hi) = x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let mut clipped = 0;
    let mut run = 0;
    for &v in x {
        if v == hi || v == lo {
            run += 1;
        } else {
            if run >= 10 {
                clipped += run;
            }
            run = 0;
        }
    }
    if run >= 10 {
        clipped += run;
    }
    clipped as f64 / x.len() as f64
}
