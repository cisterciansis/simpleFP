# Decision models for behavior (2026-10-25)

Scope: the models that **decide which behavior is happening** from frames, pose or kinematics, and whether SimpleFP should
train its own. It builds on [behavior-ai.md](behavior-ai.md) and [video-ai.md](video-ai.md), which already cover the tool
landscape (DLC, SLEAP, SimBA, DeepEthogram, Keypoint-MoSeq, A-SOiD, VLMs), the rigor protocol and pose licensing. None of
that is repeated here.
Baseline under test: `crates/video/src/classify.rs`. It computes 9 tracking channels (present, speed, motion, area,
elongation, turn, centre and edge distance, plus optional pose columns), each with a centred mean and std over 0.5 s and
2 s. It trains one binary GBDT per behavior (150 trees, depth 3, 32 quantile bins, positives up-weighted 1–20×), then
decodes bouts with hysteresis (0.6/0.4) and minimum-bout and merge-gap priors taken from the human bouts. CV holds out
whole sessions, or time blocks with a 2 s guard band.
Machine: M5 Pro, macOS 26.6, torch 2.14.1 (MPS), coremltools 9.0, timm 1.0.30, transformers 5.20.1.
"Tested" means run here; the scripts are in the session scratchpad (`.../scratchpad/decision/`, ephemeral).
Licenses come from GitHub LICENSE files and the Hugging Face API, queried 2026-10-25. **[?]** marks a claim I could not
verify from a primary source.

## TL;DR

| Question | Answer |
|---|---|
| What beats GBDT on kinematic features? | **Temporal neural models (1D dilated TCN, CNN+GRU/Transformer) trained on lots of labels**: MABe 2021, Kaggle MABe 2025 and Blau et al. 2024 all agree. At small label counts, GBDT with good windowed features ties or wins: JABS XGBoost matched a CNN (0.936 vs 0.937 frame accuracy) and beat it on limited data. **The gain comes from long temporal context plus a lot of labels, not from the model family.** |
| Does video beat kinematics? | **Only when kinematics are poor.** CalMS21: hand-crafted MARS features 0.878 F1, frozen VideoPrism features 0.849, raw keypoints 0.593 (same BiGRU head) [R1]. Video helps where pose and tracking can't see the behavior (open field: VideoPrism 0.718 vs pose 0.663) [R1]. |
| Next step on top of the GBDT | (1) **Longer, multi-scale windows plus an auto-context second pass** (a GBDT on rolled first-pass probabilities). It uses the trainer we already have, needs zero dependencies, and is the cheapest way to get TCN-like context. (2) **Calibrate** the per-frame probabilities. (3) An **explicit-duration (semi-Markov) Viterbi** decoder for multi-behavior ethograms. (4) Turn the review queue into **active learning**. |
| Video embedding worth it? | Not for tail suspension or immobility. Consider it for **grooming, rearing and social contact** once the tracker plus pose plateau. Pick **DINOv2-S/14 (Apache-2.0)** per-frame on a tracker crop (tested: 1,134 img/s on MPS, 491/s Core ML GPU, cosine 0.9997 against PyTorch), with the temporal head over its embeddings. Clip models (V-JEPA 2 ViT-L, MIT) are an offline option at about 180–250 frames/s. |
| Train our own model? | **Not a foundation model now.** We have 1 behavior, 2 animals and 6 bouts. **Do** train small supervised heads per lab (we already do). Revisit pretraining at about 100+ h of the lab's own unlabeled video **and** ≥ 5 behaviors with ≥ 50 bouts each from ≥ 8 animals. |

---

## 1. Temporal decision models on features

### 1.1 Generic action segmentation (human video benchmarks)
All of these take a per-frame feature sequence (classically I3D features) and output per-frame labels. The input could
just as well be our kinematic rows or DINOv2 embeddings.

| Model | Paper | Idea | Code license | Fit for us |
|---|---|---|---|---|
| **MS-TCN** | CVPR 2019, arXiv 1903.01945 | Stacked stages of dilated 1D convs, each refining the previous one; cross-entropy plus a truncated-MSE smoothing loss | **MIT + Commons Clause** (no selling) (`yabufarha/ms-tcn`) | Reimplement; don't copy the code |
| **MS-TCN++** | TPAMI, arXiv 2006.09220 | Dual dilated layers in stage 1 | MIT (`sj-li/MS-TCN2`) | The reference design for a "small TCN head" |
| **ASRF** | WACV 2021, arXiv 2007.06866 | An extra boundary-regression branch to suppress over-segmentation | MIT (`yiskw713/asrf`) | The boundary idea maps onto onset error |
| **C2F-TCN** | ACM MM 2021, arXiv 2105.10859 | Encoder–decoder TCN with multi-resolution ensembling | MIT | Overkill |
| **ASFormer** | BMVC 2021, arXiv 2110.08568 | Transformer with local attention and a decoder refinement | MIT | Needs more data than we have |
| **DiffAct** | ICCV 2023, arXiv 2303.17959 | Diffusion denoising of the label sequence | MIT | Heavy and iterative; research only |
| **LTContext** | ICCV 2023, arXiv 2308.11358 | Windowed plus sparse long-range attention | **CC-BY-NC-4.0** | No |

