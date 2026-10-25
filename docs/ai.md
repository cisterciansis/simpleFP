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
| 2. Pose | **shipped (bring your own model; RTMPose-m AP-10K converter in `tools/pose`)** | Keypoints per frame from any Core ML heatmap model, centred on the tracked animal; DLC-style CSV export; pose features feed the classifier | Swift Core ML runner + `<model>.pose.json` sidecar; heatmap decoding (subpixel) and crop→frame mapping in Rust (`simplefp-video::pose`). Permissive models (RTMPose/ViTPose) or SuperAnimal as a user download |
| 3. Behavior classifier | **shipped** | Lab-trained detectors from the lab's own scored bouts; cross-validated agreement shown before saving; proposals go to a review queue | Rust: windowed kinematic (+ pose) features → gradient-boosted trees → calibrated probabilities (from 10 bouts) → hysteresis bouts with decoding priors from the human bouts. Validation holds out time blocks or sessions and reports the v1 baseline and a learning curve on the same folds. App: multi-session Train sheet with learning curve, Detect menu, Review queue (least certain first; accepted proposals become training bouts). CLI: `classify-train`, `classify-predict` |
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
- **Detector v2 A/B (same held-out folds, 90839_TS, 3 bouts):**

  | Variant | Frame κ | Bout F1 |
  |---|---|---|
  | v1 (0.5 s + 2 s windows) | 0.797 | 0.75 |
  | + 5 s min/max (shipped) | 0.805 | 0.75 |
  | + 5 s and 15 s mean/std | 0.684 | 0.50 |
  | + stacked second pass | 0.41–0.64 | 0.40–0.50 |

  - Longer mean/std windows and stacking overfit at three bouts. Stacking was removed, and the long mean/std
    windows were dropped.
  - Calibration (Platt on out-of-fold scores, smoothed targets, ridge toward identity, damped Newton) and the
    out-of-fold onset threshold only switch on from 10 training bouts. Below that, the out-of-fold models see
    about one bout and mis-tune everything.
- **Unseen animal (102295_TS) with v2:** proposals at 129.5, 277.1 and 481.8 s against human onsets of 129, 275
  and 480 s, with no false positives.
- **Learning curve on 90839_TS (held-out κ):** 0.56 with 1 bout, then 0.80 with 2 and 3 bouts.
- **Two sessions, leave-one-session-out:** κ 0.87 and bout F1 0.92. The 102295 bouts were accepted model
  proposals, not independent human scoring, so treat this as a plumbing check of multi-session training.
- **Pose plumbing check:** a synthetic Core ML "darkness" model's keypoint lands a median 9.9 px from the tracker's
  centroid in 640-px frames. This validates decoding, cropping and coordinate mapping; a real pose model is still
  needed to judge accuracy.
- **RTMPose-m (AP-10K, Apache-2.0) via `tools/pose/convert_rtmpose.py`:**
  - fp32 Core ML matches PyTorch and the official ONNX export.
  - Speed: 4.1 ms/frame on the GPU (fp32), 0.6 ms/frame on the Neural Engine (fp16).
  - Top-down lab video: the keypoint midpoint sits 0.20 body-widths from the tracker centroid; 2.1 % of frames
    flip head and tail; jitter is 0.2 px; fp16 and fp32 differ by a median of 0.19 px.
  - Tail suspension: inverted mice fail. Rotation TTA barely helps (the gravity check goes from 64 % to 66 %).
  - Next step: fine-tune on labeled lab frames.

## Building on it
- New detectors write `Annotation`s with `source: model` and `model: "<name>@<version>"`, and must pass the
  agreement check against human scorers.
- The assistant's suggested changes go through `preset::with_overrides`, the same validator as Easy mode and
  `--set`. A model can never produce an invalid analysis.
