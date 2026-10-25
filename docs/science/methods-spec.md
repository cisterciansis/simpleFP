# simpleFP methods specification (science reference for the Rust engine)

Status: v1, 2026-10-25. Scope: TDT RZ/Synapse fiber photometry, 465 nm signal + 405 nm control,
GRAB-ACh3.0 (primary) and GCaMP6/7/8 (secondary), event-locked analysis of behavior (feeding, approach).

Conventions:
- `[n]` refers to the numbered references at the end. Every reference was checked against Crossref,
  the publisher page, or the authors' code on 2026-10-25.
- **UNCERTAIN** marks claims inferred or not verified from a primary source.
- **JUDGMENT** marks defaults chosen by us where the literature gives a range and no single value.
- Symbols: `S(t)` raw signal (465 nm excitation), `C(t)` raw control (405 nm), `fs` sample rate,
  `fc` low-pass cutoff, `F0(t)` baseline fluorescence, dF/F in percent unless stated.

---

## 0. Pipeline order (normative)

```
load (TDT) → align clocks → trim → mask invalid → [despike] → low-pass → decimate
→ bleaching fit per channel → detrend → robust motion regression → dF/F
→ [residual-drift removal] → QC → epochs → per-trial baseline → metrics → statistics
```

Why this order:
1. **Mask invalid segments first.** Disconnections and saturation corrupt every fit downstream.
   Keevers & Jean-Richard-dit-Bressel make this the first preprocessing step [5].
2. **Low-pass before decimation.** The low-pass is the anti-alias filter. It also comes before the
   regression because filtering improves artifact correction (shown in simulation [5]) and reduces
   the noise in the predictor (control). Predictor noise biases an OLS or IRLS slope toward 0
   (regression dilution).
3. **Bleaching before motion, and independently of it.** Simpson et al. recommend correcting
   photobleaching first, as a step separate from movement correction [4]. The two
   channels bleach at different rates because autofluorescence makes up a different share of each,
   so one scaled control trace cannot model both bleaching and motion [4].
4. **Bleaching of the indicator is corrected by division; autofluorescence by subtraction** [4].
   So the denominator of dF/F is the signal channel's own fitted baseline, after any autofluorescence
   offset is subtracted.
5. **Normalization across trials and animals comes after dF/F.** Z-scoring or detrending the raw
   channels first makes dF/F impossible to compute [5].

Known disagreement: [5] (and GuPPy [9]) fit the *raw* control to the *raw* signal with IRLS and use
the fitted control as F0: `dF/F = (S − Ĉ)/Ĉ`, as in Lerner et al. [12]. That combines bleaching,
motion and normalization in one step. It implicitly assumes motion and bleaching scale the two
channels in the same proportion, which [4] notes is generally only approximately true. We default to
detrend-then-regress (§3–§5) and keep the combined method as `motion.method = "fit_raw"` (§4,
Alternative A). Both methods are cited and implemented. Choose per dataset with the QC in §8.

---

## 1. Sensor facts

### 1.1 GRAB-ACh3.0 (GACh3.0)

| Property | Value | Source |
|---|---|---|
| Design | Human M3 muscarinic receptor plus cpEGFP; GPCR-activation-based (GRAB) | [1], [2] |
| Max ΔF/F0 (HEK293T, 100 µM ACh) | ~280% (ACh2.0: ~76%) | [1] |
| Ligand-insensitive control | GRAB-ACh3.0-mut (W200A), ΔF/F0 ≈ 1.8% | [1] |
| EC50 | ~2 µM [1]; ~2.2 µM in HEK cells, measured in [3] | [1], [3] |
| k_on / k_off (HEK, puff) | 3.12 s⁻¹ µM⁻¹ / 1.72 s⁻¹ (τ_off = 1/k_off ≈ 0.58 s) | [1] |
| τ_on / τ_off, mouse MHb–IPN slice, 2-photon, 100 ms stimulus | ~105 ms / ~3.7 s (single-exponential fits) | [1] |
| τ_on / τ_off, Drosophila mushroom body | ~0.09 s / ~0.91 s | [1] |
| Imaging bands used in [1] | 1P: 488 nm excitation, 525/50 nm emission; 2P: 920 nm | [1] |
| 1P excitation/emission peaks | Not stated numerically in the text of [1] (spectra are in Extended Data Fig. 3f). As a cpEGFP sensor, expect an excitation peak near 490–500 nm and emission near 510–520 nm. **UNCERTAIN** | — |
| Isosbestic point(s) | **384 nm and 434 nm** (Table 2 of [4]) | [4] |
| In vivo photometry setup in [1] | 465 nm + 405 nm LEDs; TDT RZ2 lock-in (459/271 Hz); 20 Hz 6th-order demodulator low-pass; 1017 Hz storage; F0 from a 2nd-order exponential fit after autofluorescence subtraction; 405 nm channel linearly regressed onto 465 nm and subtracted | [1] |

Field status of the 405 nm control for GRAB-ACh3.0:
- **405 nm is not isosbestic for GRAB-ACh3.0.** [4] lists two crossing points, 384 and 434 nm,
  and 405 nm lies between them. The 405 nm channel should therefore contain an ACh-dependent
  component of opposite sign to the 465 nm response (fluorescence falling as ACh rises).
  **UNCERTAIN**: the sign and size of this component are inferred from the crossing points; no
  primary source we found reports them directly.
- The field still uses 405 nm as an *approximate* isosbestic, or as a motion/coupling reference.
  Examples: Jing et al. [1] (linear regression); Bouabid et al. [21], who also used a null-sensor
  cohort because 405 nm may not capture every artifact (they attribute this to differences in
  hemoglobin absorbance and tissue scattering between the two wavelengths).
- The recommended control for ligand-binding (especially GPCR-based) sensors is the **binding-dead
  mutant** (GRAB-ACh3.0-mut) in separate animals or hemispheres, or an antagonist within an animal [4].
- Hemoglobin absorbs 405 nm and 465 nm differently, so hemodynamic artifacts differ between the
  channels. Uncorrected hemodynamic artifacts can even flip signal polarity [20].

Engine consequences:
1. Name the channel "control" or "reference", never "isosbestic", in GRAB presets.
2. Regress robustly (IRLS, §4) so that ACh-locked excursions are down-weighted in the fit.
3. Always run control-channel event QC (§8, `ctl_event_response`). A significant event-locked
   405 nm response means the result needs the mutant-sensor control before it is interpreted.
4. Allow `motion.method = "none"` (bleaching correction only) as a sensitivity analysis. If
   conclusions change between `irls` and `none`, report both.
