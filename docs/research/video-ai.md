# Behavior video and AI research (2026-10-25)

Scope: the foundation for roadmap v0.4 (behavior video). It covers scoring tools, pose and tracking, TDT video sync,
VLM/LLM claims, the data model, and the macOS video stack, and it ends with a recommended architecture and MVP.
Machine: M5 Pro, macOS 26.6. "Tested" means run on this machine; the probes live in the session scratchpad
(`.../scratchpad/avtest/`, ephemeral). Repo activity ("last push") comes from the GitHub API, queried 2026-10-25.
**[?]** marks a claim I could not verify from a primary source.

## TL;DR verdicts

| Area | Verdict |
|---|---|
| Our niche | No existing tool joins **per-frame TDT timestamps, a frame-accurate player, a ΔF/F cursor, scoring, and stats on one clock**. BORIS plots at most 2 external series and syncs with a single constant offset. SimBA, DeepEthogram and the rest know nothing about photometry. That join is the product. |
| Sync | Map **frame index → `Cam1` epoc onset** (block clock). Never derive block time from the nominal fps or from MP4 timestamps. Validate the counts: extra trailing frames are normal (TDT TN0987); fewer frames than epocs is an error. |
| Scoring | Copy BORIS's model: an ethogram with point and state events, modifiers, subjects, keyboard coding, and inter-rater agreement. Fix its weak spots: kappa on frames rather than 10 s windows, bout-level metrics, and review status. |
| Pose | **Import first, infer later.** Read DLC CSV, SLEAP analysis HDF5, Lightning Pose CSV, and NWB ndx-pose. On-device inference (Core ML) comes second, and only for models the user brings or licenses allow. |
| VLMs | **Not credible for frame-accurate rodent scoring in 2026.** Rodent-Bench (Feb 2026): "none of these models perform strongly enough" to assist annotation. Gemini samples video at **1 fps** by default. Use VLMs only as opt-in triage or proposal generators, behind human review. |
| AVFoundation | Tested: zero-tolerance `AVPlayer.seek` returns the exact frame for H.264/HEVC/MJPEG/MPEG-4 in MP4, **but only if frame times come from the sample table shifted by the edit list**. A naive time is wrong on B-frame files. DivX AVI (the deprecated CamHal output) **does not decode**, so transcode it on import. |

---

## 1. Annotation and scoring tools

| Tool | Approach | Input | Stack, license, last push | Users like | Users dislike, limits |
|---|---|---|---|---|---|
| **BORIS** v10.0.2 (2026-10-16) [1] | Manual event logging: point and state events, modifiers, subjects, up to 8 synced players | Video/audio, live | Python/Qt + mpv, GPL-3.0, active | Free, cross-platform, flexible ethograms, time budgets, **Cohen's kappa**, Needleman–Wunsch similarity, Python/R plugins [2] | Kappa is computed on **fixed time windows (default 10 s)** [2], which is coarse. Only **2 external data series**, aligned by one constant "start position" offset [3]. No per-frame clock and no ML. Dense UI. |
| **DeepEthogram** [4] | Supervised classification from raw pixels (optical flow + CNN + sequence model) | Video | Python/PyTorch, custom license, push 2026-10 | No pose step. Reported >90 % frame accuracy on mice/flies [4]. | Needs a GPU and long training. Reviewers asked for a limitations section [4]. Opaque features. |
| **SimBA** (Nat Neurosci 2024) [5] | Supervised classifiers on pose features, with **SHAP** explanations | DLC/SLEAP pose | Python GUI, push 2026-10 | Explainability, documentation, social-behavior presets, ROI tools | Heavy install and GUI. Depends on a separate pose step [6]. Windows-centric workflows **[?]**. |
| **JAABA** (Nat Methods 2012) [7] | Interactive supervised boosting on trajectory features | Trajectories | MATLAB, push 2026-08 | Interactive "label a few, retrain" loop. Matched experts better than commercial software [7]. | MATLAB, dated, trajectory-only |
| **B-SOiD** (Nat Commun 2021) | Unsupervised clustering of pose features | Pose | Python, GPL-3.0, **last push 2024-02** | Discovers behaviors without labels | Cluster identity is not stable across sessions **[?]**. Effectively superseded by A-SOiD. |
| **A-SOiD** (Nat Methods 2024) [8] | Active learning plus unsupervised sub-class discovery | Pose | Python/Streamlit, push 2025-06 | Claims about 90 % fewer annotations via class balancing [8] | Small community, slowing activity |
| **Keypoint-MoSeq** (Nat Methods 2024) [9] | Unsupervised AR-HMM "syllables" with keypoint-noise model | Pose | Python/JAX, push 2026-10 | Principled sub-second segmentation. Outperforms common clustering [9]. Cross-species. | Syllables need human interpretation. JAX/GPU install. Syllables don't map onto ethogram labels. |
| **VAME** (Commun Biol 2022) [10] | Variational RNN embedding + HMM motifs | Pose | Python, GPL-3.0, moved to `EthoML/VAME`, push 2026-10 | Motif discovery | Same interpretability problem as MoSeq. The repo moved [10]. |
| **LabGym** [11] | Detector plus "behavior categorizer" on animations and pose-like features | Video | Python, GPL-3.0, push 2026-10 | No coding, any species | One applied study: no speed-up beyond about 60 CPUs, and some rare behaviors consistently misdetected because training examples were scarce [11] |
| **BehaviorDEPOT** (eLife 2022) [12] | **Heuristics** (thresholds on velocity, angles) over keypoints | DLC pose | MATLAB, GPL-3.0, push 2026-10 | Transparent rules. Freezing heuristic >90 % accurate, works with tethered head-mounts (P 0.94, R 0.87, F1 0.91) [12]. | MATLAB. Heuristics must be tuned per rig. |
| **ezTrack** (Sci Rep 2019) [13] | Background subtraction: location, distance, ROI time, freezing | Video | Python notebooks, GPL-3.0, push 2026-10 | No training. Good enough for open field and fear conditioning. | Single animal, top view, constant lighting |
| **idtracker.ai** (v6, eLife RP 2025) [14] | Identity tracking for many animals; v6 frames identity as contrastive representation learning | Video | Python | Claimed up to 700× faster (440× in v1 of the preprint) [14] | Identity only, no behavior labels |
| **Live Mouse Tracker** [15] | Depth camera + RFID, real-time, rule-based social events | RGBD + RFID hardware | Java/Python, GPL-3.0 | Up to 4 mice, long sessions, rich social events [15] | Dedicated hardware |
| **JABS** (Kumar lab) | Supervised classifiers with bout-based evaluation | Pose | Python | Introduced frame- **and** bout-based (IoU) inter-annotator comparison [16] | Tied to the JAX rig format **[?]** |
| **AmadeusGPT** (NeurIPS 2023) [17] | LLM writes analysis code from natural-language behavior definitions | DLC pose | Python, Apache-2.0, push 2025-06 | Natural-language queries. Reported SOTA on MABe 2022 tasks [17]. | Cloud LLM. Correctness only as good as the generated code. |

