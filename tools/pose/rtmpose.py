"""RTMPose-m (CSPNeXt-P5 + RTMCC head) re-implemented to load the official MMPose
AP-10K checkpoint with strict key/shape matching. Reference: mmdet CSPNeXt,
mmpose RTMCCHead / RTMCCBlock (Apache-2.0)."""
import math, torch, torch.nn as nn, torch.nn.functional as F

class ConvModule(nn.Module):
    def __init__(s, i, o, k, st=1, p=0, g=1):
        super().__init__(); s.conv = nn.Conv2d(i, o, k, st, p, groups=g, bias=False); s.bn = nn.BatchNorm2d(o, eps=1e-5); s.act = nn.SiLU()
    def forward(s, x): return s.act(s.bn(s.conv(x)))

class DWSep(nn.Module):
    def __init__(s, i, o, k):
        super().__init__(); s.depthwise_conv = ConvModule(i, i, k, 1, k // 2, g=i); s.pointwise_conv = ConvModule(i, o, 1)
    def forward(s, x): return s.pointwise_conv(s.depthwise_conv(x))

class CSPNeXtBlock(nn.Module):
    def __init__(s, c, add):
        super().__init__(); s.conv1 = ConvModule(c, c, 3, 1, 1); s.conv2 = DWSep(c, c, 5); s.add = add
    def forward(s, x):
        y = s.conv2(s.conv1(x)); return x + y if s.add else y

class ChannelAttention(nn.Module):
    def __init__(s, c):
        super().__init__(); s.fc = nn.Conv2d(c, c, 1, bias=True)
    def forward(s, x): return x * F.hardsigmoid(s.fc(F.adaptive_avg_pool2d(x, 1)))

class CSPLayer(nn.Module):
    def __init__(s, i, o, n, add):
        super().__init__(); m = o // 2
        s.main_conv = ConvModule(i, m, 1); s.short_conv = ConvModule(i, m, 1); s.final_conv = ConvModule(2 * m, o, 1)
        s.blocks = nn.Sequential(*[CSPNeXtBlock(m, add) for _ in range(n)]); s.attention = ChannelAttention(2 * m)
    def forward(s, x):
        return s.final_conv(s.attention(torch.cat((s.blocks(s.main_conv(x)), s.short_conv(x)), 1)))

class SPPF(nn.Module):
    def __init__(s, i, o):
        super().__init__(); m = i // 2; s.conv1 = ConvModule(i, m, 1); s.conv2 = ConvModule(4 * m, o, 1); s.pool = nn.MaxPool2d(5, 1, 2)
    def forward(s, x):
        x = s.conv1(x); y1 = s.pool(x); y2 = s.pool(y1); return s.conv2(torch.cat([x, y1, y2, s.pool(y2)], 1))

class CSPNeXt(nn.Module):  # P5, deepen 0.67, widen 0.75
    def __init__(s):
        super().__init__()
        s.stem = nn.Sequential(ConvModule(3, 24, 3, 2, 1), ConvModule(24, 24, 3, 1, 1), ConvModule(24, 48, 3, 1, 1))
        s.stage1 = nn.Sequential(ConvModule(48, 96, 3, 2, 1), CSPLayer(96, 96, 2, True))
        s.stage2 = nn.Sequential(ConvModule(96, 192, 3, 2, 1), CSPLayer(192, 192, 4, True))
        s.stage3 = nn.Sequential(ConvModule(192, 384, 3, 2, 1), CSPLayer(384, 384, 4, True))
        s.stage4 = nn.Sequential(ConvModule(384, 768, 3, 2, 1), SPPF(768, 768), CSPLayer(768, 768, 2, False))
    def forward(s, x): return s.stage4(s.stage3(s.stage2(s.stage1(s.stem(x)))))

class ScaleNorm(nn.Module):
    def __init__(s, d, eps=1e-5):
        super().__init__(); s.scale = d ** -0.5; s.eps = eps; s.g = nn.Parameter(torch.ones(1))
    def forward(s, x): return x / (torch.norm(x, dim=-1, keepdim=True) * s.scale).clamp(min=s.eps) * s.g

class Scale(nn.Module):
    def __init__(s, d):
        super().__init__(); s.scale = nn.Parameter(torch.ones(d))
    def forward(s, x): return x * s.scale

class RTMCCBlock(nn.Module):
    def __init__(s, d=256, e=512, sdim=128):
        super().__init__(); s.e, s.s = e, sdim
        s.o = nn.Linear(e, d, bias=False); s.uv = nn.Linear(d, 2 * e + sdim, bias=False)
        s.gamma = nn.Parameter(torch.rand(2, sdim)); s.beta = nn.Parameter(torch.rand(2, sdim))
        s.ln = ScaleNorm(d); s.res_scale = Scale(d)
    def forward(s, x):
        r = x; x = s.ln(x); uv = F.silu(s.uv(x))
        u, v, base = torch.split(uv, [s.e, s.e, s.s], dim=2)
        base = base.unsqueeze(2) * s.gamma[None, None, :] + s.beta
        q, k = torch.unbind(base, dim=2)
        kernel = torch.square(F.relu(torch.bmm(q, k.permute(0, 2, 1)) / math.sqrt(s.s)))
        return s.res_scale(r) + s.o(u * torch.bmm(kernel, v))

class Head(nn.Module):
    def __init__(s, k=17):
        super().__init__()
        s.final_layer = nn.Conv2d(768, k, 7, padding=3)
        s.mlp = nn.Sequential(ScaleNorm(64), nn.Linear(64, 256, bias=False))
        s.gau = RTMCCBlock(); s.cls_x = nn.Linear(256, 512, bias=False); s.cls_y = nn.Linear(256, 512, bias=False)
    def forward(s, f):
        f = s.gau(s.mlp(torch.flatten(s.final_layer(f), 2))); return s.cls_x(f), s.cls_y(f)

class RTMPose(nn.Module):
    """Input: [1, 3, 256, 256] RGB in 0–255. Normalization (ImageNet mean/std) is part of the model."""
    def __init__(s):
        super().__init__(); s.backbone = CSPNeXt(); s.head = Head()
        s.register_buffer("mean", torch.tensor([123.675, 116.28, 103.53]).view(1, 3, 1, 1)); s.register_buffer("std", torch.tensor([58.395, 57.12, 57.375]).view(1, 3, 1, 1))
    def forward(s, x): return s.head(s.backbone((x - s.mean) / s.std))

def load(path):
    m = RTMPose().eval()
    st = torch.load(path, map_location="cpu", weights_only=False)["state_dict"]
    st = {k: v for k, v in st.items() if not k.endswith("num_batches_tracked")}
    missing, unexpected = m.load_state_dict(st, strict=False)
    assert unexpected == [] and set(missing) <= {"mean", "std"}, (missing, unexpected)
    return m
