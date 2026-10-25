"""Audio devices, volume and live noise cleanup for GameCap."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import time
from pathlib import Path

CACHE = Path.home() / ".cache" / "gamecap"
STATE = CACHE / "audio-cleanup.json"
MIC_SOURCE = "gamecap_clean_mic"
SYS_SINK = "gamecap_clean_sys"

STRENGTH = {
    "suave": {"nr": 6, "nf": -30, "highpass": 60},
    "media": {"nr": 12, "nf": -25, "highpass": 80},
    "agresiva": {"nr": 18, "nf": -20, "highpass": 100},
}


def list_gsr_devices() -> list[tuple[str, str]]:
    try:
        out = subprocess.run(
            ["gpu-screen-recorder", "--list-audio-devices"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return [("default_output", "Salida por defecto"), ("default_input", "Micrófono por defecto")]
    rows: list[tuple[str, str]] = []
    for line in (out.stdout or "").splitlines():
        if "|" not in line:
            continue
        ident, label = line.split("|", 1)
        ident, label = ident.strip(), label.strip()
        if ident:
            rows.append((ident, label or ident))
    return rows or [("default_output", "Salida por defecto")]


def outputs() -> list[tuple[str, str]]:
    return [
        (i, n)
        for i, n in list_gsr_devices()
        if i == "default_output" or "monitor" in i or "output" in i
    ]


def inputs() -> list[tuple[str, str]]:
    return [
        (i, n)
        for i, n in list_gsr_devices()
        if i == "default_input" or ("input" in i and "monitor" not in i)
    ]


def gsr_audio_arg(system: bool, mic: bool, system_dev: str, mic_dev: str) -> str:
    parts: list[str] = []
    if system:
        parts.append(system_dev or "default_output")
    if mic:
        parts.append(mic_dev or "default_input")
    return "|".join(parts) if parts else "none"


def app_audio_source(process_name: str | None, fallback: str) -> str:
    """GSR ``app:`` source for a game process, or the speaker device."""
    name = " ".join((process_name or "").split())
    if not name or "|" in name:
        return fallback or "default_output"
    if name.startswith("app:"):
        name = name.removeprefix("app:").strip()
    if not name:
        return fallback or "default_output"
    return f"app:{name}"


def gsr_audio_track_plan(
    mode: str,
    system: bool,
    mic: bool,
    system_dev: str,
    mic_dev: str,
    game_app: str | None = None,
) -> list[tuple[str, str]]:
    """Ordered (label, GSR source) for embedded streams and folder export.

    Default ``both`` + juego + mic → 3 tracks: mezcla, juego, micrófono.
    When ``game_app`` is set, the game track (and the game half of the mix)
    follows that process so it stays aligned with the picture.
    """
    speakers = system_dev or "default_output"
    game_src = app_audio_source(game_app, speakers)
    micd = mic_dev or "default_input"
    kind = (mode or "both").lower()
    planned: list[tuple[str, str]] = []
    if kind in {"mix", "both"} and (system or mic):
        mixed = gsr_audio_arg(system, mic, game_src, micd)
        if mixed != "none":
            planned.append(("mezcla", mixed))
    if kind in {"separate", "both"}:
        if system:
            planned.append(("juego", game_src))
        if mic:
            planned.append(("microfono", micd))
    seen: set[str] = set()
    unique: list[tuple[str, str]] = []
    for label, src in planned:
        if src in seen or src == "none":
            continue
        seen.add(src)
        unique.append((label, src))
    return unique or [("mezcla", "none")]


def gsr_audio_tracks(
    mode: str,
    system: bool,
    mic: bool,
    system_dev: str,
    mic_dev: str,
) -> list[str]:
    """Build GSR -a sources. mode: mix | separate | both."""
    return [src for _label, src in gsr_audio_track_plan(mode, system, mic, system_dev, mic_dev)]


def count_audio_streams(video: Path) -> int:
    try:
        out = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=index",
                "-of",
                "csv=p=0",
                str(video),
            ],
            check=False,
            timeout=30,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.TimeoutExpired):
        return 0
    if out.returncode != 0:
        return 0
    lines = [ln.strip() for ln in (out.stdout or "").splitlines() if ln.strip()]
    return len(lines)


def audio_export_dir(video: Path) -> Path:
    """Folder of extracted tracks that sits next to the recording."""
    return video.with_name(f"{video.stem}-audio")


def remove_audio_export(video: Path) -> bool:
    """Delete ``<stem>-audio`` beside a recording. True when a folder was removed."""
    folder = audio_export_dir(video)
    if folder.is_symlink() or not folder.is_dir():
        return False
    try:
        if folder.resolve().parent != video.parent.resolve():
            return False
        shutil.rmtree(folder)
    except OSError:
        raise
    return True


def export_audio_tracks(video: Path, labels: list[str] | None = None, cfg: dict | None = None) -> Path | None:
    """Copy each audio stream into ``<stem>-audio/`` as 24-bit WAV.

    Call this on the original recording, before any AAC remux, so the editor
    hears one decode of the Opus master instead of Opus followed by AAC.
    """
    if not video.is_file():
        return None
    n = count_audio_streams(video)
    if n <= 0:
        return None
    out_dir = audio_export_dir(video)
    try:
        out_dir.mkdir(parents=True, exist_ok=True)
    except OSError:
        return None
    defaults = ["mezcla", "juego", "microfono"]
    names = list(labels or [])
    while len(names) < n:
        i = len(names)
        names.append(defaults[i] if i < len(defaults) else f"pista-{i + 1}")
    written = 0
    for i in range(n):
        safe = "".join(c if c.isalnum() or c in "-_" else "-" for c in names[i]).strip("-") or f"pista-{i + 1}"
        dest = out_dir / f"{i + 1:02d}-{safe}.wav"
        cmd = [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(video),
            "-map",
            f"0:a:{i}",
        ]
        filt = editor_filter(names[i], cfg)
        if filt:
            cmd.extend(["-af", filt])
        cmd.extend(["-c:a", "pcm_s24le", "-ar", "48000", str(dest)])
        try:
            proc = subprocess.run(
                cmd,
                check=False,
                timeout=3600,
                capture_output=True,
            )
        except (OSError, subprocess.TimeoutExpired):
            proc = None
        if proc is None or proc.returncode != 0:
            # A bad filter must not drop the track. Write the plain decode.
            try:
                proc = subprocess.run(
                    [
                        "ffmpeg",
                        "-hide_banner",
                        "-loglevel",
                        "error",
                        "-y",
                        "-i",
                        str(video),
                        "-map",
                        f"0:a:{i}",
                        "-c:a",
                        "pcm_s24le",
                        "-ar",
                        "48000",
                        str(dest),
                    ],
                    check=False,
                    timeout=3600,
                    capture_output=True,
                )
            except (OSError, subprocess.TimeoutExpired):
                continue
        if proc.returncode == 0 and dest.is_file() and dest.stat().st_size > 0:
            written += 1
    if written <= 0:
        return None
    readme = out_dir / "LEEME.txt"
    try:
        lines = [
            f"Pistas extraídas de: {video.name}",
            "Son WAV de 24 bit a 48 kHz, sacadas del audio original (antes de pasar a AAC).",
            "DaVinci Resolve en Linux no reproduce el audio de dentro del vídeo.",
            "02-juego.wav es el sonido del juego tal cual, sin filtro ni límite.",
            "01-mezcla.wav es juego + micrófono, también sin procesar.",
            "03-microfono.wav es la única pista con limpieza de estática.",
            "Para oír el juego natural, usa solo 02-juego.wav.",
            "",
        ]
        for i in range(n):
            lines.append(f"{i + 1:02d}: {names[i]}")
        readme.write_text("\n".join(lines) + "\n", encoding="utf-8")
    except OSError:
        pass
    return out_dir


def set_volume_percent(kind: str, percent: int) -> None:
    percent = max(0, min(150, int(percent)))
    target = "@DEFAULT_SINK@" if kind == "sink" else "@DEFAULT_SOURCE@"
    try:
        subprocess.run(
            ["wpctl", "set-volume", target, f"{percent / 100:.2f}"],
            check=False,
            capture_output=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def current_volume_percent(kind: str) -> int:
    target = "@DEFAULT_SINK@" if kind == "sink" else "@DEFAULT_SOURCE@"
    try:
        out = subprocess.run(
            ["wpctl", "get-volume", target],
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return 100
    for token in (out.stdout or "").split():
        try:
            return int(round(float(token) * 100))
        except ValueError:
            continue
    return 100


def apply_volumes(sys_pct: int, mic_pct: int, system: bool, mic: bool) -> None:
    if system:
        set_volume_percent("sink", sys_pct)
    if mic:
        set_volume_percent("source", mic_pct)


def which_mpv() -> Path:
    p = Path("/usr/bin/mpv")
    return p if p.is_file() else Path("mpv")


def ffmpeg_filter(ambient: bool, static: bool, dirty: bool, strength: str) -> str:
    tune = STRENGTH.get(strength, STRENGTH["media"])
    parts: list[str] = []
    if ambient:
        parts.append(f"highpass=f={tune['highpass']}")
    if static:
        parts.append(f"afftdn=nr={tune['nr']}:nf={tune['nf']}:nt=w")
    if dirty:
        parts.append("adeclick")
        parts.append("adeclip")
    return ",".join(parts)


def editor_filter(label: str, cfg: dict | None = None) -> str:
    """Cleanup for the microphone WAV only.

    The game track and the mix are a straight decode of the capture. A limiter
    or noise filter on either one changes the game's own sound.
    """
    name = (label or "").strip().lower()
    if name != "microfono":
        return ""
    options = cfg or {}
    if not bool(options.get("audio_denoise_mic", True)):
        return ""
    ambient = bool(options.get("audio_denoise_ambient", True))
    static = bool(options.get("audio_denoise_static", True))
    dirty = bool(options.get("audio_denoise_dirty", True))
    strength = str(options.get("audio_denoise_strength") or "media")
    parts: list[str] = []
    chain = ffmpeg_filter(ambient, static, dirty, strength)
    if chain:
        parts.append(chain)
    parts.append("alimiter=limit=0.89:level=false:attack=5:release=50")
    return ",".join(parts)


def _pactl(*args: str) -> str:
    try:
        out = subprocess.run(["pactl", *args], capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return (out.stdout or "").strip()


def _pulse_source(gsr_id: str, fallback: str) -> str:
    ident = (gsr_id or "").removeprefix("device:").strip()
    if ident in {"", "default_input", "default_output"}:
        return fallback
    return ident


def playback_monitor() -> str:
    """Pulse source that hears the speakers. Used by the HUD game VU."""
    return _default_monitor()


def _default_monitor() -> str:
    sink = _pactl("get-default-sink") or "@DEFAULT_SINK@"
    if sink.startswith("@"):
        return sink
    return f"{sink}.monitor"


def _default_mic() -> str:
    return _pactl("get-default-source") or "@DEFAULT_SOURCE@"


def _find_gsr(substr: str) -> str | None:
    needle = substr.lower()
    for ident, name in list_gsr_devices():
        if needle in ident.lower() or needle in name.lower():
            return ident
    return None


def _wait_gsr(substr: str, tries: int = 8) -> str | None:
    for _ in range(tries):
        found = _find_gsr(substr)
        if found:
            return found
        time.sleep(0.15)
    return None


def _load_module(args: list[str]) -> str:
    ident = _pactl("load-module", *args)
    return ident if ident.isdigit() else ""


def _kill(pid: int) -> None:
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return
    try:
        os.kill(pid, 0)
        time.sleep(0.2)
        os.kill(pid, signal.SIGKILL)
    except OSError:
        return


def _default_sink() -> str:
    return _pactl("get-default-sink")


def _default_source() -> str:
    return _pactl("get-default-source")


def _restore_defaults(sink: str, source: str) -> None:
    if sink:
        _pactl("set-default-sink", sink)
    if source:
        _pactl("set-default-source", source)


def _kill_stray() -> None:
    try:
        out = subprocess.run(
            ["pgrep", "-af", "ffmpeg"],
            capture_output=True,
            text=True,
            timeout=2,
        )
    except (OSError, subprocess.TimeoutExpired):
        return
    for line in (out.stdout or "").splitlines():
        if "GameCapRec" not in line and "gamecap_clean" not in line:
            continue
        pid_s = line.split(None, 1)[0]
        try:
            _kill(int(pid_s))
        except ValueError:
            continue


def stop_cleanup() -> None:
    data: dict = {}
    if STATE.is_file():
        try:
            data = json.loads(STATE.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            data = {}
    for pid in data.get("ffpids") or []:
        try:
            _kill(int(pid))
        except (TypeError, ValueError):
            continue
    _kill_stray()
    for mid in data.get("modules") or []:
        _pactl("unload-module", str(mid))
    # Never leave echo-cancel or our virtual sinks in the live graph.
    for line in (_pactl("list", "modules", "short") or "").splitlines():
        low = line.lower()
        if "module-echo-cancel" in low or "gamecap_clean" in low:
            mid = line.split(None, 1)[0]
            if mid.isdigit():
                _pactl("unload-module", mid)
    _restore_defaults(str(data.get("prev_sink") or ""), str(data.get("prev_source") or ""))
    try:
        STATE.unlink()
    except OSError:
        pass


def start_cleanup(cfg: dict) -> tuple[str, str]:
    """Return the live Pulse devices GSR should record.

    Never route mic/game audio through ffmpeg: that delayed copy desyncs
    the voice from the video. Leftover virtual sinks from older sessions
    are unloaded first.
    """
    stop_cleanup()
    system_dev = str(cfg.get("audio_system_device") or "default_output")
    mic_dev = str(cfg.get("audio_mic_device") or "default_input")
    if mic_dev.startswith("device:"):
        mic_dev = mic_dev.removeprefix("device:").strip() or "default_input"
    if "monitor" in mic_dev.lower():
        mic_dev = "default_input"
    return system_dev, mic_dev


def _load_null_sink(name: str, description: str) -> str:
    return _load_module(
        [
            "module-null-sink",
            f"sink_name={name}",
            f"sink_properties=device.description={description}",
        ]
    )


def _spawn_ffmpeg(src: str, sink: str, filt: str) -> subprocess.Popen | None:
    """Capture src, filter, and write ONLY to our null-sink (never the speakers)."""
    env = os.environ.copy()
    env["PULSE_SINK"] = sink
    try:
        return subprocess.Popen(
            [
                "ffmpeg",
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "pulse",
                "-i",
                src,
                "-af",
                filt,
                "-f",
                "pulse",
                "-device",
                sink,
                "GameCapRec",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            env=env,
        )
    except OSError:
        return None


def list_monitors() -> list[tuple[str, str]]:
    try:
        out = subprocess.run(
            ["gpu-screen-recorder", "--list-monitors"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    rows: list[tuple[str, str]] = []
    for line in (out.stdout or "").splitlines():
        if "|" not in line:
            rows.append((line.strip(), line.strip()) if line.strip() else ("", ""))
            continue
        ident, rest = line.split("|", 1)
        ident, rest = ident.strip(), rest.strip()
        if ident:
            rows.append((ident, f"{ident} [{rest}]" if rest else ident))
    return [r for r in rows if r[0]]


def list_v4l2() -> list[tuple[str, str]]:
    try:
        out = subprocess.run(
            ["gpu-screen-recorder", "--list-v4l2-devices"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    rows: list[tuple[str, str]] = []
    seen: set[str] = set()
    for line in (out.stdout or "").splitlines():
        ident = line.split("|", 1)[0].strip()
        if ident and ident not in seen:
            seen.add(ident)
            rows.append((ident, line.replace("|", " · ")))
    return rows
