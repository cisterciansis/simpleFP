# SimpleFP

Fast, rigorous fiber photometry analysis for TDT recordings. The engine is Rust, the Mac app is native SwiftUI.
Built first for **GRAB-ACh3.0** acetylcholine imaging and designed to grow into a full behavior + photometry lab suite
(see [docs/roadmap.md](docs/roadmap.md)).

- **Native TDT reader**: reads TSQ/TEV/SEV blocks directly (no MATLAB, no Python), streams plus epocs, all on the block clock.
- **Literature-grounded pipeline**: zero-phase low-pass → integer decimation → per-channel double-exponential bleaching →
  robust IRLS control regression → ΔF/F. Every default is cited in [docs/science/methods-spec.md](docs/science/methods-spec.md).
- **Event statistics**: peri-event epochs, bootstrap CIs with the consecutive-threshold criterion, per-trial metrics,
  control-channel response check, long-format export for functional mixed models (fastFMM).
- **Declarative presets**: versioned TOML recipes with inheritance and strict validation ([docs/presets.md](docs/presets.md)).
- **Group analysis**: cohort manifests with the animal as the unit of inference. Bootstrap CIs across animals,
  between-group and within-subject contrasts, exact permutation tests, hierarchical bootstrap, Benjamini–Hochberg.
- **Lab store**: animals, cages, breeding, surgeries (virus, coordinates, fiber), sessions, exclusions and cohorts in
  one local SQLite file with a tamper-evident audit log. Frozen colony cohorts feed group analysis directly.
- **Provenance**: every run writes `run.json` with the resolved preset, its hash, engine version and BLAKE3 digests of the inputs.
- **Speed**: a 2-hour, 1 kHz, two-channel recording with 200 events and 10,000 bootstrap resamples analyzes in about 0.23 s on an M5 Pro.

## Quick start

```bash
cargo build --release
```

```bash
./target/release/simplefp demo /tmp/Demo-1
```

```bash
./target/release/simplefp run /tmp/Demo-1 --epoc Evt_
```

Real data: point `run` at a Synapse block folder (the one containing `.tsq`/`.tev`). Use `inspect` to see its stores:

```bash
./target/release/simplefp inspect "/path/to/Subject-250619-103950"
```

Manual (video-scored) events go in a CSV with header `event,time_s`, with times in seconds on the TDT block clock:

```bash
./target/release/simplefp run "/path/to/block" --events events.csv --preset grab-ach3.0-tdt
```

### Mac app

```bash
apps/macos/build.sh
```

```bash
open apps/macos/build/SimpleFP.app
```

Drop a block or a tank folder onto the window, or choose **File ▸ Open Demo Recording**. Requires Xcode 26 and Rust 1.90+.

## Outputs (`<block>/simplefp/` by default)

| File | Contents |
|---|---|
| `trace.csv` | time, decimated signal/control, bleaching baselines, motion fit, ΔF/F %, z, valid flag |
| `events/<label>/epochs.csv` | per-trial peri-event ΔF/F (baseline-subtracted), wide format |
| `events/<label>/summary.csv` | mean, SEM, bootstrap CI, significance mask |
| `events/<label>/control_summary.csv` | the same for the control channel (QC) |
| `events/<label>/metrics.csv` | per-trial mean, AUC (signed, + and − lobes), peak/trough and latencies |
| `flmm_long.csv` | long format for `fastFMM::fui()` |
| `run.json` | resolved preset + hash, QC, warnings, engine version, input digests |

Single-recording statistics treat trials as the unit and are **descriptive**. For group inference, use a cohort:

```bash
./target/release/simplefp demo-cohort /tmp/cohort
```

```bash
./target/release/simplefp cohort /tmp/cohort/cohort.toml
```

The cohort output (`simplefp-cohort/`) contains:
- `animals.csv`, `animal_curves.csv`: per-animal means;
- `group_curves.csv`, `contrast_curves.csv`: waveforms with CIs;
- `metric_summaries.csv`, `metric_tests.csv`: metric estimates with CIs, p and BH q;
- `flmm_long.csv`: trial-level data with animal, session and group columns;
- `cohort.json`: provenance;
- `sessions/`: every session's own export.

## Layout

```
crates/tdt      TDT block reader (no dependencies)
crates/core     DSP, fitting, pipeline, events/statistics, presets, simulator
crates/engine   I/O layer shared by all front-ends: load → analyze → export, cohorts
crates/lab      lab store (SQLite): colony, surgeries, sessions, cohorts, audit
crates/cli      `simplefp` command-line tool
crates/ffi      UniFFI bindings for Swift
apps/macos      SwiftUI app
presets/        built-in presets (embedded at compile time)
docs/           science spec, architecture, preset standard, roadmap, legacy audit
legacy/         original MATLAB scripts and the 2025 AI research notes (reference only)
```

Cohort manifests can also declare **window contrasts**: within-session state comparisons placed relative to events,
tested across animals.

```toml
[[window_contrast]]
name = "wake − iso"
a = { event = "awake", from_s = 30.0, to_s = 150.0 }
b = { event = "iso_off", from_s = -60.0, to_s = 0.0 }
```

## Validation

On real Synapse recordings (11 blocks), the TDT reader matches TDT's official Python reader (`tdt` on PyPI)
sample-for-sample on every stream and epoc. CamHal camera AVIs are transcoded once on import, with the frame count
verified.

`cargo test --release` runs the following:
- unit tests against SciPy golden values (Butterworth SOS design, `sosfiltfilt`);
- a TDT write→read round trip;
- end-to-end ground-truth tests on the simulator: recovered ΔF/F has r = 0.997 against the truth, and the evoked peak
  is 3.96 % against a true 4 %. No significance appears in the baseline, a ligand-dependent control is flagged, and the
  default method is checked to beat the `fit_raw` alternative under unequal bleaching.

What the old MATLAB pipeline got wrong is in [docs/legacy-audit.md](docs/legacy-audit.md). Some of those issues can
change published numbers.

## License

Not yet chosen. Until a license file is added, the code is public but all rights are reserved.
