"""Detect running games, including Minecraft and any Steam / Proton title."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

SKIP_COMM = {
    "bash",
    "zsh",
    "fish",
    "sh",
    "systemd",
    "dbus-daemon",
    "pipewire",
    "wireplumber",
    "gnome-shell",
    "gnome-session",
    "xdg-desktop-por",
    "firefox",
    "chrome",
    "chromium",
    "cursor",
    "code",
    "python3",
    "python",
    "gamecap",
    "gpu-screen-recor",
    "steamwebhelper",
    "steam-runtime-la",
    "GameOverlayUI",
    "steamoverlayvul",
    "ibus-daemon",
    "ibus-x11",
    "fusermount",
    "fusermount3",
    "xdg-document-po",
    "xdg-permission-",
}

# Steam helpers that are not the game itself (still useful for AppId env).
STEAM_HELPERS = {
    "steam",
    "steamwebhelper",
    "reaper",
    "pressure-vessel",
    "pressure-vessel-adverb",
    "steam-runtime-launcher-service",
    "gameoverlayui",
    "steamoverlayvulkanlayer",
}

# Steam tool / runtime AppIDs that are not recordable games.
STEAM_NON_GAME_APPIDS = {
    "228980",  # Steamworks Common Redistributables
    "1070560",  # Steam Linux Runtime
    "1391110",  # Steam Linux Runtime - soldier
    "1493710",  # Proton Experimental
    "1628350",  # Steam Linux Runtime 3.0 sniper
    "2180100",  # Proton Hotfix
    "2805730",  # Proton Next / etc.
    "3658110",
    "4183110",  # Steam Linux Runtime 4.0
}

_STEAM_NON_GAME_TITLE = re.compile(
    r"(proton|steam linux runtime|steamworks|steamworks common|redistributable)",
    re.I,
)


@dataclass(frozen=True)
class GameApp:
    key: str
    title: str
    detail: str
    running: bool
    pid: int = 0
    needles: tuple[str, ...] = ()


_MINECRAFT_CLIENTS: tuple[tuple[str, str], ...] = (
    ("prismlauncher", "Prism Launcher"),
    ("prism-launcher", "Prism Launcher"),
    ("org.prismlauncher", "Prism Launcher"),
    ("polymc", "PolyMC"),
    ("multimc", "MultiMC"),
    ("lunarclient", "Lunar Client"),
    ("lunar-client", "Lunar Client"),
    ("badlion", "Badlion"),
    ("featherclient", "Feather"),
    ("feather-launcher", "Feather"),
    ("feather_web", "Feather"),
    ("/feather", "Feather"),
    ("labymod", "LabyMod"),
    ("atlauncher", "ATLauncher"),
    ("gdlauncher", "GDLauncher"),
    ("sklauncher", "SKLauncher"),
    ("tlauncher", "TLauncher"),
    ("gg.dawn", "Dawn Launcher"),
    ("dawn.launcher", "Dawn Launcher"),
    ("/.dawn/", "Dawn Launcher"),
    ("minecraft-launcher", "Launcher oficial"),
    ("minecraftlauncher", "Launcher oficial"),
)

_CLIENT_RANK = {
    "Feather": 8,
    "Lunar Client": 8,
    "Badlion": 8,
    "Prism Launcher": 7,
    "PolyMC": 7,
    "MultiMC": 7,
    "LabyMod": 7,
    "ATLauncher": 6,
    "GDLauncher": 6,
    "SKLauncher": 6,
    "TLauncher": 6,
    "Dawn Launcher": 5,
    "Launcher oficial": 4,
    "Java / LWJGL": 3,
    "en ejecución": 1,
}

_MC_HINTS = (
    "net.minecraft",
    "cpw.mods",
    "net.fabricmc",
    "net.neoforged",
    "net.minecraftforge",
    "lwjgl",
    ".minecraft",
    "minecraft.jar",
    "minecraft-k",
    "/minecraft",
    "com.mojang",
)


def _read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return ""


def _cmdline(pid: int) -> str:
    raw = _read(Path(f"/proc/{pid}/cmdline"))
    return raw.replace("\x00", " ").strip().lower()


def _comm(pid: int) -> str:
    return _read(Path(f"/proc/{pid}/comm")).strip().lower()


def _exe(pid: int) -> str:
    try:
        return os.readlink(f"/proc/{pid}/exe").lower()
    except OSError:
        return ""


def _environ(pid: int) -> dict[str, str]:
    raw = _read(Path(f"/proc/{pid}/environ"))
    out: dict[str, str] = {}
    for part in raw.split("\x00"):
        if "=" not in part:
            continue
        key, val = part.split("=", 1)
        out[key] = val
    return out


def _iter_pids() -> list[int]:
    pids: list[int] = []
    try:
        for entry in Path("/proc").iterdir():
            if entry.name.isdigit():
                pids.append(int(entry.name))
    except OSError:
        return []
    return pids


def _minecraft_client(blob: str) -> str | None:
    for needle, label in _MINECRAFT_CLIENTS:
        if needle in blob:
            return label
    return None


def _is_minecraft(blob: str) -> bool:
    if _minecraft_client(blob):
        return True
    if "minecraft" in blob:
        return True
    return any(hint in blob for hint in _MC_HINTS)


def _steam_roots() -> list[Path]:
    home = Path.home()
    roots = [
        home / ".steam" / "steam",
        home / ".steam" / "root",
        home / ".local" / "share" / "Steam",
        home / ".var" / "app" / "com.valvesoftware.Steam" / ".local" / "share" / "Steam",
        home / ".var" / "app" / "com.valvesoftware.Steam" / "data" / "Steam",
    ]
    return [p for p in roots if p.is_dir()]


def _library_folders(root: Path) -> list[Path]:
    folders = [root / "steamapps"]
    vdf = root / "steamapps" / "libraryfolders.vdf"
    text = _read(vdf)
    for match in re.finditer(r'"path"\s+"([^"]+)"', text):
        path = Path(match.group(1).replace("\\\\", "/")) / "steamapps"
        if path.is_dir():
            folders.append(path)
    # Unique preserving order.
    seen: set[str] = set()
    out: list[Path] = []
    for folder in folders:
        key = str(folder.resolve()) if folder.exists() else str(folder)
        if key in seen:
            continue
        seen.add(key)
        out.append(folder)
    return out


@lru_cache(maxsize=256)
def _steam_name_for_appid(appid: str) -> str | None:
    if not appid.isdigit():
        return None
    for root in _steam_roots():
        for apps in _library_folders(root):
            acf = apps / f"appmanifest_{appid}.acf"
            if not acf.is_file():
                continue
            match = re.search(r'"name"\s+"([^"]+)"', _read(acf))
            if match:
                return match.group(1).strip()
    return None


def _pretty_folder(name: str) -> str:
    return name.replace("_", " ").replace("-", " ").strip().title()


def _needles_for(title: str, *extra: str) -> tuple[str, ...]:
    words: list[str] = []
    for part in (title, *extra):
        text = (part or "").strip().lower()
        if not text:
            continue
        words.append(text)
        words.append(text.replace("-", " "))
        words.append(text.replace(" ", "-"))
        words.append(text.replace(" ", ""))
        for token in re.split(r"[^a-z0-9]+", text):
            if len(token) >= 3:
                words.append(token)
    # Prefer longer / distinctive needles first.
    uniq: list[str] = []
    seen: set[str] = set()
    for word in sorted(set(words), key=lambda w: (-len(w), w)):
        if word in seen or word in {"the", "and", "for", "app", "game", "steam"}:
            continue
        seen.add(word)
        uniq.append(word)
    return tuple(uniq[:16])


def _slug(title: str) -> str:
    text = re.sub(r"[^a-z0-9]+", "-", (title or "game").lower()).strip("-")
    return (text or "game")[:64]


def _detect_steam_or_game(
    pid: int, comm: str, cmd: str, exe: str
) -> tuple[GameApp, int] | None:
    """Identify Steam native/Proton games and other obvious game processes.

    Returns (GameApp, score) where higher score prefers the real game process
    over Steam wrappers like reaper.
    """
    if not comm or comm in SKIP_COMM:
        return None
    blob = f"{comm} {cmd} {exe}"
    env = _environ(pid)

    appid = (env.get("SteamAppId") or env.get("SteamGameId") or "").strip()
    if appid in {"", "0"}:
        m_compat = re.search(r"steamapps/compatdata/(\d+)", blob)
        if m_compat:
            appid = m_compat.group(1)
        else:
            for key in ("STEAM_COMPAT_DATA_PATH", "SteamCompatDataPath"):
                val = env.get(key, "")
                m = re.search(r"compatdata/(\d+)", val.replace("\\", "/").lower())
                if m:
                    appid = m.group(1)
                    break

    folder = None
    m_common = re.search(r"steamapps/common/([^/]+)", blob)
    if m_common:
        folder = m_common.group(1)
    else:
        for key in ("PWD", "STEAM_COMPAT_INSTALL_PATH", "SteamAppPath"):
            val = env.get(key, "").replace("\\", "/").lower()
            m = re.search(r"steamapps/common/([^/]+)", val)
            if m:
                folder = m.group(1)
                break

    title = None
    detail_bits: list[str] = []
    if appid and appid != "0":
        if appid in STEAM_NON_GAME_APPIDS:
            return None
        title = _steam_name_for_appid(appid) or (_pretty_folder(folder) if folder else None)
        if title and _STEAM_NON_GAME_TITLE.search(title):
            return None
        detail_bits.append(f"Steam {appid}")
    if title is None and folder:
        title = _pretty_folder(folder)
        if _STEAM_NON_GAME_TITLE.search(title):
            return None
        detail_bits.append("Steam")
    if title is None and "steamapps" in blob and comm not in STEAM_HELPERS:
        title = _pretty_folder(comm)
        detail_bits.append("Steam")
    if title is None and any(
        x in blob
        for x in (
            "/proton",
            "pressure-vessel",
            "wine64-preloader",
            "wine-preloader",
            "/compatdata/",
        )
    ):
        if comm in STEAM_HELPERS or "proton" in comm:
            return None
        title = _steam_name_for_appid(appid) if appid else None
        if not title:
            return None
        if _STEAM_NON_GAME_TITLE.search(title):
            return None
        detail_bits.append("Proton")
    if title is None and any(x in blob for x in ("heroic/", "lutris/", "bottles/", "legendary", "gogdl")):
        if comm in {"heroic", "lutris", "bottles", "electron"}:
            return None
        title = _pretty_folder(comm)
        detail_bits.append("Launcher")

    if title is None:
        # Match only on process name — never substring of cmdline (false positives like nosuid→osu).
        known = {
            "cs2": "Counter-Strike 2",
            "dota2": "Dota 2",
            "hl2": "Half-Life 2",
            "gta5": "GTA V",
            "gtav": "GTA V",
            "eldenring": "Elden Ring",
            "factorio": "Factorio",
            "osu!": "osu!",
            "osu": "osu!",
            "r5apex": "Apex Legends",
        }
        title = known.get(comm)
        if title:
            detail_bits.append(comm)

    if not title:
        return None
    if comm in STEAM_HELPERS and not folder and not (appid and appid != "0"):
        return None

    score = 1
    if comm in STEAM_HELPERS:
        score = 0
    elif "steamapps/common" in blob or "compatdata" in blob:
        score = 3
    elif appid and appid != "0":
        score = 2

    key = f"steam-{appid}" if appid and appid.isdigit() and appid != "0" else _slug(title)
    needles = _needles_for(title, folder or "", comm, key)
    detail = " · ".join([*detail_bits, "en ejecución"]) if detail_bits else "en ejecución"
    return GameApp(key=key, title=title, detail=detail, running=True, pid=pid, needles=needles), score


def list_games() -> list[GameApp]:
    """Minecraft is always listed; Steam and other running games are appended."""
    mc_running = False
    mc_detail = "Cualquier cliente (Prism, Lunar, oficial, Java…)"
    mc_pid = 0
    others: dict[str, tuple[GameApp, int]] = {}

    for pid in _iter_pids():
        comm = _comm(pid)
        if not comm or comm in SKIP_COMM:
            continue
        cmd = _cmdline(pid)
        exe = _exe(pid)
        blob = f"{comm} {cmd} {exe}"
        if _is_minecraft(blob):
            mc_running = True
            client = _minecraft_client(blob)
            if client:
                detail = f"{client} · en ejecución"
            elif "java" in comm or "java" in exe:
                detail = "Java / LWJGL · en ejecución"
            else:
                detail = "en ejecución"
            new_rank = _CLIENT_RANK.get(client or "", 2 if "java" in comm else 1)
            old_name = mc_detail.split(" ·", 1)[0]
            old_rank = _CLIENT_RANK.get(old_name, 0)
            if new_rank >= old_rank:
                mc_detail = detail
            if "java" in comm or comm == "minecraft":
                mc_pid = pid
            elif mc_pid == 0 and "feather_web" not in comm and "helper" not in comm:
                mc_pid = pid
            continue

        detected = _detect_steam_or_game(pid, comm, cmd, exe)
        if not detected:
            continue
        game, score = detected
        prev = others.get(game.key)
        if prev is None or score > prev[1] or (score == prev[1] and game.pid >= prev[0].pid):
            others[game.key] = (game, score)

    games = [
        GameApp(
            key="minecraft",
            title="Minecraft",
            detail=mc_detail,
            running=mc_running,
            pid=mc_pid,
            needles=_needles_for("minecraft", "lwjgl", "feather", "lunar", "prism"),
        )
    ]
    games.extend(sorted((g for g, _ in others.values()), key=lambda g: g.title.lower()))
    return games


def is_running(key: str) -> bool:
    key = (key or "").strip().lower()
    if not key:
        return False
    for game in list_games():
        if game.key == key and game.running:
            return True
    if key == "minecraft":
        for pid in _iter_pids():
            if _is_minecraft(f"{_comm(pid)} {_cmdline(pid)} {_exe(pid)}"):
                return True
    return False


_SKIP_AUDIO_APPS = (
    "easyeffects",
    "firefox",
    "chrome",
    "chromium",
    "cursor",
    "gamecap",
    "kapture",
    "discord",
    "spotify",
    "pipewire",
    "pulseaudio",
    "gnome-shell",
)


def _pipewire_output_streams() -> list[tuple[str, int, str]]:
    """Live playback streams: (application.name, pid, binary)."""
    try:
        proc = subprocess.run(
            ["pw-dump"],
            capture_output=True,
            text=True,
            timeout=3,
        )
        if proc.returncode != 0 or not (proc.stdout or "").strip():
            return []
        items = json.loads(proc.stdout)
    except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError, TypeError, ValueError):
        return []
    rows: list[tuple[str, int, str]] = []
    for obj in items if isinstance(items, list) else []:
        if not isinstance(obj, dict) or obj.get("type") != "PipeWire:Interface:Node":
            continue
        props = (obj.get("info") or {}).get("props") or {}
        if str(props.get("media.class") or "") != "Stream/Output/Audio":
            continue
        app = str(props.get("application.name") or "").strip()
        binary = str(props.get("application.process.binary") or "").strip().lower()
        blob = f"{app} {binary}".lower()
        if not app and not binary:
            continue
        if any(skip in blob for skip in _SKIP_AUDIO_APPS):
            continue
        try:
            owner = int(props.get("application.process.id") or 0)
        except (TypeError, ValueError):
            owner = 0
        rows.append((app or binary, owner, binary))
    return rows


def _pipewire_playback_name(pid: int, comm: str = "") -> str | None:
    """PipeWire ``application.name`` of the game's playback stream.

    Flatpak/bwrap reports a sandbox PID (often 36), not the host java PID, so
    matching only the host pid misses Minecraft. Fall back to binary/name.
    """
    streams = _pipewire_output_streams()
    if pid > 0:
        for app, owner, _binary in streams:
            if owner == pid and app:
                return app
    needle = (comm or "").strip().lower()
    if needle:
        for app, _owner, binary in streams:
            if needle in {binary, app.lower()} or needle in app.lower():
                return app
    return None


def audio_process_name(game_key: str) -> str | None:
    """PipeWire application name GSR should follow with ``app:NAME``.

    Capturing that client, not the speaker output, keeps game audio on the
    same clock as the frames. Speaker capture hears the sound only after the
    playback buffer, so it lands late in the video. ALSA games show up as
    ``PipeWire ALSA [process]``.
    """
    key = (game_key or "").strip().lower()
    if not key:
        return None
    comm = ""
    pid = 0
    for game in list_games():
        if game.key != key or not game.running or game.pid <= 0:
            continue
        pid = game.pid
        comm = _comm(pid)
        if key == "minecraft" and comm not in {"java", "javaw", "minecraft"}:
            comm = "java"
        elif comm in SKIP_COMM or comm in {name.lower() for name in STEAM_HELPERS}:
            comm = "java" if key == "minecraft" else ""
            pid = 0
        break
    live = _pipewire_playback_name(pid, comm)
    if live:
        return live
    if key == "minecraft":
        live = _pipewire_playback_name(0, "java")
        if live:
            return live
        return "java"
    if not comm:
        return None
    return comm


_LAUNCHER_TITLES = (
    "prism launcher",
    "prismlauncher",
    "polymc",
    "multimc",
    "minecraft launcher",
    "atlauncher",
    "gdlauncher",
    "sklauncher",
    "tlauncher",
)


def is_launcher_title(title: str) -> bool:
    text = (title or "").strip().lower()
    if not text:
        return False
    return any(token in text for token in _LAUNCHER_TITLES)


def resolve_game_region(
    found: str | None, running: bool, monitor: tuple[int, int]
) -> str | None:
    """Pick the KMS rectangle for game capture.

    AT-SPI often sees only the launcher. gpu-screen-recorder records a
    framebuffer crop, so a 1266×600 launcher window clips Minecraft.
    If the real GL window is missing, use the whole monitor.
    """
    if found:
        return found
    if not running:
        return None
    width, height = monitor
    if width < 64 or height < 64:
        return None
    return f"{_even(int(width))}x{_even(int(height))}+0+0"


def slug(title: str) -> str:
    text = re.sub(r"[^A-Za-z0-9]+", "-", (title or "gamecap").strip()).strip("-")
    return (text or "gamecap")[:48]


def labels(games: list[GameApp]) -> list[str]:
    rows: list[str] = []
    for game in games:
        mark = "● " if game.running else "○ "
        extra = f" — {game.detail}" if game.detail else ""
        rows.append(f"{mark}{game.title}{extra}")
    return rows


def find_region(game_key: str) -> str | None:
    """Return GSR region WxH+X+Y for the selected game window, if found."""
    win = find_window(game_key)
    if win is None:
        return None
    x, y, w, h, _showing = win
    if w < 64 or h < 64:
        return None
    return f"{_even(w)}x{_even(h)}+{max(x, 0)}+{max(y, 0)}"


def find_window(game_key: str) -> tuple[int, int, int, int, bool] | None:
    """Best matching window: x, y, w, h, showing."""
    probe = Path(__file__).resolve().parent / "window_probe.py"
    needles: list[str] = []
    for game in list_games():
        if game.key == (game_key or "").lower() or game.key == game_key:
            needles = list(game.needles)
            break
    if not needles:
        needles = list(_needles_for(game_key or "minecraft"))
    cmd = [sys.executable, str(probe), game_key or "minecraft", *needles[:12]]
    try:
        out = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=2.5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    raw = (out.stdout or "").strip()
    if not raw or raw == "{}":
        return None
    try:
        data = json.loads(raw)
        x, y, w, h = int(data["x"]), int(data["y"]), int(data["w"]), int(data["h"])
        showing = bool(data.get("showing", True))
        title = str(data.get("title") or "")
    except (json.JSONDecodeError, KeyError, TypeError, ValueError):
        return None
    if is_launcher_title(title):
        return None
    if w < 64 or h < 64:
        return None
    return x, y, w, h, showing


def game_window_showing(game_key: str) -> bool:
    win = find_window(game_key)
    if win is None:
        return is_running(game_key)
    return bool(win[4])


def overview_active() -> bool:
    try:
        out = subprocess.run(
            [
                "gdbus",
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell",
                "--object-path",
                "/org/gnome/Shell",
                "--method",
                "org.freedesktop.DBus.Properties.Get",
                "org.gnome.Shell",
                "OverviewActive",
            ],
            capture_output=True,
            text=True,
            timeout=1,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return "true" in (out.stdout or "").lower()


def _even(n: int) -> int:
    return max(2, n - (n % 2))
