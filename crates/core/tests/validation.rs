//! End-to-end validation against simulated ground truth.

use simplefp_core::{dsp, events, pipeline, preset, sim, stats};

fn run(motion: preset::MotionMethod) -> (pipeline::Output, sim::Sim) {
    let s = sim::simulate(&sim::SimParams::default());
    let mut p = preset::builtin("grab-ach3.0-tdt").unwrap();
    p.motion = motion;
    let out = pipeline::run(&pipeline::Input { fs: s.fs, t0: 0.0, signal: &s.signal, control: Some(&s.control) }, &p).unwrap();
    (out, s)
}

fn truth_on_grid(out: &pipeline::Output, s: &sim::Sim) -> Vec<f64> {
    dsp::block_mean(&s.truth, out.qc.decimation).iter().map(|v| 100.0 * v).collect()
}

#[test]
fn recovers_true_dff() {
    let (out, s) = run(preset::MotionMethod::Irls { tuning: 4.685 });
    let truth = truth_on_grid(&out, &s);
    let v = out.valid.clone();
    let r = stats::pearson(&out.dff[v.clone()], &truth[v.clone()]);
    let rmse = (v.clone().map(|i| (out.dff[i] - truth[i]).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
    println!("r = {r:.4}, rmse = {rmse:.3} %, qc = {:?}", out.qc);
    assert!(r > 0.95, "r = {r}");
    assert!(rmse < 0.6, "rmse = {rmse} %");

    // Motion correction must beat no correction.
    let (raw, _) = run(preset::MotionMethod::None);
    let r_raw = stats::pearson(&raw.dff[v.clone()], &truth[v]);
    assert!(r > r_raw + 0.05, "corrected {r} vs uncorrected {r_raw}");
}

#[test]
fn evoked_response_is_significant_and_sized() {
    let (out, s) = run(preset::MotionMethod::Irls { tuning: 4.685 });
    let p = preset::builtin("grab-ach3.0-tdt").unwrap().events;
    let mut ep = events::extract(out.t0, out.fs, &out.dff, &s.events, p.pre_s, p.post_s);
    events::baseline(&mut ep, p.baseline, p.baseline_window[0], p.baseline_window[1]).unwrap();
    assert_eq!(ep.trials.len(), s.events.len());
    let ci = events::bootstrap_mean_ci(&ep.trials, p.n_boot, p.ci_level, (p.min_consecutive_s * out.fs) as usize, p.seed);
    let peak = ci.mean.iter().copied().fold(f64::MIN, f64::max);
    println!("mean peak = {peak:.3} % (true 4 %)");
    assert!((peak - 4.0).abs() < 0.6, "peak {peak}");
    let w = events::window(&ep.t, 0.2, 1.0);
    assert!(ci.significant[w].iter().all(|&b| b));
    let pre = events::window(&ep.t, -5.0, -1.0);
    assert!(!ci.significant[pre].iter().any(|&b| b), "false positive in baseline");
}

#[test]
fn flags_ligand_dependent_control() {
    let s = sim::simulate(&sim::SimParams { control_crosstalk: -0.3, ..Default::default() });
    let p = preset::builtin("grab-ach3.0-tdt").unwrap();
    let input = pipeline::Input { fs: s.fs, t0: 0.0, signal: &s.signal, control: Some(&s.control) };
    let a = simplefp_core::analysis::analyze(&input, &p, &[("evt".into(), s.events.clone())]).unwrap();
    assert!(a.events[0].control_responds);
    assert!(a.output.qc.warnings.iter().any(|w| w.contains("control channel responds")), "{:?}", a.output.qc.warnings);

    let clean = sim::simulate(&sim::SimParams::default());
    let input = pipeline::Input { fs: clean.fs, t0: 0.0, signal: &clean.signal, control: Some(&clean.control) };
    let a = simplefp_core::analysis::analyze(&input, &p, &[("evt".into(), clean.events.clone())]).unwrap();
    assert!(!a.events[0].control_responds);
}

#[test]
fn fit_raw_alternative_is_worse_under_unequal_bleaching() {
    let (out, s) = run(preset::MotionMethod::FitRaw { tuning: 1.4 });
    let truth = truth_on_grid(&out, &s);
    let v = out.valid.clone();
    // The simulator bleaches the channels non-proportionally, which fit_raw assumes away
    // (Simpson et al. 2024): the mismatch appears as slow drift. Compare transients only.
    let hp = |x: &[f64]| dsp::sosfiltfilt(&dsp::butter(2, 0.01, out.fs, dsp::Band::High), x);
    let r = stats::pearson(&hp(&out.dff[v.clone()]), &hp(&truth[v.clone()]));
    let r_drift = stats::pearson(&out.dff[v.clone()], &truth[v]);
    println!("fit_raw r = {r:.4} (with drift {r_drift:.4})");
    // A single raw-scale fit is dominated by the bleaching covariance, so it also
    // mis-scales motion artifacts. This documents why detrend-then-regress is the default.
    let (def, _) = run(preset::MotionMethod::Irls { tuning: 1.4 });
    let r_def = stats::pearson(&hp(&def.dff[out.valid.clone()]), &hp(&truth[out.valid.clone()]));
    assert!(r > 0.7 && r_def > r + 0.1, "fit_raw {r} vs default {r_def}");
}

#[test]
fn window_baseline_preserves_tonic_state_change() {
    // A tonic +5 % step at 300 s (e.g. waking from anesthesia) on top of bleaching.
    let mut s = sim::simulate(&sim::SimParams { n_events: 0, motion_per_min: 0.0, ..Default::default() });
    let step_at = (300.0 * s.fs) as usize;
    s.signal[step_at..].iter_mut().for_each(|v| *v *= 1.05);
    let mut p = preset::builtin("grab-ach3.0-tdt").unwrap();
    p.motion = preset::MotionMethod::None;
    let late = |o: &pipeline::Output| stats::mean(&o.dff[(450.0 * o.fs) as usize..(590.0 * o.fs) as usize]);
    let input = pipeline::Input { fs: s.fs, t0: 0.0, signal: &s.signal, control: Some(&s.control) };
    p.bleach = preset::BleachMethod::Window { start_s: 200.0, end_s: 290.0 };
    let w = late(&pipeline::run(&input, &p).unwrap());
    p.bleach = preset::BleachMethod::Linear;
    let l = late(&pipeline::run(&input, &p).unwrap());
    // Truth: the step times the simulator's bleaching between the reference and late windows
    // (window F0 does not correct bleaching after the reference period: conservative for increases).
    let bs = |x: f64| 1.0 + 0.25 * (-x / 60.0).exp() + 0.15 * (-x / 900.0).exp();
    let avg = |a: f64, b: f64| (0..100).map(|k| bs(a + (b - a) * k as f64 / 99.0)).sum::<f64>() / 100.0;
    let truth = 100.0 * (1.05 * avg(450.0, 590.0) / avg(200.0, 290.0) - 1.0);
    println!("late ΔF/F: window {w:.2} %, linear detrend {l:.2} %, truth {truth:.2} %");
    assert!((w - truth).abs() < 0.2, "window {w} vs truth {truth}");
    assert!(l.abs() < 0.5, "detrending should erase the step: {l}");
}
