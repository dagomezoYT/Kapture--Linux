#!/usr/bin/env python3
"""Print JSON {x,y,w,h,showing,title} for a game window. Isolated so AT-SPI faults stay here."""

from __future__ import annotations

import json
import sys

HINTS = {
    "minecraft": (
        "dawn",
        "minecraft",
        "lwjgl",
        "glfw",
        "feather",
        "lunar",
        "prism",
        "polymc",
        "multimc",
        "badlion",
        "laby",
        "fabric",
        "forge",
        "javaw",
    ),
}
SKIP = {
    "gamecap",
    "kapture",
    "nautilus",
    "software",
    "cursor",
    "steam",
    "steamwebhelper",
    "discord",
    "firefox",
    "chrome",
    "chromium",
    "easyeffects",
    "easy effects",
}

# Launcher UIs are not the game. Minecraft/SDL on Wayland often has no AT-SPI
# window, so matching "Prism Launcher" would crop the framebuffer to that UI.
SKIP_TITLES = (
    "prism launcher",
    "prismlauncher",
    "polymc",
    "multimc",
    "minecraft launcher",
    "atlauncher",
    "gdlauncher",
    "sklauncher",
    "tlauncher",
    "kapture",
    "gamecap",
)


def _needles_from_argv(key: str, extra: list[str]) -> tuple[str, ...]:
    words: list[str] = []
    if key in HINTS:
        words.extend(HINTS[key])
    words.append(key)
    words.append(key.replace("-", " "))
    words.append(key.replace("steam-", ""))
    for part in extra:
        text = (part or "").strip().lower()
        if text:
            words.append(text)
    for token in key.split("-"):
        if len(token) >= 3 and token not in {"steam", "app"}:
            words.append(token)
    # Unique, longer first.
    seen: set[str] = set()
    out: list[str] = []
    for word in sorted(set(words), key=lambda w: (-len(w), w)):
        if word in seen or len(word) < 2:
            continue
        seen.add(word)
        out.append(word)
    return tuple(out[:20])


def main() -> int:
    key = (sys.argv[1] if len(sys.argv) > 1 else "minecraft").lower()
    needles = _needles_from_argv(key, sys.argv[2:])
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    desk = Atspi.get_desktop(0)
    best = None
    best_area = 0
    for i in range(desk.get_child_count()):
        app = desk.get_child_at_index(i)
        if app is None:
            continue
        app_name = (app.get_name() or "").lower()
        for j in range(app.get_child_count()):
            node = app.get_child_at_index(j)
            if node is None:
                continue
            title = (node.get_name() or "").lower()
            blob = f"{app_name} {title}"
            if not any(n in blob for n in needles):
                continue
            if any(s in title for s in SKIP) or any(s == app_name for s in SKIP):
                continue
            if any(s in title or s in app_name for s in SKIP_TITLES):
                continue
            comp = node.get_component()
            if comp is None:
                continue
            ext = comp.get_extents(Atspi.CoordType.SCREEN)
            area = int(ext.width) * int(ext.height)
            if area < 200 * 150 or area <= best_area:
                continue
            showing = True
            try:
                states = node.get_state_set()
                showing = states.contains(Atspi.StateType.SHOWING) or states.contains(
                    Atspi.StateType.VISIBLE
                )
            except Exception:
                showing = True
            best = {
                "x": int(ext.x),
                "y": int(ext.y),
                "w": int(ext.width),
                "h": int(ext.height),
                "showing": bool(showing),
                "title": node.get_name() or "",
            }
            best_area = area
    if best is None:
        print("{}")
        return 1
    print(json.dumps(best))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
