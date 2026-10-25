# Working in this repo

- Read `README.md`, then `docs/architecture.md`. The science behind every default is in `docs/science/methods-spec.md`.
- Run `cargo test --release` before and after any change. Validation tests compare against simulated ground truth
  (`crates/core/src/sim.rs`). A method change that lowers their accuracy needs a written justification.
- Build the Mac app with `apps/macos/build.sh`. Swift code lives in `apps/macos/Sources/SimpleFP`; the bindings in
  `Generated/` are regenerated on every build.
- Keep `simplefp-core` free of I/O. Put file and format logic in `crates/tdt` or `crates/engine`.
- Presets are strict by design (`deny_unknown_fields` plus `Preset::validate`). Add keys deliberately and document them
  in `docs/presets.md`.
- Never make the time axis relative to anything except the TDT block start.
