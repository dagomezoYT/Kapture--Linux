#!/usr/bin/env python3
"""Small no-focus HUD: time, FPS, disk and thin VU bars for voice and game audio."""

from __future__ import annotations

import argparse
import json
import math
import struct
import subprocess
import sys
import threading
from pathlib import Path

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk

sys.path.insert(0, str(Path(__file__).resolve().parent))

HUD_W = 520
HUD_H = 32
MARGIN = 4
# Must stay in sync with session.HUD_CAPTURE_CUT (excluded from the video).
CAPTURE_CUT = 96
CACHE = Path.home() / ".cache" / "gamecap"
LEVELS = CACHE / "levels.json"
_LEVELS_LOCK = threading.Lock()
_LEVELS = {"voice": 0.0, "game": 0.0}


def capture_top_cut() -> int:
    """Pixels from the top of the screen that must stay out of the recording."""
    return CAPTURE_CUT


class LevelBar(Gtk.DrawingArea):
    def __init__(self, rgb: tuple[float, float, float], hot: tuple[float, float, float]) -> None:
        super().__init__()
        self.level = 0.0
        self.rgb = rgb
        self.hot = hot
        self.set_size_request(96, 5)
        self.set_hexpand(True)
        self.set_valign(Gtk.Align.CENTER)
        self.connect("draw", self._draw)

    def set_level(self, value: float) -> None:
        self.level = max(0.0, min(1.0, value))
        self.queue_draw()

    def _draw(self, _widget, cr) -> bool:
        alloc = self.get_allocation()
        w = max(alloc.width, 1)
        h = max(alloc.height, 1)
        cr.set_source_rgb(0.10, 0.12, 0.15)
        cr.rectangle(0, 0, w, h)
        cr.fill()
        fill = max(w * self.level, 2.0 if self.level > 0.02 else 0.0)
        if fill > 0:
            color = self.rgb if self.level < 0.7 else self.hot
            cr.set_source_rgb(*color)
            cr.rectangle(0, 0, fill, h)
            cr.fill()
        return False


