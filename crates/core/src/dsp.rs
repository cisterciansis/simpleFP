//! Digital filtering: Butterworth design as second-order sections (SOS) and
//! zero-phase forward–backward filtering. Matches `scipy.signal.butter(...,
//! output="sos")` and `sosfiltfilt` (odd padding, steady-state initial
//! conditions) to within floating-point rounding.

use std::f64::consts::PI;

/// One biquad: `[b0, b1, b2, a0(=1), a1, a2]`.
pub type Section = [f64; 6];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Low,
    High,
}

#[derive(Clone, Copy, Debug)]
struct C(f64, f64);

impl C {
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn div(self, o: C) -> C {
        let d = o.0 * o.0 + o.1 * o.1;
        C((self.0 * o.0 + self.1 * o.1) / d, (self.1 * o.0 - self.0 * o.1) / d)
    }
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
    fn abs(self) -> f64 {
        self.0.hypot(self.1)
    }
}

/// Butterworth filter of `order` with cutoff `fc` Hz at sample rate `fs` Hz.
///
/// Panics if `fc` is not strictly between 0 and Nyquist or `order == 0`;
/// callers validate user input before reaching here.
pub fn butter(order: usize, fc: f64, fs: f64, band: Band) -> Vec<Section> {
    assert!(order > 0 && fc > 0.0 && fc < fs / 2.0, "invalid Butterworth spec");
    let fs2 = 2.0 * fs;
    let warped = fs2 * (PI * fc / fs).tan();
    // Analog prototype poles on the left half unit circle.
    let proto: Vec<C> = (0..order)
        .map(|k| {
            let th = PI * (2 * k + 1 + order) as f64 / (2 * order) as f64;
            C(th.cos(), th.sin())
        })
        .collect();
    let (poles, zero, mut gain): (Vec<C>, C, f64) = match band {
        Band::Low => (proto.iter().map(|p| C(p.0 * warped, p.1 * warped)).collect(), C(-1.0, 0.0), warped.powi(order as i32)),
        Band::High => (proto.iter().map(|&p| C(warped, 0.0).div(p)).collect(), C(1.0, 0.0), 1.0),
    };
    // Bilinear transform of poles; analog zeros (none for LP, s=0 for HP) map to z=-1 / z=+1.
    let mut num = C(1.0, 0.0);
    let mut den = C(1.0, 0.0);
    let zpoles: Vec<C> = poles
        .iter()
        .map(|&p| {
            den = den.mul(C(fs2, 0.0).sub(p));
            C(fs2, 0.0).add(p).div(C(fs2, 0.0).sub(p))
        })
        .collect();
    if band == Band::High {
        num = C(fs2.powi(order as i32), 0.0);
    }
    gain *= num.div(den).0;

    // Pair conjugates; order sections so poles nearest the unit circle come last (scipy convention).
    let mut pairs: Vec<(C, bool)> = zpoles.iter().filter(|p| p.1 > 1e-14).map(|&p| (p, true)).collect();
    if order % 2 == 1 {
        let real = zpoles.iter().find(|p| p.1.abs() <= 1e-14).copied().unwrap_or(C(0.0, 0.0));
        pairs.push((real, false));
    }
    pairs.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
    let mut sos: Vec<Section> = pairs
        .iter()
        .map(|&(p, complex)| {
            if complex {
                [1.0, -2.0 * zero.0, 1.0, 1.0, -2.0 * p.0, p.0 * p.0 + p.1 * p.1]
            } else {
                [1.0, -zero.0, 0.0, 1.0, -p.0, 0.0]
            }
        })
        .collect();
    for c in &mut sos[0][..3] {
        *c *= gain;
    }
    sos
}

/// Steady-state initial conditions of each section for a unit step (scipy `sosfilt_zi`).
fn sos_zi(sos: &[Section]) -> Vec<[f64; 2]> {
    let mut scale = 1.0;
    sos.iter()
        .map(|s| {
            let g = (s[0] + s[1] + s[2]) / (1.0 + s[4] + s[5]);
            let z1 = s[2] - s[5] * g;
            let z0 = s[1] - s[4] * g + z1;
            let zi = [scale * z0, scale * z1];
            scale *= g;
            zi
        })
        .collect()
}