These rank on 50Salads, GTEA and Breakfast, which have dense labels, tens of classes and thousands of segments. What carries
over to rodents is the shared design: **dilated temporal convolutions + a smoothing loss + multi-stage refinement**. The
transformer and diffusion variants add accuracy only at data scales we won't have [inference].

### 1.2 Behavior-specific decision models

| Tool | Decision layer | Input | License (verified) | Lesson for us |
|---|---|---|---|---|
| **MARS** (eLife 2021) [R2] | **XGBoost** per behavior on 270 features (mean/std/min/max over 0.1, 0.37, 0.7 s) → **HMM smoothing** that enforces one label per frame | 2-mouse pose, top view | — | This is our architecture, plus an HMM. Trained on **6.95 h** of single-annotator video and tested on **mice with head-mounted cables**, the tethered case we have. |
| **JABS** (eLife RP 2025) [R3] | RF / GBM / XGBoost on windowed pose features. Grooming peaked at a **~60-frame window**. **Stitch gaps + drop short bouts**. | pose | — | XGBoost 0.9364 vs CNN 0.937 frame accuracy on 2.18 M frames. With 10 videos, about 85 % accuracy and AUROC 0.94. Halving the data cost about 1 % TPR. Stitching and filtering raised bout agreement from 49 % to 61 %. |
| **DeepEthogram** [R4] | Optical-flow + RGB CNNs → per-frame features → **TGM temporal sequence model** | raw frames | **Harvard academic non-commercial, no redistribution** (`license.txt`) | The flow + sequence-head split is the pattern for a video path. The code is unusable for us. |
| **SimBA** | RF on pose features + SHAP | pose | academic-only | Feature and explainability ideas only |
| **A-SOiD** | Active learning: iteratively add low-confidence samples, balance classes, RF | pose | Clear BSD, academic-only | The *loop* is the idea worth copying (§3.1) |
| **B-SOiD** | UMAP + HDBSCAN → RF | pose | GPL-3.0 | Unsupervised; no |
| **VAME** | VRNN + HMM motifs | pose | GPL-3.0 | Unsupervised; no |
| **Keypoint-MoSeq** | AR-HMM syllables | pose | Harvard non-commercial | Unsupervised; import its syllables only |
| **CEBRA** | Contrastive joint behavior/neural embedding | any time series | **Apache-2.0 for v0.4.0+** (v0.1–0.3.1 non-commercial). **EPFL patent WO2023143843A1** is noted in the LICENSE. | A *photometry–behavior* embedding tool, not a classifier. Interesting for "which kinematic states covary with ΔF/F". The patent matters for commercial use. |
| **daart** (Blau et al. 2024) [R5] | Dilated TCN, compared against RF and XGBoost | hand-crafted pose features | GPL-3.0 | **The TCN beat RF/XGBoost on every dataset**, with 4–10 labeled mouse videos. Hand-crafted features beat raw keypoints, and adding velocities helped every model. |
| **lightning-action** (Paninski lab) | TemporalMLP / GRU / dilated TCN | keypoints or video features | **MIT** | The cleanest permissive reference implementation of a TCN head for behavior |
| **BAMS** (arXiv 2206.07041) / its NeurIPS 2023 successor (arXiv 2303.08811) | Self-supervised multi-timescale encoders + action-prediction objective | pose | Apache-2.0 (`nerdslab/bams`) | Placed 1st overall on MABe22 mouse and fly. Representation learning, not a classifier. |
| **hBehaveMAE** (ECCV 2024) [R6] | Hierarchical masked autoencoder on raw pose | pose | **Apache-2.0** code; MABe22 / hBABEL / Shot7M2 checkpoints on Zenodo, **Apache-2.0** | Reported SOTA on MABe22 from raw pose. The only permissive pretrained *mouse* behavior encoder. Its input is 12 keypoints × 3 mice, which doesn't match our tracker, so a direct transfer is unlikely to work [?]. |
| **LISBET** (arXiv 2311.04069) | Self-supervised transformer on social keypoints | pose | PyPI 0.4.3, **no license declared** | Can't use until licensed |

