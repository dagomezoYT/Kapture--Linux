"""Single source for encode choices and the limits that keep recording stable."""

from __future__ import annotations

# GSR names. ultra/very_high stay out: on this AMD they inflate bitrate and hitch pans.
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


def normalize_quality(value: object) -> str:
    quality = str(value or "high")
    if quality in BLOCKED_QUALITIES:
        return "high"
    allowed = {key for key, _label in QUALITIES}
    return quality if quality in allowed else "high"


def clamp_keyint(value: object) -> float:
    try:
        keyint = float(value)
    except (TypeError, ValueError):
        keyint = KEYINT_DEFAULT
    return min(KEYINT_MAX, max(KEYINT_MIN, keyint))


def capture_fps(value: object, stable: bool = True) -> int:
    try:
        fps = int(value or 0)
    except (TypeError, ValueError):
        fps = STABLE_FPS_CAP
    if fps <= 0:
        fps = STABLE_FPS_CAP
    if stable:
        fps = min(fps, STABLE_FPS_CAP)
    return fps


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
