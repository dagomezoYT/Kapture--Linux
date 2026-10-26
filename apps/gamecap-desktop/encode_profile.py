"""Single source for encode choices and the limits that keep recording stable.

The safe budget is what an integrated AMD or Intel GPU records with.
A discrete AMD/Intel card, or NVIDIA when it owns the screen, may use Ultra.
Anything else records H.264 on the CPU. Callers that omit a budget and pass
``high`` or ``medium`` stay on the historical result without probing hardware.
"""

from __future__ import annotations

import json
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path

# GSR names. ultra/very_high stay out of the VA-API budget: they inflate bitrate and hitch pans.
QUALITIES = (
    ("high", "Alta (recomendada · estable)"),
    ("medium", "Media (más ligero)"),
)
BLOCKED_QUALITIES = frozenset({"ultra", "very_high"})

SCALES = (
    ("0x0", "Nativo (monitor)"),
    ("1920x1080", "1920×1080"),
    ("1280x720", "1280×720"),
)
NATIVE_SCALES = frozenset({"", "0x0", "native", "original"})

FPS = (
    (60, "60 (recomendado)"),
    (120, "120"),
    (180, "180"),
    (240, "240"),
    (0, "Auto (monitor · pesado)"),
)
STABLE_FPS_CAP = 60

CODECS = (
    ("av1", "AV1 (GPU)"),
    ("hevc", "HEVC"),
    ("h264", "H264"),
    ("hevc_hdr", "HEVC HDR"),
    ("av1_hdr", "AV1 HDR"),
)
CONTAINERS = (
    ("mkv", "MKV"),
    ("mp4", "MP4"),
)

# 1.0s spiked the encoder during camera turns; 2.0s let prediction smear.
KEYINT_DEFAULT = 1.5
KEYINT_MIN = 1.0
KEYINT_MAX = 2.0

# Used only when the monitor probe fails.
FALLBACK_MONITOR = (1920, 1200)

# Opus below 160 kbps turns game transients and a USB mic into hiss and crunch
# after the editor decode. 192 is the default the WAV export is built around.
AUDIO_BITRATES = (
    (160, "Opus 160"),
    (192, "Opus 192 (recomendado)"),
    (256, "Opus 256"),
)
AUDIO_BITRATE_DEFAULT = 192

_CODEC_KEYS = {key for key, _label in CODECS}
_CACHE_TTL_S = 12 * 60 * 60
_mem_codecs: tuple[float, frozenset[str]] | None = None
_warm_lock = threading.Lock()
_warm_started = False


@dataclass(frozen=True)
class EncodeBudget:
    """Limits applied on top of the user's saved settings."""

    name: str
    qualities: tuple
    blocked: frozenset[str]
    stable_fps_cap: int
    encoder: str | None = None


# Historical profile. AMD and Intel stay here so a machine that already
# records cleanly does not pick up a heavier preset.
LEGACY = EncodeBudget(
    name="vaapi-safe",
    qualities=QUALITIES,
    blocked=BLOCKED_QUALITIES,
    stable_fps_cap=STABLE_FPS_CAP,
)
_HIGH_PRESETS = (
    ("high", "Alta (recomendada · estable)"),
    ("very_high", "Muy alta"),
    ("ultra", "Ultra"),
    ("medium", "Media (más ligero)"),
)
DISCRETE_NVIDIA = EncodeBudget(
    name="nvenc",
    qualities=_HIGH_PRESETS,
    blocked=frozenset(),
    stable_fps_cap=240,
)
# A discrete AMD/Intel part that owns the screen, stronger than the
# integrated 780M this profile was tuned on.
DISCRETE_VAAPI = EncodeBudget(
    name="vaapi-discrete",
    qualities=_HIGH_PRESETS,
    blocked=frozenset(),
    stable_fps_cap=120,
)
SOFTWARE = EncodeBudget(
    name="software",
    qualities=(
        ("medium", "Media (CPU)"),
        ("high", "Alta (CPU)"),
    ),
    blocked=BLOCKED_QUALITIES,
    stable_fps_cap=30,
    encoder="cpu",
)


@dataclass(frozen=True)
class EncodePlan:
    fps: int
    codec: str
    quality: str
    scale: str | None
    encoder: str | None


