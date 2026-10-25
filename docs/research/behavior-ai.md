# On-device behavior AI and private LLMs (2026-10-25)

Scope: implementation choices for running vision AI and LLMs **on this Mac or on attested private endpoints**, picking up
where [video-ai.md](video-ai.md) stops. That doc already surveys the tools (BORIS, SimBA, MoSeq and the rest), the
import formats, the FrameClock, VLM evidence (Rodent-Bench) and the rigor protocol. None of that is repeated here.
Target footage: one tethered mouse, top or side view (open arena, tail suspension, social test with a second mouse),
640×480 at 10 fps, CamHal DivX transcoded to H.264.
Machine: M5 Pro, macOS 26.6, Xcode 26.2 (macOS SDK 26.2), Swift 6.2.
**Tested** means run on this machine; probes live in the session scratchpad (`.../scratchpad/ai/`, ephemeral).
Licenses come from the GitHub license API, Hugging Face model cards and LICENSE files, queried 2026-10-25.
**[?]** marks a claim I could not verify from a primary source.

## TL;DR picks

| Layer | Pick | Why | Avoid |
|---|---|---|---|
| Baseline | Rust background-subtraction centroid + motion energy (in progress) | No model, no license, explainable. Covers locomotion, ROI and freezing. | — |
| Pose runtime | **Core ML** (`coremltools` 9.0, `mlprogram`, FP16), user-supplied or user-downloaded weights | Measured: HRNet-w32 at 640×480 is 7.4 ms/frame on the ANE, so a 30 min 10 fps video takes about 2 min | Core AI until the Xcode 27 SDK is installed. ONNX Runtime unless a model won't convert. |
| Pretrained mouse pose | **SuperAnimal-TopViewMouse** (HRNet-w32 + Faster R-CNN), downloaded by the user behind a license prompt | It is the only credible pretrained top-view mouse model. Academic use only. | Bundling it. Ultralytics YOLO-pose (AGPL covers the weights you train too). |
| Permissive pose fallback | Train your own **RTMPose / ViTPose** (Apache-2.0) or **RF-DETR Keypoint** (Apache-2.0, preview) | Licenses we can ship | — |
| Segmentation | Apple **coreml-sam2.1** (Apache-2.0), box-prompted from the tracker | Official Core ML. Tiny encoder measured at 31 ms on the GPU. | SAM 3 until its custom license is reviewed |
| Behavior classifier | Rust features → **Create ML `MLBoostedTreeClassifier`** in-app → Rust bout post-processing | Zero dependencies, trains in seconds, exports Core ML | Embedding SimBA, A-SOiD or MoSeq code (academic-only or GPL) |
| VLM triage | **Qwen3.5-2B/4B** or **Qwen3-VL-4B** (Apache-2.0) through **mlx-swift-lm** 3.32.3 (MIT) | Measured: Qwen3.5-2B 4-bit runs about 0.4 s per frame warm with a 2.3 GB peak. Localization was wrong, so use it for triage only. | FastVLM (Apple research-only), Qwen2.5-VL-3B (Qwen research license), Moondream 3 (BSL) |
| Remote LLM | One `OpenAICompatible` provider. Defaults: **Tinfoil** (Swift SDK verifies attestation), **Chutes TEE** (filter `confidential_compute`) | Both run inside TEEs. Tinfoil's verification is client-side, in Swift. | Treating a base URL alone as private. OpenRouter ZDR is a policy, not cryptography. |
| Apple LLM | Foundation Models (text only on SDK 26.2). Image input and Private Cloud Compute need **macOS 27 + Xcode 27**. | Free, private, no keys | Building on it before the SDK is installed |

---

## 1. Pose estimation on-device

### 1.1 Candidates

