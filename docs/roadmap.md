# Roadmap

North star: one native app where a lab goes from cage to figure: animals and cohorts, sessions, synchronized video
and photometry, behavior scoring (manual and AI), rigorous statistics, and publication figures. Everything stays
reproducible from a preset and a manifest.

## v0.1 (this release): single-session photometry
- [x] Native TDT reader (streams, epocs, SEV), block clock throughout
- [x] Cited pipeline: low-pass → decimate → bleaching → IRLS → ΔF/F; `fit_raw`, `ols`, `none` alternatives
- [x] Peri-event epochs, bootstrap CI + consecutive threshold, metrics, control-channel response QC
- [x] Presets with inheritance and strict validation; CLI; provenance manifest; FLMM export
- [x] SwiftUI app: library, LOD trace with pan/zoom, overview, PETH + control overlay, heatmap, metrics, QC inspector, preset editor
- [x] Ground-truth simulator and validation suite

## v0.2: cohorts and correct group inference
- [x] Cohort manifests (`cohort.toml`): sessions with animal/group/condition, exclusions with reasons, contrasts
- [x] Two-stage inference with the animal as the unit:
  - per-animal mean waveforms, then group bootstrap CI;
  - between-group CI and permutation tests;
  - hierarchical bootstrap for metrics;
  - Benjamini–Hochberg correction across events and metrics
- [x] Mac cohort view: group bands, contrast curves, estimation plot, tests table; build a cohort from open recordings
- Sidecar `session.toml` next to each block (portable session metadata)
- Invalid-sample masking (dropouts, flat runs, ADC saturation with a hardware `adc_max`) that drops overlapping epochs
- Spontaneous transient detection (GuPPy two-threshold MAD; prominence method)
- Event overlap policy (`exclude_if_preceded_s`) and bout detection for feeding
- Multi-site blocks (465A/405A plus 465C/405C) side by side
- Figure export (PDF/SVG at journal sizes) from the same Canvas renderers; methods paragraph generated from `run.json`
- Parquet export; NWB writer validated against `pynwb`

## v0.3: programmable
- Luau scripting via `mlua` (sandboxed): scripts receive engine results and call Rust kernels (filters, fits,
  bootstrap), returning new traces, metrics or figures. Pin `luau0-src` to 0.22.0; 0.22.1 crashes `mlua`.
- Preset and script library with search (by sensor, region, rig); shareable as a single `.toml` + `.luau` bundle
- Agent interface: a documented JSON-over-stdio mode in the CLI so an agent can inspect blocks, propose a preset,
  run it and read QC without the GUI

## v0.4: behavior video (design: docs/research/video-ai.md)
- [x] `simplefp-video`: frame clock from the `Cam1` epoc. Frame-number gaps are errors; extra trailing frames are
  allowed, as TDT TN0987 documents; inter-frame jitter and gap QC; clock hash.
- [x] Annotations with provenance (scorer, manual/model, model version, confidence, review). Unreviewed model
  proposals never reach analysis.
- [x] Agreement metrics: frame κ + PABAK + prevalence, event F1, bout F1 at tIoU 0.5, onset error
- [x] DeepLabCut CSV import, speed, distance to ROI
- [x] Mac player:
  - sample-table frame times mapped through edit lists, zero-tolerance seeks;
  - trace playhead synchronized in both directions;
  - ethogram hotkeys for point and bout events;
  - per-scorer sidecar files;
  - "Analyze These Events" feeds the photometry pipeline;
  - demo arena video
- Next:
  - agreement view in the app;
  - blinded scoring mode;
  - pose overlay and ROI editor;
  - Lightning Pose / SLEAP h5 / NWB ndx-pose import;
  - CamHal AVI transcode on import
- Pose and tracking: run models on-device (Core ML / MLX) such as keypoints, centroid and region-of-interest occupancy;
  import from DeepLabCut, SLEAP and Bonsai
- AI-assisted scoring: vision models propose events, the user confirms; inter-rater agreement reported
- Continuous-behavior regression (speed, distance-to-food) against ΔF/F

## AI (design: docs/ai.md)
- [x] Classical tracking: arena ROI (auto, editable), centroid, speed, motion energy, immobility on the TDT clock (app + CLI)
- [x] Lab Assistant over any OpenAI-compatible provider (local MLX/Ollama, Chutes TEE, Tinfoil); validated one-click
  parameter suggestions
- [x] Easy mode parameters + Code (TOML) + CLI `--set`, all through one validator
- [x] Core ML pose runner (bring your own model + sidecar), pose features, DLC-style export
- [x] Behavior classifier (Rust gradient-boosted trees) trained on the lab's scoring, with cross-validated agreement
  and a review queue in the app; CLI `classify-train` / `classify-predict`
- [x] Detector v2: calibrated probabilities and out-of-fold onset thresholds (from 10 bouts), v1 A/B on the same
  folds, learning curve in bouts; multi-session training and an uncertainty-ordered review queue in the app;
  accepted proposals count as training bouts
- [x] RTMPose-m (AP-10K) → Core ML conversion script (`tools/pose`), SimCC decoding, rotation TTA; validated on lab video
- Next:
  - fine-tune the pose model on lab frames (inverted mice in tail suspension fail);
  - more scored sessions (≥ 4 animals, ≥ 30 bouts) to re-test long windows, stacking and calibration;
  - VLM triage;
  - two-animal identity tracking;
  - Tinfoil/Chutes attestation verification

## v0.5: lab management (design: docs/research/colony-management.md)
- [x] Local SQLite store, hash-chained audit, interval cage history, atomic weaning and surgery records
- [x] Protocol-expiry guard, derived tasks (wean, post-op, protocol, capacity, genotype), animal timeline, notes
- [x] Cohorts from the colony: freeze with members hash, blinding codes, exclusions → analysis manifest
- [x] Mac: animals table + timeline, cage board with drag-to-move, QR cage cards (PDF), tasks
- Next:
  - wean wizard UI;
  - breeding setup UI;
  - ⌘K command bar (`mv A-12 C-031`) and barcode-scanner flows;
  - keyboard census;
  - histology pass with atlas plot;
  - CSV import with dry run;
  - pedigree view;
  - surgery templates
- Later: iPhone/iPad companion with camera QR, sync with real roles (CloudKit or self-hosted), welfare score sheets

## Cross-platform
- GPUI shell for Linux/Windows on the same `engine` crate once GPUI has stable releases and accessibility
- Hardware beyond TDT: Neurophotometrics (CSV), pyPhotometry, Doric, RWD, through new readers that return the same
  `Stream`/`Epoc` types

## Science backlog (methods-spec §12)
- Settle the 405 nm behavior of GRAB-ACh3.0 on lab data (control-channel PETHs, mutant cohort)
- Compare `irls` and `fit_raw` on mutant-sensor sessions
- Native FLMM, deferred until there is a validated reference implementation to test against
