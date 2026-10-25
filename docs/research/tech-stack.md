# Tech stack research (2026-10-25)

Machine: Apple M5 Pro, macOS 26.6, Xcode 26.2 (17C52), Swift 6.2.3, rustc 1.98.0.
Versions are from crates.io (API queried 2026-10-25). "Tested" means built and run on this machine;
throwaway projects are in the session scratchpad (`.../scratchpad/stack/`).

## TL;DR verdicts

| Area | Verdict |
|---|---|
| Bend / HVM | **Don't use it.** F32 only (no f64), affine values (arrays can't be shared), compiler is "99% AI-written", no documented embedding API for Rust. |
| GPUI | **Works, but it's a moving target.** The crates.io `gpui` 0.2.2 is a year old. It builds and runs here, but only with `runtime_shaders` or once the Metal Toolchain is downloaded. No upstream accessibility. Third-party pieces (charts, inputs, a11y) live in gpui-component or gpui-ce. |
| Rust to SwiftUI | **UniFFI 0.32.2 (proc-macro, library mode)**, packaged as XCFramework/SPM. Swift 6 strict concurrency compiled with 0 warnings (tested, async included). **Pass bulk samples as bytes (`Vec<u8>` to `Data`), not `Vec<f32>`.** That was about 80x faster in testing. |
| Plotting 1e5-1e6 points | Swift Charts `LinePlot` takes **about 4 µs/point (1e6 points = 4 s per render, tested)**. Use it for summary plots only. Draw raw traces with Metal (or Canvas) from Rust-side min/max (M4) decimation to about 2 points per pixel. |
| TDT reader | One crate exists (`openreadout-ephys` 0.4.0, 2 weeks old, about 60 downloads). Use it as a reference, or vendor it. Own the reader and validate it against the Python `tdt` package on real tanks. |
| NWB | No mature Rust NWB writer. Write NWB 2.x with `hdf5-metno` 0.15 (`static` feature), or use `hdf5-pure` 0.47 for zero C dependencies. |
| DSP | Butterworth + filtfilt: `sci-rs` 0.4.1 (`butter_dyn`, `sosfiltfilt_dyn`, savgol), but it was last released 2024-11. Alternatively, own about 150 lines of SOS code checked against SciPy golden vectors. |
| Scripting | **Luau via `mlua` 0.12.2** (sandboxed). It was about 27x faster than Rhai and 9x faster than CPython in testing, and LLMs write Lua fluently. **Pin `luau0-src = 0.22.0`, because 0.22.1 (2026-10-24) panics on `Lua::new()`.** |

---

## 1. Bend / HVM

- **Current state:** `HigherOrderCO/Bend` `main` is now **"Bend 2"**, a new language: "Python + dependent types", with laws and proofs checked by the compiler. "Bend 1 programs and HVM do not carry over." It installs with `curl -fsSL https://bend-lang.com/install.sh | sh`, not cargo. The last crates are `bend-lang` 0.2.38 (2025-02, Bend 1) and `hvm` 2.0.22 (2024-08, HVM2). There are no HVM3/HVM4 crates on crates.io.
  Source: https://github.com/HigherOrderCO/Bend (README).