**Lessons for SimpleFP**
- The *workflow* scorers want is BORIS-like: ethogram, keyboard, point and state events. The *trust* they want is SimBA-like: explainable features, plus a validation report.
- There are three classifier families. **Heuristics** (BehaviorDEPOT, ezTrack, LMT) are reproducible and cheap but rig-specific. **Supervised** (SimBA, JAABA, DeepEthogram) needs labels and inherits annotator bias. **Unsupervised** (MoSeq, VAME, B-SOiD) is exploratory and its labels need interpretation. A 2025 review reaches the same split [16].
- Frame-wise classifiers fragment bouts. The usual fixes are merging gaps, minimum and maximum durations, and smoothing [16]. These belong in a declarative, hashed post-processing step, like our presets.
- Install pain (Python environment, CUDA, MATLAB) is the most-cited barrier. The 2025 review names DLC, SLEAP and SimBA as rare tools that overcame the "postdoc leaves, tool dies" problem [16]. A native app with zero install is a real differentiator. That is an inference, not a survey result.

## 2. Pose estimation and tracking

| Tool | State (2026-10) | Notes for us |
|---|---|---|
| **DeepLabCut** 3.0.2 (PyPI, 2026-09-23) [18] | PyTorch backend. TensorFlow optional, to be deprecated in 3.2 [18]. LGPL-3.0. | **SuperAnimal-TopViewMouse** (27 keypoints) and **-Quadruped** foundation models, about 10× more data-efficient when fine-tuned (Ye et al., Nat Commun 2024) [19]. **"Research use only (non-commercial)"** [18]. The model card says it is not suitable for other views or species [19]. |
| **SLEAP** 1.5.x [20] | 1.5 moved to PyTorch (`sleap-nn`, GPL-3.0). I/O split into `sleap-io` (BSD-3) [20]. | `sleap-nn` supports single-instance, top-down, bottom-up and centroid models, and exports **ONNX/TensorRT** [20]. That makes ONNX → Core ML or the ORT CoreML EP feasible **[? not tested]**. |
| **Lightning Pose** (Nat Methods 2024) [21] | MIT, active | Semi-supervised (temporal, multi-view, pose-PCA losses) plus the **Ensemble Kalman Smoother** [21]. Outputs DLC-style CSV (scorer/bodyparts/coords) [22]. |
| **SAM 2 / SAM 3** | SAM 2: Apache-2.0. SAM 3 "Segment Anything with Concepts" (Meta, custom license). | Apple ships Core ML **SAM 2.1** (tiny→large, FP16) [23]. HF notes Core ML video segmentation was "still in development" [24] **[? may be stale]**. DAMM (Detect Any Mouse) adds SAM 2 video tracking [25]. Masks give centroid, area and orientation, not keypoints. |
| **YOLO-pose** (Ultralytics) | AGPL-3.0, Core ML export (`format="coreml"`) [26] | **AGPL licensing is a problem for embedding.** A new `format="coreai"` targets Apple's Core AI runtime, which needs **macOS/iOS 27** and is in beta [27]. |
| **Apple Vision** | `VNDetectAnimalBodyPoseRequest` covers cats and dogs **[? from memory]** | Not usable for rodents. Generic trackers (`VNTrackObjectRequest`) and foreground masks may help centroid tracking. |
| **movement** (SWC) [28] | BSD-3, active | Python reference loaders: `from_dlc_file`, `from_sleap_file`, `from_lp_file`, `from_anipose_file`, `from_nwb_file`, COCO (checked in source). Use it as the golden reference for our Rust importers. |

