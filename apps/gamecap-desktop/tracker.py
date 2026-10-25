"""Keep game capture locked to the game: pause on Super/overview and meter voice."""

from __future__ import annotations

import json
import math
import os
import struct
import subprocess
import threading
import time
from pathlib import Path

import games

CACHE = Path.home() / ".cache" / "gamecap"
LEVELS = CACHE / "levels.json"
IPC = Path(os.environ.get("XDG_RUNTIME_DIR") or "/tmp") / "gamecap-gsr.sock"


def set_paused(paused: bool) -> None:
    if not IPC.exists():
        return
    try:
        subprocess.run(
            ["gsr-cli", "-ipc", str(IPC), "set-paused", "true" if paused else "false"],
            check=False,
            capture_output=True,
            timeout=1,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def _write_level(peak: float) -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    try:
        LEVELS.write_text(json.dumps({"voice": max(0.0, min(1.0, peak))}) + "\n", encoding="utf-8")
    except OSError:
        pass


def _rms(chunk: bytes) -> float:
    n = len(chunk) // 2
    if n <= 0:
        return 0.0
    samples = struct.unpack("<" + "h" * n, chunk[: n * 2])
    acc = sum(s * s for s in samples) / n
    return math.sqrt(acc) / 32768.0


def voice_loop(stop: threading.Event) -> None:
    """Peak-meter the mic only (never a speaker monitor)."""
    device = ""
    try:
        out = subprocess.run(
            ["pactl", "get-default-source"],
            capture_output=True,
            text=True,
            timeout=2,
        )
        device = (out.stdout or "").strip()
    except (OSError, subprocess.TimeoutExpired):
        device = ""
    if "monitor" in device.lower():
        device = ""
    cmd = [
        "parec",
        "--raw",
        "--rate=16000",
        "--channels=1",
        "--format=s16le",
        "--latency-msec=50",
    ]
    if device:
        cmd.extend(["-d", device])
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        return
    assert proc.stdout is not None
    peak = 0.0
    try:
        while not stop.is_set():
            chunk = proc.stdout.read(1600)
            if not chunk:
                break
            rms = _rms(chunk)
            peak = min(1.0, peak * 0.55 + rms * 3.2)
            _write_level(peak)
    finally:
        _write_level(0.0)
        try:
            proc.terminate()
        except OSError:
            pass


def lock_loop(game_key: str, stop: threading.Event) -> None:
    """Pause only on GNOME overview / game exit — no AT-SPI window probes (those stutter)."""
    paused = False
    while not stop.is_set():
        # Cheap checks only: AT-SPI window_probe was burning ~40%+ CPU and hitching the game.
        covered = games.overview_active() or not games.is_running(game_key)
        if covered != paused:
            set_paused(covered)
            paused = covered
        time.sleep(1.5)
    if paused:
        set_paused(False)


def start(game_key: str) -> threading.Event:
    stop = threading.Event()
    if game_key:
        threading.Thread(target=lock_loop, args=(game_key, stop), daemon=True).start()
    return stop


def main() -> None:
    import argparse
    import signal

    parser = argparse.ArgumentParser()
    parser.add_argument("--game", default="")
    args = parser.parse_args()
    stop = start(args.game)

    def die(*_args) -> None:
        stop.set()

    signal.signal(signal.SIGTERM, die)
    signal.signal(signal.SIGINT, die)
    while not stop.is_set():
        time.sleep(0.15)


if __name__ == "__main__":
    main()