| Model | Task | License: code / weights | Pretrained mouse? | Core ML / MLX path | Speed on M5 Pro |
|---|---|---|---|---|---|
| **DLC SuperAnimal-TopViewMouse** [1][2] | Top-down 2D keypoints. HRNet-w32 `pose_model.pth` + Faster R-CNN `detector.pt`; DLCRNet-R50 (bottom-up) variant | DLC: **LGPL-3.0**. Weights: "Modified MIT", **"academic, non-commercial purposes only"**, plus "may not be used to harm any animal deliberately" [1] | **Yes**, 27 keypoints, mostly C57BL/6J on TopViewMouse-5K. MausHaus training data is 640×480 at 30 Hz, the same resolution as ours [1]. | DLC 3.0.2 `export_model` writes **PyTorch snapshots + `pytorch_config.yaml` only. No ONNX** [3]. Path: build the model with DLC's PyTorch code → `torch.jit.trace` → `coremltools.convert(mlprogram, FP16)`. The HRNet trace converted cleanly here (random weights). Port the heatmap decode (argmax + offset) to Rust. | HRNet-w32 stride-4 head: **1.5 ms** at a 256×256 crop and **7.4 ms** at 640×480, on the ANE (§6) |
| **SLEAP** (`sleap-nn` v0.3.5, 2026-10-23) [4] | Single-instance, centroid, top-down, bottom-up, multi-class (identity) | sleap-nn **GPL-3.0**. User-trained weights belong to the user. | No public mouse zoo | `sleap-nn export --format onnx` (experimental, **fixed input H×W**) [4]. Core ML options: ORT + CoreML EP, or re-trace the PyTorch model → coremltools (coremltools dropped its ONNX frontend). | Not tested [?]. UNet backbones are similar in cost to the ResNet row below. |
| **Lightning Pose** [5] | Semi-supervised keypoints + EKS | **MIT** | No public mouse weights. README: "requires a Linux or WSL environment with an NVIDIA GPU" [5] | No export. Its ResNet-50 / ViT-S backbones trace fine in PyTorch → Core ML [?]. Import its DLC-style CSV (already planned). | ResNet-50 at 640×480: **4.2–4.7 ms** (§6) |
| **RTMPose** (MMPose) [6] | Top-down SimCC keypoints | **Apache-2.0** code. AP-10K-trained `rtmpose-m` (256², AP 72.2) with a ready ONNX zip. Data: AP-10K is **CC-BY-4.0** [7]. | No. AP-10K covers 54 species (17 kpt quadruped). Rodent coverage unconfirmed [?]. The model needs fine-tuning on lab frames. | ONNX (mmdeploy) → ORT CoreML EP, or PyTorch → coremltools. MMPose last pushed 2025-08 (slowing). | rtmpose-m is 2.6 GFLOPs, less than HRNet-w32, so expect ≤ 1.5 ms per crop [?] |
| **ViTPose** [8] | Top-down keypoints | **Apache-2.0** | AP-10K / APT-36K checkpoints exist [?]. No mouse. | PyTorch → coremltools (ViT converts well) | Not tested |
| **RF-DETR Keypoint** (preview) [9] | Detection + keypoints (DINOv2) | **Apache-2.0** (N/S/M and Keypoint). Atto/Femto/Pico/XL/2XL are **PML 1.0**. | No | ONNX export exists. Core ML untested [?]. | 9.7 ms on a T4 (vendor figure) |
| **YOLO11 / YOLO26-pose** (Ultralytics) [10] | Detection + keypoints | **AGPL-3.0, including your trained weights**: "All Ultralytics YOLO trained models fall under the AGPL-3.0 License by default" [10]. Enterprise license by quote. | No | `format="coreml"` is one line | Fast, but irrelevant under the license |
| **Apple Vision** `VNDetectAnimalBodyPoseRequest` [11] | 25 joints | OS API | **No. Cats and dogs only.** Tested: `VNRecognizeAnimalsRequest.supportedIdentifiers()` returns `[Cat, Dog]`, and the pose request found 0 observations on a mouse frame. | Built in | — |

**What AGPL means for us:** shipping an Ultralytics model or runtime inside SimpleFP makes the *whole app* a derivative work, so we must publish all source, or buy a license. Models users train with Ultralytics are AGPL too [10]. Importing their **output CSV** is fine. Running their model **in our process** is the problem. The rule from video-ai.md stands: no YOLO in-app.

**SuperAnimal in practice:** we can't redistribute the weights for non-academic users. Even for academic users, the safe move is a **user-initiated download from the Hugging Face repo** [1], showing the license text first and recording `weights_blake3` in provenance. Conversion runs once, locally, through a script we ship (Python + DLC + coremltools), or later through an in-app converter. The model card warns that it is not suitable for other views. **Tail-suspension and side views therefore need a lab-trained model** [?, card wording via video-ai.md]. The fiber tether can be mistaken for the tail. Check the `tail_*` keypoints on our footage before trusting them.

### 1.2 Conversion recipe (Core ML)

1. `torch.jit.trace(model.eval(), torch.rand(1,3,H,W))`. Fixed shapes, one model per input size. Use a 256² crop for top-down, or full frame for single-animal bottom-up.
2. `ct.convert(traced, convert_to="mlprogram", compute_precision=FLOAT16, minimum_deployment_target=macOS15)`. coremltools **9.0** is current on PyPI (9.1.dev1 pre-release). It warns that torch > 2.7 is untested, but torch 2.14.1 converted HRNet and ResNet here.
3. Keep preprocessing (resize, mean/std) and heatmap decoding **outside** the model, in Rust. Then the same decoder serves Core ML, imported results, and tests.
4. Validate numerics: run the Python reference and Core ML (`.cpuOnly`) on 50 real frames, and require keypoint error < 0.5 px. Write the result into the model's sidecar JSON.
5. Pick compute units by measurement, not assumption. In §6 the ANE won for HRNet, while the GPU won for SAM 2's encoder.