### On-device inference on Apple Silicon

| Path | Fit | Caveats |
|---|---|---|
| **Core ML** (`coremltools` from PyTorch) | Default today. Runs on ANE/GPU/CPU. Integrates with `AVAssetReader` pixel buffers. | Conversion per architecture. Validate numerics against the Python reference (CPU-only mode). |
| **ONNX Runtime + CoreML EP** | Runs exported ONNX models (sleap-nn) without per-model conversion work | Only supported ops go to Core ML. Dynamic shapes hurt. Compiled models need `ModelCacheDirectory`, which is not auto-invalidated [29]. |
| **Core AI** (`.aimodel`) | Apple's new path (WWDC26). PyTorch `torch.export` → `.aimodel` [27]. | Runtime needs **macOS 27**; this machine runs 26.6. Revisit at stable release. |
| **MLX / mlx-swift** (MIT) | Local VLMs/LLMs (the triage use case in §4) | Not a pose-model runtime in practice **[?]** |

Rule: **ship no model whose license we can't redistribute.** SuperAnimal is non-commercial (fine for academic users, but show the license on first use). YOLO is AGPL.

### Formats to import (priority order)

| Format | Layout | Rust feasibility |
|---|---|---|
| **DLC CSV** (also Lightning Pose) | Header rows `scorer`, [`individuals` for multi-animal], `bodyparts`, `coords` (x, y, likelihood). First column is the frame index. [22] | Trivial. **Start here.** |
| DLC `.h5` | pandas/PyTables `df_with_missing` (same MultiIndex) | Awkward: PyTables table layout from Rust. Prefer the CSV DLC writes alongside. |
| **SLEAP analysis `.h5`** | `tracks` (n_tracks, 2, n_nodes, n_frames), `point_scores` (n_tracks, n_nodes, n_frames), `track_occupancy` (n_frames, n_tracks), `node_names`, `track_names`, `edge_inds`. Since sleap-io 0.7, n_frames = len(video), NaN-padded [30]. **Older files are frames-first (MATLAB order).** | Easy with `hdf5-metno` (already planned for NWB). Check shapes, don't assume order [30]. |
| SLEAP `.slp` | Full labels database (HDF5, complex) | Defer. Ask the user to export analysis h5, or convert via `sleap-io`. |
| **NWB ndx-pose** | `PoseEstimation` → `PoseEstimationSeries` (data (T, 2/3), `confidence`, `timestamps`), `Skeleton` (nodes, edges), in the `behavior` processing module [31] | Write with the NWB writer (v0.2). The timestamps are our block-clock frame times. |
| Bonsai / iVn tracking CSV | iVn: `{FRAME_NUMBER},{POINT_ID,IS_VALID,CONFIDENCE,REGION,X,Y}…`, same prefix as the MP4 [32] | Trivial. Real-time R-Track/D-Track on iV2 firmware 2.0+ [33]. |

## 3. Video ↔ photometry synchronization with TDT Synapse

### What TDT documents

