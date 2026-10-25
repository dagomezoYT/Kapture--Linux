"""Recording library: folder, thumbs and disk stats."""

from __future__ import annotations

import hashlib
import shutil
import subprocess
from pathlib import Path

from settings import load as load_settings

THUMBS = Path.home() / ".cache" / "gamecap" / "thumbs"
VIDEO_EXT = {".mkv", ".mp4", ".webm"}
IMAGE_EXT = {".png", ".jpg", ".jpeg"}


def output_dir() -> Path:
    path = Path(load_settings().get("output_dir") or (Path.home() / "Videos" / "GameCap"))
    path.mkdir(parents=True, exist_ok=True)
    return path


def list_recordings(folder: Path | None = None, newest_first: bool = True) -> list[Path]:
    root = folder or output_dir()
    files = [
        p
        for p in root.iterdir()
        if p.is_file() and p.suffix.lower() in VIDEO_EXT | IMAGE_EXT and not p.name.startswith(".")
    ]
    files.sort(key=lambda p: p.stat().st_mtime, reverse=newest_first)
    return files


def total_bytes(files: list[Path]) -> int:
    total = 0
    for path in files:
        try:
            total += path.stat().st_size
        except OSError:
            continue
    return total


def fmt_bytes(n: int) -> str:
    if n >= 1_073_741_824:
        return f"{n / 1_073_741_824:.2f} GB"
    if n >= 1_048_576:
        return f"{n / 1_048_576:.0f} MB"
    return f"{n / 1024:.0f} KB"


def free_disk(folder: Path | None = None) -> tuple[str, float]:
    try:
        usage = shutil.disk_usage(folder or output_dir())
    except OSError:
        return "—", 0.0
    pct = usage.free / usage.total if usage.total else 0
    return f"{usage.free / 1_073_741_824:.1f} GB", pct


def thumb_path(video: Path) -> Path:
    digest = hashlib.sha1(str(video.resolve()).encode("utf-8")).hexdigest()[:16]
    return THUMBS / f"{digest}.jpg"


def ensure_thumb(video: Path) -> Path | None:
    if video.suffix.lower() in IMAGE_EXT:
        return video
    dest = thumb_path(video)
    try:
        if dest.is_file() and dest.stat().st_mtime >= video.stat().st_mtime:
            return dest
    except OSError:
        return None
    THUMBS.mkdir(parents=True, exist_ok=True)
    try:
        out = subprocess.run(
            [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                "1",
                "-i",
                str(video),
                "-frames:v",
                "1",
                "-vf",
                "scale=200:112:force_original_aspect_ratio=decrease,pad=200:112:(ow-iw)/2:(oh-ih)/2",
                "-q:v",
                "6",
                "-y",
                str(dest),
            ],
            check=False,
            timeout=20,
            capture_output=True,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return dest if out.returncode == 0 and dest.is_file() else None