### 1.3 What the benchmarks say actually won

| Benchmark | Winner's decision model | Labels | vs simpler |
|---|---|---|---|
| **CalMS21 / MABe 2021 Task 1** [R7] | Egocentric hand-crafted features + PCA of pairwise distances → **residual non-causal 1D-conv embedder + causal 1D-conv contexter**, semi-supervised (contrastive future prediction on unlabeled data), joint multi-task | about 10 h of labels plus 70 h unlabeled | F1 **0.864** vs 0.793 for a plain 1D-CNN on pose, and 0.829 with task-programming features. In the baseline sweep, the **1D-CNN (0.793) beat LSTM (0.675) and self-attention (0.610)**. |
| **CalMS21 Task 3** (7 new behaviors, few labels) [R7] | MS-G3D graph conv | few bouts, high imbalance | **Everyone was bad: F1 ~0.32–0.36.** Shorter bouts scored lower. **The few-shot regime is where all models fail**, so collect bouts rather than switching models. |
| **MABe22** (ICML 2023) [R8] | Representation benchmark with frozen embeddings + linear probes. Mouse "manual behaviors" F1 was only **0.18–0.28** for every method. | — | Video SSL (MAE, ρBYOL) beat trajectory-BERT on most mouse tasks, but every *behavior* probe was weak. Linear probes on generic embeddings don't make a behavior classifier. |
| **Kaggle MABe 2025** (social action recognition, multi-lab, ended ~Dec 2025) [R9][R10] | Top teams: **1D-CNN + GRU/Transformer/SqueezeFormer** on egocentric relative-keypoint features, **masked loss** for unannotated actions, **self-supervised pretraining** (1st place: future-coordinate prediction; 6th: masked-keypoint reconstruction), pretrain on all labs then fine-tune per lab, per-(lab, action) thresholds | many labs, many hours | "GBDT scored much lower alone **but gave a large gain in ensembles**" (6th place). The 4th place used XGB + NN chosen per lab. Winning F1 was only **≈0.5**, because cross-lab domain shift dominates. |
| **BEAST** (arXiv 2507.09513) [R11] | Experiment-specific ViT pretraining (MAE + temporal contrastive) → patch embeddings → TCN | CalMS21 | CalMS21 F1 0.81 (0.84 ensembled) vs **TREBA pose embeddings 0.72**. Generic ImageNet-MAE patch features got 0.74. **ImageNet MAE patches beat DINOv2 patches** on both of its datasets. Pretraining took about 25 h on 8 A40 GPUs. Code MIT, weights CC-BY-4.0. |
| **ResiFuse** (arXiv 2603.07279) [R1] | Pose motion codebook + VideoPrism context added only on high-residual frames, BiGRU head | Open Field, CalMS21, MABe22 | Supervised macro-F1, pose / VideoPrism / fused: Open Field 0.663 / 0.718 / **0.796**; CalMS21 0.593 / 0.849 / 0.865, **but hand-crafted MARS features reach 0.878**; MABe22 0.583 / 0.722 / **0.784**. Code MIT. |