| Fact | Source |
|---|---|
| iV1/iV2 (iVn) record USB3 Basler cameras "synchronized with all … fiber photometry … data". iV2 handles 2 cameras and real-time tracking. iV1 replaces the deprecated CamHal. | [32][33] |
| **Shutter Control:** *Hard Wired* (iVn `Sync` output triggers the camera, **recommended**), *Soft Wired* (trigger sent over the iCon link), *Free Running* (not recommended) | [32] (Synapse Manual p. 203) |
| Output is **MP4 only**: H.264 (recommended, about 2× H.265/MJPEG efficiency), H.265 or MJPEG, at a fixed bit rate (quality 1–5 = 2.5–22 MB/min) | [32][33] |
| iV2 examples (1 camera, H.264): 1920×1200 at 40 fps, 1280×1024 at 60, 640×400 at 200; halve for 2 cameras. iV1: 1920×1080 at 25, 1280×1024 at 40, 640×400 at 120. | [33] |
| A 10 s on-board frame buffer. If it fills, Synapse shows **"Frame Errors" = lost frames**. "Always double-check for lost frames." | [32][33] |
| **Gating / Triggered Capture Interval** can turn capture on and off mid-recording (`V1_OutEnab_1`, `V1_FrameEnab` via SynapseAPI). The frame clock therefore **can have gaps by design.** | [32] p. 204–205 |
| Frames are stored as epoc events named per camera. Docs use `Cam1`/`Cam2`. In MATLAB, `data.epocs.Cam1.onset` holds frame times and `.data` the frame number. | [34] |
| CamHal (deprecated): DIVX AVI named `{TANK}_{BLOCK}_{HalName}.avi` in the block folder. Software timer, ≤20 fps, "prone to jitter". | [32] p. 215, [35] |
| TN0987: the **video can have more frames than timestamps**. The camera timer may start after the hardware trigger, and the Cam tab keeps capturing briefly after stop. "Timestamps … always match the frames … starting from the first frame." Extra frames appear only at the end. A missed frame has no timestamp. | [36] |
| TDT's own advice: match from index 0 and drop extra frames at the end. A wrong container fps (15 vs 20) makes a 1 h video play as about 45 min (TN0981). | [34] |
| External cameras: send a per-frame TTL into a digital input (start TDT first), **or** let Synapse send frame triggers (start the camera software first) | [34] |
| OpenScope accepts video files named `Cam#.avi` / `Vid#.avi` without the block prefix | Synapse release notes [32] p. 608 |

**Naming [?]:** for iVn MP4s, TDT documents only "same prefix" as the tracking CSV. I infer `{TANK}_{BLOCK}_Cam1.mp4` from the CamHal rule. Discover files by globbing the block folder for `*Cam<n>.{mp4,avi}` and confirm with the user when the match is ambiguous.

### Recommended FrameClock algorithm

1. Read the epoc store (`Cam1`, or a user-chosen TTL store for external cameras). The existing `tdt::Epoc` already provides `onset` and `data`.
2. Count video frames from the **sample table** (AVSampleCursor, about 1 ms, no decode; see §6). N_v = video frames, N_e = epocs.
3. Rules (fail loudly, matching docs/architecture.md):
   - N_v ≥ N_e: frames k < N_e get `t_k = onset[k]`. Trailing frames are unclocked, excluded from scoring and shown greyed out. QC warning if N_v − N_e > `max_trailing_frames` (default ≈ 1 s of frames, **[?] to tune on real blocks**).
   - N_v < N_e: **error**. Frames were lost from the file and the mapping is ambiguous. Offer "align by first frame" only as an explicit, recorded override.
   - `data` (the frame-number field) must increase by exactly 1. A gap means frames are missing, which can happen with gating. **[?] Whether the iVn MP4 keeps a frame for a "Frame Error" is undocumented.** Validate on a real block with errors induced on purpose before trusting it.
   - Inter-frame interval QC: median and max IFI, the count of IFI > 1.5× median, and the effective fps. All of it goes into `run.json`.
4. Lookup. Frame→time is `t_k`. Time→frame is a binary search for the frame on screen at t, i.e. k with t_k ≤ t < t_{k+1}. An annotation on frame k gets event time `t_k` (frame onset). Document that the error is ≤ 1 frame (33 ms at 30 fps) and that exposure latency is not modeled **[?]**.
5. **Drift.** Per-frame TDT timestamps have no drift problem: every frame is on the block clock. Drift only matters when the camera free-runs with a single start TTL. Then require periodic sync pulses (TTL plus a visible LED). Fit `t_block = a + b·t_cam` by least squares, report the residual max and the ppm, and refuse when the residual > ½ frame. A typical crystal of tens of ppm adds about 0.1–0.2 s/h **[order of magnitude, not measured]**.
6. Hash the FrameClock (store name + onsets + N_v + video BLAKE3) into every annotation file and into `run.json`. A changed clock invalidates derived times loudly.

## 4. VLMs and LLMs for behavior: what's credible

