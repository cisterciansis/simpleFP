//! Descriptive statistics. NaN-free inputs are assumed; callers mask first.

pub fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

/// Sample standard deviation (n − 1).
pub fn std(x: &[f64]) -> f64 {
    let m = mean(x);
    (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (x.len() as f64 - 1.0)).sqrt()
}

/// Linear-interpolated quantile (type 7, numpy/R default), `q` in [0, 1].
pub fn quantile(x: &[f64], q: f64) -> f64 {
    let mut v = x.to_vec();
    quantile_mut(&mut v, q)
}

/// Like [`quantile`] but reorders `v` instead of copying.
pub fn quantile_mut(v: &mut [f64], q: f64) -> f64 {
    let h = (v.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    let (_, &mut a, rest) = v.select_nth_unstable_by(lo, f64::total_cmp);
    if rest.is_empty() || h == lo as f64 {
        return a;
    }
    let b = rest.iter().copied().fold(f64::INFINITY, f64::min);
    a + (h - lo as f64) * (b - a)
}

pub fn median(x: &[f64]) -> f64 {
    quantile(x, 0.5)
}

/// Median absolute deviation from the median (unscaled; ×1.4826 ≈ σ for Gaussian data).
pub fn mad(x: &[f64]) -> f64 {
    let m = median(x);
    let mut d: Vec<f64> = x.iter().map(|v| (v - m).abs()).collect();
    quantile_mut(&mut d, 0.5)
}

/// Pearson correlation.
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let (mx, my) = (mean(x), mean(y));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
    }
    sxy / (sxx * syy).sqrt()
}

/// Small, fast, seedable PRNG (SplitMix64) so resampling is reproducible from a seed.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform integer in `0..n` (Lemire's multiply-shift; bias < n/2⁶⁴).
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next_u64() as u128 * n as u128) >> 64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_match_numpy() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        assert_eq!(median(&x), 3.5);
        assert!((quantile(&x, 0.25) - 1.75).abs() < 1e-12);
        assert_eq!(quantile(&x, 1.0), 9.0);
        assert_eq!(quantile(&x, 0.0), 1.0);
        assert_eq!(mad(&[1.0, 2.0, 3.0, 4.0, 100.0]), 1.0);
    }

    #[test]
    fn rng_is_deterministic_and_bounded() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..1000 {
            let v = a.below(10);
            assert_eq!(v, b.below(10));
            assert!(v < 10);
        }
    }
}