**Bottom line for "what beats our GBDT".**
1. **More temporal context** is the first-order effect. Our windows stop at ±1 s. MARS uses 0.7 s at 30 fps, JABS peaked at about 60 frames (2 s at 30 fps), and TCNs see 10–60 s. Tail-suspension struggle/immobility is a slow behavior, so ±5–15 s context should help bout edges [inference].
2. **A temporal neural head beats a framewise tree** once labels reach several videos per behavior (Blau: 4–10 videos; CalMS21: hours). With 3–6 bouts it can't; in that regime even MS-G3D scored 0.33.
3. **Good kinematic features ≥ generic video embeddings** whenever tracking or pose observes the behavior. Video embeddings win where pose is blind (open field fine motor, grooming detail) [R1][R11].
4. **Cross-session and cross-rig shift outweighs the choice of architecture** (Kaggle 2025's F1 ≈ 0.5, per-lab fine-tuning, FPS bugs). Evaluation hygiene (§3.5) matters more than the model.

---

## 2. Video foundation models as embeddings on a Mac

### 2.1 Candidates

| Model | Type / size | Code / weights license | Mac path | Rodent evidence |
|---|---|---|---|---|
| **DINOv2** ViT-S/14 (also B, L, g) | Image SSL, 21.6 M (S) | **Apache-2.0 / Apache-2.0** (repo + HF `facebook/dinov2-small`, `timm/…lvd142m`) | **Tested**: timm → `torch.export` → coremltools `mlprogram` FP16, cosine **0.9997** against PyTorch on 3 real lab frames. (`torch.jit.trace` failed on an `int` op; `torch.export` works.) | BEAST appendix: DINOv2 patches < ImageNet-MAE patches on CalMS21 and IBL [R11]. Used frozen as a neural-encoding regressor there. |
| **DINOv3** ViT-S/16 (up to 7B) | Image SSL, 21.6 M (S) | **DINOv3 License** (custom): royalty-free use and redistribution **only under that agreement, shipped with a copy**; bans military/ITAR uses; HF repo gated "manual". The timm mirror is ungated, but the same license applies. | **Tested**: MPS works (957 img/s), but the Core ML FP16 conversion **diverged (cosine 0.18)**. RoPE or large activations probably overflow FP16 [?]. It needs FP32 or debugging. | None found |
| **SigLIP 2** B/16 (also L, So400m, g) | Image–text, 93 M vision tower | **Apache-2.0 / Apache-2.0** (`google/siglip2-base-patch16-224`) | **Tested** MPS: 468 img/s. Text tower enables zero-shot prompts. | None for rodents. Zero-shot text prompts are like the VLM case: coarse at best. |
| **V-JEPA 2** ViT-L/H/g (300 M / 600 M / 1 B), 64-frame 256/384 px | Video SSL (latent prediction) | **MIT** code. HF weights: vitl/vith tagged **MIT**, vitg tagged **Apache-2.0** | **Tested** (random init, timing only): ViT-L fp16 MPS = **11.1 clips/s at 16×256²** (178 frames/s non-overlapping) and 31 clips/s at 8×224² (250 frames/s). Core ML conversion not tried [?]. | None for rodents. PriVi (arXiv 2511.09675) builds a primate behavior model on V-JEPA. |
| **V-JEPA 2.1** (Mar 2026) | Dense-feature video SSL, up to 2 B | Repo MIT. Weights' license not confirmed [?]. Paper arXiv 2603.14482 per third-party listing [?]. | Same as V-JEPA 2 | None |
| **VideoPrism** B/L | Video–text encoder (16 frames, 288 px) | **Apache-2.0 / Apache-2.0** (`google/videoprism-base-f16r288`) | JAX/Flax only. Core ML would need a port [?]. Python JAX-on-Mac for offline use [?]. | **Best direct evidence**: frozen features + BiGRU gave CalMS21 0.849, MABe22 0.722, Open Field 0.718 [R1] |
| **VideoMAE V2** | Video MAE, distilled S/B, g (1 B) | Code MIT, **weights CC-BY-NC-4.0** (HF tags) | — | Avoid (weights) |
| **InternVideo2** | Video–text, 1 B / 6 B (+ distilled S/B/L) | Code Apache-2.0. `Stage2_1B` weights Apache-2.0 (gated auto). Distillation models: no license tag [?]. | 1 B is too heavy for long videos. Distilled models untested. | None |
| **BEAST** checkpoints | Experiment-specific ViT (MAE + contrastive) | Code MIT, weights **CC-BY-4.0** | timm-style ViT, so likely converts like DINOv2 [?] | CalMS21 0.81–0.84 [R11]. Trained on *their* rigs: it is a recipe more than a model for us. |

### 2.2 Throughput on this machine (tested)
Script: `scratchpad/decision/bench.py`, `vjepa.py`, `check.py`. Random input tensors except the Core ML numerics check.
MPS is batch 32 FP16 after 3 warm-ups. Core ML is batch 1 (Python `predict`), mean of 30 calls.

| Model | Input | MPS fp16 bs32 | Core ML CPU+ANE | Core ML CPU+GPU | 30 min @ 10 fps (18k frames) |
|---|---|---|---|---|---|
| DINOv2-S/14 | 224² | **1,134 img/s** | 361 img/s | **491 img/s** | **16 s** (MPS) / 37 s (Core ML GPU) |
| DINOv2-S/14 | 518² (timm default) | 166 img/s | — | — | 108 s |
| DINOv3-S/16 | 224² | 957 img/s | 541 img/s, **numerically wrong** | 433 img/s | 19 s (MPS) |
| SigLIP2-B/16 | 224² | 468 img/s | — | — | 38 s |
| V-JEPA 2 ViT-L | 16 × 256² clip | 11.1 clips/s = 178 frames/s | — | — | 100 s at stride 16; **~7 min at stride 4** |
| V-JEPA 2 ViT-L | 8 × 224² clip | 31 clips/s = 250 frames/s | — | — | 72 s at stride 8 |

Decoding (AVAssetReader about 2k fps at 640×480, video-ai.md §6) is not the bottleneck. **Crop on the tracker box**
before resizing to 224². A full 640×480 frame squashed to 224² leaves the mouse at about 20–40 px, which wastes most
tokens. That is an inference; validate it with a full-frame vs crop ablation. Downloads for the test: 526 MB (three
image models). Core ML packages are 41 MB each in FP16.

### 2.3 When an embedding is worth it
- **Not** for tail suspension, immobility, freezing, locomotion or ROI time. Tracker kinematics already observe these, and
  the GBDT matched human onsets within 2 s on an unseen animal.
- **Maybe** for grooming (face vs body), rearing in top view, digging, and social contact type (sniff vs allogroom). In these
  cases the blob is static while the paws move, and pose without paw keypoints is blind. ResiFuse's "add vision only where
  pose residuals are high" result [R1] supports adding embeddings as **extra columns next to kinematics, not replacing
  them**.
- **Go/no-go test:** add PCA-32 DINOv2 crop embeddings (plus 0.5/2 s rolling means) as `extra` columns to the existing
  GBDT. Keep them only if leave-one-animal-out bout F1 rises by ≥ 0.05 on a behavior with ≥ 30 bouts. If it doesn't, video
  embeddings won't save that behavior; more labels or pose will.

---

## 3. Training recipes on a Mac

### 3.1 Active learning loop (which windows to label next)
The review queue already *is* the loop. Make it deliberate:
1. **Query by committee for free.** CV already trains k fold models. Rank unlabeled windows by **fold disagreement**
   (variance of p) and by **margin** |p − 0.5|. A-SOiD's iterative low-confidence sampling is the same idea [R12].
2. **Rank bouts, not frames.** Propose a 5–15 s window around the uncertain span. Scorers label bouts, and 10 fps frames are
   too correlated to be independent queries.
3. **Diversity.** Pick at most one query per k-means cluster of the window features per round, so the queue isn't 20
   near-identical struggle onsets [standard practice, not sourced].
4. **Hard negatives.** Rejected proposals become negative labels automatically. In MARS-style pipelines most residual
   errors are false positives at transitions [R7].
5. **Keep a random audit set.** Actively chosen samples are biased, so **never report agreement on them**. Score a
   uniformly sampled, blinded subset for the numbers that go into papers (video-ai.md rigor protocol).

### 3.2 Class imbalance
- We up-weight positives 1–20×. That fixes training, but it **inflates p**, so the 0.6/0.4 hysteresis acts on biased
  probabilities. Either keep the weights and calibrate (3.4), or move the threshold. Don't do both silently.
- Count **bouts** per class. CalMS21 Task 3 shows that short, rare bouts fail regardless of model [R7].
- In multi-behavior ethograms, use a **masked loss for unscored behaviors** ("unscored" ≠ "absent"; every top Kaggle 2025
  team did this) [R10]. The annotation model should record which behaviors a scorer was actually scoring in each session.

### 3.3 Label efficiency: learning curves in bouts
- Report bout F1 (tIoU 0.5) and median onset error against the **number of training bouts and animals**. Subsample whole
  sessions in leave-one-animal-out, repeated 5× with different subsets.
- Reference points: JABS reached about 85 % accuracy from 10 videos [R3]; Blau's TCN used 4–10 mouse videos [R5]; MARS
  used about 7 h [R2]; CalMS21 Task 3's few-bout regime gave F1 ≈ 0.33 [R7].
- Starting heuristic **[?, not sourced]**: GBDT is usable at about 20–30 bouts from ≥ 4 animals; a temporal neural head
  starts to pay at about 100+ bouts from ≥ 8 animals. The curve decides, not this rule.

### 3.4 Calibration
- Fit **isotonic or Platt** calibration on **out-of-fold CV predictions** (never on training predictions) and store it in
  the model. Then 0.5 means 50 %, the annotation `confidence` field becomes meaningful, and hysteresis thresholds transfer
  between behaviors [standard practice].
- Report the **ECE** and a reliability plot in the training sheet, next to κ and bout F1.
- Under domain shift (a new rig) calibration breaks first. Flag sessions whose mean max-probability or feature
  distribution (per-feature PSI or KS against training) drifts [standard practice].

### 3.5 Domain shift and leakage
| Risk | Rule |
|---|---|
| Adjacent-frame leakage | Never split frames randomly. Session (preferably **animal**) holdout. Time blocks need a guard band ≥ the **largest feature window**: the 2 s guard is correct for ±1 s windows, but it must grow with longer windows or auto-context. |
| Same animal across sessions | Group folds by animal ID, not session. Our 90839 → 102295 test is the right kind. |
| Tuning on the test fold | Thresholds, decode priors and calibrators must come from the training folds (decode priors already do). Nested CV for any hyperparameter search. |
| Rig, lighting, frame rate | Features are already scale-free (arena-normalised, windows in seconds). Kaggle 2025's biggest swing was a **25 vs 30 fps label bug**, so assert label fps = FrameClock fps on import. |
| Annotator style | CalMS21 Task 2: annotator identity matters (0.754 → 0.809 with annotator embeddings) [R7]. Record the scorer and compute per-scorer agreement. |

---

## 4. Decision layers beyond classification

| Layer | What it adds | Credible? | Cost for us |
|---|---|---|---|
| **Hysteresis + min-bout + merge gap** (current) | Removes flicker; durations from human bouts | Yes. JABS-style stitch/filter raised bout agreement from 49 to 61 % [R3]. | Done |
| **HMM smoothing** (MARS) [R2] | Transition priors; one label per frame across behaviors | Yes; standard | ~80 lines of Rust (forward–backward or Viterbi over calibrated p) |
| **Explicit-duration HSMM / segmental Viterbi** | Duration distributions per behavior (log-normal fitted to human bouts); mutual exclusivity; posterior per bout | Yes in principle: HSMMs model non-geometric dwell times that plain HMMs can't [R13]. **No rodent study found that shows HSMM > tuned hysteresis on bout F1 [?]**. | ~150 lines of Rust, O(T·K·D_max). At 10 fps with D_max = 60 s that is 600 steps per frame per state, fine. |
| **Auto-context / stacking** | A second GBDT on first-pass probabilities rolled over 1–30 s. It learns "struggle bouts precede immobility" and similar structure, much as a stacked TCN stage does. | Yes: auto-context is the tree analogue of MS-TCN refinement stages [inference]. | **~40 lines**: reuse `train()`. The first-pass p must be **out-of-fold** to avoid leakage. |
| **Change-point detection** (PELT/BOCPD on kinematics) | Candidate boundaries for unlabeled sessions; ranks onsets for review | Moderate: unsupervised boundaries don't carry labels | Low priority |
| **Sequence models over states** (behavior-to-behavior transitions, AR-HMM) | Ethogram grammar | Useful as an *analysis* output (transition matrices vs ΔF/F), not as a decoder at our data sizes | Later |
| **LLM "reasoning"** | Turns a written ethogram definition into rule code (AmadeusGPT-style), explains features, drafts the methods text | Credible as an **authoring and audit** tool. The output is code a person reads. | Already planned (tier 1) |
| **VLM as verifier** of proposals | Yes/no on candidate clips | **Weak**: Rodent-Bench says no model is strong enough [video-ai.md]. Measured localization was wrong (behavior-ai.md). | Triage only, never decides |
| **LLM per-frame decisions** | — | **Not credible.** No temporal precision, no calibration, no benchmark support. | Don't |

---

## 4.1 Outcome on lab data (2026-10-25)
On 90839_TS (3 bouts), long mean/std windows (5–15 s) and the stacked second pass both lowered held-out κ by
0.1–0.4. Stacking was removed. Calibration and out-of-fold thresholds are gated to ≥ 10 training bouts. Numbers
are in [../ai.md](../ai.md). Re-test all three once the S0 eval set exists.

## 5. Recommendation for SimpleFP

### 5.1 What to add next on top of the GBDT (staged)

| Stage | Change | Deps | Success criterion vs current baseline (same folds) |
|---|---|---|---|
| **S0: measure** | Grow the eval set: ≥ 4 TS animals, ≥ 30 bouts, leave-one-animal-out. Add ECE and the learning curve in bouts to the Train sheet. Add a random blinded audit subset. | none | The baseline number is now trustworthy (CI from per-animal bootstrap) |
| **S1: context** | Add 5 s and 15 s windows plus min/max (MARS uses mean/std/min/max). Add **lagged** deltas (x(t+2 s) − x(t−2 s)). Widen the guard band to match. | none | Bout F1 +≥ 0.03 or median onset error −≥ 0.2 s, with no κ loss |
| **S2: auto-context** | Second-stage GBDT on out-of-fold first-stage p rolled over 1/5/15 s plus the S1 features | none | Same bar as S1; it must beat S1 itself, not only the baseline |
| **S3: calibration** | Isotonic on out-of-fold p; hysteresis on calibrated p; `confidence` = calibrated mean p | none | ECE ≤ 0.05; thresholds stop needing per-behavior tuning |
| **S4: multi-behavior decode** | Semi-Markov Viterbi with log-normal durations from human bouts and exclusive groups from the ethogram. Keep hysteresis as the default for single binary behaviors. | none | Wins **only** if bout F1 ≥ hysteresis on ≥ 3 behaviors; otherwise drop it |
| **S5: active learning** | Fold-disagreement × margin ranking of 10 s windows, k-means diversity, rejected proposals → negatives | none | Same F1 with ≥ 30 % fewer labeled bouts (simulated by replaying existing labels) |
| **S6: temporal NN head** (only if S1–S2 plateau and ≥ 100 bouts) | Dilated TCN (MS-TCN++ stage-1 design, ~50–100 k params) over the same feature rows. **Candle 0.11** (MIT/Apache-2.0, Metal) as an optional crate feature, or MLX-Swift 0.32.3 (MIT), which the app already plans for VLMs. Ship the GBDT as the fallback. | +1 crate | Beats S2 by ≥ 0.05 bout F1, paired across held-out animals (bootstrap CI excludes 0) |

Not doing (YAGNI): ASFormer / DiffAct / LTContext (data scale, and LTContext is NC); pretrained pose encoders (BAMS,
hBehaveMAE: schema mismatch); unsupervised discovery as a decider.

### 5.2 Video foundation model embeddings
- **When:** a behavior with ≥ 30 bouts that S1–S2 can't separate from tracker + pose (expected: grooming, rearing, social
  contact type). Not tail suspension.