**Core AI** (`.aimodel`, `torch.export` → `coreai-torch`) targets the macOS 27 generation. The toolchain is still marked preliminary [12]. macOS 27 shipped 2026-09-14 [13], but this machine runs 26.6 with SDK 26.2. Revisit after upgrading. Core ML keeps working.

---

## 2. Segmentation, detection, tracking

| Model | Task | License code / weights | Mouse? | Mac path | Speed |
|---|---|---|---|---|---|
| **SAM 2.1** [14] | Promptable masks (image + video memory) | **Apache-2.0** / Apache-2.0 | Zero-shot | **Apple publishes `coreml-sam2.1-{tiny,small,baseplus,large}`** (and SAM 2) on HF, Apache-2.0 [15]. They ship **image encoder + prompt encoder + mask decoder only, with no memory-attention video propagation.** To track, re-prompt each frame with the previous box or centroid. | **Tested** tiny encoder at 1024²: **30.6 ms GPU**, 107 ms ANE. Decoder not timed. That is about 9 min of encoder time for 30 min at 10 fps. |
| **EdgeTAM** [16] | SAM 2 for edge, video | **Apache-2.0** | Zero-shot | Official `export_to_coreml.py` (encoder 9.6 MB, prompt 2 MB, decoder 9.8 MB) [16]. A community 4-model video conversion claims 10 tracked fps on device [17] [?]. | Not tested |
| **SAM 3** [18] | Text-prompted "concept" segmentation ("mouse") | **SAM License** (custom, 2025-11-19) | Zero-shot by text | No Core ML | Review the license before use [?] |
| **Grounding DINO** [19] / **OWLv2** [20] | Zero-shot detection from text | Apache-2.0 / Apache-2.0 | Zero-shot | No official Core ML [?]. Heavy (Swin/ViT at 800–960 px). Use them **offline to auto-label boxes** for a small detector, not per frame. | Not tested |
| **RF-DETR N/S/M** [9] | Detection / instance seg | Apache-2.0 | Fine-tune | ONNX export. Core ML untested [?]. | 2.3–3.5 ms on a T4 (vendor) |
| **RT-DETRv2** [21] / **D-FINE** [22] / **DEIM** [23] | Real-time DETR detection | Apache-2.0 (all three) | Fine-tune | PyTorch → coremltools [?] | Not tested |
| **Create ML Object Detector** | Box detector trained in-app | Apple, user-owned output | Train on about 100–300 boxed frames [?] | Native `.mlmodel` | — |
| Apple `VNGenerateForegroundInstanceMaskRequest` | Class-agnostic salient-object masks | OS | **Tested: 0 instances** on a synthetic arena frame. Inconclusive; retest on real footage. | Built in | — |

**Recommendation for one mouse:** our Rust tracker gives the centroid and box. SAM 2.1-tiny, prompted by that box, adds a clean mask (area, elongation, orientation, nose/tail ends by mask skeleton) only when the user asks for it. That is *refinement*, not detection.

**Two mice (social test).** Identity is the hard part, not detection:
- Detection: background subtraction plus a split on blob merge, or a fine-tuned RF-DETR-N / Create ML detector when the mice touch.
- Association: Hungarian matching on IoU + Kalman motion. This is the ByteTrack recipe (MIT [24]); about 150 lines of Rust to reimplement, no dependency.
- **Identity cues specific to our rigs:** the implanted mouse carries a **tether and headcap**, while the stimulus mouse usually doesn't. Add a per-blob "tether present" feature (thin dark line from blob to the top edge in top view) and coat or size histograms. This is a domain inference, not from a paper. Validate it.
- Always surface **identity-swap candidates** (low assignment margin, merged blobs) in the review queue. SLEAP's `multi_class_topdown` models learn identity [4]. idtracker.ai is GPL-3.0 [25] and is meant for external use, not embedding.

---

## 3. Behavior classification from pose and tracks

### 3.1 Licenses decide what we can embed

| Tool | Method | License (verified in the LICENSE file) | Embed? | Reimplement? |
|---|---|---|---|---|
| Keypoint-MoSeq [26] | AR-HMM syllables (JAX) | **Harvard non-commercial academic license** | No | The model is published (SLDS + keypoint noise), but a JAX Gibbs sampler is a big port. Defer. Users can run kpms externally and we import syllables. |
| A-SOiD [27] | Active learning + RF | **"Clear BSD", academic and research use only** | No | The idea (stratified active learning, class balancing) is free to reuse. Our review queue already has the bones. |
| B-SOiD | UMAP + HDBSCAN + RF | **GPL-3.0**, last push 2024-02 | No | Yes in principle: UMAP+HDBSCAN on windowed features. Low priority, because clusters aren't stable across sessions (video-ai.md). |
| SimBA [28] | Feature extraction + RF + SHAP | **Modified BSD, "academic and research purposes only"** | No | Feature ideas (hull area, body-part distances, rolling stats at several windows) are standard kinematics. Write our own. |
| VAME [29] | VRNN + HMM | **GPL-3.0** | No | Defer |
| LISBET [30] | Self-supervised transformer on social keypoints | Not found [?] | — | Interesting for 2-mouse social, after the license is checked |
| MICE (NeurIPS 2026) [31] | Hierarchical pose foundation model, 1–4 mice | "Code and weights will be released" | — | Watch |

