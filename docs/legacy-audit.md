# Audit of the legacy MATLAB pipeline

Scope: `legacy/matlab/` — `GRAB_ACh3_0_all_in_one.m` (GRABBER v0.3.8),
`approachinitiated_3_…m` (same code, different config), `analyze_GRAB_ACh3_0_events.m`
plus `+grabACHAnalysisTools/`, and the older `new.m` / `Master_CLK_modified_0325.m`.

Findings are ordered by how much they can change a published number. Anything
already analyzed with these scripts should be spot-checked against the items marked **(results)**.

## Silent correctness bugs

1. **(results) Trial z-scoring was silently skipped with the shipped config.**
   `analyze_GRAB_ACh3_0_events.m` sets the epoch to `[-4, +4] s` but the baseline
   window to `[-10, -2] s`. `normalize_epochs_trial_by_trial` only sees samples inside
   the epoch, so the baseline is `[-4, -2] s`, which works. But any config where
   the baseline lies wholly outside the epoch (e.g. `pre_event_sec = 1`) hits the
   `< 2 samples` branch, which *returns the un-normalized epochs* with only a warning,
   while the caller still labels plots "Trial-Norm: zscore". Also, the configured
   baseline silently differs from the one actually used.
   → New engine: hard error with the exact sample count (`events::baseline`).

2. **(results) Event times are offset by the trim window.** The preprocessing script
   re-zeros time after trimming (`processed_final.time - processed_final.time(1)`),
   so the exported `Time_sec` column starts at 0 at `analysis_window_start_seconds`
   (3 s or 10 s). Event times typed into the event script are recording-relative
   (from video/TDT), so every epoch is shifted by 3–10 s unless the user subtracted
   the trim by hand.
   → New engine: one block-relative clock everywhere; trimming never moves t = 0.

3. **(results) Stream start offset ignored.** Time was built as `(0:n-1)/fs`, ignoring
   the stream's `startTime` in the TDT block. TDT epocs are relative to block start;
   the stream may start a few ms later.
   → `simplefp-tdt` keeps `start_time` and all events use the same block clock.

4. **Peak/AUC windows exceeded the epoch.** Peak and AUC windows were `[0, 10] s`
   with `post_event_sec = 4`, so metrics were computed on `[0, 4] s` while the CSV
   implied 10 s.
   → New engine validates windows against the epoch and errors.

5. **Only positive peaks were measured.** GRAB-ACh3.0 reports ACh *decreases* as
   well as increases; `quantify_event_responses` took `max()` only.
   → Metrics now report both peak and trough with latencies, and signed AUC.

6. **NaNs replaced with 0** before epoching, which creates fake dips/peaks in
   dF/F (%) units. → NaNs are never introduced by the new pipeline; gaps are masked.

7. **Sampling rate lookup never matched.** The event script looks for a
   `Sampling_Rate_Hz_Final` row that the preprocessing export never wrote, so it
   always fell back to `1/mean(diff(t))`. Harmless here, but the fallback hid the bug.

## Method issues

8. **"standard" paradigm has no motion correction.** It fits a bi-exponential to
   465 nm alone; the 405 nm channel is filtered, fitted, then discarded.
9. **"subtraction" paradigm mixes units.** `dF = (465 − fitted405) − poly2`, then
   divides by the *mean of raw 465 nm*. The numerator is detrended; the denominator
   is a constant that includes bleaching. dF/F is therefore scaled by an arbitrary
   session-level constant and is not comparable between sessions with different bleaching.
10. **"isosbestic" paradigm (F0 = fitted 405)** is the classic Lerner et al. 2015
    approach; fine only if 405 nm is a valid control for this sensor — see
    `docs/science/methods-spec.md` for GRAB-sensor caveats.
11. **Errors as SEM across trials of one animal** (`plot_average_event_trace`) treats
    trials as independent subjects. Fine as a description of one session, not as
    group inference. → Bootstrap CIs, with the resampling unit explicit.
12. **`jet` colormap** in heatmaps is perceptually non-uniform and misleading for
    signed data. → Diverging, perceptually uniform map centered on 0.
13. **`Master_CLK_modified_0325.m`** divides 465 by a moving mean of *405* and
    multiplies by 100 (a ratio, not ΔF/F), then z-scores the whole session.
14. **`new.m`** removes "spikes" by derivative MAD thresholding and interpolation.
    This can delete genuine fast ACh transients; it is not used in the new defaults.
15. **BEADS paradigm** has six hand-tuned hyper-parameters with no published
    validation for photometry; dropped from defaults (available later as a plugin).

## Engineering issues

- Hardcoded absolute paths, events typed into the script, results keyed by
  timestamped filenames → replaced by declarative presets and a run manifest.
- Duplicated scripts differing only in config (`approachinitiated_3_…`).
- Parameters exported as stringified CSV; no software version or input hash.
  → Every run writes `run.json` with preset, engine version and input file digests.