5. If the hardware allows another control wavelength, 434 nm is closer to an isosbestic point.
   **UNCERTAIN**: there is no validation of this in the photometry literature. Standard TDT
   setups provide 405/465 nm.

Interpretation constraints:
- **dF/F is a saturating function of [ACh]** (EC50 ~2 µM). Equal ΔACh produces smaller ΔF/F when
  tonic ACh is high, so amplitude comparisons across states with different tonic ACh are confounded.
- **Sensor kinetics low-pass the true ACh time course.** Rise τ ≈ 0.1 s; decay ranges from 0.6 s
  (sensor unbinding) to 3.7 s in slice, where clearance is included [1]. Do not read decay times as
  ACh clearance, and do not claim resolution below ~100 ms.
- In vivo responses are far smaller than the HEK-cell maximum. For example, ACh2.0 gives ~94% in
  HEK cells but ~3.4% in awake mouse cortex [2].
- Fluorescence *lifetime* of GRAB-ACh3.0 changes with ACh, and lifetime is independent of
  expression level and excitation power [25]. This is relevant to future cross-animal comparisons.
  Intensity photometry (the TDT case) cannot use it.

### 1.2 Newer ACh sensors

| Sensor | Facts | Status |
|---|---|---|
| rACh1h / rACh1m / rACh1l (red GRAB) | 1P excitation ~560–565 nm. rACh1h: ΔF/F0 ~500% (HEK) and ~1000% (neurons); EC50 ~0.4 µM (HEK) and ~0.2 µM (neurons). τ_on ~0.1 s for all three; τ_off 1.37–2.15 s depending on affinity. Mutant control: rAChmut | Peer reviewed [3] |
| gACh4h / gACh4m / gACh4l (green GRAB, 4th generation) | Listed in FlyBase as a high-affinity M3R–cpEGFP sensor [36]; used in some 2025 studies | **No primary characterization paper found as of 2026-10.** No preset; parameters unknown. |
| iAChSnFR (periplasmic-binding-protein based) | EC50 ~8.8 µM in HEK, measured in [3]; also responds to choline [3]; isosbestic point listed as 400–425 nm [4] | Preprint [31] |
| GACh2.0 | τ_on 280 ± 32 ms, τ_off 762 ± 75 ms (HEK, perfusion; likely overestimates); ΔF/F0 ~94% in [2], ~76% in the side-by-side comparison in [1] | [2] |

### 1.3 GCaMP6/7/8 (for the secondary preset)
- [4] lists the isosbestic point as **420–430 nm for GCaMP6s/m/f, jGCaMP7s/f/b and jGCaMP8**, and
  350 nm for jGCaMP7c. So 405 nm is near-isosbestic but below it: the control channel carries a
  small Ca²⁺-dependent component. [4] shows this as a negative control-channel signal for GCaMP6f.
- Kinetics are faster than GRAB-ACh3.0 (jGCaMP8 in particular [30]; GCaMP6 [28]; jGCaMP7 [29]),
  which justifies a higher low-pass cutoff.

---

## 2. Acquisition, loading, trimming, masking

**Recommended default**
- Read `fs` and `start_time` from each TDT store header. Never hard-code 1017.3 Hz.
  [1] reports 1017 Hz; Synapse stores at 305–1017 Hz [34].
- Use one clock for everything: block-relative seconds. Sample `i` is at `t_i = start_time + i/fs`.
  Epocs (`PtAB`, `Note`, `PC0_`, …) are block-relative onsets. Manual video annotations must be
  converted to block time (via camera-frame epocs if present). Trimming never moves t = 0.
- Store names: Synapse `_465A`/`_405A` appear as `x465A`/`x405A` in MATLAB TDTbin2mat. Pair
  channels by site letter (A, C, …).
- The demodulated streams have already passed through the Synapse lock-in low-pass (1–20 Hz,
  6th order typical [34]). Record that setting in the run manifest. The offline `fc` must not
  exceed it.
- `trim_start_s` removes LED/demodulator start-up transients.
- Masking: build `valid[i]` from saturation, flat runs and dropouts (§8). Exclude invalid samples
  from all fits. Never zero-fill them. Drop any epoch that overlaps an invalid sample.

| Param | Default | Range | Rationale |
|---|---|---|---|
| `trim_start_s` | 10 | 1–60 | JUDGMENT. GuPPy defaults to 1 s [9]; legacy used 3–10 s. Costs <1% of a 20-minute session. |
| `trim_end_s` | 0 | ≥0 | Cut only if the LED was switched off before acquisition stopped. |
| `autofluorescence_v` (per channel) | 0 (unknown) | ≥0 | Subtract before dF/F if measured (patch cord on a non-expressing or dummy target). [1] subtracted it; [4] notes F0 otherwise includes autofluorescence, which shrinks dF/F. |

---

## 3. Denoising, decimation and bleaching

### 3.1 Optional despiking
- Default **off** (TDT lock-in output rarely has electrical spikes).
- If on: running median, window ≈ 5× the spike duration, applied *before* any other filter [4].
- Do not use derivative-threshold spike deletion. It removes genuine fast transients.

### 3.2 Low-pass (zero-phase Butterworth)
- **Default:** Butterworth, order 4, as second-order sections, applied forward-backward (filtfilt,
  so the magnitude response is squared and there is no phase shift). Use even-reflection padding of
  3× the filter's settling length.

| Param | GRAB-ACh3.0 | GCaMP | Range | Rationale |
|---|---|---|---|---|
| `lowpass_hz` | **4** | **10** | 2–8 / 5–15 | GRAB-ACh3.0 τ_on ≈ 0.1 s gives a single-pole corner of 1/(2πτ) ≈ 1.6 Hz, so 4 Hz keeps the sensor's full bandwidth and removes more noise than 6 Hz. [4] reports 2–10 Hz as typical for GCaMP6f/dLight. [5] simulated 3 Hz. Akam used 10 Hz for dLight [7]. [1] used 6 Hz (Quebec system) and 20 Hz (TDT). JUDGMENT within that range. |
| `lowpass_order` | 4 | 2–6 | A 2nd-order filter rings less; a 4th-order filter rejects aliasing better. |

Alternatives: centered moving average (GuPPy default: 100 samples [9]; fpExplorer: filtfilt of a
moving average); median filter; none. A moving average has poor stop-band attenuation and is not
recommended.

### 3.3 Decimation
- **Default:** integer factor `N = max(1, floor(fs_in / target_fs))`, applied after the low-pass.
  Use a block mean, or pick every Nth sample.
- Timestamp each block mean at the block center: `t_k = t_first + (k·N + (N−1)/2)/fs_in`.
  Use `t_first + k·N/fs_in` when picking samples. A wrong timestamp shifts every epoch.
