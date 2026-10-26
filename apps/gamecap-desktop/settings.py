"""Persisted GameCap desktop settings."""

from __future__ import annotations

import json
from pathlib import Path

import encode_profile as enc

CONFIG_DIR = Path.home() / ".config" / "gamecap"
SETTINGS_PATH = CONFIG_DIR / "settings.json"
SCHEMA = 9

# Older files used these one-shot flags instead of `schema`.
_LEGACY_FLAGS = (
    (8, "motion_balance_v8"),
    (7, "hud_restored_v7"),
    (6, "size_balance_v6"),
    (5, "size_balance_v5"),
    (4, "size_balance_v4"),
    (3, "size_balance_v3"),
    (2, "size_balance_v2"),
    (1, "size_balance_v1"),
)

DEFAULTS = {
    "schema": SCHEMA,
    "hotkey": "F9",
    "hotkey_binding": "F9",
    "hotkey_screenshot": "",
    "hotkey_screenshot_binding": "",
    "hotkey_replay": "",
    "hotkey_replay_binding": "",
    "hotkey_pause": "F8",
    "hotkey_pause_binding": "F8",
    "mode": "monitor",
    "capture_mode": "monitor",
    "game_key": "minecraft",
    "game_title": "Minecraft",
    "region": None,
    "monitor_name": "",
    "v4l2_device": "/dev/video0",
    "hud": True,
    "minimize_on_record": True,
    "wait_for_game": False,
    "limit_hours": 0,
    "replay_secs": 0,
    "output_dir": str(Path.home() / "Videos" / "GameCap"),
    "container": "mkv",
    "codec": "av1",
    "quality": "high",
    "scale": "1920x1080",
    "cursor": False,
    "fps": 60,
    "frame_mode": "vfr",
    "encode_tune": "performance",
    "encode_keyint": enc.KEYINT_DEFAULT,
    "capture_stable_fps": True,
    "audio_system": True,
    "audio_mic": True,
    "audio_system_device": "default_output",
    "audio_mic_device": "default_input",
    "audio_system_vol": 100,
    "audio_mic_vol": 100,
    "audio_bitrate": enc.AUDIO_BITRATE_DEFAULT,
    "audio_tracks": "both",
    "save_audio_folder": True,
    "audio_denoise_ambient": True,
    "audio_denoise_static": True,
    "audio_denoise_dirty": True,
    "audio_denoise_system": False,
    "audio_denoise_mic": True,
    "audio_denoise_strength": "media",
    "screenshot_format": "png",
    "language": "es",
}


def load() -> dict:
    raw = _read()
    if raw is None:
        return dict(DEFAULTS)
    out = dict(DEFAULTS)
    out.update({key: raw[key] for key in DEFAULTS if key in raw})
    dirty = _upgrade(out, raw)
    if _sanitize(out):
        dirty = True
    if dirty:
        try:
            save(out)
        except OSError:
            pass
    return out


def save(data: dict) -> None:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    merged = dict(DEFAULTS)
    merged.update(data)
    merged["schema"] = SCHEMA
    SETTINGS_PATH.write_text(json.dumps(merged, indent=2) + "\n", encoding="utf-8")


def _read() -> dict | None:
    if not SETTINGS_PATH.is_file():
        return None
    try:
        data = json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return data if isinstance(data, dict) else None


def _legacy_schema(raw: dict) -> int:
    for version, flag in _LEGACY_FLAGS:
        if raw.get(flag):
            return version
    return 0


def _upgrade(out: dict, raw: dict) -> bool:
    """Apply pending one-shot upgrades. Current defaults already match schema 9."""
    if "schema" in raw:
        try:
            current = int(raw["schema"])
        except (TypeError, ValueError):
            current = 0
    else:
        current = _legacy_schema(raw)
    if current < 3:
        out["quality"] = enc.normalize_quality(out.get("quality"))
        out["scale"] = "0x0"
        out["fps"] = int(out.get("fps") or enc.STABLE_FPS_CAP) or enc.STABLE_FPS_CAP
        out["frame_mode"] = "vfr"
        out["capture_stable_fps"] = True
        out["codec"] = out.get("codec") or "av1"
    if current < 4:
        out["frame_mode"] = "vfr"
    if current < 5:
        out["encode_tune"] = "performance"
        out["frame_mode"] = "vfr"
    if current < 6:
        out["scale"] = "1920x1080"
        out["hud"] = True
        out["cursor"] = False
        out["encode_tune"] = "performance"
    if current < 7:
        out["hud"] = True
    if current < 8:
        out["encode_keyint"] = enc.KEYINT_DEFAULT
        out["encode_tune"] = "performance"
        out["frame_mode"] = "vfr"
        out["quality"] = enc.normalize_quality(out.get("quality"))
    if current < 9:
        # 96 kbps Opus was the old default. It is what made the editor WAVs hiss and crunch.
        out["audio_bitrate"] = enc.normalize_audio_bitrate(out.get("audio_bitrate"))
    out["schema"] = SCHEMA
    legacy = any(raw.get(flag) for _version, flag in _LEGACY_FLAGS)
    return current != SCHEMA or legacy or "schema" not in raw


def _sanitize(out: dict) -> bool:
    dirty = False
    quality = enc.normalize_quality(out.get("quality"))
    if out.get("quality") != quality:
        out["quality"] = quality
        dirty = True
    keyint = enc.clamp_keyint(out.get("encode_keyint"))
    if out.get("encode_keyint") != keyint:
        out["encode_keyint"] = keyint
        dirty = True
    bitrate = enc.normalize_audio_bitrate(out.get("audio_bitrate"))
    if out.get("audio_bitrate") != bitrate:
        out["audio_bitrate"] = bitrate
        dirty = True
    return dirty