### 3.2 What works (evidence)
- **Supervised beats unsupervised on labeled metrics.** Blau et al. 2024 (IBL/Paninski) compared supervised, unsupervised and semi-supervised action segmentation on fly, mouse and human data. Fully supervised **temporal convolutional networks with temporal context** did best [32]. Their mouse set: 21 pose features (distances, angles, areas), with rearing and grooming models trained on 4–10 labeled videos [32].
- Tree ensembles on hand-crafted windowed features are the standard in SimBA, JAABA and JABS. They are explainable and train in seconds. The gain from a TCN comes mostly from the temporal context, which windowed rolling features also provide. That is my inference, so test it on lab data.
- **Data requirement: there is no universal number** (no source I found gives one). JABS and others publish learning curves per behavior [33]. Rule of thumb for starting **[?, heuristic, not sourced]**: at least 50 bouts and about 2–5k positive frames per behavior, from at least 5 animals, with a matched pool of negatives. Then *measure*: the app plots held-out F1 against labeled bouts (leave-one-animal-out). Stop labeling when the curve flattens. Lab 10 fps footage halves frames per bout relative to 20–30 fps datasets, so count **bouts, not frames**.

### 3.3 Recommended in-app classifier
1. **Features (Rust, `core::behavior::features`, pure and tested):** per frame from the tracker or pose.
   - Centroid speed and acceleration (dt from the FrameClock), body length and elongation, heading change, mask or blob area, motion energy.
   - Keypoint pairwise distances and angles (when pose exists), distance to ROIs.
   - For 2 mice: inter-animal distance, nose-to-nose and nose-to-tail distances, relative heading.
   - Each feature gets rolling **mean/std/min/max over 0.3, 1, 3 s** windows, centered. Windows in seconds, not frames, so 10 and 30 fps projects share a preset.
2. **Model v1: Create ML `MLBoostedTreeClassifier`** (CreateML.framework, macOS 10.14+, present in SDK 26.2) [34]. It trains in-process on the Mac, outputs a Core ML model for inference, and needs no new dependency. This is ladder rung 3: a native platform feature. Export feature importances via **permutation importance computed in Rust**, a few lines, in place of SHAP.
3. **Post-processing (Rust, hashed preset):** probability smoothing → hysteresis threshold → min bout / max gap merge. Outputs become `Proposed` annotations (video-ai.md rigor protocol).
4. **Evaluation:** leave-one-animal-out CV. Reuse the existing agreement metrics (frame κ/PABAK, bout F1 at tIoU 0.5). Show the learning curve.
5. **Active learning (A-SOiD idea, our code):** queue the windows with the highest uncertainty and lowest class coverage for labeling.
6. **v2, only if v1 plateaus:** a small dilated TCN over the feature sequence (about 100k params), trained with **MLX-Swift** (MIT, autograd, `MLXOptimizers`) or `MLActivityClassifier` [?, whether it accepts arbitrary numeric feature columns]. A transformer isn't justified at our data sizes.

### 3.4 Self-supervised video embeddings
| Model | License code / weights | Mac feasibility |
|---|---|---|
| **V-JEPA 2** [35] | **MIT / MIT** (HF `facebook/vjepa2-vitl-fpc64-256`) | ViT-L (300M) on 64×256² clips runs in PyTorch MPS offline [?, untested]. Embeddings + a linear probe could rank "interesting" windows. Research-grade, not v0.4. |
| VideoMAE [36] | **CC-BY-NC-4.0** (repo LICENSE) | Avoid for distribution |
| BEAST (Paninski lab, 2025) [37] | Paper CC-BY. Code not linked [?]. | Experiment-specific ViT pretraining (masked AE + temporal contrastive). Promising for a lab with lots of unlabeled video. Watch. |

---

## 4. On-device VLMs and LLMs