- Decimate both channels identically (same N, same phase).

| Param | GRAB-ACh3.0 | GCaMP | Rationale |
|---|---|---|---|
| `target_fs` | 20 Hz → N = 50 → 20.345 Hz at 1017.25 Hz | 50 Hz → N = 20 → 50.86 Hz | `fs_out ≥ 5·fc`. At fc = 4 Hz (or 10 Hz for GCaMP), the order-4 filtfilt attenuates about 65 dB at the new Nyquist frequency. A 0.1 s sensor rise spans about 2 samples. |

### 3.4 Photobleaching: double exponential per channel (default)
Model, after Akam and Burgeno [7]:
```
F0(t) = K + A_s·exp(−t/τ_s) + A_f·exp(−t/(m·τ_s)),  t = seconds since first valid sample
bounds: K, A_s, A_f ∈ [0, max(x)];  τ_s ∈ [600, 36000] s;  m ∈ (0, 1)
init:   K = max/2, A_s = A_f = max/4, τ_s = 3600 s, m = 0.1
```
- Fit `S` and `C` separately on the low-passed, decimated, valid samples (bounded
  Levenberg–Marquardt).
- Why two exponentials: several sources bleach at different rates (patch-cord autofluorescence,
  tissue, fluorophore), so one exponential is too restrictive [4], [7].
- `bleach.robust` (default **false**; option): 3 outer iterations of Tukey-bisquare reweighting,
  `c = 4.685`, scale = MAD/0.6745. Use it when large phasic positive events bias the fit upward.
- Detrended traces: `S_d = S − F0_S` and `C_d = C − F0_C`. Keep `F0_S` for the dF/F denominator.
- Fallback when the fit fails, or the session is shorter than 10 min and `τ_s` hits a bound:
  `F0 = x − HP(x)` with a 2nd-order Butterworth high-pass at 0.001 Hz, zero-phase [7]. Raise a QC flag.
- Caveat: any smooth baseline absorbs real slow changes. In tonic-ACh studies (sleep, satiety
  across a session) bleaching correction and biology cannot be separated, so counterbalance
  conditions within the session [4]. Prefer the lifetime method or the mutant-sensor control for
  tonic claims.

Alternatives (implemented, not default):

| Method | Parameters | Caveat |
|---|---|---|
| High-pass | Butterworth order 2, 0.001 Hz (16.7 min period), zero-phase [7] | Removes all physiology slower than ~17 min |
| airPLS [17] (Martianova [8]) | λ = 5e4, porder = 1, itermax = 50 | λ depends on the sample count and rate, so it must be rescaled when fs changes |
| BEADS [18] (used in [1]) | fc = 0.0005 *cycles/sample* in [1]; d = 2, r = 6, λ0 = 0.5, λ1 = 1, λ2 = 4 (legacy) | fc is in cycles per **sample**: 0.0005 corresponds to 0.51 Hz at 1017 Hz but 0.0095 Hz at 19 Hz. The legacy code reused the value after resampling. Six hyperparameters, no photometry validation. Not recommended. |
| Linear trend in time (fpExplorer "Modified Polynomial Fitting") | — | Bleaching is not linear |
| Fitted control as F0 (Lerner/Keevers/GuPPy) | see §4 Alternative A | Assumes proportional bleaching in both channels [4] |

---

## 4. Motion / artifact correction (control regression)

**Default: IRLS with Tukey bisquare, control predicting signal, on detrended traces.**
```
y = S_d, x = C_d (valid samples only)
β ← OLS(y ~ 1 + x)
repeat ≤ 50 times:
    r = y − (β0 + β1·x)
    s = median(|r − median(r)|) / 0.6745            # robust scale
    u = r / (c·s);  w = (1 − u²)² if |u| < 1 else 0  # Tukey bisquare [19]
    β_new = WLS(y ~ 1 + x, w)
    stop if max|β_new − β| ≤ 1e-6·(max|β| + 1e-12)
motion(t) = β0 + β1·C_d(t);   S_mc(t) = S_d(t) − motion(t)
```

| Param | Default | Range | Rationale |
|---|---|---|---|
| `motion.tuning` (c) | **1.4** | 1.4–4.685 | [5] tested c = 4.685, 3 and 1.4; lower values (more aggressive down-weighting) gave more accurate corrected signals. The authors' code defaults to 1.4 [5]. 4.685 is the classical 95%-efficiency constant [19] and moves toward OLS. Low efficiency is irrelevant here: two parameters fit from >10⁴ samples. |
| `motion.max_iter` | 50 | — | Usually converges in fewer than 20 iterations. |
| Direction | control → signal | fixed | Predict the signal from the control [4], [5]. Never scale the signal to the control: that changes the units of the result. |
| Leverage adjustment (as in MATLAB `robustfit`) | omitted | — | Leverages h ≈ 2/n → 0 for long recordings. |

Safety rules:
- If `β1 ≤ 0`, set the motion term to 0 (no correction) and raise QC `negative_motion_slope`.
  Subtracting a negatively scaled control adds artifact. A negative slope also suggests that a
  ligand-dependent component dominates the control (§1.1).
- Report β0, β1, the weighted R², and the fraction of samples with `w = 0`.

Why not OLS: when the signal has large neural transients, OLS over-fits the control to those
dynamics, which shifts corrected transients downward and causes more type I/II/III errors in
peri-event analysis than IRLS [5]. Used by legacy code, TDT examples and older pipelines [9], [12].

Alternatives:
- **A. `fit_raw`** (Keevers [5]; GuPPy current default, IRWLS [9]; Lerner equation [12]): IRLS fit of
  low-passed raw `C` to raw `S`, then `dF/F = (S − Ĉ)/Ĉ`. Simplest validated method. It folds
  bleaching into the fit, assuming the two channels bleach proportionally.
- **B. OLS** (Lerner [12], TDT examples [34], legacy MATLAB).
- **C. Positive Lasso on standardized traces** (Martianova [8]: airPLS detrend, standardize each
  channel, `Lasso(alpha=1e-4, positive=True)`, output `zdFF = S_z − fit`). This is described as
  "robust" but is effectively non-negative least squares, and the output is in z units, not dF/F.
- **D. none**: bleaching correction only (a sensitivity analysis for GRAB sensors, §1.1).
- **E. TMAC** [26]: a Bayesian model for two-channel *imaging* with an activity-independent
  structural fluorophore. It is not validated for 405 nm photometry. Not implemented.
- A dedicated static red fluorophore (tdTomato, mCherry) can serve as the reference in place of
  the isosbestic channel [5]. Akam's example uses tdTomato [7]. Accept any
  reference channel as `control`.

---

## 5. dF/F and normalization

