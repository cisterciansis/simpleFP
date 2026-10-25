# AI in SimpleFP

Research behind the choices: [research/behavior-ai.md](research/behavior-ai.md) and [research/video-ai.md](research/video-ai.md).

## Principles
1. **Local first.** Inference runs on the Mac (Core ML, MLX). Remote models are opt-in, OpenAI-compatible, and
   preferably attested TEEs (Chutes `confidential_compute`, Tinfoil).
2. **Summaries out, data in.** The assistant receives QC, the preset and event statistics, never raw traces or video
   frames. Sending frames to a remote model would need a separate, explicit opt-in.
3. **Models propose, people decide.** Every model output is an annotation with `source = model`, a model version
   and a confidence, starting in `review = proposed`. Unreviewed proposals never reach statistics.
4. **Every model is measured.** Agreement with a human scorer (frame κ + PABAK, bout F1, onset error) is computed on
   held-out sessions before a model's output is used.
5. **Licenses travel with weights.** Non-commercial weights (e.g. DLC SuperAnimal) are downloaded by the user
   behind a license prompt and never bundled. AGPL models (Ultralytics YOLO) are not used.

## Tiers

| Tier | Status | What it does | How |
|---|---|---|---|
| 0. Classical tracking | **shipped** | Arena ROI (auto, editable), centroid, area, orientation, speed, motion energy, immobility bouts on the TDT clock | `simplefp-video::track` (Rust); Swift decodes luma with AVAssetReader; CLI `simplefp track` |
| 1. Lab Assistant | **shipped (beta)** | Questions about a recording; `SET key = value` suggestions become one-click, validated parameter changes | OpenAI-compatible provider (local `mlx_lm.server`/Ollama, Chutes, Tinfoil); key in Keychain |
| 2. Pose | **shipped (bring your own model)** | Keypoints per frame from any Core ML heatmap model, centred on the tracked animal; DLC-style CSV export; pose features feed the classifier | Swift Core ML runner + `<model>.pose.json` sidecar; heatmap decoding (subpixel) and crop→frame mapping in Rust (`simplefp-video::pose`). Permissive models (RTMPose/ViTPose) or SuperAnimal as a user download |
| 3. Behavior classifier | **shipped** | Lab-trained detectors from the lab's own scored bouts; cross-validated agreement shown before saving; proposals go to a review queue | Rust: windowed kinematic (+ pose) features → gradient-boosted trees → hysteresis bouts with decoding priors from the human bouts. Validation holds out time blocks or sessions. App: Train sheet, Detect menu, Review queue. CLI: `classify-train`, `classify-predict` |
| 4. VLM triage | later | "Find candidate social contacts", session descriptions | mlx-swift VLMs (Qwen3-VL / Gemma, Apache-2.0); seconds-scale candidates only, never coordinates or final labels |
| 5. Two animals | later | Identity-preserving tracks for social interaction | ByteTrack-style matching in Rust using the tether as an identity cue, plus a review queue for swaps |
| 6. Our own model | research | A behavior foundation model trained on lab-scored video | Starts from tier-3 data; learning curves counted in bouts; validated against humans across labs |

## Validation on lab data (2026-10-25)
- The tracker runs at about 3,000 frames/s at 320 px, and the auto-detected arena matched the box. The mouse is
  correctly absent while it is in the anesthesia nose cone.
- Problems found on real footage, now fixed:
  - camera auto-exposure caused false detections, now handled by exposure compensation plus a contrast floor;
  - H.264 keyframe re-quantization broke immobility bouts, now handled by a 0.5 s running median on motion energy;
  - the whole-frame threshold picked up lab bench and people, now handled by the enclosed-arena ROI.
- Framing differs between sessions, so the ROI is shown and editable before results are used.

## Classifier validation on lab data (2026-10-25)
- **Training:** tail suspension in 90839_TS, three human-scored bouts.
- **Held-out time blocks:** frame κ 0.80, PABAK 0.91, bout F1 0.75, median onset error 0.10 s.
- **Unseen animal (102295_TS):** exactly three proposals, starting at 128.2, 275.7 and 481.7 s. The human-scored
  starts are 129, 275 and 480 s, and there were no false positives. The AVFoundation decoder (app) and the ffmpeg
  decoder (CLI) produce identical proposals.
- **Caveat:** this is one behavior, one training animal and one test animal. Treat it as a promising first check,
  and confirm with more scored sessions.
- **Pose plumbing check:** a synthetic Core ML "darkness" model's keypoint lands a median 9.9 px from the tracker's
  centroid in 640-px frames. This validates decoding, cropping and coordinate mapping; a real pose model is still
  needed to judge accuracy.

## Building on it
- New detectors write `Annotation`s with `source: model` and `model: "<name>@<version>"`, and must pass the
  agreement check against human scorers.
- The assistant's suggested changes go through `preset::with_overrides`, the same validator as Easy mode and
  `--set`. A model can never produce an invalid analysis.
