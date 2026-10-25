//! Regression and curve fitting used for motion and photobleaching correction.

use crate::stats::{mad, median};

/// Least-squares line `y ≈ slope * x + intercept`, optionally weighted.
pub fn line(x: &[f64], y: &[f64], w: Option<&[f64]>) -> (f64, f64) {
    let wt = |i: usize| w.map_or(1.0, |w| w[i]);
    let (mut sw, mut sx, mut sy) = (0.0, 0.0, 0.0);
    for i in 0..x.len() {
        sw += wt(i);
        sx += wt(i) * x[i];
        sy += wt(i) * y[i];
    }
    let (mx, my) = (sx / sw, sy / sw);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for i in 0..x.len() {
        let dx = x[i] - mx;
        sxx += wt(i) * dx * dx;
        sxy += wt(i) * dx * (y[i] - my);
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (slope, my - slope * mx)
}

/// Tukey bisquare weights for residuals `r` with tuning constant `c`
/// (4.685 gives 95% efficiency under Gaussian noise). Scale is the normalized MAD.
pub fn bisquare_weights(r: &[f64], c: f64) -> Vec<f64> {
    // Floor the scale so a near-perfect fit (MAD ≈ 0) still rejects gross outliers.
    let peak = r.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    let s = (1.4826 * mad(r)).max(1e-9 * peak);
    if s == 0.0 {
        return vec![1.0; r.len()];
    }
    r.iter()
        .map(|&ri| {
            let u = ri / (c * s);
            if u.abs() < 1.0 { (1.0 - u * u).powi(2) } else { 0.0 }
        })
        .collect()
}

/// Result of a (robust) line fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineFit {
    pub slope: f64,
    pub intercept: f64,
    /// Fraction of samples the final IRLS pass gave zero weight (0 for OLS).
    pub zero_weight_frac: f64,
}

/// Robust line fit by iteratively reweighted least squares with bisquare weights
/// (Holland & Welsch 1977). Downweights large transients so the fit tracks shared
/// (artifact) variance only. Lower `c` rejects more aggressively; Keevers &
/// Jean-Richard-dit-Bressel (2025) found c = 1.4 most accurate for photometry.
pub fn robust_line(x: &[f64], y: &[f64], c: f64, max_iter: usize) -> LineFit {
    let (mut m, mut b) = line(x, y, None);
    // Re-centre the OLS start on the median residual: with an aggressive c (1.4),
    // an intercept biased by large transients can otherwise push *every* clean
    // sample outside the bisquare support and stall IRLS at the OLS answer.
    b += median(&x.iter().zip(y).map(|(xi, yi)| yi - (m * xi + b)).collect::<Vec<_>>());
    let mut zero = 0.0;
    for _ in 0..max_iter {
        let r: Vec<f64> = x.iter().zip(y).map(|(xi, yi)| yi - (m * xi + b)).collect();
        let w = bisquare_weights(&r, c);
        zero = w.iter().filter(|&&v| v == 0.0).count() as f64 / w.len() as f64;
        let (m2, b2) = line(x, y, Some(&w));
        let done = (m2 - m).abs().max((b2 - b).abs()) <= 1e-6 * (m.abs().max(b.abs()) + 1e-12);
        (m, b) = (m2, b2);
        if done {
            break;
        }
    }
    LineFit { slope: m, intercept: b, zero_weight_frac: zero }
}

/// Solve the symmetric positive-definite system `a x = b` in place (Cholesky).
/// Returns `false` if `a` is not positive definite.
fn cholesky_solve<const N: usize>(a: &mut [[f64; N]; N], b: &mut [f64; N]) -> bool {
    for j in 0..N {
        let mut d = a[j][j];
        for k in 0..j {
            d -= a[j][k] * a[j][k];
        }
        if d <= 0.0 || !d.is_finite() {
            return false;
        }
        a[j][j] = d.sqrt();
        for i in j + 1..N {
            let mut s = a[i][j];
            for k in 0..j {
                s -= a[i][k] * a[j][k];
            }
            a[i][j] = s / a[j][j];
        }
    }
    for i in 0..N {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i][k] * b[k];
        }
        b[i] = s / a[i][i];
    }
    for i in (0..N).rev() {
        let mut s = b[i];
        for k in i + 1..N {
            s -= a[k][i] * b[k];
        }
        b[i] = s / a[i][i];
    }
    true
}