### 5.1 Session trace (default)
```
dFF(t) = 100 · S_mc(t) / (F0_S(t) − autofluorescence_S)        [% dF/F]
```
Pitfalls:
- F0 includes autofluorescence unless it is subtracted, so dF/F then underestimates the sensor
  response and does not fully normalize differences in expression [4].
- Never divide by a constant session mean when the numerator has been detrended (legacy
  "subtraction" paradigm). The denominator must follow bleaching of the indicator [4].
- Never compute dF/F from z-scored or high-passed channels [5].
- A ratio (`S/C`) or `dFF_S − dFF_C` (fpExplorer "Modified") is not dF/F. Do not use either.

Optional residual-drift removal after dF/F (default **off**): smoothed moving median (window
60 s), preferred over a moving mean, which over-fits phasic dynamics [5]. Alternative: high-pass
at 0.001 Hz [7]. Per-trial baselining (§6) already removes slow drift for event analysis.

### 5.2 Normalization options (secondary outputs)

| Name | Formula | Use | Caveat |
|---|---|---|---|
| `dff` (default) | above | All quantification | Expression differences remain [4] |
| `robust_z_session` | `(dFF − median)/(1.4826·MAD)` over valid samples | Transient detection; display | Signal-dependent denominator (below) |
| `z_session` | `(dFF − mean)/SD` | Display only | Inflated by events |
| `null_z` | `dFF / sqrt(mean(dFF²))` | Keevers' alternative; keeps 0 at the IRLS baseline [5] | Same variance caveat |
| `z_trial` | `(epoch − mean_b)/max(SD_b, σ_noise)` | Legacy compatibility | A small baseline SD inflates z |

Z-score caveat (normative text for reports): the z denominator includes the signal's own
variability. Manipulations that change release, uptake or tonic level change that denominator,
which masks or invents effects. Session z-scoring can remove variation of experimental interest, such as learning or group
effects [4]. On pharmacologically manipulated spontaneous dLight signals, the z-score method missed a
D1-antagonist frequency drop that manual %ΔF/F analysis detected (p = 0.55 vs p < 0.01) [16].
**Quantify in % dF/F. Use z only for detection thresholds and display.**

The MAD constant must be explicit. The engine reports `1.4826·MAD` (consistent with SD for
Gaussian data). GuPPy's "modified z" uses `0.6745·(x − median)/MAD`, which is equivalent [9].
fpExplorer divides by an unscaled MAD, which inflates z by 1.48× (see §10).

---

## 6. Event-locked analysis

### 6.1 Epochs
- Epoch for each event onset `t_e`: samples `i_e + k`, where `i_e = round((t_e − t_0)·fs)` and
  `k ∈ [round(pre·fs), round(post·fs)]`. Time axis `k/fs`. Nearest-sample error ≤ 0.5/fs (25 ms at
  20 Hz), well below τ_on.
- Drop an epoch if any part of it falls outside the data, in the trimmed region, or on an invalid
  sample. Report the number dropped per event type.
- Overlap policy: flag an event if a *different* event type falls in its baseline window. Optional
  `exclude_if_preceded_s` drops it instead. For feeding, bites within a bout are not independent
  events; analyze bout onsets, or model the overlap (FLMM covariates).
- **Validation (hard error):** baseline ⊂ [pre, 0]; response window ⊂ [0, post]; baseline must
  contain ≥ 2 s of samples. Legacy failures are documented in `docs/legacy-audit.md`.

| Param | GRAB-ACh3.0 | GCaMP | Rationale |
|---|---|---|---|
| `pre_s`, `post_s` | −10, +20 | −10, +20 | Same as GuPPy defaults [9] and the TDT example window [34]. A +20 s window covers the slow ACh decay (τ_off up to ~4 s [1]) plus consummatory bouts. |
| `baseline_window` | [−5, −1] s | [−5, −1] s | JUDGMENT. Ends 1 s before onset to exclude anticipatory ramps and manual-scoring jitter (self-paced approach events start before the scored frame). GuPPy uses [−5, 0] [9]; TDT example [−10, −6] [34]. |
| `response_window` | [0, 5] s | [0, 3] s | JUDGMENT. About one sensor decay plus the behavior. AUC depends strongly on the chosen window [4], so the window must be fixed before analysis and reported. |

### 6.2 Per-trial baseline (default: subtract)
`ΔdFF_trial(t) = dFF(t) − mean(dFF over baseline_window)`, in % dF/F.
`z_trial` is optional (§5.2). Do not divide by the baseline SD by default.
FLMM can instead model baseline covariates directly [14].

### 6.3 Summary metrics (per trial, then averaged per animal)

| Metric | Definition | Pitfall |
|---|---|---|
| `mean_resp` (primary) | mean of ΔdFF over `response_window` | Least biased; choose the window a priori |
| `auc` | trapezoid integral over `response_window` (%·s), signed | Equals mean × duration. Positive and negative lobes cancel, so also report `auc_pos` and `auc_neg` |
| `peak`, `peak_latency` | max of ΔdFF in the window and its time | The max of a noisy trace is biased upward, with bias growing with noise and window length. Compare only at equal noise and window. Latency is unstable on flat responses |
| `trough`, `trough_latency` | min, for ACh dips/pauses | Same as peak |
| `half_decay_s` (optional) | time from peak to 50% | Reflects sensor kinetics plus clearance (§1.1) |

The waveform (whole time course) is the primary result. Summary metrics are secondary, because
they discard temporal information and lose power [13], [14].

### 6.4 Spontaneous transient detection (optional; non-event analyses)
- **Default: GuPPy two-threshold MAD algorithm** [9], run on dF/F rather than z (amplitudes are
  then in %, per [16]):
  1. Split into 15 s windows.
  2. In each window, compute the median `m` and MAD (unscaled).
  3. Exclude samples above `m + HAFT·MAD`, with HAFT = 2.
  4. Recompute `m′` and `MAD′` on the remaining samples.
  5. Mark samples above `m′ + 3·MAD′`.
  6. Peaks are the local maxima of the thresholded trace.
- Report frequency (per minute), amplitude (% dF/F), and optionally width.
- Alternative: prominence-based peaks (`prominence ≥ 3·σ_noise`, `width ≥ 0.2 s` for GRAB-ACh3.0).
  This is in the spirit of the "Prominence Method" of [16], which uses z only for timestamps and
  quantifies in %ΔF/F. Prominence thresholds are JUDGMENT.

---

## 7. Statistics

### 7.1 Units of inference (normative)
- **The animal is the experimental unit.** Trials within an animal are not independent. Treating
  them as independent (SEM across trials, t-tests on pooled trials) inflates false positives [4],
  [23].