### 4.1 Models
| Model | Sizes | License (HF card / LICENSE) | MLX Swift (`mlx-swift-lm` 3.32.3, `Libraries/MLXVLM/Models`) | Notes |
|---|---|---|---|---|
| **Qwen3.5** (natively multimodal, `image-text-to-text`) | 0.8B, 2B, 4B, 9B, 27B, 35B-A3B | **Apache-2.0** | `Qwen35.swift`, `Qwen35MoE.swift` | **Tested** 2B 4-bit (1.75 GB): prefill **1,869 tok/s**, decode **180 tok/s**, peak **2.3 GB**. 355 prompt tokens per 640×480 frame. |
| **Qwen3-VL** | 2B, 4B, 8B (+MoE) | **Apache-2.0** | `Qwen3VL.swift`, `Qwen3VLMoE.swift` | 4B 4-bit is 3.1 GB. Has video input. |
| Qwen2.5-VL | 3B / 7B | **3B: Qwen Research License (non-commercial)**. 7B: Apache-2.0. | `Qwen25VL.swift` | Skip the 3B |
| **Gemma 4** | E2B, E4B, 31B (`any-to-any`) | **Apache-2.0** (Gemma 4 license link) | `Gemma4.swift` | Good alternative family |
| Gemma 3 / 3n | 4B / E4B | Gemma Terms of Use (use policy) | `Gemma3.swift` | Prefer Gemma 4 |
| **SmolVLM2** | 256M, 500M (video), 2.2B | **Apache-2.0** | `SmolVLM2.swift` | Smallest; weak reasoning |
| Moondream 2 / 3 | 2B / 9B-A2B | 2: Apache-2.0. **3: BSL 1.1, no third-party service.** | Python mlx-vlm only [?] | 3 is restricted for embedding in a paid product |
| FastVLM | 0.5B–7B | **Apple AMLR, research-only** | `FastVLM.swift` | Don't ship |
| **Apple Foundation Models** [38] | On-device ~3B [?] | OS API | — | SDK 26.2 (installed): **text only**. I checked the `.swiftinterface`: no `Attachment`, no `contextSize`. **macOS 27:** image `Attachment` in prompts, `contextSize` (8,192 on-device), and `PrivateCloudComputeLanguageModel` (32K context, reasoning levels, entitlement required, "no cloud API costs" under 2M first-time downloads, daily per-user limits) [38]. `mlx-swift-lm` ships `MLXFoundationModels`, which plugs MLX models into `LanguageModelSession` (27 SDK). |

**Test result that matters:** I asked Qwen3.5-2B to locate the mouse in a synthetic arena frame. It answered "(411, 411), sitting". The true centroid is about (345, 222). **VLMs must not supply coordinates or timings.** Use them only for coarse, seconds-scale triage questions ("is the mouse grooming in this 2 s clip? yes/no + confidence") on frames the tracker has already localized and cropped. That agrees with Rodent-Bench (video-ai.md §4).

**Throughput for triage:** at about 0.4 s per frame warm, scoring one keyframe per 2 s window of a 30 min session (900 windows) takes about 6 min with a 2B model. Multi-frame clips cost proportionally more image tokens. Run it as a background job with progress, and never auto-accept.

**In-app runtime:** `mlx-swift` 0.32.3 + `mlx-swift-lm` 3.32.3 (MIT), with `swift-huggingface` / `swift-transformers` for download and tokenizer. Download weights on first use into Application Support, never bundled, and pin the HF commit hash in provenance. Python `mlx-vlm` 0.7.10 (MIT) is the prototyping and benchmark tool. It also has `mlx_vlm.server` (OpenAI-compatible), useful for testing the provider abstraction locally.

---

## 5. Private and remote LLMs

| Provider | Isolation | API | Vision models (live) | Client-side verification | Notes |
|---|---|---|---|---|---|
| **Chutes** [39][40] | Intel TDX CVM + NVIDIA confidential compute (PPCIe). LUKS root unlocks only after attestation. cosign-signed images only. Miners are anonymous and permissionless, so **only TEE chutes protect data in use from the host** [41]. | OpenAI-compatible `https://llm.chutes.ai/v1`, Bearer `cpk_…`. Evidence: `GET api.chutes.ai/chutes/{id}/evidence?nonce=<64 hex>` (authenticated) [39]. | **Tested `/v1/models`: 16 models, 14 with `confidential_compute: true`.** Image input: `gemma-4-31B-turbo-TEE`, `Qwen3.6-27B-TEE`, `Qwen3.8-27B-TEE`, `Qwen3.5-397B-A17B-TEE`; image+video: `Kimi-K2.6-TEE`, `Kimi-K3-TEE`. | Chutes says parsed evidence ≠ attestation unless **DCAP validation** ran [39]. **E2EE** (ML-KEM-768, decrypt only inside the TEE) is optional and not default [40]. No first-party Swift verifier. `teep` (AGPL-3.0, Go) is a local verifying proxy for Chutes, Tinfoil, Phala and others [42]. | Privacy policy: "We do not log, store, or persist the content of your API requests or responses" [43]. Without E2EE, TLS terminates at Chutes' gateway, so the policy is the guarantee there. Pricing from ~$0.02/M tokens. |
| **Tinfoil** [44] | Secure enclaves (CPU + GPU CC). Configs are public repos (e.g. `tinfoilsh/confidential-kimi-k3`). | OpenAI-compatible, `https://inference.tinfoil.sh/v1` (direct access "not recommended": it skips verification) [45] | `gemma4-31b`, `kimi-k3`, `deepseek-v4-1-flash` [46] | **Swift SDK `TinfoilAI` (Apache-2.0)** wraps MacPaw's OpenAI client and verifies attestation and supply chain on every connection, failing closed [47]. There is also a menu-bar proxy app. | Best verifiable story for a Swift app |
| **Phala / RedPill** [48] | GPU TEE (H100/H200) + TDX gateway | OpenAI-compatible (`api.redpill.ai`; docs disagree on the host [?]) | Subset of 200+ gateway models run in TEE [?] | Attestation report + signed responses; Python verifier | Check which models are actually TEE-hosted |
| **Apple PCC** (macOS 27) [38] | Apple PCC, researcher-verifiable | `FoundationModels` only, no HTTP API | Vision on-device; PCC multimodality [?] | Done by the OS | Entitlement application. Not OpenAI-compatible. |
| **OpenRouter + ZDR** [49] | None (routing policy) | OpenAI-compatible; `"provider": {"zdr": true}` | Many | None, contractual only | Never call this "private"; call it "no-retention policy" |