| Evidence | Result |
|---|---|
| **Rodent-Bench** (arXiv 2602.18540, Feb 2026) [37] | Gemini-2.5-Pro/Flash and Qwen-VL-Max on 10–35 min rodent videos (social, grooming, scratching, freezing). Metrics: second-wise accuracy, macro-F1, mAP, MI, MCC. **"None of these models perform strongly enough"** to assist annotation. They fail at temporal segmentation, long videos and subtle states. Grooming detection was modest. |
| **MTT-Bench / MouseRank** (arXiv 2604.22492) [38] | Social dominance from video: GPT-4o zero-shot about **21 %** accuracy at 32 frames per video |
| **MouseGPT** (arXiv 2503.10212) [39] | Fine-tuned InternVL2/MiniCPM VLM. GPT-4o *generated* the training captions. Evaluated by expert ratings, **with no frame-level metrics against keypoint methods**. The authors admit it misses brief behaviors (single scratches, twitches) because it works per image. |
| Camera-trap zero-shot VLMs (Dussert et al., MEE 2025) [40][41] | CogVLM reached about 96 % on *coarse* image-level behavior (ungulates). Results were highly prompt-sensitive, came from a single dataset, and used citizen-science labels. |
| **Gemini video input** [42] | Default **1 fps** sampling. Docs warn that "fast action sequences might lose detail". Timestamps are MM:SS. Custom fps is available in static mode. |
| AmadeusGPT [17]; 2025 review [16] | LLMs turning *user-written heuristic definitions* into code are interpretable and reproducible, but "may fail to capture complex, subtle behaviors" [16] |

**Credible now:** (a) LLM-assisted *ethogram writing*, meaning operational definitions and key maps; (b) LLM → code over pose features (AmadeusGPT-style), where the code is the auditable artifact; (c) VLM **triage** at seconds scale, e.g. "find candidate rearing bouts", feeding a human review queue; (d) coarse states such as asleep vs active.
**Hype:** zero-shot, frame-accurate scoring of subtle rodent behavior as a replacement for human or pose-based classifiers. Also any "AI scored" claim without a held-out human comparison.

### Rigor protocol (what the app enforces)
- Every model-made annotation carries `source = model{id, version, weights BLAKE3, prompt/params hash}`, a confidence, and `review = proposed`. **Statistics use only `accepted` or `manual` annotations by default.** The `run.json` records the counts per source and review status.
- Validation set: before using a classifier's output, a human double-scores a random, **blinded** subset (randomized video order, condition hidden). The app reports agreement on it.
- Metrics, per behavior (computed in Rust):
  - **Frame-wise Cohen's κ** (binary per behavior plus multi-class). Also report **prevalence and PABAK**, because κ collapses for rare behaviors (the "kappa paradox"). That is textbook knowledge, not from a cited source.
  - **Frame-level P/R/F1**, macro-averaged across classes because of imbalance (CalMS21 convention) [43]
  - **Bout-level F1 at a tIoU threshold** (JABS uses 0.3/0.5/0.7) plus the BANOS decomposition: segment F1, tIoU, onset/offset error, intra-bout continuity [16]
  - **Point events:** a match within ±n frames (BORIS uses a ±window [2]). Report the onset error distribution.
  - **Downstream agreement:** counts, total duration and latency per session (ICC or Bland–Altman). These are the numbers that reach the paper.
- Cloud VLMs send video off the machine. Make them opt-in per project, with a data-policy warning, and prefer local models (MLX) when available.

## 5. Data model

All types live in Rust (`crates/core`, pure), are serialized as JSON/TOML sidecars, and are hashed into `run.json`. **Frame index is the canonical coordinate for anything observed on video. Block time is always derived through the FrameClock.** That way a corrected clock re-times every annotation without editing it.

```text
Video        { id, path, blake3, codec, width, height, n_frames, pts_movie: [CMTime as (value, timescale)],
               camera: "Cam1", block: path }
FrameClock   { video_id, source: TdtEpoc{store} | TtlEpoc{store} | Fitted{a, b, residual_max} ,
               t: Vec<f64> /* block s, frame k */, n_unclocked_tail, gaps: Vec<(k, missing)>, qc, hash }
Ethogram     { id, version, behaviors: [{ key, name, kind: Point|State, hotkey, exclusive_group?, color,
               definition /* operational text */, modifiers: [..] }], subjects: [..], hash }   // TOML, like presets
Annotation   { id (ULID), video_id, behavior, subject?, modifiers{},
               span: Point{frame} | Bout{start, end_excl},
               scorer, source: Manual | Model{id, version, weights_blake3, params_hash} | Import{tool, file_blake3},
               confidence?: f32, review: Manual | Proposed | Accepted | Rejected | Edited{from},
               created, note? }
AnnotationSet{ video_id, scorer, ethogram_hash, frameclock_hash, blinded: bool, annotations: [..] } // one file per scorer
PoseTrack    { video_id, source{tool, file_blake3, model}, skeleton{nodes, edges}, individuals,
               xy: [f32; F×I×N×2] (NaN = missing), conf: [f32; F×I×N] }        // binary cache, stays in Rust
Roi          { name, shape: Polygon[[x,y]] | Circle{c, r} | Rect, video_id }
Calibration  { px_per_cm, from: two points + known distance }                  // physical scale
Features     { centroid(nodes), speed_cm_s (dt from FrameClock, not fps), dist_to_roi (signed, edge),
               in_roi (hysteresis + min dwell), heading; params in a preset `[behavior]` table }
```