- **Numeric types:** "Numbers are Nat, U32 and F32 only: no U64, I64 or F64 (Metal has no f64)." Bend 1/HVM2 had u24/i24/**f24**. Bend 2 moved to F32. "F32 is axiomatic", and the README notes that backends may differ, for example in NaN handling.
- **Targets:** C, **Metal**, CUDA and JavaScript (JS is single-core). GPU runs need Metal or CUDA 12, and the limit is "one GPU per program". Parallelism happens only for balanced divide-and-conquer recursion.
- **IO / FFI:** Effects are print, env, time, sleep, spawn, channels, files, TCP and UDP. Missing libraries "can be added as foreigns", but this is undocumented. The compiler emits one C file per program. **No library or embedding API for calling Bend from Rust is documented.** You would have to vendor the generated C.
- **Maturity:** "BEND IS YOUNG. EXPECT BUGS"; "The compiler (not kernel) is 99% AI-written and not yet fully audited"; no debugger, profiler or REPL; strings are linked lists; values are affine, so arrays and closures cannot be shared.
- **For DSP (IIR filtfilt, regression, bootstrap):** **No.** IIR filtering and least-squares regression need f64. Cascaded f32 IIR at fs of about 1 kHz with low cutoffs is numerically fragile. Affine arrays fight in-place signal buffers. Bootstrap is embarrassingly parallel, and `rayon` already saturates the M5 Pro cores. If you need GPU work, Metal compute via `objc2-metal`, or MLX, is the sane route.

## 2. GPUI (Zed)

- **crates.io status:**
  - `gpui` **0.2.2**, published 2025-10-22. It has not been updated in a year, while Zed `main` keeps moving.
  - `gpui-pre` **0.3.9** (2026-10-12) is a "gpui-pre snapshot of zed@93d7456". It is what `gpui-component` 0.8 pins (`gpui-pre =0.3.9`).
  - The community fork is `gpui-ce` **0.3.0** (2026-10-20). It declares an `accesskit ^0.24` dependency.
  - gpui.rs says: "For the near future, GPUI is tied to Zed". Expect API churn and pin exact versions or git revs.
- **How to depend:** `gpui = { version = "0.2.2", features = ["runtime_shaders"] }`, or `gpui-pre = "=0.3.9"`, or a git dependency on `zed-industries/zed` pinned to a rev.
- **macOS requirements, tested here:**
  - A clean `cargo build` of 453 crates took 24.5 s wall, then **failed**: `metal shader compilation failed: cannot execute tool 'metal' due to missing Metal Toolchain`. Xcode 26 no longer bundles the toolchain.
  - Fix A (global, not done): `xcodebuild -downloadComponent MetalToolchain`.
  - Fix B: the `runtime_shaders` feature, which compiles shaders at runtime. The build then succeeded (9.7 s incremental), and the hello-world window ran for 5 s without errors.
- **Components and charts:**
  - Upstream GPUI is low-level: div/flex, canvas `paint_path`, and an `EntityInputHandler` input example, but no ready-made text field.
  - `gpui-component` **0.8.0** (longbridge/gpui-kit, 18.6k stars, published 2026-10-19) has 60+ components: Input/TextArea, a Tree-sitter code editor, virtualized Table/List, dock layout and **built-in charts**. It powers the commercial Longbridge Pro app.
  - Plotting crates: `plotters-gpui` 0.1.1, `gpui-plot`, `gpui-px`, `gpui-d3rs`, all small or stale. For 1e6-sample traces you would write a custom canvas element anyway.
- **Accessibility:** None in upstream GPUI (no AccessKit or VoiceOver). An "accessbility" branch in gpui-component (2026-06) adds AccessKit roles, but I could not confirm it was merged. gpui-ce depends on accesskit, but its coverage is unverified. **This is a real gap for an academic tool.**
- **Third-party apps:** about 100 are listed on https://github.com/zed-industries/awesome-gpui. Examples: Zed, Longbridge Pro, GitComet, Cap (screen recorder), Futureboard Studio (DAW), many database and HTTP clients.
- **Verdict:** Choose GPUI only if Linux/Windows parity matters more than native macOS feel and accessibility. For macOS-first, SwiftUI plus a Rust core is the safer bet.

## 3. Rust and SwiftUI interop

- **UniFFI** `uniffi` **0.32.2** (2026-09-23, Mozilla, about 5.4M downloads in 90 days).
  - The docs say Swift 6 support is "partial", and that async code may not be Sendable (issue #2448). https://mozilla.github.io/uniffi-rs/latest/swift/overview.html
  - **Tested:** a proc-macro crate with a Record, an Object holding a Mutex, an `async fn`, and an object passed into `Task.detached`. `swiftc -swift-version 6 -strict-concurrency=complete` gave **0 errors and 0 warnings**, and the program ran correctly.
  - **Tested bulk transfer:**
    - `Vec<f32>` to `[Float]`: 1e6 samples took **159-169 ms**, 1e7 took **1.5 s**. Elements are serialized one at a time.
    - `Vec<u8>` to `Data`, then a `bindMemory` cast: 1e6 took **2.0 ms**, 1e7 took **20.8 ms**.
    - **Rule: send bulk arrays as bytes**, or better, keep data in Rust and hand Swift only decimated render buffers.
- **swift-bridge** 0.1.59 (2026-01). Lower overhead and some zero-copy, but it is pre-1.0, has a single maintainer and a smaller community. Not needed.
- **cargo-swift** 0.11.1 (2026-05, about 4k downloads in 90 days). It wraps UniFFI into an SPM package and XCFramework. It is convenient, but a 10-line script is equally good and has fewer moving parts.
- **Best practice:**
  1. Use a `fpcore-ffi` crate with `crate-type = ["staticlib", "lib"]`, UniFFI proc-macros, and a bundled `uniffi-bindgen` bin.
  2. Run `cargo build --release --target aarch64-apple-darwin`. Add x86_64 plus `lipo` only if Intel support is wanted.
  3. Run `uniffi-bindgen generate --library <lib> --language swift --out-dir gen`.
  4. Run `xcodebuild -create-xcframework -library libfpcore_ffi.a -headers gen/headers -output FPCore.xcframework`.
  5. Wrap it as a local SPM `binaryTarget` plus the generated `.swift`.
  6. Long jobs go through `async fn`, on Rust's own thread pool. Report progress with a callback trait, which must be Sendable.
- **Swift Charts with large series, tested with `ImageRenderer` at 1600x400:**

  | Points | `LinePlot` (vectorized, macOS 15+) | `LineMark` |
  |---|---|---|
  | 1e4 | 0.09 s | 0.04 s |
  | 1e5 | 0.40 s | 0.39 s |
  | 1e6 | **4.1 s** | n/a |

  That is about 4 µs/point, so it is not interactive above about 1e4 points. Forum reports agree (https://developer.apple.com/forums/thread/740314).
  - **Plan:** Rust does min/max-per-pixel (M4) decimation, about 3,200 points for 1600 px wide, which gives exact visual envelopes. Swift draws that with SwiftUI `Canvas` (fine at about 1e4 segments), or with a `MTKView` line-strip for zoom/pan at 120 Hz.
  - Swift Charts is fine for PETHs, AUC bars and heatmaps (`RectanglePlot`).
  - Crates: `minmaxlttb` 0.1.4 and `lttb` 0.2.0 exist, but M4 is about 20 lines and should be owned.

## 4. Rust crates

| Need | Recommended | Version (last release) | Notes |
|---|---|---|---|
| TDT tank (tsq/tev/sev/Tbk) | `openreadout-ephys` | 0.4.0 (2026-10-18; created 2026-10-03; ~60 dl) | ~2k LOC TDT module: streams, snippets and epocs, .sev naming, tests on synthetic fixtures. **Too new to depend on blindly**: vendor or port it, and golden-test against the Python `tdt.read_block` on real tanks. No other Rust TDT crate exists. |
| HDF5 / NWB | `hdf5-metno` | 0.15.0 (2026-09) | Maintained fork of `hdf5` (the original is 0.8.1, abandoned). `features=["static","zlib"]` builds libhdf5 from source, so there is no Homebrew dependency. |
| HDF5, pure Rust | `hdf5-pure` | 0.47.0 (2026-09) | No C, writes in place, active. Check NWB compatibility (it needs references and attributes) before choosing it. |
| NWB schema | none mature | `consus-nwb` 0.4.0 (~140 dl), `nwbview` (viewer) | Write the minimal NWB 2.x layout yourself (TimeSeries, `/acquisition`, specs in `/specifications`), then validate with `pynwb`/`nwbinspector` in CI. |
| Butterworth + filtfilt | `sci-rs` | 0.4.1 (2024-11, stale) | Has `butter_dyn`, `sosfiltfilt_dyn`, `sosfilt_zi_dyn`, `savgol_filter_dyn`, mirroring the SciPy API. Alternatives: `scirs2-signal` 0.6.5 (huge SciPy clone, active), `filtfilt` 0.2.1, `numra-signal` (tiny). Recommendation: own a SOS butter + sosfiltfilt with odd-padding (about 150 lines) and assert against SciPy outputs. |
| Linear algebra | `faer` | 0.24.4 (2026-06, now on Codeberg) | Fastest pure-Rust dense LA (QR/least squares). `nalgebra` 0.35.0 if a crate requires it. |
| Robust regression (IRLS Huber/Tukey) | own IRLS on `faer`, or `anofox-regression` | 0.5.37 (2026-10, active) | IRLS is about 40 lines. `solow-robust` 0.7.4 is very new. |
| Curve fit (Levenberg-Marquardt) | `levenberg-marquardt` | 0.15.0 (2025-08) | rust-cv. Depends on `nalgebra ^0.34` (one version behind). `argmin` 0.11.0 for general optimization. |
| Statistics / distributions | `statrs` | 0.19.1 (2026-08) | Distributions, t/CDFs. Bootstrap: own (`rand` + `rayon`; about 20 lines, seedable for reproducibility). |
| Arrow / Parquet | `arrow`, `parquet` | 60.0.0 | Apache. `polars` 0.55.2 if you want dataframes, but it is a heavy compile. |
| Parallelism | `rayon` | 1.12.0 | |
| SIMD | `pulp` 0.22.3 (used by faer) or `wide` 1.7.1 | | `std::simd` is still nightly-only on 1.98 (E0554, tested). NEON via `std::arch::aarch64` is stable. Autovectorization usually suffices. |
| FFT (if needed) | `rustfft` 6.4.1 / `realfft` 3.5.0 | | |

## 5. Embedded scripting

The same loop (`sum sqrt(i)` for i up to 1e7) was run in each, in release mode on this machine:

| Option | Version | 1e7 loop | Sandbox | LLM fluency | Packaging |
|---|---|---|---|---|---|
| **Luau** (`mlua`, feature `luau`) | 0.12.2 | **60 ms** | built in (`lua.sandbox(true)`), memory and interrupt limits | high (Lua/Roblox corpus) | vendored C++, about 1 MB, no system dependency |
| Rhai | 1.27.0 | 1,632 ms | safe by construction | low-medium (niche syntax) | pure Rust |
| CPython (PyO3 0.30.1) | 3.x | 528 ms | **none**: a script can do anything | highest; numpy/scipy | must bundle an interpreter (tens of MB), plus GIL and notarization pain |
| WASM (wasmtime 50 / extism 1.41) | | near native | strong | indirect (the agent must write Rust/AssemblyScript plus a build step) | heavy toolchain for users |

- **Recommendation: Luau via mlua.**
  - Scripts orchestrate. Heavy kernels (filter, fit, bootstrap) stay in Rust, exposed as host functions on userdata arrays, so per-sample loops never run in script.
  - Presets stay declarative (TOML/JSON) and call the same host API.
  - Give the agent a typed API stub (Luau supports type annotations) and examples in `presets/`.
  - Optionally add "export to Python" (Parquet/NWB plus a generated notebook) for power users, rather than embedding CPython.
- **Gotcha found:** `luau0-src` 0.22.1+luau744 (released 2026-10-24) makes `mlua` 0.12.2 panic in `Lua::new()` with "attempt to load a text chunk (mode is 'b')". Pinning `cargo update -p luau0-src --precise 0.22.0+luau740` fixes it. The Cargo.lock should pin it until mlua or luau0-src ships a fix.