class Hud(Gtk.Window):
    def __init__(self, status_file: Path, stop_file: Path, mic: str, game_audio: str) -> None:
        super().__init__()
        self.status_file = status_file
        self.stop_file = stop_file
        self.mic = mic
        self.game_audio = game_audio
        self._stop_meter = threading.Event()
        self.set_title("GameCap HUD")
        self.set_decorated(False)
        self.set_resizable(False)
        self.set_keep_above(True)
        self.set_accept_focus(False)
        self.set_focus_on_map(False)
        self.set_skip_taskbar_hint(True)
        self.set_skip_pager_hint(True)
        self.set_type_hint(Gdk.WindowTypeHint.DOCK)
        self.set_size_request(HUD_W, HUD_H)
        self.set_default_size(HUD_W, HUD_H)
        self.add_events(Gdk.EventMask.BUTTON_PRESS_MASK)
        self.connect("button-press-event", self.on_click)
        self.connect("realize", self.place)
        self.connect("map-event", self.place)
        self.connect("destroy", self._shutdown)

        css = Gtk.CssProvider()
        css.load_from_data(
            b"""
            .hud {
                background: rgba(8, 8, 10, 0.88);
                border-radius: 8px;
                padding: 4px 10px;
            }
            .dot { color: #ff4d4d; font-weight: bold; font-size: 13px; }
            .txt { color: #f4f4f5; font: 12px monospace; }
            .vu-label { color: #9aa3b2; font-size: 9px; font-weight: bold; }
            """
        )
        Gtk.StyleContext.add_provider_for_screen(
            Gdk.Screen.get_default(),
            css,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )

        root = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        root.get_style_context().add_class("hud")
        self.dot = Gtk.Label(label="●")
        self.dot.get_style_context().add_class("dot")
        self.txt = Gtk.Label(label="REC  00:00   — FPS")
        self.txt.get_style_context().add_class("txt")

        vu_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        vu_col.set_valign(Gtk.Align.CENTER)
        voice_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        vlab = Gtk.Label(label="VOZ")
        vlab.get_style_context().add_class("vu-label")
        vlab.set_width_chars(5)
        vlab.set_xalign(0.0)
        self.voice = LevelBar((0.13, 0.84, 0.38), (0.25, 0.95, 0.45))
        self.voice.set_size_request(72, 5)
        voice_row.pack_start(vlab, False, False, 0)
        voice_row.pack_start(self.voice, True, True, 0)
        game_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        glab = Gtk.Label(label="JUEGO")
        glab.get_style_context().add_class("vu-label")
        glab.set_width_chars(5)
        glab.set_xalign(0.0)
        self.game = LevelBar((0.34, 0.72, 0.98), (0.56, 0.85, 1.0))
        self.game.set_size_request(72, 5)
        game_row.pack_start(glab, False, False, 0)
        game_row.pack_start(self.game, True, True, 0)
        vu_col.pack_start(voice_row, False, False, 0)
        vu_col.pack_start(game_row, False, False, 0)

        root.pack_start(self.dot, False, False, 0)
        root.pack_start(self.txt, False, False, 0)
        root.pack_start(vu_col, True, True, 0)
        self.add(root)
        threading.Thread(target=self._meter_loop, args=("voice", self.mic, self.voice), daemon=True).start()
        threading.Thread(
            target=self._meter_loop, args=("game", self.game_audio, self.game), daemon=True
        ).start()
        GLib.timeout_add(1000, self.refresh)

    def place(self, *_args) -> bool:
        screen = self.get_screen()
        w = screen.get_width()
        y = max(0, min(MARGIN, CAPTURE_CUT - HUD_H - 2))
        self.move(max(w - HUD_W - MARGIN, 0), y)
        gdk_win = self.get_window()
        if gdk_win is not None:
            try:
                gdk_win.set_override_redirect(True)
            except Exception:
                pass
        return False

    def on_click(self, *_args) -> bool:
        try:
            self.stop_file.write_text("stop\n", encoding="utf-8")
        except OSError:
            pass
        self._shutdown()
        Gtk.main_quit()
        return True

    def _shutdown(self, *_args) -> None:
        self._stop_meter.set()
        _write_level("voice", 0.0)
        _write_level("game", 0.0)

    def _meter_loop(self, kind: str, device: str, bar: LevelBar) -> None:
        import time

        cmd = [
            "parec",
            "--raw",
            "--rate=8000",
            "--channels=1",
            "--format=s16le",
            "--latency-msec=100",
        ]
        if device:
            cmd.extend(["-d", device])
        elif kind == "game":
            return
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
        last_ui = 0.0
        gain = 8.0 if kind == "game" else 16.0
        try:
            while not self._stop_meter.is_set():
                chunk = proc.stdout.read(1600)
                if not chunk:
                    break
                rms = _rms(chunk)
                spoken = min(1.0, rms * gain)
                peak = spoken if spoken > peak else peak * 0.78
                now = time.monotonic()
                if now - last_ui >= 0.12:
                    last_ui = now
                    GLib.idle_add(bar.set_level, peak)
                    _write_level(kind, peak)
        finally:
            _write_level(kind, 0.0)
            try:
                proc.terminate()
            except OSError:
                pass

    def refresh(self) -> bool:
        if not self.status_file.is_file():
            return True
        try:
            data = json.loads(self.status_file.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return True
        if data.get("recording") is False and float(data.get("elapsed_s") or 0) > 0.5:
            self._shutdown()
            Gtk.main_quit()
            return False
        elapsed = int(float(data.get("elapsed_s") or 0))
        fps = float(data.get("fps") or 0)
        hh, rem = divmod(elapsed, 3600)
        mm, ss = divmod(rem, 60)
        clock = f"{hh}:{mm:02d}:{ss:02d}" if hh else f"{mm:02d}:{ss:02d}"
        fps_s = f"{fps:.0f} FPS" if fps > 0 else "— FPS"
        free_gb = float(data.get("free_gb") or 0)
        disk_s = f"{free_gb:.0f} GB" if free_gb > 0 else "— GB"
        paused = bool(data.get("paused"))
        try:
            flag = CACHE / "paused"
            if flag.is_file():
                paused = flag.read_text(encoding="utf-8").strip() in {"1", "true", "True"}
        except OSError:
            pass
        pause_s = "  PAUSA" if paused else ""
        self.txt.set_text(f"REC  {clock}   {fps_s}   {disk_s}{pause_s}")
        self.place()
        return True


def _write_level(kind: str, peak: float) -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    value = max(0.0, min(1.0, peak))
    with _LEVELS_LOCK:
        _LEVELS[kind] = value
        payload = json.dumps(dict(_LEVELS)) + "\n"
    try:
        LEVELS.write_text(payload, encoding="utf-8")
    except OSError:
        pass


def _rms(chunk: bytes) -> float:
    n = len(chunk) // 2
    if n <= 0:
        return 0.0
    samples = struct.unpack("<" + "h" * n, chunk[: n * 2])
    acc = sum(s * s for s in samples) / n
    return math.sqrt(acc) / 32768.0


def _mic_from_settings(cli: str) -> str:
    if cli.strip() and "monitor" not in cli.lower():
        return cli.removeprefix("device:").strip()
    try:
        from settings import load

        ident = str(load().get("audio_mic_device") or "").removeprefix("device:").strip()
        if ident in {"", "default_input"}:
            ident = ""
        if "monitor" in ident.lower():
            ident = ""
        if ident:
            return ident
    except (ImportError, OSError, ValueError, KeyError, TypeError):
        pass
    try:
        out = subprocess.run(
            ["pactl", "get-default-source"],
            capture_output=True,
            text=True,
            timeout=2,
        )
        src = (out.stdout or "").strip()
        if src and "monitor" not in src.lower():
            return src
    except (OSError, subprocess.TimeoutExpired):
        pass
    return ""


def _game_from_settings(cli: str) -> str:
    ident = (cli or "").removeprefix("device:").strip()
    if ident and ident not in {"default_output", "none", "@DEFAULT_SINK@"}:
        if not ident.endswith(".monitor") and "monitor" not in ident.lower():
            ident = f"{ident}.monitor"
        return ident
    try:
        from audio import playback_monitor

        monitor = playback_monitor()
        if monitor and not monitor.startswith("@"):
            return monitor
    except (ImportError, OSError, ValueError, TypeError):
        pass
    try:
        out = subprocess.run(
            ["pactl", "get-default-sink"],
            capture_output=True,
            text=True,
            timeout=2,
        )
        sink = (out.stdout or "").strip()
        if sink and not sink.startswith("@"):
            return f"{sink}.monitor"
    except (OSError, subprocess.TimeoutExpired):
        pass
    return ""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--status-file", required=True)
    parser.add_argument("--stop-file", required=True)
    parser.add_argument("--mic", default="")
    parser.add_argument("--game-audio", default="")
    args = parser.parse_args()
    Gtk.init(None)
    hud = Hud(
        Path(args.status_file),
        Path(args.stop_file),
        _mic_from_settings(args.mic),
        _game_from_settings(args.game_audio),
    )
    hud.show_all()
    Gtk.main()
    return 0


if __name__ == "__main__":
    sys.exit(main())
