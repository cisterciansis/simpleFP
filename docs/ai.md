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
| 2. Pose | next | Keypoints (nose, ears, body, tail base) per frame | Core ML runner. Permissive models first (RTMPose / ViTPose, Apache-2.0); SuperAnimal-TopViewMouse as a user download; import DLC/SLEAP/Lightning Pose |
| 3. Behavior classifier | next | Lab-trained detectors (grooming, rearing, eating, freezing…) from scored sessions | Rust kinematic features → Create ML boosted trees in-app → Rust bout cleanup → proposals → review |
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

## Building on it
- New detectors write `Annotation`s with `source: model` and `model: "<name>@<version>"`, and must pass the
  agreement check against human scorers.
- The assistant's suggested changes go through `preset::with_overrides`, the same validator as Easy mode and
  `--set`. A model can never produce an invalid analysis.