**Derived events:** ROI entries and exits, and bout onsets and offsets, become `EventSource::Manual`-style event sets. The existing PETH/bootstrap pipeline analyzes them unchanged. Continuous features are resampled (linear on block time) onto the decimated photometry grid for regression.

**Caution:** continuous behavior vs ΔF/F regression is autocorrelated. Use lagged cross-correlation with a **circular-shift null**, never naive p-values. Kernel GLMs come later.

**On-disk layout** (inside `<block>/simplefp/behavior/`): `ethogram.toml`, `<video>/frameclock.json`,
`<video>/annotations.<scorer>.json`, `<video>/proposals.<model>.json`, `<video>/pose.<source>.bin` (+ `.json` header), `rois.json`, and `agreement.json`. Proposals stay apart from human sets. Accepting a proposal copies it into the scorer's set with an `Edited{from}`/`Accepted` link.

## 6. macOS implementation (tested)

Probe: 120 s, 30 fps (3600 frames), 640×480. Each frame's luma encodes `k mod 28`, so every returned frame can be checked.
The numbers are optimistic (synthetic, low-entropy content). Real 1080p footage will be slower, but the correctness results carry over.

| File | Playable | Sample-table frames | Sequential decode (AVAssetReader) | `AVPlayer.seek` (0 tolerance) + `AVPlayerItemVideoOutput` |
|---|---|---|---|---|
| H.264, GOP 300, 3 B-frames | yes | 3600 in 1.4 ms | 3600/3600 correct, about 1.9k fps | 15 ms/seek, **0/60 wrong** |
| H.264, GOP 10, no B | yes | 3600 | correct, about 2.0k fps | 1.5 ms, 0/60 |
| HEVC (hvc1), GOP 60 | yes | 3600 | correct, about 2.0k fps | 3.0 ms, 0/60 (reader `timeRange` start: 1/60 off by one) |
| MJPEG in MP4 | yes | 3600 | correct, about 2.3k fps | 0.9 ms, 0/60 |
| MPEG-4 Part 2 in MP4 | yes | 3600 | correct | 2.0 ms, 0/60 |
| **DivX AVI** (CamHal) | **no** (container parses, decode fails) | 3600 | 0 frames | fails |
| DivX → `ffmpeg -c:v libx264 -g 15 -bf 0 -fps_mode passthrough` | yes | 3600 (count preserved) | correct | 1.6 ms, 0/60 |

**Findings that drive the design**
1. **Edit lists.** On B-frame files (ffmpeg H.264/HEVC defaults, and likely iVn H.264 **[?]**), track media PTS start at 2 frames (0.0667 s) while movie time starts at 0. Seeking to raw sample-cursor PTS was wrong on **60/60** seeks. Mapping through `track.segments[].timeMapping` (media → movie) fixed it to 0/60.
2. **Use exact `CMTime`, never `Double` seconds.** Rounding seconds to a 90 kHz timescale gave 2–4/60 off-by-one frames on files without B-frames.
3. `AVSampleCursor` reads the whole PTS table in about 1 ms with no decoding. This is how we get N_v for the FrameClock and the frame↔movie-time table, and it handles VFR [44].
4. Long GOPs cost seek latency (15 ms vs 1.5 ms here, more on real footage). Frame stepping while paused stays fine. For heavy scrubbing, offer an optional short-GOP proxy (H.264, GOP ≈ 15, no B-frames). Verify the frame count after transcoding.
5. CamHal AVIs must be transcoded on import, which needs a user-installed `ffmpeg` **[?] or a bundled decoder; decide on licensing**. Keep the original as the source of truth, and check that frame counts match before and after.

**Player architecture**
- `AVPlayerView`/`AVPlayerLayer` for playback (0.1–4× rates). `AVPlayerItemVideoOutput` + display link for overlays (pose, ROIs) drawn with Metal or Canvas at the frame actually on screen (`itemTimeForDisplay`).
- Paused stepping: ±1 frame means seeking to `movieTime[k±1]` with zero tolerance. That is deterministic, unlike `step(byCount:)`, whose `currentTime` reportedly lags [45].
- Cursor sync: while playing, a periodic time observer maps movie time → k (binary search) → `t_k` → trace cursor. Clicking the trace maps block t → k → seek. Frame times are small (2 h × 30 fps = 216k f64 = 1.7 MB) and cross the FFI once as bytes (tech-stack rule).
- Bulk work (pose inference, centroid tracking, thumbnails): `AVAssetReader` sequential decode into IOSurface-backed `CVPixelBuffer`s, then Core ML or Rust. Never seek per frame for analysis.