- **Which:** **DINOv2-S/14, Apache-2.0**, on a tracker-centred crop at 224², via Core ML. Tested here: 491 img/s on the
  GPU, 0.9997 cosine to PyTorch, 41 MB FP16. That is about 40 s per 30 min session. Feed PCA-32 + rolling stats as `extra`
  columns. This is safe to bundle, or download on demand with provenance.
  - **Alternative to test second:** VideoPrism-B (Apache-2.0), which has the best rodent evidence [R1] but is JAX-only
    (offline Python).
  - **Avoid:** VideoMAE V2 (CC-BY-NC weights) and InternVideo2 distilled models (license unclear). Avoid DINOv3 in-app
    until the FP16 Core ML divergence is fixed and the custom license is accepted (it is redistributable with the
    agreement, but it is not Apache).
- **V-JEPA 2 ViT-L (MIT)** only as an offline research job (about 7 min per session at stride 4). Clip embeddings would
  need temporal pooling that blurs 10 fps onsets [inference].

### 5.3 Train our own model: now or later?
- **Now:** keep training **per-lab supervised heads** (GBDT → S2 → maybe a TCN). That *is* our model, and the data it needs
  is scored bouts, not compute.
- **Later, with gates:** self-supervised pretraining on the lab's own video (BEAST recipe, MIT code; or a masked-feature
  model over our kinematic + embedding sequences, like the Kaggle 2025 winners' future-prediction pretraining) only when
  **all** of these hold:
  - ≥ 100 h of unlabeled lab video (we have a handful of sessions);
  - ≥ 5 behaviors × ≥ 50 bouts × ≥ 8 animals of human labels for evaluation;
  - S2 + DINOv2 columns have plateaued on the learning curve.

  BEAST pretraining took about 25 h on 8 A40s [R11]. On an M5 Pro, expect days per run (an order-of-magnitude guess
  [?]), so rent a GPU for pretraining and do only inference and heads on the Mac.