/// In-place cascade filter (transposed direct form II) with initial states `zi * x0`.
fn sosfilt(sos: &[Section], zi: &[[f64; 2]], x: &mut [f64]) {
    let x0 = x[0];
    for (s, z) in sos.iter().zip(zi) {
        let (mut z0, mut z1) = (z[0] * x0, z[1] * x0);
        for v in x.iter_mut() {
            let xi = *v;
            let y = s[0] * xi + z0;
            z0 = s[1] * xi - s[4] * y + z1;
            z1 = s[2] * xi - s[5] * y;
            *v = y;
        }
    }
}

/// Zero-phase filtering (scipy `sosfiltfilt`, `padtype="odd"`, default padlen).
/// Inputs shorter than the pad length are filtered with the longest pad that fits.
pub fn sosfiltfilt(sos: &[Section], x: &[f64]) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return x.to_vec();
    }
    let trivial = sos.iter().filter(|s| s[2] == 0.0).count().min(sos.iter().filter(|s| s[5] == 0.0).count());
    let pad = (3 * (2 * sos.len() + 1 - trivial)).min(n - 1);
    let (a, b) = (x[0], x[n - 1]);
    let mut ext = Vec::with_capacity(n + 2 * pad);
    ext.extend((1..=pad).rev().map(|i| 2.0 * a - x[i]));
    ext.extend_from_slice(x);
    ext.extend((1..=pad).map(|i| 2.0 * b - x[n - 1 - i]));
    let zi = sos_zi(sos);
    sosfilt(sos, &zi, &mut ext);
    ext.reverse();
    sosfilt(sos, &zi, &mut ext);
    ext.reverse();
    ext[pad..pad + n].to_vec()
}

/// Integer-factor decimation by block averaging (boxcar anti-alias + downsample).
/// Each output sample is the mean of `factor` inputs; a trailing partial block is dropped.
/// The output sample `k` is centred at input index `k*factor + (factor-1)/2`.
pub fn block_mean(x: &[f64], factor: usize) -> Vec<f64> {
    assert!(factor > 0);
    x.chunks_exact(factor).map(|c| c.iter().sum::<f64>() / factor as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden values from scipy 1.x: butter(4, 6, 'low', fs=1017.2526, output='sos').
    const SOS_LP: [Section; 2] = [
        [1.1237191128818911e-07, 2.2474382257637821e-07, 1.1237191128818911e-07, 1.0, -1.9324763407221284, 0.9338041552235216],
        [1.0, 2.0, 1.0, 1.0, -1.9706845924922198, 0.9720386600812727],
    ];

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    #[test]
    fn butter_matches_scipy() {
        let sos = butter(4, 6.0, 1017.2526, Band::Low);
        for (s, g) in sos.iter().zip(SOS_LP.iter()) {
            for (a, b) in s.iter().zip(g) {
                assert!(close(*a, *b, 1e-9), "{a} vs {b}");
            }
        }
        let hp = butter(2, 0.01, 20.0, Band::High);
        let g = [0.9977810241029409, -1.9955620482058818, 0.9977810241029409, 1.0, -1.995557124345789, 0.9955669720659747];
        for (a, b) in hp[0].iter().zip(g) {
            assert!(close(*a, b, 1e-9), "{a} vs {b}");
        }
        assert_eq!(butter(3, 1.0, 100.0, Band::Low).len(), 2);
    }

    #[test]
    fn zi_matches_scipy() {
        let zi = sos_zi(&SOS_LP);
        let g = [[0.00033840452535195074, -0.00031599611336649824], [0.999661483102795, -0.9717001431840663]];
        for (z, e) in zi.iter().zip(g) {
            assert!(close(z[0], e[0], 1e-9) && close(z[1], e[1], 1e-9));
        }
    }

    #[test]
    fn filtfilt_matches_scipy() {
        let fs = 1017.2526;
        let x: Vec<f64> = (0..3000)
            .map(|i| {
                let t = i as f64 / fs;
                (2.0 * PI * t).sin() + 0.5 * (2.0 * PI * 40.0 * t).sin() + 0.1 * t
            })
            .collect();
        let y = sosfiltfilt(&butter(4, 6.0, fs, Band::Low), &x);
        let want = [(0, 0.08365340769109528), (1, 0.0850669526563597), (100, 0.5852389192879525), (1500, 0.3066202832340995), (2999, -0.3326278374160964)];
        for (i, v) in want {
            assert!(close(y[i], v, 1e-8), "y[{i}]={} want {v}", y[i]);
        }
    }

    #[test]
    fn block_mean_drops_tail() {
        assert_eq!(block_mean(&[1.0, 3.0, 5.0, 7.0, 9.0], 2), vec![2.0, 6.0]);
    }
}
