//! Group (cohort) inference with the **animal as the experimental unit**
//! (methods-spec §7; Aarts et al. 2014 Nat Neurosci 17:491).
//!
//! Two-stage: trials are averaged within animal × condition × event, then
//! group statistics are computed across animals.
//! - Group curves: bootstrap CI of the mean of per-animal waveforms, expanded by
//!   √(n/(n−1)), with the consecutive-sample rule (Jean-Richard-dit-Bressel 2020).
//! - Within-subject contrasts (condition A − B): the same, on per-animal differences.
//! - Between-group contrasts: bootstrap CI of the difference of group means
//!   (animals resampled within each group), half-width expanded pointwise so the
//!   bootstrap variance matches the unbiased two-sample variance.
//! - Scalar metrics: per-animal means; hierarchical bootstrap CIs (Saravanan et
//!   al. 2020); exact or Monte-Carlo permutation tests (label shuffle between
//!   groups, sign flips within subjects); Benjamini–Hochberg q across all tests.

use crate::events::{self, MeanCi, Metrics, min_consecutive};
use crate::stats::{self, Rng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Minimum animals per cell for a CI or test.
pub const MIN_ANIMALS: usize = 3;
const MC_PERMUTATIONS: usize = 20_000;

/// One recording's trial-level results for one event, plus who/what it was.
pub struct SessionEvent<'a> {
    pub session: &'a str,
    pub animal: &'a str,
    pub group: &'a str,
    pub condition: &'a str,
    pub event: &'a str,
    pub t: &'a [f64],
    pub trials: &'a [Vec<f64>],
    pub metrics: &'a [Metrics],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContrastSpec {
    /// `group[0] − group[1]` within one condition.
    Between { groups: [String; 2], condition: String },
    /// `condition[0] − condition[1]` within the same animals of one group.
    Within { conditions: [String; 2], group: String },
}

impl ContrastSpec {
    pub fn label(&self) -> String {
        match self {
            ContrastSpec::Between { groups, condition } => format!("{} − {} ({condition})", groups[0], groups[1]),
            ContrastSpec::Within { conditions, group } => format!("{} − {} ({group})", conditions[0], conditions[1]),
        }
    }
}

pub struct GroupOpts {
    pub n_boot: usize,
    pub ci_level: f64,
    pub min_run: usize,
    pub seed: u64,
}

