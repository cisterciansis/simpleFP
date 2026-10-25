# Architecture

```
            ┌──────────── front-ends ─────────────┐
            │  apps/macos (SwiftUI)   crates/cli  │   future: GPUI (Linux/Windows), Luau scripts, web
            └──────┬──────────────────────┬───────┘
                   │ UniFFI (crates/ffi)  │
            ┌──────▼──────────────────────▼───────┐
            │ crates/engine   load · analyze · export · provenance
            └──────┬──────────────────────┬───────┘
          ┌────────▼───────┐      ┌───────▼────────┐
          │ crates/tdt     │      │ crates/core    │  dsp · fit · stats · pipeline
          │ TSQ/TEV/SEV    │      │ events · preset · analysis · sim
          └────────────────┘      └────────────────┘
```

## Principles

- **The core is pure.** `simplefp-core` takes slices and returns values, with no I/O, threads or globals except
  rayon in the bootstrap. Everything is deterministic from a seed, so results reproduce bit-for-bit across machines.
- **One clock.** Every time is seconds on the TDT block clock. Trimming excludes samples from fitting; it never moves t = 0.
- **Presets are data.** Every analysis decision is in a TOML preset that is validated, hashed and written to `run.json`.
  Code changes the *methods*; presets choose among them.
- **Fail loudly.** Invalid windows, missing stores, non-positive baselines and unknown preset keys are errors, not
  silent fallbacks. The few deliberate fallbacks (high-pass bleaching when the double exponential is implausible,
  disabling motion correction on a negative slope) are recorded as QC warnings.
- **Heavy data stays in Rust.** Swift never receives full traces. It asks for a min/max envelope sized to the view in
  pixels (`Session::envelope`), so drawing costs O(pixels) whatever the recording length. Bulk arrays cross the FFI as
  bytes (about 80× faster than UniFFI element lists; see docs/research/tech-stack.md).

## Technology decisions (2026-10)

| Decision | Why | Revisit when |
|---|---|---|
| Rust core | f64 numerics, SIMD-friendly, one engine for every front-end | — |
| **SwiftUI + UniFFI** for the Mac app | Truly native (Liquid Glass, inspector, tables, menus, accessibility); UniFFI 0.32 compiles cleanly under Swift 6 strict concurrency | — |
| **GPUI deferred** | Works (gpui 0.2.2 / `gpui-ce`), but the crates.io release lags Zed, there is no accessibility, and it needs the `runtime_shaders` workaround on Xcode 26. Reserved for a Linux/Windows shell on the same `engine` crate. | GPUI ships stable releases with accessibility |
| **Bend not used** | Bend 2 numerics are Nat/U32/F32 only (no f64), there is no Rust FFI, and the compiler self-describes as unaudited. Scientific IIR filtering and regression need f64. | Bend gains f64 and an embeddable runtime |
| Own DSP and fitting (~400 lines) | Validated against SciPy golden values; avoids stale crates (`sci-rs` last released 2024-11) | — |
| Plots: SwiftUI `Canvas` + engine envelopes | Swift Charts takes about 4 s per 10⁶ points; Canvas on about 3k envelope points is instant | Metal if we need 120 Hz pan on many channels |
| CSV + JSON outputs | Universally readable; FLMM-ready long format | Parquet/NWB when cohort pipelines need them |
| Luau (`mlua`) for user scripts | Fastest embeddable sandboxed option measured; agents write Lua well (roadmap) | — |

## Adding a method

1. Implement it in `crates/core` as a pure function with a unit test (golden values or a ground-truth simulation).
2. Add a variant to the relevant preset enum (`BleachMethod`, `MotionMethod`, …) with bounds in `Preset::validate`.
3. Cite it in `docs/science/methods-spec.md` and state when to prefer it.
4. If it changes the default, add a validation test showing it does at least as well on the simulator.

## Building the Mac app

`apps/macos/build.sh` performs these steps:
1. builds `simplefp-ffi` as a static library (`MACOSX_DEPLOYMENT_TARGET=15.0`);
2. generates the Swift bindings;
3. wraps the static library in `Frameworks/SimpleFPCore.xcframework`;
4. builds the SwiftPM executable;
5. assembles and ad-hoc signs `build/SimpleFP.app`.

Generated files are gitignored. For distribution, use a rustup toolchain: Homebrew's Rust std is built for macOS 26
and makes the linker warn when targeting 15. Developer ID signing and notarization are also needed.