**How to integrate (one abstraction, three backends).** This lives in Swift, not Rust: networking and models are app concerns, and `core` stays pure.

```swift
protocol LLMProvider: Sendable {
    var id: ProviderID { get }                    // provenance: provider, model, endpoint, revision
    var privacy: PrivacyLevel { get }             // .local | .attestedTEE(verified: Bool) | .policyOnly
    func respond(_ messages: [Message], images: [CGImage], schema: JSONSchema?) async throws -> Response
}
// LocalMLX (mlx-swift-lm) · AppleFoundation (macOS 27: SystemLanguageModel / PCC) ·
// OpenAICompatible(baseURL, keychainKey, model) → Chutes, Tinfoil (via TinfoilAI for verification),
// OpenRouter (zdr: true), local mlx_vlm.server / Ollama.
```

Rules:
- API keys live in the **Keychain**. Base URLs come from an allowlist plus explicit user entry.
- Remote is **off by default, per project**. Show what leaves the machine: prefer sending **derived text** (feature summaries, ethogram definitions) over frames. Frames require a second opt-in.
- `PrivacyLevel.attestedTEE(verified: true)` only when a verifier actually ran: TinfoilAI, or Chutes DCAP/E2EE. Otherwise the UI says "TEE claimed, not verified".
- Store provider, model, `confidential_compute`, verification result and prompt hash in `Annotation.source = Model{…}`. Outputs are always `review = Proposed`.
- Use structured JSON outputs (OpenAI `response_format` or `@Generable` on Apple), validated against the schema at the boundary. Reject unknown fields.

---

## 6. Measurements on this machine (tested)

Setup: Python 3.12 venv, torch 2.14.1, coremltools 9.0, timm 1.0.30 (random weights, no downloads), mlx-vlm 0.7.10.
Core ML `mlprogram` FP16, batch 1, macOS 15 target. Times are the mean of 30 `predict()` calls after 5 warm-ups, **including Python overhead**. In-app Swift calls with IOSurface buffers should be equal or faster.

| Model | Input | CPU+ANE | CPU+GPU | ALL |
|---|---|---|---|---|
| HRNet-w32 + 27-kpt 1×1 head (stride-4 branch; SuperAnimal-like) | 256×256 | **1.5 ms** | 5.1 ms | 6.4 ms |
| same | 480×640 | **7.4 ms** | 9.6 ms | 9.9 ms |
| ResNet-50 backbone + head (DLC/LP-like, stride 32) | 256×256 | 1.0 ms | 2.8 ms | 1.0 ms |
| same | 480×640 | 4.7 ms | **4.1 ms** | 4.2 ms |
| Apple `coreml-sam2.1-tiny` image encoder (downloaded, 64 MB) | 1024×1024 | 107 ms | **30.6 ms** | 30.6 ms |
| Qwen3.5-2B-MLX-4bit (mlx-vlm), 1 image + prompt | 640×480 | prefill 355 tok at 1,869 tok/s; decode 180 tok/s; 5.1 s wall incl. load; peak 2.3 GB | | |
| Apple Vision foreground mask + animal pose + recognizer | 640×480 | 1.27 s for the first call (model load); 0 detections on a synthetic frame | | |

Implications: pose (7.4 ms/frame full frame) and decoding (about 2k fps, video-ai.md §6) let a 30 min, 10 fps session (18k frames) be **posed in about 2–3 min** on the ANE. Add a detector for top-down crops [? Faster R-CNN not timed]. SAM 2 per frame is the expensive step (about 9 min), so run it on demand or on keyframes. The fact that `ALL` was slower than `CPU_AND_NE` for HRNet shows compute-unit choice must be measured per model. Persist the winner per model in its sidecar.

---

## 7. Training on a Mac

