#!/usr/bin/env python3
"""Save a screenshot into the GameCap videos folder."""

from __future__ import annotations

import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import settings as cfg


def output_dir() -> Path:
    path = Path(cfg.load().get("output_dir") or (Path.home() / "Videos" / "GameCap"))
    path.mkdir(parents=True, exist_ok=True)
    return path


def take_screenshot() -> Path | None:
    data = cfg.load()
    ext = "jpg" if data.get("screenshot_format") == "jpg" else "png"
    dest = output_dir() / f"gamecap-{datetime.now().strftime('%Y%m%d-%H%M%S')}.{ext}"
    tools = []
    if shutil.which("gnome-screenshot"):
        tools.append(["gnome-screenshot", "-f", str(dest)])
    if shutil.which("grim"):
        tools.append(["grim", str(dest)])
    if shutil.which("spectacle"):
        tools.append(["spectacle", "-b", "-n", "-o", str(dest)])
    for cmd in tools:
        try:
            out = subprocess.run(cmd, check=False, capture_output=True, timeout=20)
        except (OSError, subprocess.TimeoutExpired):
            continue
        if out.returncode == 0 and dest.is_file():
            return dest
    return None


def main() -> int:
    path = take_screenshot()
    return 0 if path is not None else 1


if __name__ == "__main__":
    sys.exit(main())
