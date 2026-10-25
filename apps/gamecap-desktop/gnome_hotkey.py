"""Install GNOME custom shortcuts so hotkeys work in-game."""

from __future__ import annotations

import ast
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "org.gnome.settings-daemon.plugins.media-keys"
LIST_KEY = "custom-keybindings"

ACTIONS = (
    ("record", "gamecap", "GameCap grabar/parar", "toggle"),
    ("pause", "gamecap-pause", "GameCap pausar", "pause"),
    ("screenshot", "gamecap-shot", "GameCap captura", "screenshot"),
    ("replay", "gamecap-replay", "GameCap replay", "replay"),
)


def _run(args: list[str]) -> str:
    out = subprocess.run(args, check=False, capture_output=True, text=True)
    return (out.stdout or "").strip()


def _item(slug: str) -> tuple[str, str]:
    path = f"/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/{slug}/"
    return path, f"{SCHEMA}.custom-keybinding:{path}"


def _set(item: str, key: str, value: str) -> None:
    subprocess.run(["gsettings", "set", item, key, value], check=False)


def _paths() -> list[str]:
    raw = _run(["gsettings", "get", SCHEMA, LIST_KEY])
    try:
        return list(ast.literal_eval(raw)) if raw else []
    except (SyntaxError, ValueError):
        return []


def _ensure_path(path: str) -> None:
    paths = _paths()
    if path in paths:
        return
    paths.append(path)
    subprocess.run(["gsettings", "set", SCHEMA, LIST_KEY, str(paths)], check=False)


def install(binding: str) -> bool:
    return install_named("record", binding)


def _gtk_accel(name: str) -> str:
    raw = (name or "").strip()
    if not raw:
        return ""
    # Normalize keypad / function keys for GNOME custom-keybindings.
    if raw.lower().startswith("kp_"):
        return "KP_" + raw.split("_", 1)[1].upper()
    if raw[:1] in "Ff" and raw[1:].isdigit():
        return raw.upper()
    if raw.startswith("<") and "KP_" in raw.upper():
        # Keep modifiers; normalize KP token.
        return raw.replace("kp_", "KP_").replace("Kp_", "KP_")
    return raw


def install_named(action: str, binding: str) -> bool:
    meta = next((row for row in ACTIONS if row[0] == action), None)
    if meta is None:
        return False
    _, slug, name, arg = meta
    path, item = _item(slug)
    _ensure_path(path)
    _set(item, "name", name)
    _set(item, "command", f"{ROOT / 'ejecutar.sh'} {arg}")
    _set(item, "binding", _gtk_accel(binding) if binding else "")
    return True


def install_all(cfg: dict) -> None:
    install_named("record", str(cfg.get("hotkey_binding") or "F9"))
    install_named("pause", str(cfg.get("hotkey_pause_binding") or "F8"))
    install_named("screenshot", str(cfg.get("hotkey_screenshot_binding") or ""))
    install_named("replay", str(cfg.get("hotkey_replay_binding") or ""))


def current_binding() -> str:
    _, item = _item("gamecap")
    return _run(["gsettings", "get", item, "binding"]).strip("'")
