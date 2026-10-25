# The preset standard (schema 1)

A preset is a TOML file that fully determines an analysis. It is the unit a lab shares, a paper cites (by id and
BLAKE3 hash, both in `run.json`), and an AI agent edits. Built-ins live in `presets/` and are embedded in the binary.

## Rules

- **Strict:** unknown keys are errors (`pre_sec` instead of `pre_s` fails loudly), and so are out-of-range values.
- **Inheritance:** `extends = "<id>"` deep-merges the parent's tables, so a child states only what differs. A method
  table that switches `method` (e.g. `[bleach] method = "window"`) replaces the parent's table whole. Parents
  are resolved from the built-ins and from `.toml` files in the same directory (the file stem is the id). Cycles are
  rejected.
- **Units are in the key name:** `_s` seconds, `_hz` hertz, `_nm` nanometres. Percent values are plain numbers.
- **Times are relative to the event** in `[events]`; times in `[preprocess]` are relative to the recording.

## Reference

| Key | Type | Meaning | Validation |
|---|---|---|---|
| `schema` | int | Preset schema version | must be 1 |
| `id`, `name`, `description` | string | Identity; `id` is what `--preset` and `extends` use | — |
| `extends` | string | Parent preset id | resolved before validation |
| `references` | [string] | Citations for this recipe | — |
| `[sensor] name, analyte` | string | e.g. `"GRAB-ACh3.0"`, `"acetylcholine"` | — |
| `[sensor] excitation_nm, control_nm` | number | LED wavelengths | — |
| `[sensor] control_is_isosbestic` | bool | False for GRAB sensors at 405 nm | — |
| `[sensor] tau_on_s, tau_off_s` | number | Sensor kinetics (documentation and future defaults) | — |
| `[channels] signal, control` | [string] | Candidate TDT store names, first match wins | signal non-empty |
| `[preprocess] trim_start_s, trim_end_s` | s | Excluded from fits and statistics; the clock is unchanged | 0–3600 |
| `[preprocess] lowpass_hz` | Hz or omitted | Zero-phase Butterworth; omit to disable | < target_fs/2 |
| `[preprocess] lowpass_order` | int | Filter order (applied twice by filtfilt) | 1–8 |
| `[preprocess] target_fs` | Hz | Decimate by `floor(fs/target_fs)` with block means | 1–1000 |
| `[bleach] method` | `"double_exp"` (`robust`), `"linear"`, `"highpass"` (`cutoff_hz`), `"window"` (`start_s`, `end_s`), `"none"` | Per-channel bleaching model. `window` sets F0 to the median over a reference period, for sessions with a tonic state change that detrending would erase. It is conservative for increases, because bleaching after the window is not corrected. | cutoff 1e-4–0.1; window ≥ 2 s |
| `[motion] method` | `"irls"` (`tuning`), `"ols"`, `"fit_raw"` (`tuning`), `"none"` | Control→signal artifact regression | tuning 1–10 |
| `[output] robust_z` | bool | Session z uses median/MAD instead of mean/SD | — |
| `[events] pre_s, post_s` | s | Epoch extent | 0–600 |
| `[events] trace` | `"dff"` or `"z"` | Which session trace epochs are cut from | — |
| `[events] baseline` | `"none"`, `"subtract"`, `"zscore"`, `"robust_zscore"` | Per-trial baseline | — |
| `[events] baseline_window` | [s, s] | Inside `[-pre_s, 0]`, ≥ 2 s long | hard error |
| `[events] metric_window` | [s, s] | Window for mean/AUC/peak/trough | inside the epoch |
| `[events] n_boot, ci_level, seed` | int, 0–1, int | Bootstrap CI settings | 100–1e6, 0.5–0.999 |
| `[events] min_consecutive_s` | s | Minimum run of CI-excludes-0 samples; use 1/lowpass_hz | 0–60 |

## Example: a lab override

```toml
schema = 1
extends = "grab-ach3.0-tdt"
id = "my-lab-feeding"
name = "My lab · feeding"
description = "GRAB-ACh3.0 with a longer post-event window for consummatory bouts."

[channels]
signal = ["465C"]
control = ["405C"]

[events]
post_s = 30.0
metric_window = [0.0, 10.0]
```

Run it:

```bash
simplefp run /path/to/block --preset presets/my-lab-feeding.toml --epoc PC0_
```

## For agents

To create a preset, copy the closest built-in with `extends` and change only what the experiment requires. Justify
each changed number against `docs/science/methods-spec.md`, and run `simplefp run` on the demo block
(`simplefp demo`) to confirm it validates. Never weaken validation to make a preset pass.