- **Two-stage default:** average trials within animal × condition, then make group inference on
  the per-animal waveforms. [4] reports that this two-stage approach gives results comparable to
  mixed models.
- Single-animal statistics across trials are descriptive. Label them "trial-level, n = trials, one animal".

### 7.2 Waveform inference: bootstrap CI + consecutive threshold (default)
Following Jean-Richard-dit-Bressel, Clifford & McNally [13] and their code:
```
rows = per-animal mean waveforms (n animals); require n ≥ 3
for b in 1..B: resample n rows with replacement; m_b(t) = mean of resampled rows
L(t), U(t) = percentiles α/2, 1−α/2 of {m_b(t)}
expand: mid = (U+L)/2; half = (U−L)/2·sqrt(n/(n−1)); CI = [mid − half, mid + half]
sig(t) = CI excludes 0
keep only runs of sig with length ≥ K samples, K = ceil(min_consecutive_s · fs)
```

| Param | Default | Rationale |
|---|---|---|
| `n_boot` (B) | 5000 | [13] used 1000. 5000 makes 2.5% tails stable; cost is negligible in Rust. |
| `ci_level` | 0.95 | — |
| Expansion | sqrt(n/(n−1)) | Corrects small-sample narrowness of the percentile CI; implemented in the authors' `CIadjust.m` [13]. See also [33]. |
| `min_consecutive_s` | **1/fc**: 0.25 s (GRAB-ACh3.0, fc = 4 Hz); 0.10 s (GCaMP, fc = 10 Hz) | [13] recommends a run length equal to the low-pass period; in simulation this brought family-wise error to or below α. Without a threshold, FWER was very high. At the presets' fs, both give K = 6 samples. |
| RNG | seeded, recorded in the manifest | Reproducibility |

- Within-subject contrasts (condition A − B): per-animal difference waveforms, then the same bCI.
- Between-group contrasts: resample within each group and form the CI of the difference of means.
  Alternatively, run a permutation test: shuffle animal labels, 10 000 permutations or all
  permutations if fewer, two-sided pointwise p, same consecutive rule. [13] found permutation
  tests conservative and lowest in power at small n; bCI was most powerful.
- Alternative for multiple comparisons: cluster-based permutation (cluster mass) [24].

### 7.3 Trial-level modelling: functional linear mixed models (recommended for publication)
- FLMM (Loewinger, Cui, Lovinger & Pereira [14], built on fast univariate inference [15]):
  1. Fit a pointwise LMM at every time point.
  2. Smooth β(s) across time.
  3. Build a pointwise CI from an analytic covariance (Gaussian case) or a bootstrap.
  4. Build a **joint 95% CI** that accounts for testing across the whole trial.
- [14] reports joint coverage near nominal, type I error near nominal, and higher power than
  permutation tests, AUC t-tests or LMMs on summaries. It also argues FLMM reduces the need for
  trial-baseline normalization and smoothing.
- Software: R package `fastFMM` (`fui()`, lme4 syntax), e.g.
  `fui(dff ~ condition + (1 | animal/session), data)`. Select random slopes by AIC/BIC [14].
  About 1.2 min for more than 3 200 trials on an M1 Max [14].
- **Engine scope (decision):** v1 does not implement FLMM natively. It exports long-format CSV and
  Parquet (`animal, session, trial, event, condition, covariates…, t, dff, dff_baseline_sub`) for
  `fastFMM`. A native implementation is deferred until a validated reference exists to test against.

### 7.4 Summary-metric inference
- Default: LMM `metric ~ condition + (1 | animal)` (export), or a paired t / Wilcoxon test on
  per-animal means (engine).
- CIs for metric means: **hierarchical bootstrap**. Resample animals with replacement, then trials
  within each resampled animal [22]. B = 5000.
- Report effect sizes with CIs and exact p-values. Correct across event types or metrics
  (Benjamini–Hochberg FDR) [4].

---

## 8. Quality control (computed every run; thresholds are JUDGMENT heuristics, not literature standards)

| Metric | Definition | Flag if |
|---|---|---|
| `saturation_frac` | Fraction of raw samples ≥ `0.99·adc_max` (hardware parameter; no default), or in flat runs of ≥ 10 identical consecutive values | > 0 |
| `dropouts` | Segments where both channels fall below 50% of their 60 s running median for ≥ 0.5 s, or a simultaneous jump > 20·σ_noise | any (masked) |
| `bleach_pct_S`, `bleach_pct_C` | `100·(1 − F0(t_end)/F0(t_0))` per channel | > 50%, or a fit at its bounds |
| `motion_r` | Pearson r(S_d, C_d) before correction | reported |
| `motion_beta1`, `motion_r2_w`, `irls_zero_w_frac` | §4 outputs | β1 ≤ 0; zero-weight fraction > 0.3 |
| `sigma_noise` | `1.4826·MAD(diff(dFF))/√2` (% dF/F; white-noise estimate at the analysis fs) | reported |
| `snr_event` | \|mean_resp\| (animal mean) / σ_noise | reported |
| `ctl_event_response` | bCI of the control-channel peri-event trace `100·C_d/F0_C`, with the same baseline and consecutive rule | significant excursion → result needs the mutant-sensor control (§1.1) |
| `residual_drift` | Slope of a linear fit to dF/F (% per 10 min) | \|slope\| > 2·σ_noise per 10 min |
| `raw_level` | Median raw S and C (V); ratio to autofluorescence if known | S < 5 × autofluorescence |
| `events` | Counts used/dropped per type, and overlaps | any dropped |

Plots required for every run: raw S/C with F0 fits; detrended S_d against motion(t); dF/F with
masked segments; control-channel PETH beside signal PETH; per-trial heatmap (diverging,
perceptually uniform, centered at 0).

---

## 9. Reporting checklist (written to `run.json` and a methods paragraph)
1. Sensor, construct, serotype, promoter, titer, volume, coordinates. [4] notes these are
   inconsistently reported. Fiber core and NA; LED wavelengths and power at the fiber tip.
2. Hardware: TDT model, lock-in frequencies, demodulator low-pass and order, storage fs.
3. Every parameter in §2–§7 with value, the preset id and its hash, engine version, input digests,
   and RNG seed.
4. Control used (405 nm reference, mutant sensor, static fluorophore) and the `ctl_event_response` result.
5. Bleaching model and fit parameters; IRLS β and tuning constant; dF/F formula and denominator;
   autofluorescence handling.
6. Event definitions, source (epoc or video), scoring method and blinding, exclusions and counts.
7. Statistical unit, n animals / sessions / trials, test, CI method (B, expansion, consecutive
   rule), exact p, effect size.