/// Trial-averaged waveform and metrics of one animal in one cell.
#[derive(Debug, Clone, Serialize)]
pub struct AnimalCurve {
    pub event: String,
    pub animal: String,
    pub group: String,
    pub condition: String,
    pub n_sessions: usize,
    pub n_trials: usize,
    pub mean: Vec<f64>,
    pub metrics: MetricMeans,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct MetricMeans {
    pub mean: f64,
    pub auc: f64,
    pub peak: f64,
    pub trough: f64,
}

pub const METRICS: [&str; 4] = ["mean", "auc", "peak", "trough"];

fn metric(m: &Metrics, name: &str) -> f64 {
    match name {
        "mean" => m.mean,
        "auc" => m.auc,
        "peak" => m.peak,
        _ => m.trough,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupCurve {
    pub event: String,
    pub group: String,
    pub condition: String,
    pub n_animals: usize,
    pub mean: Vec<f64>,
    /// `None` with fewer than [`MIN_ANIMALS`].
    pub ci: Option<MeanCi>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContrastCurve {
    pub event: String,
    pub contrast: ContrastSpec,
    pub label: String,
    pub n: [usize; 2],
    pub diff: Vec<f64>,
    pub lo: Vec<f64>,
    pub hi: Vec<f64>,
    pub significant: Vec<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricSummary {
    pub event: String,
    pub group: String,
    pub condition: String,
    pub metric: String,
    pub n_animals: usize,
    pub mean: f64,
    /// Hierarchical bootstrap CI (animals, then trials within animal).
    pub lo: f64,
    pub hi: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricTest {
    pub event: String,
    pub contrast: String,
    pub metric: String,
    pub n: [usize; 2],
    pub estimate: f64,
    pub lo: f64,
    pub hi: f64,
    /// Two-sided permutation p (exact when feasible).
    pub p: f64,
    pub exact: bool,
    /// Benjamini–Hochberg adjusted p across every metric test in the analysis.
    pub q: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupAnalysis {
    pub t: Vec<f64>,
    pub animals: Vec<AnimalCurve>,
    pub groups: Vec<GroupCurve>,
    pub contrasts: Vec<ContrastCurve>,
    pub metric_summaries: Vec<MetricSummary>,
    pub metric_tests: Vec<MetricTest>,
    pub warnings: Vec<String>,
}

/// Linear interpolation of `(ts, y)` onto `t` (NaN outside `ts`).
fn resample(ts: &[f64], y: &[f64], t: &[f64]) -> Vec<f64> {
    t.iter()
        .map(|&x| {
            let j = ts.partition_point(|&v| v < x);
            if j == 0 {
                if (ts[0] - x).abs() < 1e-9 { y[0] } else { f64::NAN }
            } else if j >= ts.len() {
                f64::NAN
            } else {
                let f = (x - ts[j - 1]) / (ts[j] - ts[j - 1]);
                y[j - 1] * (1.0 - f) + y[j] * f
            }
        })
        .collect()
}

fn col_mean(rows: &[&[f64]], m: usize) -> Vec<f64> {
    let mut acc = vec![0.0; m];
    for r in rows {
        acc.iter_mut().zip(r.iter()).for_each(|(a, v)| *a += v);
    }
    acc.iter_mut().for_each(|a| *a /= rows.len() as f64);
    acc
}

/// Per-replicate deterministic RNG so results do not depend on thread count.
fn rng_for(seed: u64, b: usize) -> Rng {
    Rng::new(seed ^ (b as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Bootstrap CI of the difference of two independent means, pointwise.
fn diff_ci(a: &[Vec<f64>], b: &[Vec<f64>], o: &GroupOpts) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<bool>) {
    let m = a[0].len();
    let (na, nb) = (a.len(), b.len());
    let ra: Vec<&[f64]> = a.iter().map(Vec::as_slice).collect();
    let rb: Vec<&[f64]> = b.iter().map(Vec::as_slice).collect();
    let (ma, mb) = (col_mean(&ra, m), col_mean(&rb, m));
    let diff: Vec<f64> = ma.iter().zip(&mb).map(|(x, y)| x - y).collect();
    let boots: Vec<Vec<f64>> = (0..o.n_boot)
        .into_par_iter()
        .map(|k| {
            let mut rng = rng_for(o.seed, k);
            let sa: Vec<&[f64]> = (0..na).map(|_| ra[rng.below(na)]).collect();
            let sb: Vec<&[f64]> = (0..nb).map(|_| rb[rng.below(nb)]).collect();
            col_mean(&sa, m).iter().zip(col_mean(&sb, m)).map(|(x, y)| x - y).collect()
        })
        .collect();
    let alpha = (1.0 - o.ci_level) / 2.0;
    let (lo, hi): (Vec<f64>, Vec<f64>) = (0..m)
        .into_par_iter()
        .map(|j| {
            let var = |rows: &[&[f64]]| stats::std(&rows.iter().map(|r| r[j]).collect::<Vec<_>>()).powi(2);
            let (va, vb) = (var(&ra), var(&rb));
            // Bootstrap variance of a mean is s²(n−1)/n²; scale it back to s²/n.
            let unbiased = va / na as f64 + vb / nb as f64;
            let boot = va * (na as f64 - 1.0) / (na * na) as f64 + vb * (nb as f64 - 1.0) / (nb * nb) as f64;
            let expand = if boot > 0.0 { (unbiased / boot).sqrt() } else { 1.0 };
            let mut col: Vec<f64> = boots.iter().map(|b| b[j]).collect();
            let (l, u) = (stats::quantile_mut(&mut col, alpha), stats::quantile_mut(&mut col, 1.0 - alpha));
            let (mid, half) = ((l + u) / 2.0, (u - l) / 2.0 * expand);
            (mid - half, mid + half)
        })
        .unzip();
    let raw: Vec<bool> = lo.iter().zip(&hi).map(|(l, h)| *l > 0.0 || *h < 0.0).collect();
    (diff, lo, hi, min_consecutive(&raw, o.min_run))
}

/// Visit every `k`-subset of `0..n` (as a membership mask).
fn for_each_subset(n: usize, k: usize, mut f: impl FnMut(&[bool])) {
    let mut idx: Vec<usize> = (0..k).collect();
    let mut mask = vec![false; n];
    loop {
        mask.iter_mut().for_each(|m| *m = false);
        idx.iter().for_each(|&i| mask[i] = true);
        f(&mask);
        let Some(i) = (0..k).rev().find(|&i| idx[i] != i + n - k) else { return };
        idx[i] += 1;
        for j in i + 1..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

fn binom(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// Two-sided permutation p for a difference of means between two groups.
fn perm_between(a: &[f64], b: &[f64], seed: u64) -> (f64, bool) {
    let all: Vec<f64> = a.iter().chain(b).copied().collect();
    let (n, k) = (all.len(), a.len());
    let total: f64 = all.iter().sum();
    let stat = |sum_a: f64| (sum_a / k as f64 - (total - sum_a) / (n - k) as f64).abs();
    let obs = stat(a.iter().sum());
    let tol = 1e-12 * (1.0 + obs);
    if binom(n, k) <= MC_PERMUTATIONS as f64 {
        let (mut hits, mut count) = (0usize, 0usize);
        for_each_subset(n, k, |mask| {
            let s: f64 = all.iter().zip(mask).filter(|(_, m)| **m).map(|(v, _)| v).sum();
            hits += (stat(s) >= obs - tol) as usize;
            count += 1;
        });
        (hits as f64 / count as f64, true)
    } else {
        let mut rng = Rng::new(seed);
        let mut idx: Vec<usize> = (0..n).collect();
        let mut hits = 0;
        for _ in 0..MC_PERMUTATIONS {
            for i in 0..k {
                let j = i + rng.below(n - i);
                idx.swap(i, j);
            }
            let s: f64 = idx[..k].iter().map(|&i| all[i]).sum();
            hits += (stat(s) >= obs - tol) as usize;
        }
        // +1 correction keeps Monte-Carlo p strictly positive (Phipson & Smyth 2010).
        ((hits + 1) as f64 / (MC_PERMUTATIONS + 1) as f64, false)
    }
}

/// Two-sided sign-flip permutation p for paired differences.
fn perm_paired(d: &[f64], seed: u64) -> (f64, bool) {
    let n = d.len();
    let obs = stats::mean(d).abs();
    let tol = 1e-12 * (1.0 + obs);
    if n <= 14 {
        let total = 1usize << n;
        let hits = (0..total)
            .filter(|bits| {
                let s: f64 = d.iter().enumerate().map(|(i, v)| if bits >> i & 1 == 1 { -v } else { *v }).sum();
                (s / n as f64).abs() >= obs - tol
            })
            .count();
        (hits as f64 / total as f64, true)
    } else {
        let mut rng = Rng::new(seed);
        let hits = (0..MC_PERMUTATIONS)
            .filter(|_| {
                let s: f64 = d.iter().map(|v| if rng.below(2) == 0 { -v } else { *v }).sum();
                (s / n as f64).abs() >= obs - tol
            })
            .count();
        ((hits + 1) as f64 / (MC_PERMUTATIONS + 1) as f64, false)
    }
}

/// Benjamini–Hochberg adjusted p-values (step-up, monotone).
pub fn bh(p: &[f64]) -> Vec<f64> {
    let n = p.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
    let mut q = vec![0.0; n];
    let mut running: f64 = 1.0;
    for (rank_from_top, &i) in order.iter().enumerate() {
        let rank = n - rank_from_top;
        running = running.min(p[i] * n as f64 / rank as f64);
        q[i] = running.min(1.0);
    }
    q
}

fn percentile_ci(mut v: Vec<f64>, level: f64) -> (f64, f64) {
    let a = (1.0 - level) / 2.0;
    (stats::quantile_mut(&mut v, a), stats::quantile_mut(&mut v, 1.0 - a))
}

/// Hierarchical bootstrap of a mean: resample animals, then trials within each.
fn hier_boot(trials: &[Vec<f64>], o: &GroupOpts, salt: u64) -> (f64, f64) {
    let n = trials.len();
    let boots: Vec<f64> = (0..o.n_boot)
        .into_par_iter()
        .map(|k| {
            let mut rng = rng_for(o.seed ^ salt, k);
            (0..n)
                .map(|_| {
                    let a = &trials[rng.below(n)];
                    (0..a.len()).map(|_| a[rng.below(a.len())]).sum::<f64>() / a.len() as f64
                })
                .sum::<f64>()
                / n as f64
        })
        .collect();
    percentile_ci(boots, o.ci_level)
}

/// Simple bootstrap CI of a mean (or a difference of two means when `b` is given).
fn boot_mean_diff(a: &[f64], b: Option<&[f64]>, o: &GroupOpts, salt: u64) -> (f64, f64) {
    let draw = |x: &[f64], rng: &mut Rng| (0..x.len()).map(|_| x[rng.below(x.len())]).sum::<f64>() / x.len() as f64;
    let boots: Vec<f64> = (0..o.n_boot)
        .into_par_iter()
        .map(|k| {
            let mut rng = rng_for(o.seed ^ salt, k);
            draw(a, &mut rng) - b.map_or(0.0, |b| draw(b, &mut rng))
        })
        .collect();
    percentile_ci(boots, o.ci_level)
}

fn salt(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

pub fn analyze(sessions: &[SessionEvent], contrasts: &[ContrastSpec], o: &GroupOpts) -> Result<GroupAnalysis, String> {
    let Some(first) = sessions.iter().find(|s| !s.trials.is_empty()) else {
        return Err("no session contributed any trials".into());
    };
    let t = first.t.to_vec();
    let m = t.len();
    let mut warnings = vec![];

    // Stage 1: pool trials per (event, animal, condition). Grids that differ
    // (e.g. a rig at another sample rate) are interpolated onto the first.
    type Key = (String, String, String);
    struct Cell {
        group: String,
        sessions: Vec<String>,
        rows: Vec<Vec<f64>>,
        metrics: Vec<Metrics>,
    }
    let mut cells: BTreeMap<Key, Cell> = BTreeMap::new();
    for s in sessions.iter().filter(|s| !s.trials.is_empty()) {
        let key = (s.event.to_string(), s.animal.to_string(), s.condition.to_string());
        let cell = cells.entry(key).or_insert_with(|| Cell { group: s.group.to_string(), sessions: vec![], rows: vec![], metrics: vec![] });
        if cell.group != s.group {
            return Err(format!("animal '{}' is listed in groups '{}' and '{}'; an animal belongs to one group", s.animal, cell.group, s.group));
        }
        cell.sessions.push(s.session.to_string());
        let same = s.t.len() == m && s.t.iter().zip(&t).all(|(a, b)| (a - b).abs() < 1e-9);
        for r in s.trials {
            let row = if same { r.clone() } else { resample(s.t, r, &t) };
            if row.iter().any(|v| v.is_nan()) {
                return Err(format!("session '{}' epochs do not cover the cohort epoch window; use one preset for the cohort", s.session));
            }
            cell.rows.push(row);
        }
        cell.metrics.extend_from_slice(s.metrics);
    }
    let animals: Vec<AnimalCurve> = cells
        .iter()
        .map(|((event, animal, condition), c)| {
            let rows: Vec<&[f64]> = c.rows.iter().map(Vec::as_slice).collect();
            let avg = |f: &dyn Fn(&Metrics) -> f64| c.metrics.iter().map(f).sum::<f64>() / c.metrics.len() as f64;
            AnimalCurve {
                event: event.clone(),
                animal: animal.clone(),
                group: c.group.clone(),
                condition: condition.clone(),
                n_sessions: c.sessions.len(),
                n_trials: c.rows.len(),
                mean: col_mean(&rows, m),
                metrics: MetricMeans { mean: avg(&|x| x.mean), auc: avg(&|x| x.auc), peak: avg(&|x| x.peak), trough: avg(&|x| x.trough) },
            }
        })
        .collect();

    // Stage 2: per (event, group, condition) curves.
    let mut by_cell: BTreeMap<(String, String, String), Vec<usize>> = BTreeMap::new();
    for (i, a) in animals.iter().enumerate() {
        by_cell.entry((a.event.clone(), a.group.clone(), a.condition.clone())).or_default().push(i);
    }
    let mut groups = vec![];
    let mut metric_summaries = vec![];
    for ((event, group, condition), idx) in &by_cell {
        let rows: Vec<Vec<f64>> = idx.iter().map(|&i| animals[i].mean.clone()).collect();
        let refs: Vec<&[f64]> = rows.iter().map(Vec::as_slice).collect();
        let n = rows.len();
        if n < MIN_ANIMALS {
            warnings.push(format!("{event} · {group} · {condition}: {n} animal(s) < {MIN_ANIMALS}; no CI or tests"));
        }
        let ci = (n >= MIN_ANIMALS).then(|| events::bootstrap_mean_ci(&rows, o.n_boot, o.ci_level, o.min_run, o.seed));
        groups.push(GroupCurve { event: event.clone(), group: group.clone(), condition: condition.clone(), n_animals: n, mean: col_mean(&refs, m), ci });
        for name in METRICS {
            let trials: Vec<Vec<f64>> = idx
                .iter()
                .map(|&i| {
                    let a = &animals[i];
                    cells[&(a.event.clone(), a.animal.clone(), a.condition.clone())].metrics.iter().map(|x| metric(x, name)).collect()
                })
                .collect();
            let means: Vec<f64> = trials.iter().map(|v| stats::mean(v)).collect();
            let (lo, hi) = if n >= MIN_ANIMALS { hier_boot(&trials, o, salt(&format!("{event}{group}{condition}{name}"))) } else { (f64::NAN, f64::NAN) };
            metric_summaries.push(MetricSummary { event: event.clone(), group: group.clone(), condition: condition.clone(), metric: name.into(), n_animals: n, mean: stats::mean(&means), lo, hi });
        }
    }

    // Contrasts: explicit, or all pairwise groups per condition plus all condition pairs per group.
    let event_names: Vec<String> = by_cell.keys().map(|k| k.0.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let specs: Vec<ContrastSpec> = if contrasts.is_empty() { default_contrasts(&animals) } else { contrasts.to_vec() };
    let mut curves = vec![];
    let mut tests = vec![];
    for event in &event_names {
        for spec in &specs {
            let pick = |g: &str, c: &str| -> Vec<&AnimalCurve> { animals.iter().filter(|a| &a.event == event && a.group == g && a.condition == c).collect() };
            match spec {
                ContrastSpec::Between { groups: [ga, gb], condition } => {
                    let (a, b) = (pick(ga, condition), pick(gb, condition));
                    if a.len() < MIN_ANIMALS || b.len() < MIN_ANIMALS {
                        warnings.push(format!("{event} · {}: needs ≥ {MIN_ANIMALS} animals per side (have {}, {})", spec.label(), a.len(), b.len()));
                        continue;
                    }
                    let ra: Vec<Vec<f64>> = a.iter().map(|x| x.mean.clone()).collect();
                    let rb: Vec<Vec<f64>> = b.iter().map(|x| x.mean.clone()).collect();
                    let (diff, lo, hi, significant) = diff_ci(&ra, &rb, o);
                    curves.push(ContrastCurve { event: event.clone(), contrast: spec.clone(), label: spec.label(), n: [a.len(), b.len()], diff, lo, hi, significant });
                    for name in METRICS {
                        let va: Vec<f64> = a.iter().map(|x| metric_of(x, name)).collect();
                        let vb: Vec<f64> = b.iter().map(|x| metric_of(x, name)).collect();
                        let s = salt(&format!("{event}{}{name}", spec.label()));
                        let (p, exact) = perm_between(&va, &vb, o.seed ^ s);
                        let (lo, hi) = boot_mean_diff(&va, Some(&vb), o, s);
                        tests.push(MetricTest { event: event.clone(), contrast: spec.label(), metric: name.into(), n: [va.len(), vb.len()], estimate: stats::mean(&va) - stats::mean(&vb), lo, hi, p, exact, q: f64::NAN });
                    }
                }
                ContrastSpec::Within { conditions: [ca, cb], group } => {
                    let (a, b) = (pick(group, ca), pick(group, cb));
                    let pairs: Vec<(&AnimalCurve, &AnimalCurve)> = a.iter().filter_map(|x| b.iter().find(|y| y.animal == x.animal).map(|y| (*x, *y))).collect();
                    if pairs.len() < MIN_ANIMALS {
                        warnings.push(format!("{event} · {}: {} animal(s) recorded in both conditions < {MIN_ANIMALS}", spec.label(), pairs.len()));
                        continue;
                    }
                    let unpaired = a.len() + b.len() - 2 * pairs.len();
                    if unpaired > 0 {
                        warnings.push(format!("{event} · {}: {unpaired} animal(s) without both conditions excluded from the paired contrast", spec.label()));
                    }
                    let rows: Vec<Vec<f64>> = pairs.iter().map(|(x, y)| x.mean.iter().zip(&y.mean).map(|(p, q)| p - q).collect()).collect();
                    let ci = events::bootstrap_mean_ci(&rows, o.n_boot, o.ci_level, o.min_run, o.seed);
                    curves.push(ContrastCurve { event: event.clone(), contrast: spec.clone(), label: spec.label(), n: [pairs.len(), pairs.len()], diff: ci.mean, lo: ci.lo, hi: ci.hi, significant: ci.significant });
                    for name in METRICS {
                        let d: Vec<f64> = pairs.iter().map(|(x, y)| metric_of(x, name) - metric_of(y, name)).collect();
                        let s = salt(&format!("{event}{}{name}", spec.label()));
                        let (p, exact) = perm_paired(&d, o.seed ^ s);
                        let (lo, hi) = boot_mean_diff(&d, None, o, s);
                        tests.push(MetricTest { event: event.clone(), contrast: spec.label(), metric: name.into(), n: [d.len(), d.len()], estimate: stats::mean(&d), lo, hi, p, exact, q: f64::NAN });
                    }
                }
            }
        }
    }
    let q = bh(&tests.iter().map(|x| x.p).collect::<Vec<_>>());
    tests.iter_mut().zip(q).for_each(|(x, q)| x.q = q);
    Ok(GroupAnalysis { t, animals, groups, contrasts: curves, metric_summaries, metric_tests: tests, warnings })
}

fn metric_of(a: &AnimalCurve, name: &str) -> f64 {
    match name {
        "mean" => a.metrics.mean,
        "auc" => a.metrics.auc,
        "peak" => a.metrics.peak,
        _ => a.metrics.trough,
    }
}

fn default_contrasts(animals: &[AnimalCurve]) -> Vec<ContrastSpec> {
    let set = |f: fn(&AnimalCurve) -> &String| animals.iter().map(f).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    let (groups, conds) = (set(|a| &a.group), set(|a| &a.condition));
    let mut out = vec![];
    for c in &conds {
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                out.push(ContrastSpec::Between { groups: [groups[i].clone(), groups[j].clone()], condition: c.clone() });
            }
        }
    }
    for g in &groups {
        for i in 0..conds.len() {
            for j in i + 1..conds.len() {
                out.push(ContrastSpec::Within { conditions: [conds[i].clone(), conds[j].clone()], group: g.clone() });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bh_matches_r() {
        // R: p.adjust(c(0.01, 0.04, 0.03, 0.2), "BH") = 0.04 0.0533 0.0533 0.20
        let q = bh(&[0.01, 0.04, 0.03, 0.2]);
        for (a, b) in q.iter().zip([0.04, 0.16 / 3.0, 0.16 / 3.0, 0.2]) {
            assert!((a - b).abs() < 1e-12, "{q:?}");
        }
    }

    #[test]
    fn exact_permutations() {
        // All 3-of-6 splits: only the observed split and its mirror reach |Δ| = 3.
        let (p, exact) = perm_between(&[4.0, 5.0, 6.0], &[1.0, 2.0, 3.0], 0);
        assert!(exact && (p - 2.0 / 20.0).abs() < 1e-12, "{p}");
        // Sign flips of 4 equal positive differences: only all + or all − reach the mean.
        let (p, exact) = perm_paired(&[1.0, 1.0, 1.0, 1.0], 0);
        assert!(exact && (p - 2.0 / 16.0).abs() < 1e-12, "{p}");
        let mut c = 0;
        for_each_subset(5, 2, |_| c += 1);
        assert_eq!(c, 10);
    }

    #[test]
    fn diff_ci_expansion_matches_two_sample_variance() {
        let mut rng = Rng::new(9);
        let mut rows = |n: usize, mu: f64| -> Vec<Vec<f64>> { (0..n).map(|_| vec![mu + rng.below(1000) as f64 / 1000.0 - 0.5]).collect() };
        let (a, b) = (rows(5, 1.0), rows(6, 0.0));
        let o = GroupOpts { n_boot: 20_000, ci_level: 0.95, min_run: 1, seed: 3 };
        let (d, lo, hi, _) = diff_ci(&a, &b, &o);
        let col = |r: &[Vec<f64>]| r.iter().map(|v| v[0]).collect::<Vec<_>>();
        let se = (stats::std(&col(&a)).powi(2) / 5.0 + stats::std(&col(&b)).powi(2) / 6.0).sqrt();
        // Expanded half-width ≈ 1.96 · unbiased SE (normal approximation, loose tolerance).
        let half = (hi[0] - lo[0]) / 2.0;
        assert!((half / (1.96 * se) - 1.0).abs() < 0.15, "half {half} vs {}", 1.96 * se);
        assert!(lo[0] < d[0] && d[0] < hi[0]);
    }
}
