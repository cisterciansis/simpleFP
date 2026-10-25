//! Ground-truth photometry simulator for validation and demo data.
//!
//! signal  = B_s(t) · (1 + dF/F_true(t)) + m_s · motion(t) + noise
//! control = B_c(t)                       + m_c · motion(t) + noise
//! B(t) are double-exponential bleaching curves; `motion` is a sparse train of
//! smooth bumps shared by both channels; the true ΔF/F is a sum of
//! difference-of-exponentials transients (sensor kinetics) at event times,
//! including negative-going ones (ACh dips).

use crate::stats::Rng;

pub struct Sim {
    pub fs: f64,
    pub signal: Vec<f64>,
    pub control: Vec<f64>,
    /// True ΔF/F (fraction, not %).
    pub truth: Vec<f64>,
    /// Event times (s) that evoke a positive transient.
    pub events: Vec<f64>,
}

pub struct SimParams {
    pub fs: f64,
    pub duration_s: f64,
    pub n_events: usize,
    /// Peak true ΔF/F of an evoked transient (fraction).
    pub amplitude: f64,
    pub rise_tau_s: f64,
    pub decay_tau_s: f64,
    pub motion_per_min: f64,
    pub noise_frac: f64,
    /// Ligand-dependent component in the control, as a fraction of the signal's
    /// ΔF/F (negative = opposite sign, as expected for GRAB sensors at 405 nm).
    pub control_crosstalk: f64,
    pub seed: u64,
}

impl Default for SimParams {
    fn default() -> Self {
        // Kinetics roughly GRAB-ACh3.0-like; amplitudes typical of in vivo striatal ACh.
        SimParams { fs: 1017.2526, duration_s: 600.0, n_events: 20, amplitude: 0.04, rise_tau_s: 0.15, decay_tau_s: 0.8, motion_per_min: 3.0, noise_frac: 0.004, control_crosstalk: 0.0, seed: 1 }
    }
}

fn gauss(rng: &mut Rng) -> f64 {
    // Box–Muller
    let u1 = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    let u2 = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    (-2.0 * u1.max(1e-300).ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

pub fn simulate(p: &SimParams) -> Sim {
    let mut rng = Rng::new(p.seed);
    let n = (p.duration_s * p.fs) as usize;
    let t = |i: usize| i as f64 / p.fs;

    // Events spread over the session after a settling period, at least 15 s apart.
    let span = p.duration_s - 40.0;
    let events: Vec<f64> = (0..p.n_events).map(|k| 25.0 + span * (k as f64 + 0.3 + 0.4 * (rng.below(1000) as f64 / 1000.0)) / p.n_events as f64).collect();
    // Spontaneous dips between events.
    let dips: Vec<f64> = events.windows(2).map(|w| 0.5 * (w[0] + w[1])).collect();

    let kernel_len = (8.0 * p.decay_tau_s * p.fs) as usize;
    let peak_norm = {
        // Normalise the difference of exponentials to unit peak.
        let (r, d) = (p.rise_tau_s, p.decay_tau_s);
        let tp = r * d / (d - r) * (d / r).ln();
        (-tp / d).exp() - (-tp / r).exp()
    };
    let kernel: Vec<f64> = (0..kernel_len).map(|i| ((-t(i) / p.decay_tau_s).exp() - (-t(i) / p.rise_tau_s).exp()) / peak_norm).collect();
    let mut truth = vec![0.0; n];
    let mut add = |at: f64, amp: f64| {
        let i0 = (at * p.fs) as usize;
        for (k, v) in kernel.iter().enumerate() {
            if i0 + k < n {
                truth[i0 + k] += amp * v;
            }
        }
    };
    events.iter().for_each(|&e| add(e, p.amplitude));
    dips.iter().for_each(|&e| add(e, -0.4 * p.amplitude));

    let n_motion = (p.motion_per_min * p.duration_s / 60.0) as usize;
    let mut motion = vec![0.0; n];
    for _ in 0..n_motion {
        let c = rng.below(n);
        let w = (0.3 + 0.7 * rng.below(1000) as f64 / 1000.0) * p.fs;
        let a = if rng.below(2) == 0 { -1.0 } else { 1.0 } * (0.02 + 0.06 * rng.below(1000) as f64 / 1000.0);
        let lo = c.saturating_sub((4.0 * w) as usize);
        let hi = (c + (4.0 * w) as usize).min(n);
        for (i, m) in motion.iter_mut().enumerate().take(hi).skip(lo) {
            *m += a * (-0.5 * ((i as f64 - c as f64) / w).powi(2)).exp();
        }
    }

    let bs = |x: f64| 1.0 + 0.25 * (-x / 60.0).exp() + 0.15 * (-x / 900.0).exp();
    let bc = |x: f64| 0.6 + 0.10 * (-x / 50.0).exp() + 0.08 * (-x / 700.0).exp();
    let mut signal = Vec::with_capacity(n);
    let mut control = Vec::with_capacity(n);
    for i in 0..n {
        let x = t(i);
        signal.push(bs(x) * (1.0 + truth[i]) + 0.9 * motion[i] * bs(x) + p.noise_frac * gauss(&mut rng));
        control.push(bc(x) * (1.0 + p.control_crosstalk * truth[i]) + 0.9 * motion[i] * bc(x) * 1.35 + p.noise_frac * gauss(&mut rng));
    }
    Sim { fs: p.fs, signal, control, truth, events }
}