8. QC table (§8) per session; exclusion criteria stated before analysis.
9. Code and data availability [4].

---

## 10. Presets (exact values)

Both presets share the §0 pipeline. Differences from the presets currently in `presets/` are
listed after the tables. The science values below take precedence.

### 10.1 `grab-ach3.0-tdt`
```toml
[sensor]
name = "GRAB-ACh3.0"; analyte = "acetylcholine"
excitation_nm = 465; control_nm = 405; control_is_isosbestic = false
tau_on_s = 0.1; tau_off_s = 0.6        # HEK k_off; slice τ_off ~3.7 s [1]

[channels]   # pair by site letter
signal = ["x465A", "_465A", "465A"]; control = ["x405A", "_405A", "405A"]

[preprocess]
trim_start_s = 10.0; trim_end_s = 0.0
despike = false
lowpass_hz = 4.0; lowpass_order = 4; zero_phase = true
target_fs = 20.0                         # N = floor(fs_in/20) → 20.345 Hz at 1017.25 Hz
decimate = "block_mean"                  # center-of-block timestamps

[bleach]
method = "double_exp"; robust = false
tau_slow_bounds_s = [600.0, 36000.0]; fast_ratio_bounds = [0.0, 1.0]
fallback = "highpass_0.001hz"

[motion]
method = "irls_detrended"; weight = "bisquare"; tuning = 1.4; max_iter = 50
clamp_negative_slope = true

[dff]
denominator = "F0_signal_minus_autofluor"; percent = true
residual_detrend = "none"

[events]
pre_s = -10.0; post_s = 20.0
baseline = "subtract"; baseline_window = [-5.0, -1.0]
response_window = [0.0, 5.0]
metrics = ["mean_resp", "auc", "auc_pos", "auc_neg", "peak", "peak_latency", "trough", "trough_latency"]

[stats]
unit = "animal"; ci = "bootstrap_percentile_expanded"; n_boot = 5000; ci_level = 0.95
min_consecutive_s = 0.25                 # 1/lowpass_hz
seed = 1
export_flmm = true

[transients]   # off unless requested
method = "guppy_mad"; window_s = 15.0; haft = 2.0; thresh_mad = 3.0; on = "dff"
```

### 10.2 `gcamp-tdt` (GCaMP6/7/8)
Same as 10.1 except:
```toml
[sensor]
name = "GCaMP"; analyte = "calcium"; control_is_isosbestic = false   # near-isosbestic; true point 420–430 nm [4]
[preprocess]
lowpass_hz = 10.0; target_fs = 50.0      # N = 20 → 50.86 Hz
[events]
response_window = [0.0, 3.0]
[stats]
min_consecutive_s = 0.10
```
GCaMP6s (slow decay): consider `lowpass_hz = 5` and `response_window = [0, 5]`. JUDGMENT.

Differences from the presets as they were before this spec (all adopted on 2026-10-25, with `bleach.robust = false`):

| Field | Current file | This spec | Rationale |
|---|---|---|---|
| `lowpass_hz` (GRAB) | 6 | 4 | §3.2. 6 Hz is within range and acceptable. |
| `motion.tuning` | 4.685 | 1.4 | §4, [5] |
| `events.pre/post` | 5 / 10 | −10 / +20 | §6.1. Long ACh decay and feeding bouts. |
| `min_consecutive_s` | 0.5 | 1/fc (0.25 / 0.10) | §7.2, [13]. 0.5 s is valid only for fc = 2 Hz. |
| `bleach.robust` | true | false (option) | Either is defensible. Robust reduces upward bias from phasic events. |

---

## 11. Critique of `fp_research1.txt` and `fp_research2.txt`

Both reports use superscript reference numbers but contain **no bibliography**, so none of their
claims can be traced. The items below were checked against primary sources.

### 11.1 `fp_research1.txt`
1. **"405 nm … isosbestic control" for GACh3.0 (§1.1.1): wrong as stated.** [4] lists GRAB-ACh3.0
   isosbestic points at 384 and 434 nm. 405 nm is an approximate reference at best [21].
2. **"The work by Seltzer & Barker provides … methodology for applying IRLS":** no such paper
   found. The peer-reviewed IRLS-for-photometry source is Keevers & Jean-Richard-dit-Bressel
   2025 [5]. Treat as a likely hallucinated citation.
3. **"bioRxiv paper by Hart et al." criticizing z-scoring:** not found. The phrase the report
   quotes (z-scoring removing variation of experimental interest) matches Simpson et al. [4]. The empirical
   z-score critique is Wallace et al. 2025 [16]. Misattributed.
4. **"The eLife paper by PNAS RP95802":** garbled. The paper is Loewinger et al., *eLife*
   13:RP95802 (2025) [14], not PNAS.
5. **fpExplorer "Modified Polynomial Fitting" might be robust regression:** it is not. The code
   (`normalize_dff`):
   - fits a *linear trend in time* to each channel after discarding samples beyond ±2 SD;
   - computes dF/F per channel and subtracts dF/F₄₀₅ from dF/F₄₆₅;
   - shifts the result by the mean of its negative values.
   "Standard Polynomial Fitting" (`normalize_pMat`) is OLS of control onto signal after a ±2 SD
   cut. fpExplorer's "Z-score" divides by an unscaled MAD (`scipy.stats.median_abs_deviation`
   default scale = 1), inflating values 1.48× relative to SD-based z [35]. The report speculated
   instead of reading the code.
6. **Jing et al. 2020 details (6 Hz / 20 Hz low-pass, BEADS 0.0005 cycles/sample, 2nd-order
   exponential F0, NREM-SD z-score, 405 regression):** verified correct [1]. However, the report
   (and the legacy code) do not note that BEADS `fc` is in cycles per **sample**, so the value does
   not transfer across sampling rates (§3.4).
7. **Recommends Z-scores "for primary event-related analysis" with a −10 to −2 s baseline:**
   defensible for detection, but contradicts the evidence that z-scores distort amplitude
   comparisons [4], [16]. This spec quantifies in baseline-subtracted % dF/F (§5.2, §6.2).
8. **"dF/F = (Signal_Raw − Fitted_Control)/Fitted_Control … inherently incorporates corrections
   for … photobleaching":** presented without the caveat that this assumes proportional bleaching
   and motion in both channels [4].
9. **"Low-pass 10–20 Hz for ACh3.0" (table) versus "15–20 Hz" (workflow):** inconsistent, and
   higher than needed given τ_on ≈ 0.1 s (§3.2). Not harmful, just noisier.
10. **Statistics:** suggests paired/unpaired t-tests on metrics without specifying the unit
    (animal vs trial). This is the most consequential omission [23].