## 7. Recommended architecture and MVP

```text
 SwiftUI app ─ VideoPane (AVPlayerView + overlay) ─ TracePane (existing Canvas) ─ EthogramBar (hotkeys)
      │  AVFoundation: sample table → n_frames, movie times; decode for overlays/inference (Swift only)
      │  UniFFI (bytes for arrays)
 crates/engine ─ discover videos in block · build/persist FrameClock · load/save AnnotationSets · import pose
      │
 crates/core   ─ behavior::{frameclock, annotation, agreement, features, roi}  (pure, tested, no I/O)
 crates/tdt    ─ unchanged (Cam1 is just an Epoc)
```
Video decoding stays in Swift (AVFoundation). Rust never touches pixels in the MVP. The CLI can still compute agreement and features from sidecar files and pose CSVs, taking `n_frames` from `frameclock.json`.

### MVP (v0.4.0): in order, each with its check
1. **FrameClock**: `core::behavior::frameclock` with the count, gap and IFI rules from §3, plus engine discovery of `*Cam<n>.{mp4,avi}`. *Check:* simulator blocks (`write_block` already writes epocs) with trailing extras, missing frames and gating gaps.
2. **Synchronized player**: frame-accurate stepping (edit-list-correct table), bidirectional trace cursor, jump-to-event from the PETH and trace. *Check:* the luma-coded probe video as a UI test fixture.
3. **Manual scoring**: ethogram TOML, hotkeys, point and state events (exclusive groups), per-scorer `AnnotationSet`, and blinded mode (hide condition, shuffle order). Bouts export to the existing event pipeline as onset/offset event sets and duration metrics. This replaces the hand-made `event,time_s` CSV.
4. **Agreement**: frame κ + PABAK + prevalence, frame F1, bout F1 at tIoU 0.5, onset error, and session count/duration ICC between two scorer files, written to `agreement.json`. *Check:* hand-computed small cases (assert tests).
5. **Pose import + features**: DLC/LP CSV and SLEAP analysis h5 (checked against `movement` loaders), overlay, ROI polygon editor + px/cm calibration, centroid speed, time-in-ROI, ROI entries as events, features resampled to `trace.csv` time.

### Deferred (v0.4.x), with reasons
- **Background-subtraction centroid tracker** (ezTrack-like, in Rust): covers locomotion and ROI work with no model. First in line after MVP.
- **Core ML inference** of user-supplied models (SLEAP ONNX, converted DLC), and SuperAnimal-TopViewMouse behind a license prompt. Core AI once macOS 27 is stable.
- **Model proposals** (heuristics first, then supervised on pose features with SHAP-like feature reporting) through the `Proposed → Accepted` review queue.
- **VLM triage**: opt-in, local (MLX) preferred, seconds-scale candidates only, never auto-accepted.
- SAM 2/3 masks, multi-animal identity, 3D/multi-camera, `.slp` reading, NWB ndx-pose writing (with the v0.2 NWB writer).

### Open questions (need lab input)
1. Which capture path does the lab use: iV1/iV2 (MP4 + `Cam1`), CamHal (AVI), or an external camera with TTL? **I need one real block with video** to confirm file naming, whether `Cam1.data` is 0- or 1-based, and Frame-Error behavior.
2. Which behaviors come first (feeding, grooming, rearing, freezing, social)? That decides whether pose import is needed for the first paper or manual scoring is enough.
3. Is shipping or using non-commercial (SuperAnimal) or AGPL (Ultralytics) models acceptable for the lab's distribution plans?