| Goal | Practical recipe | Labels needed |
|---|---|---|
| **Pose (lab model)** | Label in-app (keypoint clicks on frames sampled by k-means on appearance; export DLC `CollectedData` CSV/COCO JSON). Then **fine-tune SuperAnimal-TopViewMouse** in DLC 3 for top view (about 10× more data-efficient [2]), or train RTMPose/ViTPose (Apache) for side and tail-suspension views. Train in Python with PyTorch **MPS** (DLC/SLEAP MPS support unverified [?]) or on a rented GPU. Convert with our script (§1.2). | Classic DLC: about **100–200 frames** for good single-animal accuracy (Mathis 2018) [?]. SuperAnimal fine-tune: fewer. Test the plateau on a held-out video. |
| **Box detector** (2 mice) | Create ML Object Detection in-app, or RF-DETR-N fine-tune (Python). Bootstrap boxes with Grounding DINO/OWLv2 offline, then human-correct. | About 100–300 boxed frames [?] |
| **Behavior classifier** | In-app Create ML boosted trees on Rust features (§3.3). Seconds to train. | Count bouts, not frames; use the learning curve (§3.2) |
| **VLM adaptation** | Not recommended. If ever: LoRA via `mlx-vlm` or `mlx-swift-lm` (both support LoRA). MouseGPT-style captions are unvalidated (video-ai.md). | — |