11. **Sample-size claim "8–12+ animals per group is common":** unsourced. Ignore it.

### 11.2 `fp_research2.txt`
1. **"isosbestic … ~405–415 nm" where GACh3.0 emission is "largely independent of" ACh: wrong**
   (same as 11.1-1). It also omits the field's recommended mutant-sensor control [4].
2. **TMAC presented as the state of the art for isosbestic photometry:** TMAC [26] was developed
   for two-channel *imaging* with an activity-independent structural fluorophore (C. elegans
   whole-brain imaging). We found no validation on 405/465 nm photometry. Overstated.
3. **"rACh1h, which is reported to have faster kinetics than GACh3.0":** not supported. rACh1h has
   τ_on ~0.1 s (as GACh3.0 in slice) and τ_off 1.37–2.15 s, slower than GACh3.0's HEK τ_off
   ≈ 0.58 s [1], [3]. Likely wrong. "Acquired at 10 Hz" refers to 10 Hz excitation pulses in [3].
4. **"GACh3.0 … measure rapid decreases … within 250 ms"** (ref ^4^): not found in [1]. Unverified.
5. **Nyquist reasoning ("sensor responds in 200–250 ms, so ≥ 8–10 Hz sampling"):** confuses
   response time with bandwidth. Bandwidth is set by τ (~1.6 Hz corner for τ_on = 0.1 s). The
   conclusion (20–50 Hz) is fine.
6. **"Detrending before isosbestic correction … interferes"** is presented as settled. [4]
   recommends the opposite order (bleaching first, independently). The disagreement is real and
   not acknowledged (§0).
7. **GuPPy described as least-squares polynomial fit only:** true for the 2021 paper version. The
   current GuPPy default is IRWLS (Tukey bisquare), with OLS optional [9]. Outdated.
8. **Ortiz-Guzman et al. 2024:** the paper exists (*eNeuro* 11(3), ENEURO.0369-23.2024) [27]. The
   report correctly admits it did not see the methods, then still spends a section on it. Its
   GACh3.0 processing remains unverified.
9. **GACh2.0 kinetics "rise ~200 ms, decay ~700 ms" and GACh3.0 EC50 "~2.2 µM":** approximately
   correct (280/762 ms [2]; ~2 µM [1]; 2.2 µM in [3]).
10. **Correct points worth keeping:** zero-phase filtering; IRLS over OLS; moving median over
    moving mean for residual detrending (matches [5]); caution on z-scores for spontaneous signals
    (matches [16]); GRAB-ACh3.0 lifetime change (matches [25]).

---

## 12. Open questions / uncertain items
- The sign and size of GRAB-ACh3.0's ACh-dependent component at 405 nm (inferred, §1.1). Resolve
  with a mutant-sensor cohort or a 405 nm PETH from lab data.
- Whether `fit_raw` or `irls_detrended` is more accurate for GRAB-ACh3.0 on lab data. Compare on
  sessions with ground-truth artifacts (mutant cohort) and the simulator in `crates/core/src/sim.rs`.
- gACh4h/m/l parameters (no primary paper found).
- Ma et al. [25]: journal version reported as *Science Advances* 2024. DOI not verified; the
  preprint DOI is cited.

---