## Sources
1. BORIS releases: https://github.com/olivierfriard/BORIS/releases
2. BORIS analysis (kappa, Needleman–Wunsch): https://www.boris.unito.it/user_guide/analysis/
3. BORIS observations (external data, offsets, 8 players): https://www.boris.unito.it/user_guide/observations/
4. DeepEthogram, eLife 2021: https://elifesciences.org/articles/63377 · reviews https://elifesciences.org/articles/63377/peer-reviews
5. SimBA, Nat Neurosci 2024 (doi 10.1038/s41593-024-01649-9): https://pmc.ncbi.nlm.nih.gov/articles/PMC11268425
6. SimBA primer: https://www.ncbi.nlm.nih.gov/pmc/articles/PMC11296943/ · https://irp.nida.nih.gov/?p=13846
7. JAABA: https://jaaba.sourceforge.net/ · https://edspace.american.edu/openbehavior/project/jaaba/
8. A-SOiD preprint: https://www.biorxiv.org/content/10.1101/2022.11.04.515138.full.pdf (Nat Methods 2024 version: from memory **[?]**)
9. Keypoint-MoSeq, Nat Methods 2024: https://www.ncbi.nlm.nih.gov/pmc/articles/PMC11245396/
10. VAME: https://link.springer.com/10.1038/s42003-022-04080-7 · https://zenodo.org/records/13787335
11. LabGym applied study: https://arxiv.org/pdf/2310.07812
12. BehaviorDEPOT, eLife 2022: https://elifesciences.org/articles/74314 · https://pmc.ncbi.nlm.nih.gov/articles/PMC9398447/
13. ezTrack, Sci Rep 2019: https://pmc.ncbi.nlm.nih.gov/articles/PMC6934800
14. idtracker.ai v6: https://elifesciences.org/reviewed-preprints/107602
15. Live Mouse Tracker: https://edspace.american.edu/openbehavior/project/live-mouse-tracker/
16. Chindemi, Bellone, Girard 2025 review (BANOS, JABS metrics): https://arxiv.org/pdf/2508.04255 · https://github.com/BelloneLab/BANOS
17. AmadeusGPT: https://papers.neurips.cc/paper_files/paper/2023/hash/1456560769bbc38e4f8c5055048ea712-Abstract-Conference.html
18. DeepLabCut PyPI: https://pypi.org/project/deeplabcut/ · https://github.com/DeepLabCut/DeepLabCut
19. SuperAnimal (Ye et al. 2024): https://arxiv.org/abs/2203.07436v2 · https://huggingface.co/mwmathis/DeepLabCutModelZoo-SuperAnimal-TopViewMouse
20. SLEAP docs/changelog: https://docs.sleap.ai/latest/changelog · sleap-nn: https://nn.sleap.ai/latest/
21. Lightning Pose: https://www.ncbi.nlm.nih.gov/pmc/articles/PMC10168383/
22. DLC-style index (Lightning Pose API): https://lightning-pose.readthedocs.io/en/latest/api/lightning_pose.utils.predictions.make_dlc_pandas_index.html
23. Apple Core ML SAM 2.1: https://huggingface.co/apple/coreml-sam2.1-large/tree/main
24. HF segment-anything-2 (Core ML status): https://github.com/huggingface/segment-anything-2
25. DAMM: https://www.biorxiv.org/content/10.1101/2024.01.18.576153.full.pdf
26. Ultralytics Core ML: https://docs.ultralytics.com/es/integrations/coreml
27. Ultralytics Core AI: https://docs.ultralytics.com/de/integrations/coreai · background https://9to5mac.com/2026/03/01/apple-replacing-core-ml-with-modernized-core-ai-framework-for-ios-27-at-wwdc/
28. movement: https://github.com/neuroinformatics-unit/movement
29. ONNX Runtime CoreML EP: https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html
30. SLEAP analysis HDF5: https://io.sleap.ai/latest/formats/analysis_h5/ · legacy order https://sleap.ai/api/sleap.info.write_tracking_h5.html
31. ndx-pose: https://github.com/rly/ndx-pose
32. Synapse Manual (iVn p. 202–208; CamHal p. 215; release notes p. 608): https://www.tdt.com/files/manuals/SynapseManual.pdf
33. iVn hardware reference (updated 2026-10-14): https://tdt.com/files/manuals/hardware/iVn.pdf · iV2: https://www.tdt.com/files/manuals/hardware/iV2.pdf
34. TDT video review and scoring: https://tdt.com/docs/sdk/offline-data-analysis/video-review-and-scoring/
35. TDT CamHal / video processors: https://tdt.com/docs/synapse/hardware/cam-hal/ · https://tdt.com/docs/synapse/hardware/video-processors/
36. TDT TN0987: https://tdt.com/docs/technotes/tn/TN0987/
37. Rodent-Bench: https://arxiv.org/abs/2602.18540
38. MTT-Bench: https://arxiv.org/html/2604.22492v1
39. MouseGPT: https://arxiv.org/html/2503.10212
40. Dussert et al. 2025, MEE: https://besjournals.onlinelibrary.wiley.com/doi/full/10.1111/2041-210X.70059 (paywalled, 403; preprint https://www.biorxiv.org/content/10.1101/2024.04.05.588078.full.pdf)
41. Methods blog summary: https://methodsblog.com/2025/07/31/can-we-identify-animal-behaviours-from-camera-traps-without-training-new-ai-models/
42. Gemini video understanding: https://ai.google.dev/gemini-api/docs/video-understanding
43. CalMS21 / MABe mouse dataset: https://arxiv.org/pdf/2104.02710
44. AVSampleCursor: https://developer.apple.com/documentation/avfoundation/avsamplecursor
45. AVPlayer transport and `step(byCount:)` lag: https://developer.apple.com/documentation/avfoundation/controlling-the-transport-behavior-of-a-player · https://developer.apple.com/forums/thread/772651
