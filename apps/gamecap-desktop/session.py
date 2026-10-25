"""Shared recording session (UI + global hotkey)."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import threading
import time
from datetime import datetime
from pathlib import Path

import encode_profile as enc
import gpu as gpu_mod
from settings import load as load_settings

ROOT = Path(__file__).resolve().parents[2]
HUD = Path(__file__).resolve().parent / "hud.py"
TRACKER = Path(__file__).resolve().parent / "tracker.py"
CACHE = Path.home() / ".cache" / "gamecap"
STATUS = CACHE / "status.json"
STOP = CACHE / "stop"
PID = CACHE / "record.pid"
HUD_PID = CACHE / "hud.pid"
TRACKER_PID = CACHE / "tracker.pid"
PAUSED = CACHE / "paused"
LAST_OUTPUT = CACHE / "last_output.txt"
LAST_AUDIO_LABELS = CACHE / "last_audio_labels.json"
POWER_PROFILE_RESTORE = CACHE / "power_profile_restore.txt"
NICE_RESTORE = CACHE / "nice_restore.json"
# Keep in sync with hud.py CAPTURE_CUT — strip excluded from the encoded video.
HUD_CAPTURE_CUT = 96
_MONITOR_CACHE: tuple[float, int, int, int] | None = None


def which_gamecap() -> Path:
    binary = ROOT / "bin" / "gamecap"
    if binary.is_file():
        return binary
    return Path("gamecap")


def _monitor_info() -> tuple[int, int, int]:
    """Return width, height and refresh (0 if the probe did not report Hz)."""
    global _MONITOR_CACHE
    now = time.monotonic()
    if _MONITOR_CACHE is not None and now - _MONITOR_CACHE[0] < 30:
        return _MONITOR_CACHE[1], _MONITOR_CACHE[2], _MONITOR_CACHE[3]
    width, height = enc.FALLBACK_MONITOR
    hz = 0
    try:
        out = subprocess.run(
            ["gpu-screen-recorder", "--list-monitors"],
            capture_output=True,
            text=True,
            timeout=3,
        )
        parsed = _parse_monitor_line(out.stdout or "")
        if parsed is not None:
            width, height, hz = parsed
    except (OSError, subprocess.TimeoutExpired):
        pass
    _MONITOR_CACHE = (now, width, height, hz)
    return width, height, hz


def _parse_monitor_line(text: str) -> tuple[int, int, int] | None:
    for line in text.splitlines():
        dim = line.split("|", 1)[-1].strip().split()[0]
        hz = 0
        if "@" in dim:
            dim, hz_s = dim.split("@", 1)
            try:
                hz = int(float(hz_s))
            except ValueError:
                hz = 0
        if "x" not in dim:
            continue
        try:
            w_s, h_s = dim.lower().split("x", 1)
            return max(int(w_s), 64), max(int(h_s), 64), hz
        except ValueError:
            continue
    return None


def _monitor_px() -> tuple[int, int]:
    width, height, _hz = _monitor_info()
    return width, height


def monitor_size_label() -> str:
    width, height = _monitor_px()
    return f"{width}×{height}"


def monitor_idle_label() -> str:
    width, height, hz = _monitor_info()
    if hz > 0:
        return f"{width}×{height}  ·  {hz} Hz"
    return f"{width}×{height}"


def _parse_region(spec: str) -> tuple[int, int, int, int] | None:
    raw = spec.strip().replace(" ", "")
    try:
        size, pos = raw.split("+", 1)
        width, height = size.lower().split("x", 1)
        x, y = pos.split("+", 1)
        return int(width), int(height), int(x), int(y)
    except ValueError:
        return None


def _region_without_hud(spec: str | None) -> str:
    """Crop the HUD strip out of the KMS rectangle so the overlay never enters the file."""
    cut = HUD_CAPTURE_CUT
    mon_w, mon_h = _monitor_px()
    if spec:
        parsed = _parse_region(spec)
        if parsed is not None:
            width, height, x, y = parsed
            if y < cut:
                gap = cut - y
                y += gap
                height -= gap
            width = max(width - (width % 2), 2)
            height = max(height - (height % 2), 64)
            return f"{width}x{height}+{max(x, 0)}+{max(y, 0)}"
    width = mon_w - (mon_w % 2)
    height = max(mon_h - cut, 64)
    height -= height % 2
    return f"{width}x{height}+0+{cut}"


def _pid_alive(path: Path) -> bool:
    if not path.is_file():
        return False
    try:
        pid = int(path.read_text(encoding="utf-8").strip())
    except ValueError:
        return False
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def is_recording() -> bool:
    return _pid_alive(PID)


def ui_is_open() -> bool:
    """True when the Kapture window process is running."""
    me = os.getpid()
    try:
        entries = list(Path("/proc").iterdir())
    except OSError:
        return False
    for entry in entries:
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid == me:
            continue
        try:
            cmd = (entry / "cmdline").read_bytes().replace(b"\0", b" ")
        except OSError:
            continue
        if b"cursorsandbox" in cmd:
            continue
        if b"gamecap-desktop/ui.py" in cmd:
            return True
    return False


def _boost_power_for_recording() -> None:
    """Prefer performance power profile while recording (restored on stop)."""
    try:
        got = subprocess.run(
            ["powerprofilesctl", "get"],
            check=False,
            capture_output=True,
            text=True,
            timeout=2,
        )
    except (OSError, subprocess.TimeoutExpired):
        return
    prev = (got.stdout or "").strip()
    if not prev or prev == "performance":
        return
    try:
        POWER_PROFILE_RESTORE.write_text(prev + "\n", encoding="utf-8")
        subprocess.run(
            ["powerprofilesctl", "set", "performance"],
            check=False,
            capture_output=True,
            timeout=2,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def _tame_ibus_helpers() -> None:
    """Drop hung/heavy IBus UI helpers; keep xkb layouts (us/latam) intact."""
    patterns = (
        "/usr/libexec/ibus-extension-gtk3",
        "/usr/libexec/ibus-ui-emojier",
        "/usr/share/ibus-typing-booster/engine/main.py",
    )
    me = os.getpid()
    for pat in patterns:
        try:
            out = subprocess.run(
                ["pgrep", "-f", "--", pat],
                capture_output=True,
                text=True,
                timeout=1,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        for line in (out.stdout or "").splitlines():
            try:
                pid = int(line.strip())
            except ValueError:
                continue
            if pid == me:
                continue
            try:
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass


def _lower_background_noise() -> None:
    """Renice known hitch sources while recording (restored on stop)."""
    _tame_ibus_helpers()
    patterns = (
        ("-f", "ibus-extension-gtk3"),
        ("-f", "ibus-typing-booster"),
        ("-x", "easyeffects"),
        ("-f", "feather_web_helper"),
    )
    saved: list[dict[str, int]] = []
    seen: set[int] = set()
    for flag, name in patterns:
        try:
            out = subprocess.run(
                ["pgrep", flag, name],
                capture_output=True,
                text=True,
                timeout=1,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        for line in (out.stdout or "").splitlines():
            try:
                pid = int(line.strip())
            except ValueError:
                continue
            if pid in seen:
                continue
            seen.add(pid)
            try:
                prev = os.getpriority(os.PRIO_PROCESS, pid)
            except OSError:
                continue
            saved.append({"pid": pid, "nice": prev})
            try:
                os.setpriority(os.PRIO_PROCESS, pid, 15)
            except OSError:
                pass
    if saved:
        try:
            NICE_RESTORE.write_text(json.dumps(saved) + "\n", encoding="utf-8")
        except OSError:
            pass


def _restore_background_noise() -> None:
    if not NICE_RESTORE.is_file():
        return
    try:
        rows = json.loads(NICE_RESTORE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, TypeError):
        rows = []
    for row in rows:
        try:
            pid = int(row["pid"])
            nice = int(row["nice"])
            os.setpriority(os.PRIO_PROCESS, pid, nice)
        except (OSError, KeyError, TypeError, ValueError):
            pass
    try:
        NICE_RESTORE.unlink()
    except OSError:
        pass


def _restore_power_profile() -> None:
    if not POWER_PROFILE_RESTORE.is_file():
        return
    try:
        prev = POWER_PROFILE_RESTORE.read_text(encoding="utf-8").strip()
        POWER_PROFILE_RESTORE.unlink(missing_ok=True)
    except OSError:
        return
    if not prev:
        return
    try:
        subprocess.run(
            ["powerprofilesctl", "set", prev],
            check=False,
            capture_output=True,
            timeout=2,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def _cleanup_when_idle() -> None:
    import audio as audio_mod

    for _ in range(80):
        if not is_recording():
            break
        time.sleep(0.1)
    audio_mod.stop_cleanup()
    _restore_background_noise()
    _restore_power_profile()
    # Wait for GSR to finish muxing before extracting streams.
    for _ in range(50):
        if not is_recording():
            break
        time.sleep(0.1)
    time.sleep(0.4)
    cfg = load_settings()
    video = _last_recording_path()
    if video is None:
        return
    fps = enc.capture_fps(cfg.get("fps"), bool(cfg.get("capture_stable_fps", True)))
    labels = _audio_labels()
    # WAV first, from the Opus master. The clock lock below re-encodes to AAC
    # and deletes the MKV; exporting after that is what made the edit sound harsh.
    if cfg.get("save_audio_folder", True):
        audio_mod.export_audio_tracks(video, labels, cfg)
    pacing = _frame_pacing(video, fps)
    if pacing["retime"]:
        fixed = _lock_frame_clock(video, fps)
        if fixed != video:
            _point_recording(fixed)
            video = fixed
        else:
            pacing["retime"] = False
    _write_chequeo(video, fps, pacing)


def _audio_labels() -> list[str] | None:
    if not LAST_AUDIO_LABELS.is_file():
        return None
    try:
        raw = json.loads(LAST_AUDIO_LABELS.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, TypeError):
        return None
    if not isinstance(raw, list):
        return None
    return [str(item) for item in raw]


def _stream_duration(path: Path, spec: str) -> float | None:
    try:
        out = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-select_streams",
                spec,
                "-show_entries",
                "stream=duration",
                "-of",
                "csv=p=0",
                str(path),
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    for line in (out.stdout or "").splitlines():
        token = line.strip().split(",")[0]
        try:
            return float(token)
        except ValueError:
            continue
    return None


def _video_pts(path: Path) -> list[float]:
    try:
        out = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "packet=pts_time",
                "-of",
                "csv=p=0",
                str(path),
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=180,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    pts: list[float] = []
    for line in (out.stdout or "").splitlines():
        token = line.split(",", 1)[0].strip()
        try:
            pts.append(float(token))
        except ValueError:
            continue
    return pts


def _frame_pacing(video: Path, fps: int) -> dict:
    pts = _video_pts(video)
    audio_span = _stream_duration(video, "a:0")
    video_span = (pts[-1] - pts[0]) if len(pts) >= 2 else None
    deltas = [b - a for a, b in zip(pts, pts[1:]) if b > a]
    dropped = enc.dropped_frames(deltas, fps)
    measured = 0.0
    if video_span and video_span > 0 and len(pts) >= 2:
        measured = (len(pts) - 1) / video_span
    return {
        "frames": len(pts),
        "dropped": dropped,
        "audio_span": audio_span,
        "video_span": video_span,
        "measured_fps": measured,
        # No timestamps: still try the lock. The stream-duration check rejects a desync.
        "retime": True if len(pts) < 2 else enc.should_retime(len(pts), fps, audio_span, video_span),
    }


def _write_chequeo(video: Path, fps: int, pacing: dict) -> None:
    frames = int(pacing.get("frames") or 0)
    dropped = int(pacing.get("dropped") or 0)
    measured = float(pacing.get("measured_fps") or 0)
    ratio = (dropped / frames) if frames else 0.0
    if pacing.get("retime"):
        clock = "Se igualó el reloj a {0} fps. La cantidad de fotogramas coincide con el audio.".format(fps)
    else:
        clock = (
            "No se reescribió el reloj: faltan fotogramas y hacerlo adelantaría la imagen respecto al sonido."
        )
    lines = [
        f"Vídeo: {video.name}",
        f"Fotogramas: {frames}",
        f"FPS medido: {measured:.2f}  ·  objetivo: {fps}",
        f"Fotogramas perdidos: {dropped} ({ratio * 100:.2f}%)",
        clock,
        "Audio de edición: WAV 24 bit, 48 kHz, sacado del audio original.",
        "02-juego.wav es el sonido del juego tal cual, sin filtro ni límite.",
        "La limpieza de estática queda solo en la pista del micrófono.",
        "",
    ]
    try:
        video.with_name(f"{video.stem}-chequeo.txt").write_text("\n".join(lines), encoding="utf-8")
    except OSError:
        return


def _lock_frame_clock(video: Path, fps: int) -> Path:
    """Rewrite the video onto an exact 1/fps grid and store AAC audio.

    gpu-screen-recorder stores frame durations as 16 ms, so editors treat the
    clip as 62.5 fps and play the picture slower than the sound. Opus is what
    mpv plays in sync, but DaVinci Resolve does not time that codec correctly.
    AAC in an MP4 at 90 kHz is the combination Resolve keeps locked to the image.
    """
    if fps <= 0 or video.suffix.lower() not in {".mkv", ".webm", ".mp4"}:
        return video
    timescale = 90_000
    if timescale % fps != 0:
        return video
    step = timescale // fps
    try:
        if video.stat().st_size < 4096:
            return video
        free = shutil.disk_usage(video.parent).free
        if free < video.stat().st_size + 64 * 1024 * 1024:
            return video
    except OSError:
        return video
    dest = video.with_suffix(".mp4")
    tmp = video.with_name(f".{video.stem}.clock.mp4")
    cmd = [
        "ffmpeg",
        "-y",
        "-v",
        "error",
        "-i",
        str(video),
        "-map",
        "0",
        "-c:v",
        "copy",
        "-c:a",
        "aac",
        "-b:a",
        "256k",
        "-ar",
        "48000",
        "-video_track_timescale",
        str(timescale),
        "-bsf:v",
        f"setts=time_base=1/{timescale}:prescale=1:pts=N*{step}:dts=N*{step}:duration={step}",
        "-movflags",
        "+faststart",
        str(tmp),
    ]
    try:
        proc = subprocess.run(cmd, check=False, capture_output=True, timeout=900)
    except (OSError, subprocess.TimeoutExpired):
        tmp.unlink(missing_ok=True)
        return video
    if proc.returncode != 0 or not tmp.is_file():
        tmp.unlink(missing_ok=True)
        return video
    # Format duration follows the longest stream, so a shortened picture still
    # looks fine next to a full-length soundtrack. Compare the streams themselves.
    video_after = _stream_duration(tmp, "v:0")
    audio_after = _stream_duration(tmp, "a:0")
    if video_after and audio_after and abs(video_after - audio_after) / audio_after > 0.03:
        tmp.unlink(missing_ok=True)
        return video
    try:
        if dest.exists() and dest.resolve() != tmp.resolve():
            dest.unlink()
        os.replace(tmp, dest)
        if video.resolve() != dest.resolve():
            video.unlink(missing_ok=True)
    except OSError:
        tmp.unlink(missing_ok=True)
        return video
    return dest


def _point_recording(path: Path) -> None:
    try:
        LAST_OUTPUT.write_text(str(path), encoding="utf-8")
    except OSError:
        pass
    if not STATUS.is_file():
        return
    try:
        data = json.loads(STATUS.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return
    if not isinstance(data, dict):
        return
    data["path"] = str(path)
    try:
        STATUS.write_text(json.dumps(data) + "\n", encoding="utf-8")
    except OSError:
        pass


def _last_recording_path() -> Path | None:
    for candidate in (STATUS, LAST_OUTPUT):
        try:
            if candidate == STATUS and STATUS.is_file():
                data = json.loads(STATUS.read_text(encoding="utf-8"))
                path = Path(str(data.get("path") or ""))
                if path.is_file():
                    return path
            elif candidate == LAST_OUTPUT and LAST_OUTPUT.is_file():
                path = Path(LAST_OUTPUT.read_text(encoding="utf-8").strip())
                if path.is_file():
                    return path
        except (OSError, json.JSONDecodeError, TypeError, ValueError):
            continue
    return None


def _kill_pidfile(path: Path) -> None:
    if not _pid_alive(path):
        return
    try:
        pid = int(path.read_text(encoding="utf-8").strip())
        os.kill(pid, 15)
    except (OSError, ValueError):
        pass


def _stop_tracker() -> None:
    _kill_pidfile(TRACKER_PID)


def stop_recording() -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    STOP.write_text("stop\n", encoding="utf-8")
    _clear_paused_flag()
    _stop_tracker()
    threading.Thread(target=_cleanup_when_idle, daemon=True).start()


def start_recording() -> subprocess.Popen | None:
    if is_recording():
        return None
    CACHE.mkdir(parents=True, exist_ok=True)
    if STOP.exists():
        STOP.unlink()
    _clear_paused_flag()
    cfg = dict(load_settings())
    capture = str(cfg.get("capture_mode") or cfg.get("mode") or "monitor")
    if capture == "game":
        import games as games_mod

        key = str(cfg.get("game_key") or "minecraft")
        found = games_mod.resolve_game_region(
            games_mod.find_region(key), games_mod.is_running(key), _monitor_px()
        )
        if not found:
            raise OSError(
                "Juego no graba la pantalla. Abre la ventana del juego y pulsa REC. "
                "Para el monitor entero elige Pantalla."
            )
        cfg["_game_region"] = found
    _boost_power_for_recording()
    gpu_mod.prepare_for_recording()
    _lower_background_noise()
    output = Path(cfg.get("output_dir") or (Path.home() / "Videos" / "GameCap"))
    output.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    container = str(cfg.get("container") or "mkv")
    ext = "mp4" if container == "mp4" else "mkv"
    if capture == "game":
        import games as games_mod

        slug = games_mod.slug(str(cfg.get("game_title") or "Minecraft"))
        name = f"{slug}-{stamp}.{ext}"
    else:
        name = f"gamecap-{stamp}.{ext}"
    STATUS.write_text(
        '{"recording":true,"paused":false,"elapsed_s":0,"fps":0,"file_mb":0,"free_gb":0,"gsr_pid":0,"path":""}\n',
        encoding="utf-8",
    )
    import audio as audio_mod
    import games as games_mod
    import tracker

    # Leave the speaker volume alone. Rewriting it at record start changes
    # how loud the game already is. The mic slider still applies.
    audio_mod.apply_volumes(
        int(cfg.get("audio_system_vol") or 100),
        int(cfg.get("audio_mic_vol") or 100),
        False,
        True,
    )
    system_dev, mic_dev = audio_mod.start_cleanup(cfg)
    # Always record ≥3 tracks: mezcla + juego + mic (also exported to a folder on stop).
    # In game mode the game track follows the process, not the speaker buffer.
    game_app = None
    if capture == "game":
        game_app = games_mod.audio_process_name(str(cfg.get("game_key") or "minecraft"))
    plan = audio_mod.gsr_audio_track_plan(
        "both", True, True, system_dev, mic_dev, game_app=game_app
    )
    try:
        LAST_OUTPUT.write_text(str(output / name), encoding="utf-8")
        LAST_AUDIO_LABELS.write_text(
            json.dumps([label for label, _src in plan], ensure_ascii=False),
            encoding="utf-8",
        )
    except OSError:
        pass
    cmd = [
        str(which_gamecap()),
        "record",
        "--now",
        "--output-dir",
        str(output),
        "--output-name",
        name,
        "--status-file",
        str(STATUS),
        "--stop-file",
        str(STOP),
    ]
    limit = int(cfg.get("limit_hours") or 0)
    replay = int(cfg.get("replay_secs") or 0)
    if replay > 0:
        cmd.extend(["--replay-secs", str(replay)])
    elif limit > 0:
        cmd.extend(["--duration-secs", str(limit * 3600)])
    else:
        cmd.append("--until-stop")
    _append_capture_target(cmd, cfg)
    cmd.extend(["--ipc", str(tracker.IPC)])
    cmd.extend(_encode_args(cfg, container))
    for _label, track in plan:
        cmd.extend(["--audio", track])
    bitrate = enc.normalize_audio_bitrate(cfg.get("audio_bitrate"))
    if bitrate > 0:
        cmd.extend(["--audio-bitrate", str(bitrate)])
    proc = subprocess.Popen(
        cmd,
        cwd=str(ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    PID.write_text(str(proc.pid), encoding="utf-8")
    # Do not spawn AT-SPI tracker while recording — it hitch the game.
    # Pause is available via hotkey; overview auto-pause is intentionally off.
    _stop_tracker()
    if cfg.get("hud", True):
        spawn_hud(str(mic_dev or ""), audio_mod.playback_monitor())
    return proc


def _append_capture_target(cmd: list[str], cfg: dict) -> None:
    """Each capture mode records only its own target.

    Juego is the game window. Pantalla is the monitor. They do not share a target.
    """
    mode = str(cfg.get("mode") or "monitor")
    capture = str(cfg.get("capture_mode") or mode)
    monitor = str(cfg.get("monitor_name") or "")
    if capture == "device" or (capture != "game" and mode == "device"):
        cmd.extend(["--device", str(cfg.get("v4l2_device") or "/dev/video0")])
        return
    if capture == "game":
        if "_game_region" in cfg:
            region = str(cfg.get("_game_region") or "")
        else:
            import games as games_mod

            key = str(cfg.get("game_key") or "minecraft")
            region = games_mod.resolve_game_region(
                games_mod.find_region(key), games_mod.is_running(key), _monitor_px()
            ) or ""
        if not region:
            raise OSError(
                "Juego no graba la pantalla. Abre la ventana del juego y pulsa REC. "
                "Para el monitor entero elige Pantalla."
            )
        cmd.extend(["--region", _region_without_hud(region)])
        if monitor:
            cmd.extend(["--monitor-name", monitor])
        return
    region = None
    if mode in {"region", "area", "window"} and cfg.get("region"):
        region = str(cfg.get("region"))
    cmd.extend(["--region", _region_without_hud(region)])
    if monitor:
        cmd.extend(["--monitor-name", monitor])


def _encode_args(cfg: dict, container: str) -> list[str]:
    fps = enc.capture_fps(cfg.get("fps"), bool(cfg.get("capture_stable_fps", True)))
    args = [
        "--fps",
        str(fps),
        "--codec",
        str(cfg.get("codec") or "av1"),
        "--container",
        container,
        "--quality",
        enc.normalize_quality(cfg.get("quality")),
        "--cursor",
        "yes" if cfg.get("cursor", True) else "no",
        "--frame-mode",
        str(cfg.get("frame_mode") or "vfr"),
        "--tune",
        str(cfg.get("encode_tune") or "performance"),
        "--keyint",
        f"{enc.clamp_keyint(cfg.get('encode_keyint')):.1f}",
    ]
    scale = str(cfg.get("scale") or "0x0").strip()
    if not enc.is_native_scale(scale):
        args.extend(["--scale", scale])
    return args


def spawn_tracker(game_key: str) -> None:
    _stop_tracker()
    cmd = ["python3", str(TRACKER)]
    if game_key:
        cmd.extend(["--game", game_key])
    proc = subprocess.Popen(
        cmd,
        cwd=str(ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    TRACKER_PID.write_text(str(proc.pid), encoding="utf-8")


def spawn_hud(mic: str = "", game_audio: str = "") -> None:
    stop_hud()
    env = os.environ.copy()
    env["GDK_BACKEND"] = "x11"
    cmd = [
        "python3",
        str(HUD),
        "--status-file",
        str(STATUS),
        "--stop-file",
        str(STOP),
    ]
    if mic and mic not in {"default_input", "none"}:
        cmd.extend(["--mic", mic])
    if game_audio and game_audio not in {"none", "@DEFAULT_SINK@"}:
        cmd.extend(["--game-audio", game_audio])
    hud = subprocess.Popen(
        cmd,
        env=env,
        cwd=str(ROOT),
        start_new_session=True,
    )
    HUD_PID.write_text(str(hud.pid), encoding="utf-8")


def stop_hud() -> None:
    if not _pid_alive(HUD_PID):
        return
    try:
        pid = int(HUD_PID.read_text(encoding="utf-8").strip())
        os.kill(pid, 15)
    except (OSError, ValueError):
        pass


def save_replay() -> bool:
    pid = _gsr_pid()
    if pid <= 1:
        return False
    try:
        os.kill(pid, signal.SIGUSR1)
    except OSError:
        return False
    return True


def _paused_flag() -> bool:
    if not PAUSED.is_file():
        return False
    try:
        return PAUSED.read_text(encoding="utf-8").strip() in {"1", "true", "True"}
    except OSError:
        return False


def _clear_paused_flag() -> None:
    try:
        if PAUSED.exists():
            PAUSED.unlink()
    except OSError:
        pass
    _set_status_paused(False)


def _write_paused_flag(paused: bool) -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    try:
        PAUSED.write_text("1\n" if paused else "0\n", encoding="utf-8")
    except OSError:
        pass
    _set_status_paused(paused)


def _gsr_pid() -> int:
    if STATUS.is_file():
        try:
            data = json.loads(STATUS.read_text(encoding="utf-8"))
            pid = int(data.get("gsr_pid") or 0)
            if pid > 1:
                try:
                    os.kill(pid, 0)
                    return pid
                except OSError:
                    pass
        except (OSError, json.JSONDecodeError, TypeError, ValueError):
            pass
    try:
        out = subprocess.run(
            ["pgrep", "-n", "-x", "gpu-screen-recorder"],
            capture_output=True,
            text=True,
            timeout=1,
        )
        pid = int((out.stdout or "").strip().splitlines()[0])
        return pid if pid > 1 else 0
    except (OSError, subprocess.TimeoutExpired, ValueError, IndexError):
        return 0


def _set_status_paused(paused: bool) -> None:
    if not STATUS.is_file():
        return
    try:
        data = json.loads(STATUS.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, TypeError, ValueError):
        return
    data["paused"] = bool(paused)
    try:
        STATUS.write_text(json.dumps(data, ensure_ascii=False) + "\n", encoding="utf-8")
    except OSError:
        pass


def _notify(title: str, body: str) -> None:
    try:
        subprocess.Popen(
            ["notify-send", "-a", "Kapture", "-i", "media-playback-pause", title, body],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        pass


def _ipc_set_paused(paused: bool) -> bool:
    import tracker

    if not tracker.IPC.exists():
        return False
    try:
        r = subprocess.run(
            ["gsr-cli", "-ipc", str(tracker.IPC), "set-paused", "true" if paused else "false"],
            check=False,
            capture_output=True,
            text=True,
            timeout=2,
        )
        return r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def toggle_pause() -> str:
    """Pause/resume with absolute set-paused (avoids double-toggle desync)."""
    if not is_recording():
        return "idle"
    want_pause = not _paused_flag()
    ok = _ipc_set_paused(want_pause)
    if not ok:
        pid = _gsr_pid()
        if pid <= 1:
            return "error"
        # SIGUSR2 only toggles; keep local flag as source of truth for next press.
        try:
            os.kill(pid, signal.SIGUSR2)
        except OSError:
            return "error"
        ok = True
    if not ok:
        return "error"
    _write_paused_flag(want_pause)
    if want_pause:
        _notify("Grabación en pausa", "Pulsa la misma tecla para reanudar.")
        return "pause"
    _notify("Grabación reanudada", "Seguimos grabando.")
    return "resume"


def toggle() -> str:
    if is_recording():
        stop_recording()
        return "stop"
    if not ui_is_open():
        _notify("Kapture está cerrado", "Abre el programa para empezar a grabar.")
        return "closed"
    start_recording()
    return "start"