Create ML has **no keypoint-model training** (only object detection, classifiers, and activity/action classifiers on Apple's *human* body pose) [34] [?]. Lightning Pose officially needs Linux + NVIDIA [5].

---

## 8. Recommended architecture for SimpleFP

```text
 AVAssetReader (Swift) ──► CVPixelBuffer frames (IOSurface)
     │
     ├─► [1] Classical tracker (Rust, building now): background model → blob → centroid, box, area,
     │        elongation, motion energy, freezing. Two mice: Hungarian + Kalman + tether/size identity cues.
     │        Output: Track{frame → (x,y,box,area,me)}, QC (lost frames, swaps).
     │
     ├─► [2] Core ML pose runner (Swift, thin): model = user-supplied .mlpackage + sidecar JSON
     │        {skeleton, input size, mean/std, decode=heatmap|simcc, compute_units, license, weights_blake3}.
     │        Crops from [1] boxes (top-down) or full frame. Raw tensors → Rust decoder → PoseTrack.
     │        SuperAnimal: user downloads + converts behind a license prompt; never bundled.
     │        Optional: SAM 2.1-tiny mask refinement prompted by [1] boxes, on demand.
     │
     ├─► [3] Behavior classifier (Rust features → Create ML boosted trees → Rust post-processing)
     │        Trained in-app from accepted annotations; LOAO CV + learning curve; outputs Proposed bouts.
     │
     ├─► [4] VLM triage (mlx-swift-lm, Qwen3.5/Qwen3-VL/Gemma 4, Apache-2.0, downloaded on demand)
     │        Crops from [1], seconds-scale yes/no questions, ranks windows for human review; never coordinates.
     │
     └─► [5] LLMProvider (Swift): LocalMLX | AppleFoundation (macOS 27) | OpenAICompatible
              (Tinfoil verified, Chutes TEE, OpenRouter ZDR). Uses: ethogram drafting, feature→rule code
              (AmadeusGPT-style, auditable), report text. Opt-in per project; provenance on every output.
```

**Order of work.**
1. Finish the tracker [1] and its features. *Check:* synthetic video with a known path, plus the luma probe.
2. The Rust feature set + Create ML classifier [3] on tracker features alone. That covers freezing, immobility in tail suspension and locomotor bouts before any pose exists.
3. Pose runner [2] with a converted SuperAnimal model on a real lab video. *Check:* compare against DLC Python output on 50 frames.
4. VLM triage [4]
5. Remote providers [5]

**Open questions for the lab:**
1. Commercial plans? SuperAnimal, SimBA, A-SOiD and MoSeq are all academic-only.
2. Are tethered and stimulus mice visually distinct (coat or tether) in social tests?
3. May any frames leave the machine, even to an attested TEE (IRB/IACUC or institutional data policy)?

## Sources
1. SuperAnimal-TopViewMouse model card and license: https://huggingface.co/mwmathis/DeepLabCutModelZoo-SuperAnimal-TopViewMouse
2. Ye et al., SuperAnimal (Nat Commun 2024): https://arxiv.org/abs/2203.07436
3. DLC 3 export API (`export_model`): https://github.com/DeepLabCut/DeepLabCut/blob/main/deeplabcut/pose_estimation_pytorch/apis/export.py · v3.0.2 changelog: https://github.com/DeepLabCut/DeepLabCut/blob/main/changelog/3_0_2/v3_0_2.md
4. sleap-nn export guide: https://nn.sleap.ai/latest/guides/export/ · https://nn.sleap.ai/dev/api/export/cli · repo (GPL-3.0): https://github.com/talmolab/sleap-nn
5. Lightning Pose (MIT): https://github.com/paninski-lab/lightning-pose
6. RTMPose project (AP-10K model, ONNX SDK): https://github.com/open-mmlab/mmpose/tree/main/projects/rtmpose
7. AP-10K (CC-BY-4.0): https://github.com/AlexTheBad/AP-10K · paper https://arxiv.org/abs/2108.12617
8. ViTPose (Apache-2.0): https://github.com/ViTAE-Transformer/ViTPose
9. RF-DETR (Apache-2.0 / PML 1.0 table): https://github.com/roboflow/rf-detr
10. Ultralytics licensing: https://www.ultralytics.com/license · YOLO26: https://github.com/ultralytics/yolo26
11. Vision animal pose (WWDC23 10045): https://developer.apple.com/videos/play/wwdc2023/10045 · https://developer.apple.com/documentation/vision/vndetectanimalbodyposerequest
12. Core AI workflow (via Ultralytics docs): https://docs.ultralytics.com/integrations/coreai
13. macOS 27 release date: https://www.mactech.com/2026/09/09/macos-27-will-arrive-on-september-14-along-with-other-os-updates/amp/
14. SAM 2 (Apache-2.0): https://github.com/facebookresearch/sam2
15. Apple Core ML SAM 2.1: https://huggingface.co/apple/coreml-sam2.1-tiny · https://huggingface.co/apple/coreml-sam2.1-large
16. EdgeTAM (Apache-2.0, Core ML export): https://github.com/facebookresearch/EdgeTAM
17. EdgeTAM Core ML video (community): https://huggingface.co/AhmadYarAI/EdgeTAM-CoreML-Video
18. SAM 3 (SAM License): https://github.com/facebookresearch/sam3
19. Grounding DINO: https://github.com/IDEA-Research/GroundingDINO
20. OWLv2: https://huggingface.co/google/owlv2-base-patch16-ensemble
21. RT-DETR: https://github.com/lyuwenyu/RT-DETR
22. D-FINE: https://github.com/Peterande/D-FINE
23. DEIM: https://github.com/Intellindust-AI-Lab/DEIM
24. ByteTrack (MIT): https://github.com/FoundationVision/ByteTrack
25. idtracker.ai: https://pypi.org/project/idtrackerai · v6 https://elifesciences.org/reviewed-preprints/107602
26. Keypoint-MoSeq license: https://github.com/dattalab/keypoint-moseq/blob/main/LICENSE.md
27. A-SOiD license: https://github.com/YttriLab/A-SOID
28. SimBA license: https://github.com/sgoldenlab/simba
29. VAME (GPL-3.0): https://github.com/EthoML/VAME
30. LISBET: https://arxiv.org/abs/2311.04069 · https://pypi.org/project/lisbet
31. MICE (NeurIPS 2026): https://neurips.cc/virtual/2026/poster/150591
32. Blau et al. 2024, action segmentation paradigms: https://arxiv.org/abs/2407.16727
33. JABS-AL learning curves: https://elifesciences.org/reviewed-preprints/107259/figures
34. Create ML `MLBoostedTreeClassifier`: https://developer.apple.com/documentation/createml/mlboostedtreeclassifier · Create ML: https://developer.apple.com/machine-learning/create-ml
35. V-JEPA 2 (MIT): https://github.com/facebookresearch/vjepa2 · https://huggingface.co/facebook/vjepa2-vitl-fpc64-256
36. VideoMAE (CC-BY-NC-4.0): https://github.com/MCG-NJU/VideoMAE
37. BEAST: https://arxiv.org/abs/2507.09513
38. WWDC26 "What's new in Foundation Models" (241): https://developer.apple.com/videos/play/wwdc2026/241/ · PCC session notes: https://wwdcnotes.com/documentation/wwdc26-319-build-with-the-new-apple-foundation-model-on-private-cloud-compute/
39. Chutes private inference: https://chutes.ai/agents/private
40. Chutes, verifiable private inference: https://chutes.ai/news/private-ai-inference-verifiable-confidential-llm-serving
41. Chutes TEE architecture: https://chutes.ai/news/confidential-compute-for-ai-inference-how-chutes-delivers-verifiable-privacy-with-trusted-execution-environments
42. teep (AGPL-3.0 verifying proxy): https://github.com/13rac1/teep
43. Chutes privacy policy: https://chutes.ai/privacy
44. Tinfoil docs: https://docs.tinfoil.sh/
45. Tinfoil direct API: https://docs.tinfoil.sh/sdk/direct-api-access.md
46. Tinfoil vision models: https://docs.tinfoil.sh/models/vision.md
47. Tinfoil Swift SDK: https://docs.tinfoil.sh/sdk/swift-sdk · https://github.com/tinfoilsh/tinfoil-swift
48. Phala confidential AI API: https://docs.phala.com/phala-cloud/confidential-ai/confidential-model/confidential-ai-api
49. OpenRouter ZDR: https://openrouter.ai/docs/guides/features/zdr
50. mlx-swift-lm (MIT, VLM list): https://github.com/ml-explore/mlx-swift-lm · mlx-vlm: https://github.com/Blaizzy/mlx-vlm · coremltools: https://pypi.org/project/coremltools/