/// Weighted Levenberg–Marquardt for `N` parameters.
/// `model(p, i) -> (prediction, gradient)` evaluates sample `i`.
pub fn levenberg_marquardt<const N: usize>(
    y: &[f64],
    w: &[f64],
    mut p: [f64; N],
    max_iter: usize,
    model: impl Fn(&[f64; N], usize) -> (f64, [f64; N]),
) -> [f64; N] {
    let cost = |p: &[f64; N]| (0..y.len()).map(|i| w[i] * (y[i] - model(p, i).0).powi(2)).sum::<f64>();
    let mut lambda = 1e-3;
    let mut c = cost(&p);
    for _ in 0..max_iter {
        let mut jtj = [[0.0; N]; N];
        let mut jtr = [0.0; N];
        for i in 0..y.len() {
            let (f, g) = model(&p, i);
            let r = y[i] - f;
            for a in 0..N {
                jtr[a] += w[i] * g[a] * r;
                for b in 0..=a {
                    jtj[a][b] += w[i] * g[a] * g[b];
                }
            }
        }
        let mut improved = false;
        while lambda < 1e12 {
            let mut m = jtj;
            for a in 0..N {
                for b in 0..a {
                    m[b][a] = m[a][b];
                }
                m[a][a] *= 1.0 + lambda;
                m[a][a] += 1e-300;
            }
            let mut step = jtr;
            if cholesky_solve(&mut m, &mut step) {
                let mut q = p;
                q.iter_mut().zip(step).for_each(|(qi, s)| *qi += s);
                let cq = cost(&q);
                if cq.is_finite() && cq <= c {
                    let rel = (c - cq) / c.max(f64::MIN_POSITIVE);
                    p = q;
                    c = cq;
                    lambda = (lambda / 10.0).max(1e-12);
                    improved = rel > 1e-12;
                    break;
                }
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    p
}

/// Fitted double exponential `a·e^(−t/τ₁) + b·e^(−t/τ₂) + c` with `τ₁ ≤ τ₂`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiExp {
    pub a: f64,
    pub tau1: f64,
    pub b: f64,
    pub tau2: f64,
    pub c: f64,
}

impl BiExp {
    pub fn eval(&self, t: f64) -> f64 {
        self.a * (-t / self.tau1).exp() + self.b * (-t / self.tau2).exp() + self.c
    }
}

/// Fit a double exponential decay to `y(t)`, as used for photobleaching.
///
/// Time constants are optimised in log space so they stay positive. Amplitudes
/// are constrained ≥ 0 by fitting their square roots, which keeps the curve
/// monotonically decaying (it cannot bend up to absorb a sustained real signal
/// increase). With `robust`, bisquare IRLS downweights transients.
pub fn fit_biexp(t: &[f64], y: &[f64], robust: bool) -> BiExp {
    let n = y.len();
    let span = (t[n - 1] - t[0]).max(f64::EPSILON);
    let tail = median(&y[n - n / 10..]);
    let head = median(&y[..(n / 50).max(1)]);
    let amp = (head - tail).max(0.0);
    // p = [√a, ln τ1, √b, ln τ2, c]
    let p0 = [(0.5 * amp).sqrt(), (span / 20.0).ln(), (0.5 * amp).sqrt(), (span / 2.0).ln(), tail];
    let model = |p: &[f64; 5], i: usize| {
        let (ta, tb) = (p[1].exp(), p[3].exp());
        let (ea, eb) = ((-(t[i] - t[0]) / ta).exp(), (-(t[i] - t[0]) / tb).exp());
        let (a, b) = (p[0] * p[0], p[2] * p[2]);
        let dt = t[i] - t[0];
        (a * ea + b * eb + p[4], [2.0 * p[0] * ea, a * ea * dt / ta, 2.0 * p[2] * eb, b * eb * dt / tb, 1.0])
    };
    let mut w = vec![1.0; n];
    let mut p = levenberg_marquardt(y, &w, p0, 200, model);
    if robust {
        for _ in 0..10 {
            let r: Vec<f64> = (0..n).map(|i| y[i] - model(&p, i).0).collect();
            w = bisquare_weights(&r, 4.685);
            p = levenberg_marquardt(y, &w, p, 100, model);
        }
    }
    let (mut f1, mut f2) = ((p[0] * p[0], p[1].exp()), (p[2] * p[2], p[3].exp()));
    if f1.1 > f2.1 {
        std::mem::swap(&mut f1, &mut f2);
    }
    // Model is in t − t₀; fold the shift into amplitudes so eval() takes absolute t.
    let s = |amp: f64, tau: f64| amp * (t[0] / tau).exp();
    BiExp { a: s(f1.0, f1.1), tau1: f1.1, b: s(f2.0, f2.1), tau2: f2.1, c: p[4] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_exact() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [1.0, 3.0, 5.0, 7.0];
        let (m, b) = line(&x, &y, None);
        assert!((m - 2.0).abs() < 1e-12 && (b - 1.0).abs() < 1e-12);
    }

    #[test]
    fn robust_line_ignores_transients() {
        let mut rng = crate::stats::Rng::new(1);
        let mut noise = || (rng.below(1_000_000) as f64 / 1e6 - 0.5) * 0.2;
        let x: Vec<f64> = (0..2000).map(|i| (i as f64 * 0.37).sin()).collect();
        let mut y: Vec<f64> = x.iter().map(|v| 0.8 * v + 2.0 + noise()).collect();
        y[100..200].iter_mut().for_each(|v| *v += 5.0); // a big "real" transient
        for c in [1.4, 4.685] {
            let f = robust_line(&x, &y, c, 50);
            assert!((f.slope - 0.8).abs() < 0.01 && (f.intercept - 2.0).abs() < 0.01, "c={c} {f:?}");
            assert!(f.zero_weight_frac >= 0.04, "transient samples should be rejected: {f:?}");
        }
        let m = robust_line(&x, &y, 1.4, 50).slope;
        let (m_ols, _) = line(&x, &y, None);
        assert!((m_ols - 0.8).abs() > (m - 0.8).abs());
    }

    #[test]
    fn biexp_recovers_parameters() {
        let truth = BiExp { a: 3.0, tau1: 40.0, b: 2.0, tau2: 900.0, c: 10.0 };
        let t: Vec<f64> = (0..20 * 1800).map(|i| 5.0 + i as f64 / 20.0).collect();
        let y: Vec<f64> = t.iter().enumerate().map(|(i, &ti)| truth.eval(ti) + 0.01 * ((i * 7919) % 13) as f64 / 13.0 - 0.005).collect();
        let f = fit_biexp(&t, &y, true);
        for &ti in &[5.0, 60.0, 600.0, 1800.0] {
            assert!((f.eval(ti) - truth.eval(ti)).abs() < 0.01, "t={ti} {f:?}");
        }
        assert!((f.tau1 - 40.0).abs() / 40.0 < 0.05 && (f.tau2 - 900.0).abs() / 900.0 < 0.05, "{f:?}");
    }
}