## References
1. Jing M, Li Y, Zeng J, Huang P, Skirzewski M, Kljakic O, Peng W, Qian T, et al. An optimized acetylcholine sensor for monitoring in vivo cholinergic activity. *Nat Methods* 17(11):1139–1146 (2020). doi:10.1038/s41592-020-0953-2
2. Jing M, Zhang P, Wang G, Feng J, Mesik L, Zeng J, Jiang H, Wang S, et al. A genetically encoded fluorescent acetylcholine indicator for in vitro and in vivo studies. *Nat Biotechnol* 36(8):726–737 (2018). doi:10.1038/nbt.4184
3. Xie S, Miao X, Li G, Zheng Y, Li M, Ji E, Wang J, Li S, et al., Li Y. Red-shifted GRAB acetylcholine sensors for multiplex imaging in vivo. *Nat Neurosci* 29(8):2012–2022 (2026). doi:10.1038/s41593-026-02325-w (preprint doi:10.1101/2024.12.22.627112)
4. Simpson EH, Akam T, Patriarchi T, Blanco-Pozo M, Burgeno LM, Mohebi A, Cragg SJ, Walton ME. Lights, fiber, action! A primer on in vivo fiber photometry. *Neuron* 112(5):718–739 (2024). doi:10.1016/j.neuron.2023.11.016
5. Keevers LJ, Jean-Richard-dit-Bressel P. Obtaining artifact-corrected signals in fiber photometry via isosbestic signals, robust regression, and dF/F calculations. *Neurophotonics* 12(2):025003 (2025). doi:10.1117/1.NPh.12.2.025003 (code: github.com/philjrdb/RegressionSim, `IRLS_dFF.m`, `nullZ.m`)
6. Akam T, Walton ME. pyPhotometry: Open source Python based hardware and software for fiber photometry data acquisition. *Sci Rep* 9:3521 (2019). doi:10.1038/s41598-019-39724-y
7. Akam T, Burgeno L. ThomasAkam/photometry_preprocessing: Neuron Primer (code, 2023). Zenodo doi:10.5281/zenodo.10103973
8. Martianova E, Aronson S, Proulx CD. Multi-fiber photometry to record neural activity in freely-moving animals. *J Vis Exp* (152):e60278 (2019). doi:10.3791/60278 (code: github.com/katemartian/Photometry_data_processing)
9. Sherathiya VN, Schaid MD, Seiler JL, Lopez GC, Lerner TN. GuPPy, a Python toolbox for the analysis of fiber photometry data. *Sci Rep* 11:24212 (2021). doi:10.1038/s41598-021-03626-9. Current defaults: guppy.readthedocs.io (parameters reference, accessed 2026-10-25); source github.com/LernerLab/GuPPy (`analysis/transients.py`).
10. Bruno CA, O'Brien C, Bryant S, Mejaes JI, Estrin DJ, Pizzano C, Barker DJ. pMAT: An open-source software suite for the analysis of fiber photometry data. *Pharmacol Biochem Behav* 201:173093 (2021). doi:10.1016/j.pbb.2020.173093
11. Bridge MF, Wilson LR, Panda S, Stevanovic KD, Letsinger AC, McBride S, Cushman JD. FiPhA: an open-source platform for fiber photometry analysis. *Neurophotonics* 11(1):014305 (2024). doi:10.1117/1.NPh.11.1.014305
12. Lerner TN, Shilyansky C, Davidson TJ, Evans KE, Beier KT, Zalocusky KA, Crow AK, Malenka RC, et al. Intact-brain analyses reveal distinct information carried by SNc dopamine subcircuits. *Cell* 162(3):635–647 (2015). doi:10.1016/j.cell.2015.07.014
13. Jean-Richard-dit-Bressel P, Clifford CWG, McNally GP. Analyzing event-related transients: confidence intervals, permutation tests, and consecutive thresholds. *Front Mol Neurosci* 13:14 (2020). doi:10.3389/fnmol.2020.00014 (code: github.com/philjrdb/ERTsimulation, `boot_CI.m`, `CIadjust.m`, `consec_idx.m`)
14. Loewinger G, Cui E, Lovinger D, Pereira F. A statistical framework for analysis of trial-level temporal dynamics in fiber photometry experiments. *eLife* 13:RP95802 (2025). doi:10.7554/eLife.95802 (R package fastFMM: github.com/gloewing/fastFMM)
15. Cui E, Leroux A, Smirnova E, Crainiceanu CM. Fast univariate inference for longitudinal functional models. *J Comput Graph Stat* 31(1):219–230 (2022). doi:10.1080/10618600.2021.1950006
16. Wallace CW, DiMarco EK, Slinkard CY, Shaughnessy R, Holleran KM, Centanni SW, Lapish CC, Jones SR. Fiber photometry analysis of spontaneous dopamine signals: the z-scored data are not the data. *ACS Chem Neurosci* 16:3239–3256 (2025). doi:10.1021/acschemneuro.5c00078
17. Zhang ZM, Chen S, Liang YZ. Baseline correction using adaptive iteratively reweighted penalized least squares. *Analyst* 135(5):1138–1146 (2010). doi:10.1039/b922045c
18. Ning X, Selesnick IW, Duval L. Chromatogram baseline estimation and denoising using sparsity (BEADS). *Chemometr Intell Lab Syst* 139:156–167 (2014). doi:10.1016/j.chemolab.2014.09.014
19. Holland PW, Welsch RE. Robust regression using iteratively reweighted least-squares. *Commun Stat Theory Methods* 6(9):813–827 (1977). doi:10.1080/03610927708827533
20. Zhang WT, Chao THH, Yang Y, Wang TW, Lee SH, Oyarzabal EA, Zhou J, Nonneman R, et al. Spectral fiber photometry derives hemoglobin concentration changes for accurate measurement of fluorescent sensor activity. *Cell Rep Methods* 2(7):100243 (2022). doi:10.1016/j.crmeth.2022.100243
21. Bouabid S, Zhang L, Vu MAT, Tang K, Graham BM, Noggle CA, Howe MW. Distinct spatially organized striatum-wide acetylcholine dynamics for the learning and extinction of Pavlovian associations. *Nat Commun* 16:5169 (2025). doi:10.1038/s41467-025-60462-5
22. Saravanan V, Berman GJ, Sober SJ. Application of the hierarchical bootstrap to multi-level data in neuroscience. *Neurons Behav Data Anal Theory* 3(5) (2020). Preprint doi:10.1101/819334
23. Aarts E, Verhage M, Veenvliet JV, Dolan CV, van der Sluis S. A solution to dependency: using multilevel analysis to accommodate nested data. *Nat Neurosci* 17(4):491–496 (2014). doi:10.1038/nn.3648
24. Maris E, Oostenveld R. Nonparametric statistical testing of EEG- and MEG-data. *J Neurosci Methods* 164(1):177–190 (2007). doi:10.1016/j.jneumeth.2007.03.024
25. Ma P, Chen P, Tilden EI, Aggarwal S, Oldenborg A, Chen Y. Fluorescence lifetime enables high-resolution analysis of neuromodulator dynamics across time and animals. bioRxiv (2022). doi:10.1101/2022.09.28.510014 (journal version reported as *Sci Adv* 2024; DOI not verified)
26. Creamer MS, Chen KS, Leifer AM, Pillow JW. Correcting motion induced fluorescence artifacts in two-channel neural imaging. *PLoS Comput Biol* 18(9):e1010421 (2022). doi:10.1371/journal.pcbi.1010421
27. Ortiz-Guzman J, Swanson JL, Tantry EK, Kochukov M, Ung K, Addison AP, Srivastava S, Belfort BD, et al., Arenkiel BR. Cholinergic basal forebrain connectivity to the basolateral amygdala modulates food intake. *eNeuro* 11(3):ENEURO.0369-23.2024 (2024). doi:10.1523/ENEURO.0369-23.2024
28. Chen TW, Wardill TJ, Sun Y, Pulver SR, Renninger SL, Baohan A, Schreiter ER, Kerr RA, et al. Ultrasensitive fluorescent proteins for imaging neuronal activity. *Nature* 499:295–300 (2013). doi:10.1038/nature12354
29. Dana H, Sun Y, Mohar B, Hulse BK, Kerlin AM, Hasseman JP, Tsegaye G, Tsang A, et al. High-performance calcium sensors for imaging activity in neuronal populations and microcompartments. *Nat Methods* 16:649–657 (2019). doi:10.1038/s41592-019-0435-6
30. Zhang Y, Rózsa M, Liang Y, Bushey D, Wei Z, Zheng J, Reep D, Broussard GJ, et al. Fast and sensitive GCaMP calcium indicators for imaging neural populations. *Nature* 615:884–891 (2023). doi:10.1038/s41586-023-05828-9
31. Borden PM, Zhang P, Shivange AV, Marvin JS, Cichon J, Dan C, Podgorski K, Figueiredo A, et al. A fast genetically encoded fluorescent sensor for faithful in vivo acetylcholine detection in mice, fish, worms and flies. bioRxiv (2020). doi:10.1101/2020.02.07.939504
32. Valley MT, Moore MG, Zhuang J, Mesa N, Castelli D, Sullivan D, Reimers M, Waters J. Separation of hemodynamic signals from GCaMP fluorescence measured with wide-field imaging. *J Neurophysiol* 123(1):356–366 (2020). doi:10.1152/jn.00304.2019
33. Hesterberg TC. What teachers should know about the bootstrap: resampling in the undergraduate statistics curriculum. *Am Stat* 69(4):371–386 (2015). doi:10.1080/00031305.2015.1089789
34. Tucker-Davis Technologies. Fiber Photometry Gizmo (Synapse) and Fiber Photometry Epoch Averaging Example (Python/MATLAB SDK). tdt.com/docs (accessed 2026-10-25).
35. fpExplorer source, github.com/ilo21/fpExplorer, `fpExplorer_src/fpExplorer_functions.py` (`normalize_dff`, `normalize_pMat`; accessed 2026-10-25).
36. FlyBase. GRAB(ACh4h), FBto0001083 (release FB2026_03).