def budget_for(vendor: str, integrated: bool = True) -> EncodeBudget:
    """Budget for the GPU that owns the connected display.

    Integrated AMD/Intel (a laptop panel) stays on the safe preset. A discrete
    AMD/Intel card, or any NVIDIA that owns the screen, may use Ultra.
    """
    if vendor == "nvidia":
        return DISCRETE_NVIDIA
    if vendor in {"amd", "intel"} and not integrated:
        return DISCRETE_VAAPI
    if vendor in {"amd", "intel", "", "unknown"}:
        return LEGACY
    return SOFTWARE


def allows_ultra(budget: EncodeBudget) -> bool:
    return any(key == "ultra" for key, _label in budget.qualities) and "ultra" not in budget.blocked


def active_budget() -> EncodeBudget:
    try:
        import gpu as gpu_mod

        chosen = gpu_mod.choose_record_gpu(gpu_mod.discover())
    except OSError:
        return LEGACY
    if chosen is None:
        return LEGACY
    return budget_for(chosen.vendor, chosen.integrated)


def labels(rows: tuple) -> list[str]:
    return [label for _value, label in rows]


def index_of(rows: tuple, value: object, aliases: dict | None = None) -> int:
    wanted = aliases.get(value, value) if aliases else value
    for index, (key, _label) in enumerate(rows):
        if key == wanted or str(key) == str(wanted):
            return index
    return 0


def value_at(rows: tuple, index: int):
    if not rows:
        raise ValueError("empty choice list")
    safe = min(max(int(index), 0), len(rows) - 1)
    return rows[safe][0]


def normalize_quality(value: object, budget: EncodeBudget | None = None) -> str:
    quality = str(value or "high")
    if budget is None and quality in {"high", "medium"}:
        return quality
    chosen = budget if budget is not None else active_budget()
    if quality in chosen.blocked:
        return _fallback_quality(chosen)
    allowed = {key for key, _label in chosen.qualities}
    if quality in allowed:
        return quality
    return _fallback_quality(chosen)


def _fallback_quality(budget: EncodeBudget) -> str:
    allowed = {key for key, _label in budget.qualities}
    if "high" in allowed:
        return "high"
    return budget.qualities[0][0]


def clamp_keyint(value: object) -> float:
    try:
        keyint = float(value)
    except (TypeError, ValueError):
        keyint = KEYINT_DEFAULT
    return min(KEYINT_MAX, max(KEYINT_MIN, keyint))


def capture_fps(value: object, stable: bool = True, cap: int | None = None) -> int:
    limit = STABLE_FPS_CAP if cap is None else max(int(cap), 1)
    try:
        fps = int(value or 0)
    except (TypeError, ValueError):
        fps = STABLE_FPS_CAP
    if fps <= 0:
        fps = STABLE_FPS_CAP
    if stable:
        fps = min(fps, limit)
    return fps


def _codec_family(codec: str) -> str:
    if codec.startswith("av1"):
        return "av1"
    if codec.startswith("hevc"):
        return "hevc"
    return "h264"


def resolve_codec(requested: object, supported: frozenset[str] | None) -> str:
    """Keep the requested codec when the probe is missing or confirms it.

    A confirmed list that lacks the request falls through AV1, HEVC, then H.264.
    An empty or failed probe does not rewrite the request.
    """
    codec = str(requested or "av1")
    if codec not in _CODEC_KEYS:
        codec = "av1"
    if not supported:
        return codec
    if codec in supported:
        return codec
    family = _codec_family(codec)
    for cand in ("av1", "av1_hdr", "hevc", "hevc_hdr", "h264"):
        if cand in supported and cand in _CODEC_KEYS and _codec_family(cand) == family:
            return cand
    for cand in ("av1", "hevc", "h264"):
        if cand in supported and cand in _CODEC_KEYS:
            return cand
    return codec


def plan_encode(
    codec: object,
    quality: object,
    fps: object,
    stable: bool,
    scale: object,
    budget: EncodeBudget,
    supported: frozenset[str] | None,
) -> EncodePlan:
    chosen = resolve_codec(codec, supported)
    encoder = budget.encoder
    if encoder == "cpu":
        chosen = "h264"
    scale_s = str(scale or "0x0").strip()
    if encoder == "cpu" and is_native_scale(scale_s):
        scale_s = "1280x720"
    return EncodePlan(
        fps=capture_fps(fps, stable, budget.stable_fps_cap),
        codec=chosen,
        quality=normalize_quality(quality, budget),
        scale=None if is_native_scale(scale_s) else scale_s,
        encoder=encoder,
    )


