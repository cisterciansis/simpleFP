"""Build the RTMPose-m (AP-10K, 17 animal keypoints) Core ML pose model for SimpleFP.

Downloads the official OpenMMLab checkpoint, verifies its SHA-256, loads it into the
re-implementation in rtmpose.py (strict key/shape match), converts to Core ML
(fp16 for the Neural Engine and fp32), checks Core ML against PyTorch, and writes the
`.pose.json` sidecar the app needs. Weights are not redistributed by SimpleFP: you
download them from OpenMMLab under their terms (MMPose: Apache-2.0; AP-10K
annotations: CC-BY-4.0; the checkpoint was pretrained on COCO and AI Challenger).

    uv run --with torch==2.7.1 --with numpy --with coremltools python tools/pose/convert_rtmpose.py OUT_DIR
"""

import hashlib, json, pathlib, sys, urllib.request

import coremltools as ct
import numpy as np
import torch

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from rtmpose import load  # noqa: E402

URL = "https://download.openmmlab.com/mmpose/v1/projects/rtmposev1/rtmpose-m_simcc-ap10k_pt-aic-coco_210e-256x256-7a041aa1_20230206.pth"
SHA256 = "896e3665d849ef7eb9b6ec0995955796cc9810f024fa0aa0bdc18acb0d68bf52"
BODYPARTS = ["L_Eye", "R_Eye", "Nose", "Neck", "Root_of_tail", "L_Shoulder", "L_Elbow", "L_F_Paw", "R_Shoulder", "R_Elbow", "R_F_Paw",
             "L_Hip", "L_Knee", "L_B_Paw", "R_Hip", "R_Knee", "R_B_Paw"]


def main(out: pathlib.Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    ckpt = out / "rtmpose-m-ap10k.pth"
    if not ckpt.exists():
        print("downloading", URL)
        urllib.request.urlretrieve(URL, ckpt)
    digest = hashlib.sha256(ckpt.read_bytes()).hexdigest()
    if digest != SHA256:
        ckpt.unlink()
        sys.exit(f"checksum mismatch ({digest}); refusing to use the file")
    model = load(ckpt)
    traced = torch.jit.trace(model, torch.rand(1, 3, 256, 256) * 255)
    rng = np.random.default_rng(0)
    probe = rng.uniform(0, 255, (1, 3, 256, 256)).astype(np.float32)
    with torch.no_grad():
        rx, ry = (t.numpy() for t in model(torch.from_numpy(probe)))
    for name, prec in [("fp16", ct.precision.FLOAT16), ("fp32", ct.precision.FLOAT32)]:
        ml = ct.convert(traced, inputs=[ct.TensorType(name="image", shape=(1, 3, 256, 256))], outputs=[ct.TensorType(name="simcc_x"), ct.TensorType(name="simcc_y")],
                        convert_to="mlprogram", compute_precision=prec, minimum_deployment_target=ct.target.macOS14)
        ml.author = "OpenMMLab RTMPose-m (AP-10K); converted with SimpleFP tools/pose"
        ml.license = "MMPose Apache-2.0; AP-10K CC-BY-4.0; pretrained on COCO + AI Challenger (see their terms)"
        ml.short_description = "Top-down animal pose (17 AP-10K keypoints). Input: RGB 0-255 crop 256x256 (normalization inside). Outputs: SimCC logits, split ratio 2."
        path = out / f"rtmpose-m-ap10k-{name}.mlpackage"
        ml.save(str(path))
        o = ml.predict({"image": probe})
        err = max(float(np.abs(o["simcc_x"] - rx).max()), float(np.abs(o["simcc_y"] - ry).max()))
        print(f"{path.name}: max |Core ML − PyTorch| = {err:.4f} logits")
        if name == "fp32" and err > 1e-3:
            sys.exit("fp32 conversion does not match PyTorch")
        json.dump({"input": "image", "width": 256, "height": 256, "output": "simcc_x", "output_y": "simcc_y", "decoder": "simcc", "simcc_split_ratio": 2.0,
                   "channels_last": False, "crop_scale": 4.0, "input_scale": 1.0, "rotations": [0], "bodyparts": BODYPARTS},
                  open(out / f"rtmpose-m-ap10k-{name}.pose.json", "w"), indent=2)
    print("done →", out)


if __name__ == "__main__":
    main(pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "rtmpose-out"))