- **Success criterion for any "own model":** it beats the then-current GBDT stack by ≥ 0.05 bout F1 on ≥ 5 held-out
  animals from ≥ 2 rigs, at equal or better ECE, and runs in ≤ 2 min per 30 min session on the Mac.

---

## Sources
- R1. Learning When Visual Context Matters for Mouse Behavior Analysis (ResiFuse), arXiv 2603.07279: https://arxiv.org/abs/2603.07279 · code (MIT): https://github.com/BRAINML-GT/ResiFuse
- R2. MARS, eLife 2021 (doi 10.7554/eLife.63720): https://elifesciences.org/articles/63720
- R3. JABS, eLife reviewed preprint 107259 (2025/2026): https://elifesciences.org/reviewed-preprints/107259
- R4. DeepEthogram, eLife 2021: https://elifesciences.org/articles/63377 · license: https://github.com/jbohnslav/deepethogram/blob/master/license.txt
- R5. Blau et al. 2024, action segmentation paradigms, arXiv 2407.16727: https://arxiv.org/abs/2407.16727 · daart (GPL-3.0): https://github.com/ablau100/daart · lightning-action (MIT): https://github.com/paninski-lab/lightning-action
- R6. hBehaveMAE, ECCV 2024 (doi 10.1007/978-3-031-73039-9_7): https://www.biorxiv.org/content/10.1101/2024.08.06.606796 · code (Apache-2.0): https://github.com/amathislab/BehaveMAE · weights (Apache-2.0): https://zenodo.org/records/13790191
- R7. CalMS21 / MABe 2021, arXiv 2104.02710: https://arxiv.org/abs/2104.02710 · winners: https://discourse.aicrowd.com/t/announcing-the-winner-for-multi-agent-behavior-challenge/5801
- R8. MABe22, ICML 2023, arXiv 2207.10553: https://arxiv.org/abs/2207.10553
- R9. Kaggle MABe 2025: https://www.kaggle.com/competitions/MABe-mouse-behavior-detection · 4th place: https://www.kaggle.com/competitions/MABe-mouse-behavior-detection/writeups/4th-place-solution-xgb-nn-ensemble
- R10. 6th-place retrospective (yu4u): https://speakerdeck.com/yu4u/kagglekonpeteisiyon-mabe-challenge-social-action-recognition-in-mice-zhen-rifan-ri · 2nd-place retrospective (DeNA): https://www.docswell.com/s/DeNA_Tech/Z8W439-2026-02-25-154748
- R11. BEAST, arXiv 2507.09513: https://arxiv.org/abs/2507.09513 · code (MIT): https://github.com/paninski-lab/beast · weights (CC-BY-4.0): https://huggingface.co/collections/paninski-lab/beast
- R12. A-SOiD preprint: https://www.biorxiv.org/content/10.1101/2022.11.04.515138v2
- R13. Hidden Markov and semi-Markov models (review), arXiv 2105.11490: https://arxiv.org/abs/2105.11490
- BAMS, arXiv 2206.07041: https://arxiv.org/abs/2206.07041 · NeurIPS 2023 successor, arXiv 2303.08811: https://arxiv.org/abs/2303.08811 · code: https://github.com/nerdslab/bams
- Action segmentation: MS-TCN https://arxiv.org/abs/1903.01945 (https://github.com/yabufarha/ms-tcn) · MS-TCN++ https://arxiv.org/abs/2006.09220 (https://github.com/sj-li/MS-TCN2) · ASRF https://arxiv.org/abs/2007.06866 · C2F-TCN https://arxiv.org/abs/2105.10859 · ASFormer https://arxiv.org/abs/2110.08568 · DiffAct https://arxiv.org/abs/2303.17959 · LTContext https://arxiv.org/abs/2308.11358 (https://github.com/LTContext/LTContext, CC-BY-NC-4.0)
- CEBRA license (Apache-2.0 for v0.4.0+, patent note): https://github.com/AdaptiveMotorControlLab/CEBRA/blob/main/LICENSE.md
- LISBET: https://arxiv.org/abs/2311.04069 · https://pypi.org/project/lisbet
- DINOv2, arXiv 2304.07193: https://github.com/facebookresearch/dinov2 · https://huggingface.co/facebook/dinov2-small
- DINOv3, arXiv 2508.10104: https://github.com/facebookresearch/dinov3/blob/main/LICENSE.md
- SigLIP 2, arXiv 2502.14786: https://huggingface.co/google/siglip2-base-patch16-224
- V-JEPA 2, arXiv 2506.09985: https://github.com/facebookresearch/vjepa2 · https://huggingface.co/facebook/vjepa2-vitl-fpc64-256 · V-JEPA 2.1 (third-party listing): https://sotaverified.org/papers/260314482
- VideoPrism: https://github.com/google-deepmind/videoprism · https://huggingface.co/google/videoprism-base-f16r288
- VideoMAE V2, arXiv 2303.16727: https://github.com/OpenGVLab/VideoMAEv2 · https://huggingface.co/OpenGVLab/VideoMAEv2-Base
- InternVideo2, arXiv 2403.15377: https://github.com/OpenGVLab/InternVideo · https://huggingface.co/OpenGVLab/InternVideo2-Stage2_1B-224p-f4
- PriVi (primate V-JEPA), arXiv 2511.09675: https://arxiv.org/abs/2511.09675
- Rodent social-behavior ML review (BANOS), arXiv 2508.04255: https://arxiv.org/abs/2508.04255
- Rust/Swift ML: candle 0.11 https://crates.io/crates/candle-core · burn 0.22.1 https://crates.io/crates/burn · mlx-swift 0.32.3 https://github.com/ml-explore/mlx-swift