def parse_gsr_info(text: str) -> tuple[str, frozenset[str]]:
    """Parse `gpu-screen-recorder --info` into (vendor, codec set)."""
    section = ""
    vendor = ""
    codecs: list[str] = []
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("section="):
            section = line.split("=", 1)[1]
            continue
        if section == "gpu_info" and line.startswith("vendor|"):
            vendor = line.split("|", 1)[1].strip().lower()
        elif section == "video_codecs" and line:
            codecs.append(line)
    return vendor, frozenset(codecs)


def _codec_cache_path() -> Path:
    return Path.home() / ".cache" / "gamecap" / "gsr-codecs.json"


def cached_codecs() -> frozenset[str] | None:
    """Codecs GSR reported for the display GPU. None means 'do not rewrite'."""
    global _mem_codecs
    now = time.monotonic()
    if _mem_codecs is not None and now - _mem_codecs[0] < _CACHE_TTL_S:
        return _mem_codecs[1]
    path = _codec_cache_path()
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        age = time.time() - path.stat().st_mtime
    except (OSError, json.JSONDecodeError, TypeError, ValueError):
        return None
    if age < 0 or age > _CACHE_TTL_S:
        return None
    raw = payload.get("codecs") if isinstance(payload, dict) else None
    if not isinstance(raw, list) or not raw:
        return None
    found = frozenset(str(item) for item in raw if str(item))
    if not found:
        return None
    _mem_codecs = (now, found)
    return found


def warm_codec_cache() -> None:
    """Refresh GSR's codec list off the UI thread. A miss leaves the request unchanged."""
    global _warm_started
    if cached_codecs() is not None:
        return
    with _warm_lock:
        if _warm_started:
            return
        _warm_started = True
    threading.Thread(target=_refresh_gsr_codecs, name="gsr-codecs", daemon=True).start()


def _refresh_gsr_codecs() -> None:
    global _mem_codecs, _warm_started
    try:
        out = subprocess.run(
            ["gpu-screen-recorder", "--info"],
            check=False,
            capture_output=True,
            text=True,
            timeout=8,
        )
    except (OSError, subprocess.TimeoutExpired):
        with _warm_lock:
            _warm_started = False
        return
    _vendor, codecs = parse_gsr_info(out.stdout or "")
    if not codecs:
        with _warm_lock:
            _warm_started = False
        return
    path = _codec_cache_path()
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps({"codecs": sorted(codecs)}) + "\n", encoding="utf-8")
        tmp.replace(path)
    except OSError:
        with _warm_lock:
            _warm_started = False
        return
    _mem_codecs = (time.monotonic(), codecs)


def is_native_scale(value: object) -> bool:
    return str(value or "0x0").strip() in NATIVE_SCALES


def normalize_audio_bitrate(value: object) -> int:
    """Snap a stored bitrate onto the rates that stay clean in an editor."""
    try:
        rate = int(value or 0)
    except (TypeError, ValueError):
        rate = 0
    if rate < 160:
        return AUDIO_BITRATE_DEFAULT
    allowed = [key for key, _label in AUDIO_BITRATES]
    if rate in allowed:
        return rate
    higher = [key for key in allowed if key >= rate]
    return higher[0] if higher else allowed[-1]


def dropped_frames(deltas_s: list[float], fps: int) -> int:
    """Count frames skipped between packets. A gap of 2.1 steps is one drop."""
    if fps <= 0:
        return 0
    step = 1.0 / fps
    dropped = 0
    for delta in deltas_s:
        if delta > step * 1.75:
            missed = int(round(delta / step)) - 1
            if missed > 0:
                dropped += missed
    return dropped


def should_retime(frame_count: int, fps: int, audio_span: float | None, video_span: float | None) -> bool:
    """True when stamping frames onto 1/fps still matches the soundtrack.

    gpu-screen-recorder often labels 60 fps pictures as 16 ms (62.5 fps).
    Renumbering those frames fixes the editor. If frames were actually
    dropped, the same renumbering runs the picture ahead of the sound.
    """
    if fps <= 0 or frame_count < 2:
        return False
    retimed = frame_count / fps
    if audio_span and audio_span > 0:
        return abs(retimed - audio_span) / audio_span <= 0.03
    if video_span and video_span > 0:
        measured = (frame_count - 1) / video_span
        return abs(measured - fps) / fps <= 0.08
    return False
