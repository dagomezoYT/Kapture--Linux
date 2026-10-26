#!/usr/bin/env python3
"""GameCap desktop — layout tipo NexRec: preview, biblioteca y panel de grabación."""

from __future__ import annotations

import json
import subprocess
import sys
import threading
from datetime import date, datetime
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Gdk", "4.0")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk, Pango

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))

import audio as audio_mod
import encode_profile as enc
import games
import gpu as gpu_mod
import gnome_hotkey
import i18n
import icons
import library
import session
import settings as cfg
from controller import RecordingController, RecordingState
from screenshot import take_screenshot

PICKER = ROOT / "scripts" / "pick-region.py"
ACCENT = "#e10618"
MUTED = "#9aa3b2"

CSS = b"""
window.gamecap { background: #0b0e14; color: #ececec; }
.topbar { background: #0b0e14; padding: 10px 16px; }
.win-btns { padding-left: 8px; }
.win-btn {
  min-width: 34px;
  min-height: 28px;
  padding: 4px 8px;
  border-radius: 8px;
  background: transparent;
  color: #c5ccd6;
}
.win-btn:hover { background: #1b212c; color: #fff; }
.win-btn.close:hover { background: #e10618; color: #fff; }
.brand { font-weight: 800; font-size: 20px; letter-spacing: 0.6px; }
.brand-sub { color: #7b8494; font-size: 9px; letter-spacing: 1.4px; }
.brand-accent { color: #e10618; }
.nav-btn {
  background: transparent;
  color: #e8eaed;
  border-radius: 18px;
  padding: 6px 12px;
  min-height: 34px;
  font-weight: 600;
}
.nav-btn:checked { color: #fff; border-bottom: 2px solid #e10618; background: transparent; border-radius: 0; }
.kicker { color: #8b93a0; font-size: 11px; font-weight: 700; padding: 4px 8px; }
.quick-btn {
  background: transparent;
  color: #c5ccd6;
  border-radius: 10px;
  padding: 8px 10px;
  min-height: 38px;
}
.transport {
  padding: 4px 8px 8px 8px;
}
.vu {
  min-width: 48px;
  min-height: 8px;
  margin-top: 6px;
  margin-bottom: 6px;
}
.vu trough {
  min-height: 8px;
  border-radius: 4px;
  background: #1a1f2a;
  border: 1px solid #2a3344;
}
.vu block.filled {
  border-radius: 3px;
  background: #3dd68c;
}
.vu-label {
  color: #8b95a8;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.04em;
}
.rec {
  min-width: 56px;
  min-height: 56px;
  border-radius: 28px;
  border: 3px solid #e10618;
  background: #12161e;
  color: #e10618;
  font-weight: 800;
  font-size: 12px;
}
.rec.recording {
  background: #e10618;
  color: #fff;
  border-radius: 10px;
}
.link-btn {
  background: transparent;
  color: #8b93a0;
  padding: 0;
}
.back-btn {
  background: transparent;
  color: #c5ccd6;
  padding: 4px 0;
  min-height: 28px;
}
.back-btn:hover { color: #fff; }
.sidebar {
  background: #0b0e14;
  padding: 8px 10px;
  min-width: 0;
}
.side-btn {
  background: transparent;
  color: #c5ccd6;
  border-radius: 10px;
  padding: 8px 10px;
  min-height: 40px;
}
.side-btn:checked { background: #e10618; color: #fff; }
.preview-frame {
  background: #10141c;
  border-radius: 12px;
  min-height: 140px;
}
.capture-stage {
  background: #0d1118;
  border-radius: 10px;
  padding: 24px;
}
.capture-title { font-size: 18px; font-weight: 700; color: #f2f4f8; }
.capture-sub { color: #d7dce4; font-size: 14px; }
.library {
  background: #0b0e14;
  padding: 8px 0 0 0;
  min-height: 0;
}
.library-empty {
  color: #8b93a0;
  font-size: 13px;
  padding: 36px 12px;
}
.thumb-card {
  background: #171c26;
  border-radius: 8px;
  border: 1px solid #2a3140;
}
.rec-pill { color: #fff; font-weight: 800; font-size: 13px; }
.rec-dot { color: #e10618; font-size: 14px; }
.clock { color: #fff; font-weight: 700; font-size: 13px; }
.metrics { color: #d7dce4; font-size: 12px; }
.tab {
  background: transparent;
  color: #8b93a0;
  border-radius: 0;
  border-bottom: 2px solid transparent;
  padding: 6px 10px;
}
.tab:checked { color: #fff; border-bottom: 2px solid #e10618; }
.side {
  background: #12161e;
  border-radius: 14px;
  padding: 14px 12px 10px 12px;
  min-width: 400px;
}
.section-title { font-size: 14px; font-weight: 700; color: #f2f4f8; }
.field-label { color: #b4bcc8; font-size: 12px; min-width: 0; }
.hint { color: #8b93a0; font-size: 12px; }
.mode-btn {
  min-width: 0;
  min-height: 40px;
  border-radius: 10px;
  background: #1b212c;
  color: #d0d5de;
  padding: 6px 8px;
}
.mode-btn:checked { background: #e10618; color: #fff; }
.mode-lbl { font-size: 11px; }
.statusline {
  background: #0b0e14;
  color: #8b93a0;
  padding: 6px 14px;
  font-size: 12px;
}
.keycap {
  background: #11141a;
  color: #fff;
  padding: 1px 7px;
  border-radius: 3px;
  font-weight: 700;
  font-family: monospace;
}
.folder-box {
  background: #171c26;
  border: 1px solid #2a3140;
  border-radius: 8px;
  padding: 4px 8px;
}
.folder-entry {
  background: transparent;
  color: #e8eaed;
  border: none;
  min-height: 28px;
  box-shadow: none;
}
.thumb-name { font-size: 11px; color: #d0d3d8; padding: 4px 6px 2px 6px; }
.thumb-meta { font-size: 10px; color: #8b93a0; padding: 0 6px 8px 6px; }
.dur-badge {
  background: rgba(17, 20, 26, 0.86);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
  padding: 2px 6px;
  border-radius: 4px;
}
.preview-hint { color: #8b93a0; font-size: 11px; }
.expand-btn {
  min-width: 34px;
  min-height: 28px;
  padding: 4px 8px;
  border-radius: 8px;
  background: rgba(17, 20, 26, 0.82);
  color: #c5ccd6;
}
.expand-btn:hover { background: #1b212c; color: #fff; }
.menu-item {
  background: transparent;
  color: #ececec;
  border-radius: 6px;
  padding: 8px 12px;
}
.menu-danger { color: #e10618; }
flowboxchild { padding: 2px; border-radius: 10px; border: 2px solid transparent; }
flowboxchild:selected { border-color: #e10618; }
.badge {
  background: #11141a;
  color: #fff;
  font-size: 9px;
  font-weight: 800;
  padding: 1px 5px;
  border-radius: 2px;
}
.stat-muted { color: #8b93a0; font-size: 12px; }
.toolbar-btn {
  min-width: 36px;
  min-height: 32px;
  background: #1b212c;
  color: #e8eaed;
  border-radius: 8px;
}
.gear {
  min-width: 28px;
  min-height: 28px;
  background: #1b212c;
  border-radius: 8px;
}
dropdown {
  min-width: 0;
}
dropdown > button {
  background: #171c26;
  color: #ececec;
  min-height: 32px;
  min-width: 0;
  border-radius: 8px;
  border: 1px solid #2a3140;
}
entry {
  background: #171c26;
  color: #ececec;
  border: 1px solid #2a3140;
  border-radius: 8px;
  min-height: 32px;
}
switch:checked { background: #e10618; }
.live-on { color: #e10618; font-weight: 800; }
.ultra-box {
  border: 1px solid #3a2a22;
  border-radius: 10px;
  padding: 8px 10px;
  background: #141820;
}
"""

NAV = (
    ("grabar", "live", "nav.record"),
    ("audio", "audio", "nav.audio"),
    ("library", "folder", "nav.library"),
    ("hotkeys", "keyboard", "nav.hotkeys"),
    ("perf", "timer", "nav.perf"),
    ("settings", "settings", "nav.settings"),
)
SIDE = (
    ("game", "gamepad", "side.game"),
    ("monitor", "monitor", "side.screen"),
    ("area", "area", "side.window"),
    ("device", "device", "side.device"),
)
AUDIO_MIX = (
    (True, True, "audio.mix.both"),
    (True, False, "audio.mix.game"),
    (False, True, "audio.mix.mic"),
    (False, False, "audio.mix.none"),
)
AUDIO_TRACKS = (
    ("both", "audio.tracks.both"),
    ("mix", "audio.tracks.mix"),
    ("separate", "audio.tracks.separate"),
)
_FPS_I18N = {60: "fps.60", 120: "fps.120", 180: "fps.180", 240: "fps.240", 0: "fps.auto"}
_SCALE_I18N = {"0x0": "scale.native", "1920x1080": "scale.1080", "1280x720": "scale.720"}
_CODEC_I18N = {key: f"codec.{key}" for key, _label in enc.CODECS}
_CONTAINER_I18N = {key: f"container.{key}" for key, _label in enc.CONTAINERS}


def output_dir() -> Path:
    path = Path(cfg.load().get("output_dir") or (Path.home() / "Videos" / "GameCap"))
    path.mkdir(parents=True, exist_ok=True)
    return path


def fmt_time(seconds: float) -> str:
    total = max(int(seconds), 0)
    hh, rem = divmod(total, 3600)
    mm, ss = divmod(rem, 60)
    if hh:
        return f"{hh}:{mm:02d}:{ss:02d}"
    return f"{mm:02d}:{ss:02d}"


def fmt_size(path: Path) -> str:
    try:
        n = path.stat().st_size
    except OSError:
        return "—"
    if n >= 1_073_741_824:
        return f"{n / 1_073_741_824:.1f} GB"
    return f"{n / 1_048_576:.0f} MB"


def fmt_when(path: Path) -> str:
    try:
        ts = datetime.fromtimestamp(path.stat().st_mtime)
    except OSError:
        return "—"
    today = date.today()
    if ts.date() == today:
        day = i18n.t("date.today")
    elif (today - ts.date()).days == 1:
        day = i18n.t("date.yesterday")
    else:
        day = ts.strftime("%d/%m")
    return f"{day}, {ts.strftime('%H:%M')}"


_DURATION_CACHE: dict[str, tuple[float, str]] = {}


def probe_duration(path: Path) -> str:
    try:
        mtime = path.stat().st_mtime
    except OSError:
        return "—"
    key = str(path)
    hit = _DURATION_CACHE.get(key)
    if hit is not None and hit[0] == mtime:
        return hit[1]
    try:
        out = subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)],
            capture_output=True,
            text=True,
            timeout=5,
        )
        text = fmt_time(float((out.stdout or "0").strip() or 0))
    except (OSError, subprocess.TimeoutExpired, ValueError):
        text = "—"
    _DURATION_CACHE[key] = (mtime, text)
    if len(_DURATION_CACHE) > 256:
        # Drop arbitrary oldest-ish entries to keep the map bounded.
        for old in list(_DURATION_CACHE.keys())[:64]:
            _DURATION_CACHE.pop(old, None)
    return text


def gtk_accel(name: str) -> str:
    raw = (name or "").strip() or "F9"
    if raw.lower().startswith("kp_"):
        return "KP_" + raw.split("_", 1)[1].upper()
    if raw[:1] in "Ff" and raw[1:].isdigit():
        return raw.upper()
    if len(raw) == 1:
        return raw.lower()
    return raw


def _dropdown_factory(width_chars: int) -> Gtk.SignalListItemFactory:
    """Keep long device names from becoming the column's minimum width."""
    factory = Gtk.SignalListItemFactory()

    def setup(_factory: Gtk.SignalListItemFactory, item: Gtk.ListItem) -> None:
        label = Gtk.Label(xalign=0)
        label.set_ellipsize(Pango.EllipsizeMode.END)
        label.set_width_chars(width_chars)
        label.set_hexpand(True)
        item.set_child(label)

    def bind(_factory: Gtk.SignalListItemFactory, item: Gtk.ListItem) -> None:
        label = item.get_child()
        obj = item.get_item()
        if isinstance(label, Gtk.Label) and isinstance(obj, Gtk.StringObject):
            label.set_label(obj.get_string())

    factory.connect("setup", setup)
    factory.connect("bind", bind)
    return factory


def _choice_labels(rows: tuple, keys: dict) -> list[str]:
    return [i18n.t(keys.get(key, str(key))) for key, _label in rows]


def _quality_labels(rows: tuple) -> list[str]:
    labels = []
    for key, raw in rows:
        token = f"quality.{key}_cpu" if "CPU" in str(raw) else f"quality.{key}"
        labels.append(i18n.t(token))
    return labels


def _record_gpu_name() -> str:
    try:
        chosen = gpu_mod.choose_record_gpu(gpu_mod.discover())
    except OSError:
        return "GPU"
    return chosen.name if chosen is not None else "GPU"


def combo(values: list[str], selected: int) -> Gtk.DropDown:
    widget = Gtk.DropDown(model=Gtk.StringList.new(values or ["—"]))
    widget.set_factory(_dropdown_factory(14))
    widget.set_list_factory(_dropdown_factory(22))
    widget.set_selected(max(0, min(selected, max(len(values) - 1, 0))))
    widget.set_hexpand(True)
    return widget


class GameCapWindow(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application) -> None:
        super().__init__(application=app, title="Kapture")
        self.add_css_class("gamecap")
        self._form_rows: list[Gtk.Box] = []
        self._layout_narrow = False
        self._layout_short = False
        self._fit_default_size()
        self.set_decorated(False)
        self.set_icon_name("gamecap")
        self.controller = RecordingController()
        self.was_recording = False
        self.current: Path | None = None
        self._save_lock = False
        self.grabbing: str | None = None
        self._icon_btns: list[tuple[Gtk.ToggleButton, Gtk.Image, str, int]] = []
        self.game_apps: list[games.GameApp] = []
        self.lib_filter = "videos"
        self._stat_tick = 0
        self._player_win: Gtk.Window | None = None
        self._player_video: Gtk.Video | None = None
        self._player_mpv: subprocess.Popen | None = None
        self._thumb_busy = False
        data = cfg.load()
        i18n.set_language(data.get("language") or "es")
        self._i18n: list[tuple] = []
        self._ultra_lock = False

        provider = Gtk.CssProvider()
        provider.load_from_data(CSS)
        display = Gdk.Display.get_default()
        if display is not None:
            Gtk.StyleContext.add_provider_for_display(
                display, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )

        self._save_lock = True
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        root.append(self._topbar())
        self.page_stack = Gtk.Stack()
        self.page_stack.set_vexpand(True)
        self.page_stack.add_named(self._page_grabar(data), "grabar")
        self.page_stack.add_named(self._page_library(), "library")
        root.append(self.page_stack)
        status = Gtk.Box(spacing=12)
        status.add_css_class("statusline")
        self.status = Gtk.Label(label=i18n.t("status.ready"), xalign=0)
        self.status.set_hexpand(True)
        self.status.set_ellipsize(Pango.EllipsizeMode.END)
        self.sys_lbl = Gtk.Label(label="CPU —  |  GPU —  |  RAM —  |  Disco —", xalign=1)
        self.sys_lbl.add_css_class("stat-muted")
        self.sys_lbl.set_ellipsize(Pango.EllipsizeMode.END)
        status.append(self.status)
        status.append(self.sys_lbl)
        root.append(status)
        self.set_content(root)
        key = Gtk.EventControllerKey()
        key.connect("key-pressed", self._on_key)
        self.add_controller(key)
        self.connect("close-request", self._on_close_request)
        self.connect("notify::maximized", self._on_maximized_changed)
        self._apply_initial_mode(data)
        self._save_lock = False
        self._recolor_icons()
        self.reload_library()
        gnome_hotkey.install_all(data)
        self._refresh_hotkey_chip()
        self._refresh_rec_key()
        self._refresh_shot_key()
        self._refresh_replay_key()
        self._refresh_pause_key()
        GLib.timeout_add(500, self.poll)

    def _topbar(self) -> Gtk.Widget:
        bar = Gtk.Box(spacing=18)
        bar.add_css_class("topbar")
        brand_path = Path(__file__).resolve().parent / "branding" / "window-logo-ui.png"
        if brand_path.is_file():
            brand = Gtk.Picture.new_for_filename(str(brand_path))
            brand.set_content_fit(Gtk.ContentFit.CONTAIN)
            brand.set_can_shrink(True)
            brand.set_size_request(120, 36)
            brand.set_halign(Gtk.Align.START)
            brand.set_valign(Gtk.Align.CENTER)
            brand.set_tooltip_text("Kapture")
        else:
            brand = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            row = Gtk.Box(spacing=0)
            a = Gtk.Label(label="KAP")
            a.add_css_class("brand")
            b = Gtk.Label(label="TURE")
            b.add_css_class("brand")
            b.add_css_class("brand-accent")
            row.append(a)
            row.append(b)
            sub = Gtk.Label(label="RECORD WHAT MATTERS", xalign=0)
            sub.add_css_class("brand-sub")
            brand.append(row)
            brand.append(sub)
        bar.append(brand)
        nav = Gtk.Box(spacing=2)
        nav.set_hexpand(False)
        nav.set_halign(Gtk.Align.START)
        self.nav_btns: dict[str, Gtk.ToggleButton] = {}
        first = None
        for key, icon, label_key in NAV:
            btn, _img = self._text_icon(icon, 16, "nav-btn", i18n.t(label_key), horizontal=True, ellipsize=False)
            self._bind_caption(btn, label_key)
            if first is None:
                first = btn
                btn.set_active(True)
            else:
                btn.set_group(first)
            btn.connect("toggled", self._on_nav, key)
            self.nav_btns[key] = btn
            nav.append(btn)
        bar.append(nav)
        spacer = Gtk.Box()
        spacer.set_hexpand(True)
        bar.append(spacer)
        bar.append(self._window_controls())
        handle = Gtk.WindowHandle()
        handle.set_child(bar)
        return handle

    def _window_controls(self) -> Gtk.Box:
        box = Gtk.Box(spacing=4)
        box.add_css_class("win-btns")
        box.set_valign(Gtk.Align.CENTER)
        min_btn = self._win_btn("minimize", i18n.t("win.min"), self._do_minimize)
        self._bind(min_btn, "win.min", "tooltip")
        self.max_btn = self._win_btn("maximize", i18n.t("win.max"), self._do_maximize)
        close_btn = self._win_btn("close", i18n.t("win.close"), self._do_close)
        self._bind(close_btn, "win.close", "tooltip")
        close_btn.add_css_class("close")
        box.append(min_btn)
        box.append(self.max_btn)
        box.append(close_btn)
        return box

    def _win_btn(self, icon: str, tip: str, callback) -> Gtk.Button:
        btn = Gtk.Button()
        btn.add_css_class("win-btn")
        btn.set_tooltip_text(tip)
        btn.set_focus_on_click(False)
        img = icons.image(icon, 14, MUTED)
        btn.set_child(img)
        btn._icon_name = icon  # type: ignore[attr-defined]
        btn._icon_img = img  # type: ignore[attr-defined]
        btn.connect("clicked", callback)
        return btn

    def _do_minimize(self, *_args) -> None:
        self.minimize()

    def _do_maximize(self, *_args) -> None:
        if self.is_maximized():
            self.unmaximize()
        else:
            self.maximize()

    def _do_close(self, *_args) -> None:
        self.close()

    def _on_close_request(self, *_args) -> bool:
        self._close_player()
        return False

    def _on_maximized_changed(self, *_args) -> None:
        if not hasattr(self, "max_btn"):
            return
        name = "restore" if self.is_maximized() else "maximize"
        tip = i18n.t("win.restore") if self.is_maximized() else i18n.t("win.max")
        img = getattr(self.max_btn, "_icon_img", None)
        if isinstance(img, Gtk.Image):
            icons.paint(img, name, 14, MUTED)
            self.max_btn._icon_name = name  # type: ignore[attr-defined]
        self.max_btn.set_tooltip_text(tip)

    def _fit_default_size(self) -> None:
        """One stable size. Do not grow with the screen or the form text."""
        width, height = 1280, 800
        display = Gdk.Display.get_default()
        if display is not None:
            monitor = display.get_monitors().get_item(0)
            if monitor is not None:
                geo = monitor.get_geometry()
                width = min(width, max(960, geo.width - 80))
                height = min(height, max(640, geo.height - 96))
        self.set_default_size(width, height)
        self.set_size_request(min(1120, width), min(700, height))

    def do_size_allocate(self, width: int, height: int, baseline: int) -> None:
        Adw.ApplicationWindow.do_size_allocate(self, width, height, baseline)
        if not hasattr(self, "grab_body"):
            return
        narrow = width < (1100 if self._layout_narrow else 980)
        short = height < (840 if self._layout_short else 760)
        if narrow == self._layout_narrow and short == self._layout_short:
            return
        self._layout_narrow = narrow
        self._layout_short = short
        self._sync_layout()

    def _sync_layout(self) -> None:
        narrow = self._layout_narrow
        short = self._layout_short
        self.grab_body.set_orientation(
            Gtk.Orientation.VERTICAL if narrow else Gtk.Orientation.HORIZONTAL
        )
        self.side_rail.set_orientation(
            Gtk.Orientation.HORIZONTAL if narrow else Gtk.Orientation.VERTICAL
        )
        self.side_rail.set_hexpand(narrow)
        self.side_kicker.set_visible(not narrow)
        self.side_spacer.set_visible(not narrow)
        self.quick_box.set_visible(True)
        self.quick_box.set_orientation(
            Gtk.Orientation.HORIZONTAL if narrow else Gtk.Orientation.VERTICAL
        )
        quick_title = self.quick_box.get_first_child()
        if isinstance(quick_title, Gtk.Label):
            quick_title.set_visible(not narrow)
        for btn in (*self.side_btns.values(), *self.quick_btns):
            caption = getattr(btn, "_caption", None)
            if isinstance(caption, Gtk.Label):
                caption.set_visible(not narrow)
            btn.set_hexpand(narrow)
        for btn in self.nav_btns.values():
            caption = getattr(btn, "_caption", None)
            if isinstance(caption, Gtk.Label):
                caption.set_visible(True)
        self.settings_panel.set_hexpand(False)
        self.sys_lbl.set_visible(not (narrow and short))
        if hasattr(self, "lib_flow"):
            self.lib_flow.set_max_children_per_line(2 if narrow else 5)
        for row in self._form_rows:
            row.set_orientation(Gtk.Orientation.VERTICAL if narrow else Gtk.Orientation.HORIZONTAL)
            label = row.get_first_child()
            if isinstance(label, Gtk.Label):
                label.set_size_request(-1 if narrow else 108, -1)

    def _page_grabar(self, data: dict) -> Gtk.Widget:
        body = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        body.set_margin_start(10)
        body.set_margin_end(10)
        body.set_margin_bottom(8)
        body.set_vexpand(True)
        self.grab_body = body
        self.side_rail = self._sidebar()
        self.settings_panel = self._right(data)
        center = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        center.set_hexpand(True)
        center.set_vexpand(True)
        center.set_size_request(480, -1)
        self.center_col = center
        self.preview = self._preview()
        self.transport = self._transport()
        center.append(self.preview)
        center.append(self.transport)
        center.append(self._library())
        body.append(self.side_rail)
        body.append(center)
        body.append(self.settings_panel)
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_vexpand(True)
        scroll.set_hexpand(True)
        scroll.set_propagate_natural_width(False)
        scroll.set_propagate_natural_height(False)
        scroll.set_child(body)
        return scroll

    def _placeholder(self, text: str) -> Gtk.Label:
        lab = Gtk.Label(label=text)
        lab.add_css_class("hint")
        lab.set_vexpand(True)
        return lab

    def _sidebar(self) -> Gtk.Box:
        rail = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        rail.add_css_class("sidebar")
        rail.set_size_request(168, -1)
        rail.set_hexpand(False)
        self.side_kicker = Gtk.Label(label=i18n.t("side.capture"), xalign=0)
        self._bind(self.side_kicker, "side.capture")
        self.side_kicker.add_css_class("kicker")
        rail.append(self.side_kicker)
        self.side_btns: dict[str, Gtk.ToggleButton] = {}
        self.mode_btns = self.side_btns
        first = None
        for key, icon, label_key in SIDE:
            btn, _img = self._text_icon(icon, 18, "side-btn", i18n.t(label_key), horizontal=True)
            self._bind_caption(btn, label_key)
            if first is None:
                first = btn
            else:
                btn.set_group(first)
            btn.connect("toggled", self._on_mode, key)
            self.side_btns[key] = btn
            rail.append(btn)
        self.side_spacer = Gtk.Box()
        self.side_spacer.set_vexpand(True)
        rail.append(self.side_spacer)
        self.quick_btns: list[Gtk.Button] = []
        self.quick_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        quick = Gtk.Label(label=i18n.t("quick.access"), xalign=0)
        quick.add_css_class("kicker")
        self._bind(quick, "quick.access")
        self.quick_box.append(quick)
        self.quick_box.append(self._quick("user", "quick.profile", lambda *_: self._show_page("settings")))
        self.quick_box.append(self._quick("keyboard", "quick.hotkeys", lambda *_: self._open_panel("hotkeys")))
        self.quick_box.append(self._quick("globe", "quick.language", self._language_pop))
        rail.append(self.quick_box)
        return rail

    def _quick(self, icon: str, key: str, callback) -> Gtk.Button:
        btn = Gtk.Button()
        btn.add_css_class("quick-btn")
        box = Gtk.Box(spacing=8)
        box.append(icons.image(icon, 16, MUTED))
        lab = Gtk.Label(label=i18n.t(key), xalign=0)
        lab.set_ellipsize(Pango.EllipsizeMode.END)
        box.append(lab)
        btn.set_child(box)
        btn._caption = lab  # type: ignore[attr-defined]
        self._bind(lab, key)
        btn.set_tooltip_text(i18n.t(key))
        self._bind(btn, key, "tooltip")
        btn.connect("clicked", callback)
        self.quick_btns.append(btn)
        return btn

    def _preview(self) -> Gtk.Overlay:
        """Capture target view only — never the library video player."""
        overlay = Gtk.Overlay()
        overlay.add_css_class("preview-frame")
        overlay.set_hexpand(True)
        overlay.set_vexpand(True)
        overlay.set_size_request(-1, 180)
        stage = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        stage.add_css_class("capture-stage")
        stage.set_halign(Gtk.Align.FILL)
        stage.set_valign(Gtk.Align.CENTER)
        stage.set_hexpand(True)
        stage.set_vexpand(True)
        self.capture_badge = Gtk.Label(label=i18n.t("preview.badge"), xalign=0)
        self.capture_badge.add_css_class("kicker")
        self._bind(self.capture_badge, "preview.badge")
        self.capture_title = Gtk.Label(label=i18n.t("preview.screen"), xalign=0)
        self.capture_title.add_css_class("capture-title")
        self.capture_title.set_wrap(True)
        self.capture_sub = Gtk.Label(
            label=i18n.t("preview.sub"),
            xalign=0,
        )
        self._bind(self.capture_sub, "preview.sub")
        self.capture_sub.add_css_class("capture-sub")
        self.capture_sub.set_wrap(True)
        self.capture_sub.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        self.capture_sub.set_max_width_chars(36)
        self.capture_sub.set_hexpand(True)
        stage.append(self.capture_badge)
        stage.append(self.capture_title)
        stage.append(self.capture_sub)
        overlay.set_child(stage)
        top = Gtk.Box()
        top.set_halign(Gtk.Align.FILL)
        top.set_valign(Gtk.Align.START)
        top.set_margin_top(12)
        top.set_margin_start(14)
        top.set_margin_end(14)
        rec = Gtk.Box(spacing=6)
        self.live = Gtk.Label(label="●")
        self.live.add_css_class("rec-dot")
        rec_lab = Gtk.Label(label="REC")
        rec_lab.add_css_class("rec-pill")
        self.clock = Gtk.Label(label="00:00:00")
        self.clock.add_css_class("clock")
        rec.append(self.live)
        rec.append(rec_lab)
        rec.append(self.clock)
        spacer = Gtk.Box()
        spacer.set_hexpand(True)
        self.metrics = Gtk.Label(label=session.monitor_idle_label())
        self.metrics.add_css_class("metrics")
        self.metrics.set_ellipsize(Pango.EllipsizeMode.END)
        top.append(rec)
        top.append(spacer)
        top.append(self.metrics)
        overlay.add_overlay(top)
        self.preview_hint = Gtk.Label(label=i18n.t("preview.hint"))
        self._bind(self.preview_hint, "preview.hint")
        self.preview_hint.add_css_class("preview-hint")
        self.preview_hint.set_halign(Gtk.Align.END)
        self.preview_hint.set_valign(Gtk.Align.END)
        self.preview_hint.set_margin_bottom(10)
        self.preview_hint.set_margin_end(12)
        overlay.add_overlay(self.preview_hint)
        self._player_win: Gtk.Window | None = None
        self._player_video: Gtk.Video | None = None
        self._player_mpv: subprocess.Popen | None = None
        self._thumb_busy = False
        return overlay

    def _close_player(self) -> None:
        """Release decoder/buffers before opening another clip or quitting."""
        if self._player_mpv is not None:
            try:
                self._player_mpv.terminate()
            except OSError:
                pass
            self._player_mpv = None
        if self._player_video is not None:
            try:
                stream = self._player_video.get_media_stream()
                if stream is not None:
                    stream.set_playing(False)
                    stream.seek(0)
            except Exception:
                pass
            try:
                self._player_video.set_file(None)
            except Exception:
                pass
            self._player_video = None
        if self._player_win is not None:
            try:
                self._player_win.destroy()
            except Exception:
                pass
            self._player_win = None

    def _open_player(self, path: Path, fullscreen: bool = False) -> None:
        """Play with mpv only — no placeholder GTK window (avoids the empty secondary screen)."""
        if not path.is_file():
            self.status.set_label("Ese archivo ya no está en disco.")
            return
        self._close_player()

        mpv_bin = Path("/usr/bin/mpv")
        if not mpv_bin.is_file():
            self.status.set_label("Instala mpv para reproducir (Gtk.Video se evita: gasta demasiada RAM).")
            return

        cmd = [
            "mpv",
            "--force-window=yes",
            "--keep-open=yes",
            "--hwdec=auto-safe",
            "--cache=no",
            "--demuxer-max-bytes=16MiB",
            "--demuxer-max-back-bytes=8MiB",
            "--vd-lavc-threads=2",
            "--osc=yes",
            "--title=" + path.name,
            str(path),
        ]
        if fullscreen:
            cmd.insert(1, "--fs")
        else:
            width = max(640, self.get_width() or 960)
            height = max(420, self.get_height() or 640)
            cmd.insert(1, f"--geometry={int(width * 0.8)}x{int(height * 0.8)}")
        try:
            self._player_mpv = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        except OSError:
            self.status.set_label("No se pudo abrir mpv.")
            return
        self.status.set_label(f"● Reproduciendo {path.name} (mpv)")

    def _refresh_capture_preview(self) -> None:
        mode = next((k for k, b in self.mode_btns.items() if b.get_active()), "monitor")
        labels = {
            "game": i18n.t("side.game"),
            "monitor": i18n.t("preview.full"),
            "area": i18n.t("side.window"),
            "device": i18n.t("preview.device"),
        }
        title = labels.get(mode, i18n.t("side.capture"))
        detail = i18n.t("preview.will")
        if mode == "game":
            game = self._selected_game() if hasattr(self, "game_drop") else None
            if game is not None:
                title = game.title
                detail = i18n.t("preview.game_only", title=game.title)
                detail += i18n.t("preview.game_seen") if game.running else i18n.t("preview.game_open")
            else:
                title = i18n.t("side.game")
                detail = i18n.t("preview.game_empty")
        elif mode == "monitor":
            mon = ""
            if getattr(self, "mon_ids", None):
                i = min(self.mon_drop.get_selected(), len(self.mon_ids) - 1)
                mon = self.mon_ids[i]
            title = i18n.t("preview.monitor_named", name=mon).strip() if mon else i18n.t("preview.full")
            detail = i18n.t("preview.monitor")
        elif mode == "area":
            title = i18n.t("side.window")
            detail = i18n.t("preview.area")
        elif mode == "device":
            title = i18n.t("side.device")
            detail = i18n.t("preview.device_detail")
        if hasattr(self, "capture_title"):
            self.capture_title.set_label(title)
            self.capture_sub.set_label(detail)

    def _transport(self) -> Gtk.Widget:
        bar = Adw.WrapBox()
        bar.add_css_class("transport")
        bar.set_halign(Gtk.Align.CENTER)
        bar.set_hexpand(True)
        bar.set_child_spacing(12)
        bar.set_line_spacing(8)
        bar.set_justify(Adw.JustifyMode.NONE)
        bar.set_wrap_policy(Adw.WrapPolicy.MINIMUM)
        self.mic_slider_box = Gtk.Box(spacing=8)
        self.mic_slider_box.set_valign(Gtk.Align.CENTER)
        self.mic_slider_box.append(icons.image("audio", 18, MUTED))
        self.mic_vol.set_size_request(96, -1)
        self.mic_vol.set_tooltip_text("Volumen del micrófono")
        self.mic_slider_box.append(self.mic_vol)
        vu_box = Gtk.Box(spacing=4)
        vu_box.set_valign(Gtk.Align.CENTER)
        vu_lab = Gtk.Label(label="VOZ")
        vu_lab.add_css_class("vu-label")
        vu_box.append(vu_lab)
        self.vu = Gtk.LevelBar()
        self.vu.set_mode(Gtk.LevelBarMode.CONTINUOUS)
        self.vu.set_min_value(0)
        self.vu.set_max_value(1)
        self.vu.set_value(0)
        self.vu.add_css_class("vu")
        self.vu.set_tooltip_text("Nivel de voz del micrófono (sube al hablar)")
        vu_box.append(self.vu)
        self.mic_slider_box.append(vu_box)
        self.rec_btn = Gtk.Button(label="REC")
        self.rec_btn.add_css_class("rec")
        self.rec_btn.set_tooltip_text("Grabar / parar")
        self.rec_btn.connect("clicked", lambda *_: self.toggle_record())
        shot = Gtk.Button()
        shot.add_css_class("toolbar-btn")
        shot.set_child(icons.image("camera", 18, MUTED))
        shot.set_tooltip_text("Captura de pantalla")
        shot.connect("clicked", self._do_screenshot)
        self.spk_box = Gtk.Box(spacing=8)
        self.spk_box.append(icons.image("audio", 18, MUTED))
        self.sys_vol.set_size_request(96, -1)
        self.spk_box.append(self.sys_vol)
        bar.append(self.mic_slider_box)
        bar.append(self.rec_btn)
        bar.append(shot)
        bar.append(self.spk_box)
        return bar

    def _library(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.add_css_class("library")
        box.set_vexpand(False)
        self.lib_box = box
        row = Gtk.Box(spacing=8)
        title = Gtk.Label(label=i18n.t("library.recent"), xalign=0)
        title.add_css_class("section-title")
        title.set_hexpand(True)
        self._bind(title, "library.recent")
        row.append(title)
        all_btn = Gtk.Button(label=i18n.t("library.all"))
        self._bind(all_btn, "library.all")
        all_btn.add_css_class("link-btn")
        all_btn.connect("clicked", lambda *_: self._show_page("library"))
        row.append(all_btn)
        box.append(row)
        self.lib_summary = Gtk.Label(label="", xalign=0)
        self.lib_summary.add_css_class("stat-muted")
        box.append(self.lib_summary)
        self.sort_drop = combo(
            [i18n.t("library.sort.recent"), i18n.t("library.sort.name"), i18n.t("library.sort.size")],
            0,
        )
        self.sort_drop.connect("notify::selected", lambda *_: self.reload_library())
        self.lib_stack = Gtk.Stack()
        self.lib_stack.set_vexpand(False)
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.NEVER)
        scroll.set_min_content_height(140)
        scroll.set_max_content_height(200)
        self.lib_scroll = scroll
        self.flow = Gtk.FlowBox()
        self.flow.set_valign(Gtk.Align.START)
        self.flow.set_halign(Gtk.Align.START)
        self.flow.set_hexpand(True)
        self.flow.set_max_children_per_line(12)
        self.flow.set_min_children_per_line(1)
        self.flow.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.flow.set_homogeneous(False)
        self.flow.set_row_spacing(8)
        self.flow.set_column_spacing(8)
        self.flow.connect("child-activated", self._on_card)
        scroll.set_child(self.flow)
        empty = Gtk.Label(label=i18n.t("library.empty"))
        self._bind(empty, "library.empty")
        empty.add_css_class("library-empty")
        empty.set_halign(Gtk.Align.CENTER)
        empty.set_valign(Gtk.Align.CENTER)
        self.lib_stack.add_named(scroll, "list")
        self.lib_stack.add_named(empty, "empty")
        self.lib_stack.set_visible_child_name("empty")
        box.append(self.lib_stack)
        return box

    def _page_library(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(18)
        box.set_margin_end(18)
        box.set_margin_top(8)
        head = Gtk.Box(spacing=8)
        head.append(self._back_row())
        title = Gtk.Label(label=i18n.t("library.title"), xalign=0)
        title.add_css_class("section-title")
        title.set_hexpand(True)
        self._bind(title, "library.title")
        head.append(title)
        self.tab_files = Gtk.ToggleButton(label=i18n.t("library.videos"))
        self.tab_prev = Gtk.ToggleButton(label=i18n.t("library.shots"))
        self._bind(self.tab_files, "library.videos")
        self._bind(self.tab_prev, "library.shots")
        self.tab_files.add_css_class("tab")
        self.tab_prev.add_css_class("tab")
        self.tab_prev.set_group(self.tab_files)
        self.tab_files.set_active(True)
        self.tab_files.connect("toggled", self._on_lib_kind)
        head.append(self.tab_files)
        head.append(self.tab_prev)
        box.append(head)
        hint = Gtk.Label(
            label=i18n.t("library.help"),
            xalign=0,
        )
        self._bind(hint, "library.help")
        hint.set_wrap(True)
        hint.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        hint.add_css_class("hint")
        box.append(hint)
        scroll = Gtk.ScrolledWindow()
        scroll.set_vexpand(True)
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.lib_flow = Gtk.FlowBox()
        self.lib_flow.set_valign(Gtk.Align.START)
        self.lib_flow.set_max_children_per_line(5)
        self.lib_flow.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.lib_flow.set_homogeneous(True)
        self.lib_flow.set_row_spacing(10)
        self.lib_flow.set_column_spacing(10)
        self.lib_flow.connect("child-activated", self._on_card)
        scroll.set_child(self.lib_flow)
        box.append(scroll)
        return box

    def _right(self, data: dict) -> Gtk.Box:
        side = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        side.add_css_class("side")
        side.set_vexpand(True)
        side.set_hexpand(False)
        side.set_size_request(400, -1)
        side.set_overflow(Gtk.Overflow.HIDDEN)
        title = Gtk.Label(label=i18n.t("record.title"), xalign=0)
        title.add_css_class("section-title")
        self._bind(title, "record.title")
        title.set_wrap(True)
        title.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        side.append(title)
        self.config_stack = Gtk.Stack()
        self.config_stack.set_vexpand(True)
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_vexpand(True)
        scroll.set_propagate_natural_width(False)
        scroll.set_propagate_natural_height(False)
        scroll.set_child(self.config_stack)
        clamp = Adw.Clamp()
        clamp.set_maximum_size(400)
        clamp.set_tightening_threshold(360)
        clamp.set_child(scroll)
        self._build_live(data)
        self.config_stack.add_named(self._build_audio_box(data), "audio")
        self.config_stack.add_named(self._build_camera_box(data), "camera")
        self.config_stack.add_named(self._build_overlay_box(data), "overlay")
        self.config_stack.add_named(self._build_hotkeys_box(data), "hotkeys")
        self.config_stack.add_named(self._build_timer_box(data), "perf")
        self.config_stack.add_named(self._build_settings_box(data), "settings")
        side.append(clamp)
        self.hotkey_chip = Gtk.Label()
        self.hotkey_chip.add_css_class("keycap")
        hint = Gtk.Box(spacing=6)
        hint.set_halign(Gtk.Align.CENTER)
        pre = Gtk.Label(label=i18n.t("record.shortcut"))
        self._bind(pre, "record.shortcut")
        pre.add_css_class("hint")
        hint.append(pre)
        hint.append(self.hotkey_chip)
        side.append(hint)
        return side

    def _text_icon(
        self,
        name: str,
        size: int,
        css: str,
        label: str,
        horizontal: bool = False,
        ellipsize: bool = True,
    ) -> tuple[Gtk.ToggleButton, Gtk.Image]:
        btn = Gtk.ToggleButton()
        btn.add_css_class(css)
        orient = Gtk.Orientation.HORIZONTAL if horizontal else Gtk.Orientation.VERTICAL
        box = Gtk.Box(orientation=orient, spacing=8)
        box.set_halign(Gtk.Align.START if horizontal else Gtk.Align.CENTER)
        img = icons.image(name, size, MUTED)
        box.append(img)
        lab = Gtk.Label(label=label)
        lab.set_wrap(False)
        if ellipsize:
            lab.set_ellipsize(Pango.EllipsizeMode.END)
            lab.set_max_width_chars(18)
        else:
            lab.set_ellipsize(Pango.EllipsizeMode.NONE)
        lab.set_xalign(0)
        box.append(lab)
        btn.set_child(box)
        btn._caption = lab  # type: ignore[attr-defined]
        btn.set_tooltip_text(label)
        self._icon_btns.append((btn, img, name, size))
        btn.connect("toggled", self._recolor_icons)
        return btn, img

    def _icon_button(self, name: str, size: int, css: str, chip: str = "") -> tuple[Gtk.ToggleButton, Gtk.Image]:
        btn = Gtk.ToggleButton()
        btn.add_css_class(css)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        box.set_halign(Gtk.Align.CENTER)
        img = icons.image(name, size, MUTED)
        box.append(img)
        if chip:
            lab = Gtk.Label(label=chip)
            lab.add_css_class("mode-lbl")
            box.append(lab)
        btn.set_child(box)
        self._icon_btns.append((btn, img, name, size))
        btn.connect("toggled", self._recolor_icons)
        return btn, img

    def _recolor_icons(self, *_args) -> None:
        for btn, img, name, size in self._icon_btns:
            color = "#ffffff" if btn.get_active() else MUTED
            icons.paint(img, name, size, color)

    def _hint(self, text: str) -> Gtk.Label:
        lab = Gtk.Label(label=text, xalign=0)
        lab.add_css_class("hint")
        lab.set_wrap(True)
        lab.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        lab.set_max_width_chars(36)
        lab.set_hexpand(True)
        return lab

    def _row(self, text: str, widget: Gtk.Widget, extra: Gtk.Widget | None = None) -> Gtk.Box:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lab = Gtk.Label(label=text, xalign=0)
        lab.add_css_class("field-label")
        lab.set_wrap(True)
        lab.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        lab.set_size_request(108, -1)
        row.append(lab)
        controls = Gtk.Box(spacing=8)
        controls.set_hexpand(True)
        widget.set_hexpand(True)
        controls.append(widget)
        if extra is not None:
            controls.append(extra)
        row.append(controls)
        self._form_rows.append(row)
        return row

    def _switch_row(self, text: str, widget: Gtk.Widget) -> Gtk.Box:
        row = Gtk.Box(spacing=8)
        lab = Gtk.Label(label=text, xalign=0)
        lab.add_css_class("field-label")
        lab.set_wrap(True)
        lab.set_hexpand(True)
        row.append(lab)
        row.append(widget)
        self._form_rows.append(row)
        return row

    def _heading(self, text: str, popover: Gtk.Popover | None = None) -> Gtk.Box:
        row = Gtk.Box(spacing=8)
        lab = Gtk.Label(label=text, xalign=0)
        lab.add_css_class("section-title")
        lab.set_hexpand(True)
        row.append(lab)
        if popover is not None:
            btn = Gtk.MenuButton()
            btn.add_css_class("gear")
            btn.set_child(icons.image("gear", 16, MUTED))
            btn.set_popover(popover)
            row.append(btn)
        return row

    def _popover(self, *widgets: Gtk.Widget) -> Gtk.Popover:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_margin_top(10)
        box.set_margin_bottom(10)
        box.set_margin_start(12)
        box.set_margin_end(12)
        for w in widgets:
            box.append(w)
        pop = Gtk.Popover()
        pop.set_child(box)
        return pop

    def _build_live(self, data: dict) -> None:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        current = data.get("capture_mode") or data.get("mode") or "monitor"
        if current in {"region", "window"}:
            current = "area"
        if current not in self.side_btns:
            current = "monitor"

        self.game_apps = games.list_games()
        self.game_drop = combo(games.labels(self.game_apps), self._game_index(data.get("game_key") or "minecraft"))
        self.game_drop.connect(
            "notify::selected",
            lambda *_: (self._persist(), self._refresh_capture_preview()),
        )
        refresh_games = Gtk.Button(label="↻")
        refresh_games.add_css_class("gear")
        refresh_games.connect("clicked", lambda *_: self._reload_games())
        self.game_row = self._labeled_row("record.game", self.game_drop, refresh_games)
        self.game_hint = self._hint(i18n.t("record.game_hint"))
        self._bind(self.game_hint, "record.game_hint")
        self.wait_game = Gtk.CheckButton(label=i18n.t("record.wait_game"))
        self._bind(self.wait_game, "record.wait_game")
        self.wait_game.set_active(bool(data.get("wait_for_game", False)))

        budget = enc.active_budget()
        self._budget = budget
        self._quality_rows = budget.qualities
        self.fmt = combo(_choice_labels(enc.CONTAINERS, _CONTAINER_I18N), enc.index_of(enc.CONTAINERS, data.get("container") or "mkv"))
        self.codec = combo(_choice_labels(enc.CODECS, _CODEC_I18N), enc.index_of(enc.CODECS, data.get("codec") or "av1"))
        self.quality = combo(
            _quality_labels(budget.qualities),
            enc.index_of(budget.qualities, enc.normalize_quality(data.get("quality"), budget)),
        )
        self.size = combo(
            _choice_labels(enc.SCALES, _SCALE_I18N),
            enc.index_of(enc.SCALES, str(data.get("scale") or "0x0"), {"native": "0x0", "original": "0x0"}),
        )
        fps_value = data.get("fps")
        self.fps = combo(_choice_labels(enc.FPS, _FPS_I18N), enc.index_of(enc.FPS, enc.STABLE_FPS_CAP if fps_value is None else int(fps_value)))
        mix_i = 0
        if data.get("audio_system", True) and data.get("audio_mic", False):
            mix_i = 0
        elif data.get("audio_system", True):
            mix_i = 1
        elif data.get("audio_mic", False):
            mix_i = 2
        else:
            mix_i = 3
        self.audio_mix = combo([i18n.t(row[2]) for row in AUDIO_MIX], mix_i)
        track_i = next(
            (i for i, (k, _) in enumerate(AUDIO_TRACKS) if k == data.get("audio_tracks")),
            0,
        )
        self.audio_tracks = combo([i18n.t(row[1]) for row in AUDIO_TRACKS], track_i)
        ins = audio_mod.inputs() or [("default_input", "Micrófono por defecto")]
        self.in_ids = [i for i, _ in ins]
        self.in_drop = combo([n for _, n in ins], self._index(self.in_ids, data.get("audio_mic_device")))
        self.hud = Gtk.Switch()
        self.hud.set_active(bool(data.get("hud", True)))
        self.show_timer = Gtk.Switch()
        self.show_timer.set_active(bool(data.get("hud", True)))
        self.cursor = Gtk.Switch()
        self.cursor.set_active(bool(data.get("cursor", True)))
        mons = audio_mod.list_monitors() or [("eDP-1", "eDP-1")]
        self.mon_ids = [i for i, _ in mons]
        self.mon_drop = combo([n for _, n in mons], self._index(self.mon_ids, data.get("monitor_name")))
        cams = audio_mod.list_v4l2() or [("/dev/video0", "/dev/video0")]
        self.cam_ids = [i for i, _ in cams]
        self.cam_drop = combo([n for _, n in cams], self._index(self.cam_ids, data.get("v4l2_device")))
        outs = audio_mod.outputs() or [("default_output", "Salida por defecto")]
        self.out_ids = [i for i, _ in outs]
        self.out_drop = combo([n for _, n in outs], self._index(self.out_ids, data.get("audio_system_device")))
        self.sys_vol = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 150, 1)
        self.sys_vol.set_value(int(data.get("audio_system_vol") or 100))
        self.sys_vol.set_draw_value(False)
        self.sys_vol.set_hexpand(True)
        self.mic_vol = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 150, 1)
        self.mic_vol.set_value(int(data.get("audio_mic_vol") or 100))
        self.mic_vol.set_draw_value(False)
        self.mic_vol.set_hexpand(True)
        self.mic_vol.connect("value-changed", self._on_mic_vol)
        self.sys_audio = Gtk.CheckButton()
        self.mic_audio = Gtk.CheckButton()
        self.sys_audio.set_active(bool(data.get("audio_system", True)))
        self.mic_audio.set_active(bool(data.get("audio_mic", False)))

        self.folder_entry = Gtk.Entry()
        self.folder_entry.set_text(str(output_dir()))
        self.folder_entry.add_css_class("folder-entry")
        self.folder_entry.set_hexpand(True)
        self.folder_entry.connect("activate", self._on_folder_typed)
        browse = Gtk.Button()
        browse.add_css_class("toolbar-btn")
        browse.set_child(icons.image("folder", 16, MUTED))
        browse.set_tooltip_text(i18n.t("record.folder_tip"))
        self._bind(browse, "record.folder_tip", "tooltip")
        browse.connect("clicked", self._choose_folder)
        folder_row = Gtk.Box(spacing=6)
        folder_row.add_css_class("folder-box")
        folder_row.append(self.folder_entry)
        folder_row.append(browse)

        for w in (self.fmt, self.codec, self.size, self.fps, self.mon_drop, self.cam_drop, self.out_drop, self.audio_mix, self.audio_tracks, self.in_drop):
            w.connect("notify::selected", lambda *_: self._persist())
        self.quality.connect("notify::selected", self._on_quality_selected)
        for w in (self.hud, self.show_timer, self.cursor):
            w.connect("notify::active", lambda *_: self._persist())
        self.wait_game.connect("toggled", lambda *_: self._persist())
        self.sys_vol.connect("value-changed", self._on_sys_vol)

        box.append(self._labeled_row("record.format", self.fmt))
        box.append(self._labeled_row("record.codec", self.codec))
        box.append(self._labeled_row("record.quality", self.quality))
        box.append(self._ultra_section(_record_gpu_name()))
        box.append(self._labeled_row("record.resolution", self.size))
        box.append(self._labeled_row("record.fps", self.fps))
        box.append(self._labeled_row("record.audio", self.audio_mix))
        self.stable_hint = self._hint(
            i18n.t("record.stable", fps=enc.STABLE_FPS_CAP, keyint=f"{enc.KEYINT_DEFAULT:.1f}")
        )
        self._bind(self.stable_hint, "record.stable", fps=enc.STABLE_FPS_CAP, keyint=f"{enc.KEYINT_DEFAULT:.1f}")
        box.append(self.stable_hint)
        self.gpu_hint = self._hint(gpu_mod.describe_current())
        box.append(self.gpu_hint)
        self.tracks_hint = self._hint(i18n.t("record.tracks"))
        self._bind(self.tracks_hint, "record.tracks")
        box.append(self.tracks_hint)
        advanced = self._heading(i18n.t("record.advanced"))
        self._bind(advanced.get_first_child(), "record.advanced")
        box.append(advanced)
        box.append(self.game_row)
        box.append(self.game_hint)
        box.append(self._labeled_row("record.mic", self.in_drop))
        box.append(self._labeled_switch("record.hud", self.hud))
        box.append(self._labeled_switch("record.cursor", self.cursor))
        folder_caption = Gtk.Label(label=i18n.t("record.folder"), xalign=0)
        self._bind(folder_caption, "record.folder")
        box.append(folder_caption)
        box.append(folder_row)
        self.config_stack.add_named(box, "record")
        self.game_row.set_visible(current == "game")
        self.game_hint.set_visible(current == "game")
        self._sync_ultra_from_quality()

    def _labeled_row(self, key: str, widget: Gtk.Widget, extra: Gtk.Widget | None = None) -> Gtk.Box:
        row = self._row(i18n.t(key), widget, extra)
        self._bind(row.get_first_child(), key)
        return row

    def _labeled_switch(self, key: str, widget: Gtk.Widget) -> Gtk.Box:
        row = self._switch_row(i18n.t(key), widget)
        self._bind(row.get_first_child(), key)
        return row

    def _ultra_section(self, gpu_name: str) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        box.add_css_class("ultra-box")
        title = Gtk.Label(label=i18n.t("ultra.title"), xalign=0)
        title.add_css_class("section-title")
        body = self._hint(i18n.t("ultra.body", gpu=gpu_name))
        self.ultra_check = Gtk.CheckButton(label=i18n.t("ultra.use"))
        self.ultra_check.connect("toggled", self._on_ultra_toggled)
        box.append(title)
        box.append(body)
        box.append(self.ultra_check)
        box.set_visible(enc.allows_ultra(self._budget))
        self.ultra_box = box
        self._bind(title, "ultra.title")
        self._bind(body, "ultra.body", gpu=gpu_name)
        self._bind(self.ultra_check, "ultra.use")
        return box

    def _bind(self, widget, key: str, attr: str = "label", **fmt: object) -> None:
        self._i18n.append((widget, key, attr, dict(fmt)))

    def _bind_caption(self, btn: Gtk.Widget, key: str) -> None:
        caption = getattr(btn, "_caption", None)
        if caption is not None:
            self._bind(caption, key)
        self._bind(btn, key, "tooltip")

    def _write_i18n(self, widget, key: str, attr: str, fmt: dict) -> None:
        text = i18n.t(key, **fmt)
        if attr == "tooltip":
            widget.set_tooltip_text(text)
        else:
            widget.set_label(text)

    def _set_choice(self, widget: Gtk.DropDown, labels: list[str]) -> None:
        selected = widget.get_selected()
        widget.set_model(Gtk.StringList.new(labels or ["—"]))
        widget.set_selected(max(0, min(selected, max(len(labels) - 1, 0))))

    def _refill_choices(self) -> None:
        self._save_lock = True
        self._set_choice(self.fmt, _choice_labels(enc.CONTAINERS, _CONTAINER_I18N))
        self._set_choice(self.codec, _choice_labels(enc.CODECS, _CODEC_I18N))
        self._set_choice(self.quality, _quality_labels(self._quality_rows))
        self._set_choice(self.size, _choice_labels(enc.SCALES, _SCALE_I18N))
        self._set_choice(self.fps, _choice_labels(enc.FPS, _FPS_I18N))
        self._set_choice(self.audio_mix, [i18n.t(row[2]) for row in AUDIO_MIX])
        self._set_choice(self.audio_tracks, [i18n.t(row[1]) for row in AUDIO_TRACKS])
        self._set_choice(
            self.sort_drop,
            [i18n.t("library.sort.recent"), i18n.t("library.sort.name"), i18n.t("library.sort.size")],
        )
        self._save_lock = False
        self._sync_ultra_from_quality()

    def _apply_language(self) -> None:
        for widget, key, attr, fmt in self._i18n:
            self._write_i18n(widget, key, attr, fmt)
        self._refill_choices()
        if hasattr(self, "gpu_hint"):
            self.gpu_hint.set_label(gpu_mod.describe_current())
        self._refresh_capture_preview()
        self._on_maximized_changed()
        self.status.set_label(self.controller.status_text())
        self.reload_library()

    def _on_quality_selected(self, *_args) -> None:
        self._sync_ultra_from_quality()
        self._persist()

    def _sync_ultra_from_quality(self) -> None:
        if not hasattr(self, "ultra_check") or self._ultra_lock:
            return
        key = enc.value_at(self._quality_rows, self.quality.get_selected())
        self._ultra_lock = True
        self.ultra_check.set_active(key == "ultra")
        self._ultra_lock = False

    def _on_ultra_toggled(self, btn: Gtk.CheckButton) -> None:
        if self._ultra_lock or not self.ultra_box.get_visible():
            return
        target = "ultra" if btn.get_active() else "high"
        index = enc.index_of(self._quality_rows, target)
        if self.quality.get_selected() != index:
            self.quality.set_selected(index)

    def _back_row(self, label: str = "← Volver a Grabar") -> Gtk.Button:
        btn = Gtk.Button(label=label)
        btn.add_css_class("back-btn")
        btn.set_halign(Gtk.Align.START)
        btn.set_has_frame(False)
        btn.connect("clicked", lambda *_: self._show_page("grabar"))
        return btn

    def _build_audio_box(self, data: dict) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.denoise_ambient = Gtk.CheckButton(label=i18n.t("audio.ambient"))
        self.denoise_static = Gtk.CheckButton(label=i18n.t("audio.static"))
        self.denoise_dirty = Gtk.CheckButton(label=i18n.t("audio.dirty"))
        self.denoise_system = Gtk.CheckButton(label=i18n.t("audio.game_raw"))
        self.denoise_system.set_sensitive(False)
        self.denoise_system.set_active(False)
        self.denoise_mic = Gtk.CheckButton(label=i18n.t("audio.mic"))
        self._bind(self.denoise_ambient, "audio.ambient")
        self._bind(self.denoise_static, "audio.static")
        self._bind(self.denoise_dirty, "audio.dirty")
        self._bind(self.denoise_system, "audio.game_raw")
        self._bind(self.denoise_mic, "audio.mic")
        self.denoise_ambient.set_active(bool(data.get("audio_denoise_ambient", True)))
        self.denoise_static.set_active(bool(data.get("audio_denoise_static", True)))
        self.denoise_dirty.set_active(bool(data.get("audio_denoise_dirty", True)))
        self.denoise_mic.set_active(bool(data.get("audio_denoise_mic", True)))
        self.denoise_str = combo(
            ["Suave", "Media", "Agresiva"],
            {"suave": 0, "media": 1, "agresiva": 2}.get(data.get("audio_denoise_strength"), 1),
        )
        self.abitrate = combo(
            enc.labels(enc.AUDIO_BITRATES),
            enc.index_of(enc.AUDIO_BITRATES, enc.normalize_audio_bitrate(data.get("audio_bitrate"))),
        )
        for w in (self.denoise_ambient, self.denoise_static, self.denoise_dirty, self.denoise_system, self.denoise_mic):
            w.connect("toggled", lambda *_: self._persist())
        self.denoise_str.connect("notify::selected", lambda *_: self._persist())
        self.abitrate.connect("notify::selected", lambda *_: self._persist())
        self.save_audio_folder = Gtk.Switch()
        self.save_audio_folder.set_active(bool(data.get("save_audio_folder", True)))
        self.save_audio_folder.connect("notify::active", lambda *_: self._persist())
        hint = Gtk.Label(
            label=i18n.t("audio.note"),
            xalign=0,
            wrap=True,
        )
        self._bind(hint, "audio.note")
        hint.add_css_class("hint")
        box.append(self._back_row())
        audio_head = self._heading(i18n.t("nav.audio"))
        self._bind(audio_head.get_first_child(), "nav.audio")
        box.append(audio_head)
        box.append(hint)
        box.append(self._labeled_switch("audio.save_folder", self.save_audio_folder))
        box.append(self._labeled_row("audio.tracks_row", self.audio_tracks))
        box.append(self._labeled_row("audio.output", self.out_drop))
        box.append(self._labeled_row("record.quality", self.abitrate))
        box.append(self.denoise_ambient)
        box.append(self.denoise_static)
        box.append(self.denoise_dirty)
        box.append(self.denoise_system)
        box.append(self.denoise_mic)
        box.append(self._labeled_row("audio.strength", self.denoise_str))
        return box

    def _build_camera_box(self, data: dict) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.shot_fmt = combo(["PNG", "JPG"], 0 if data.get("screenshot_format") != "jpg" else 1)
        self.shot_fmt.connect("notify::selected", lambda *_: self._persist())
        shot = Gtk.Button(label="Capturar ahora")
        shot.connect("clicked", self._do_screenshot)
        self.shot_key = Gtk.Label(xalign=0)
        grab = Gtk.Button(label="Asignar tecla de captura")
        grab.connect("clicked", lambda *_: self._grab("screenshot"))
        box.append(self._back_row())
        box.append(self._heading("Capturas"))
        box.append(self._row("Formato", self.shot_fmt))
        box.append(self._row("Cámara / HDMI", self.cam_drop))
        box.append(self.shot_key)
        box.append(grab)
        box.append(shot)
        return box

    def _build_timer_box(self, data: dict) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.replay = combo(
            ["Desactivado", "15 s", "30 s", "60 s", "120 s"],
            {0: 0, 15: 1, 30: 2, 60: 3, 120: 4}.get(int(data.get("replay_secs") or 0), 0),
        )
        self.limit = combo(
            ["Ilimitado", "1 h", "2 h", "4 h"],
            {0: 0, 1: 1, 2: 2, 4: 3}.get(int(data.get("limit_hours") or 0), 0),
        )
        self.replay.connect("notify::selected", lambda *_: self._persist())
        self.limit.connect("notify::selected", lambda *_: self._persist())
        self.replay_key = Gtk.Label(xalign=0)
        grab = Gtk.Button(label="Asignar tecla de replay")
        grab.connect("clicked", lambda *_: self._grab("replay"))
        box.append(self._back_row())
        box.append(self._heading("Rendimiento y replay"))
        box.append(self._row("Buffer replay", self.replay))
        box.append(self._row("Tope de toma", self.limit))
        box.append(self.replay_key)
        box.append(grab)
        return box

    def _build_settings_box(self, data: dict) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.min_on_rec = Gtk.Switch()
        self.min_on_rec.set_active(bool(data.get("minimize_on_record", True)))
        self.min_on_rec.connect("notify::active", lambda *_: self._persist())
        folder = Gtk.Button(label="Abrir carpeta de vídeos")
        folder.connect("clicked", lambda *_: subprocess.Popen(["xdg-open", str(output_dir())], start_new_session=True))
        box.append(self._back_row())
        box.append(self._heading("Ajustes"))
        box.append(self._switch_row("Minimizar al grabar", self.min_on_rec))
        box.append(self._row("Pantalla activa", self.mon_drop))
        box.append(folder)
        box.append(self.wait_game)
        return box

    def _build_overlay_box(self, data: dict) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.append(self._back_row())
        box.append(self._heading("Superposición"))
        note = Gtk.Label(
            label="El HUD (tiempo, FPS, GB y voz) solo se ve en pantalla para guiarte. No se graba: la captura recorta esa franja superior.",
            xalign=0,
            wrap=True,
        )
        note.add_css_class("hint")
        box.append(note)
        return box

    def _build_hotkeys_box(self, data: dict) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.rec_key = Gtk.Label(xalign=0)
        self.pause_key_b = Gtk.Label(xalign=0)
        self.shot_key_b = Gtk.Label(xalign=0)
        self.replay_key_b = Gtk.Label(xalign=0)
        for lab in (self.rec_key, self.pause_key_b, self.shot_key_b, self.replay_key_b):
            lab.set_wrap(True)
            lab.set_use_markup(True)
        grab = Gtk.Button(label="Cambiar tecla")
        grab.connect("clicked", lambda *_: self._grab("record"))
        grab_pause = Gtk.Button(label="Cambiar tecla")
        grab_pause.connect("clicked", lambda *_: self._grab("pause"))
        grab_shot = Gtk.Button(label="Cambiar tecla")
        grab_shot.connect("clicked", lambda *_: self._grab("screenshot"))
        grab_rep = Gtk.Button(label="Cambiar tecla")
        grab_rep.connect("clicked", lambda *_: self._grab("replay"))
        hint = Gtk.Label(
            label="Atajos globales (funcionan aunque el juego esté en primer plano). Pulsa «Cambiar tecla» y luego la tecla deseada.",
            xalign=0,
            wrap=True,
        )
        hint.add_css_class("hint")
        box.append(self._back_row())
        box.append(self._heading("Atajos"))
        box.append(hint)
        box.append(self._hotkey_row("Grabar / Parar", self.rec_key, grab))
        box.append(self._hotkey_row("Pausar / Reanudar", self.pause_key_b, grab_pause))
        box.append(self._hotkey_row("Captura de pantalla", self.shot_key_b, grab_shot))
        box.append(self._hotkey_row("Guardar replay", self.replay_key_b, grab_rep))
        return box

    def _hotkey_row(self, title: str, key_label: Gtk.Label, btn: Gtk.Button) -> Gtk.Box:
        row = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        head = Gtk.Label(label=title, xalign=0)
        head.add_css_class("section-title")
        key_label.set_margin_start(2)
        btn.set_halign(Gtk.Align.START)
        row.append(head)
        row.append(key_label)
        row.append(btn)
        return row

    def _apply_initial_mode(self, data: dict) -> None:
        current = data.get("capture_mode") or "monitor"
        if current in {"region", "window"}:
            current = "area"
        if current in self.side_btns:
            self.side_btns[current].set_active(True)
        if current in self.mode_btns:
            self.mode_btns[current].set_active(True)
        self._refresh_capture_preview()

    def _game_index(self, key: object) -> int:
        wanted = str(key or "minecraft")
        for i, game in enumerate(self.game_apps):
            if game.key == wanted:
                return i
        return 0

    def _selected_game(self) -> games.GameApp | None:
        if not self.game_apps:
            return None
        idx = min(self.game_drop.get_selected(), len(self.game_apps) - 1)
        return self.game_apps[max(idx, 0)]

    def _selected_game_key(self) -> str:
        game = self._selected_game()
        return game.key if game else "minecraft"

    def _reload_games(self) -> None:
        selected = self._selected_game_key()
        self.game_apps = games.list_games()
        self.game_drop.set_model(Gtk.StringList.new(games.labels(self.game_apps) or ["Minecraft"]))
        self.game_drop.set_selected(self._game_index(selected))

    def _index(self, ids: list[str], value: object) -> int:
        try:
            return ids.index(str(value))
        except ValueError:
            return 0

    def _on_nav(self, btn: Gtk.ToggleButton, key: str) -> None:
        if not btn.get_active():
            return
        # Top nav is always the escape hatch (Nielsen: user control and freedom).
        if key == "library":
            self.page_stack.set_visible_child_name("library")
            return
        self.page_stack.set_visible_child_name("grabar")
        panel = {
            "grabar": "record",
            "audio": "audio",
            "hotkeys": "hotkeys",
            "perf": "perf",
            "settings": "settings",
        }.get(key, "record")
        self.config_stack.set_visible_child_name(panel)
        if key == "hotkeys":
            self._refresh_rec_key()
            self._refresh_shot_key()
            self._refresh_replay_key()
            self._refresh_pause_key()

    def _show_page(self, key: str) -> None:
        btn = self.nav_btns.get(key)
        if btn is not None:
            btn.set_active(True)
            return
        # Unknown key → safe return to recording.
        self.nav_btns["grabar"].set_active(True)

    def _open_panel(self, name: str) -> None:
        """Open a side panel without trapping the user (top nav stays usable)."""
        self.page_stack.set_visible_child_name("grabar")
        mapping = {"audio": "audio", "perf": "perf", "settings": "settings", "hotkeys": "hotkeys"}
        nav_key = mapping.get(name)
        if nav_key in self.nav_btns:
            self.nav_btns[nav_key].set_active(True)
            if nav_key == "hotkeys":
                self._refresh_rec_key()
                self._refresh_shot_key()
                self._refresh_replay_key()
                self._refresh_pause_key()
        else:
            self.nav_btns["grabar"].set_active(True)
            self.config_stack.set_visible_child_name(name)

    def _language_pop(self, btn: Gtk.Button) -> None:
        old = getattr(self, "_lang_pop", None)
        if old is not None:
            old.unparent()
        pop = Gtk.Popover()
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        box.set_margin_top(8)
        box.set_margin_bottom(8)
        box.set_margin_start(12)
        box.set_margin_end(12)
        group = None
        current = i18n.language()
        for code, name in i18n.LANGUAGES:
            row = Gtk.CheckButton(label=name)
            if group is None:
                group = row
            else:
                row.set_group(group)
            row.connect("toggled", self._on_language, code)
            if code == current:
                row.set_active(True)
            box.append(row)
        pop.set_child(box)
        pop.set_parent(btn)
        pop.popup()
        self._lang_pop = pop

    def _on_language(self, btn: Gtk.CheckButton, code: str) -> None:
        if not btn.get_active() or i18n.language() == code:
            return
        i18n.set_language(code)
        data = cfg.load()
        data["language"] = i18n.language()
        cfg.save(data)
        self._apply_language()

    def _on_lib_kind(self, *_args) -> None:
        self.lib_filter = "videos" if self.tab_files.get_active() else "shots"
        self.reload_library()

    def _on_side(self, btn: Gtk.ToggleButton, key: str) -> None:
        if not btn.get_active():
            return
        if key in self.mode_btns:
            self.config_stack.set_visible_child_name("record")
            if not self.mode_btns[key].get_active():
                self.mode_btns[key].set_active(True)
            return
        self.config_stack.set_visible_child_name(key)

    def _on_mode(self, btn: Gtk.ToggleButton, mode: str) -> None:
        if not btn.get_active():
            return
        if hasattr(self, "game_row"):
            self.game_row.set_visible(mode == "game")
            self.game_hint.set_visible(mode == "game")
        self.config_stack.set_visible_child_name("record")
        self._refresh_capture_preview()
        if self._save_lock:
            return
        if mode == "game":
            self._reload_games()
            self.wait_game.set_active(True)
            self._persist()
            self._refresh_capture_preview()
            key = self._selected_game_key()
            app = next((g for g in self.game_apps if g.key == key), None)
            if app and app.running:
                self.status.set_label(i18n.t("status.game_open", title=app.title))
            else:
                self.status.set_label(i18n.t("status.game_wait"))
            return
        if mode == "area":
            data = cfg.load()
            data["capture_mode"] = "area"
            data["mode"] = "region"
            cfg.save(data)
            self._pick_region()
            return
        self._persist()

    def _pick_region(self) -> None:
        if not PICKER.is_file():
            self.status.set_label("No está el selector de recuadro.")
            return
        try:
            out = subprocess.run(["python3", str(PICKER)], capture_output=True, text=True, check=False)
        except OSError as err:
            self.status.set_label(str(err))
            return
        spec = (out.stdout or "").strip()
        if out.returncode != 0 or not spec:
            return
        data = cfg.load()
        data["region"] = spec
        data["mode"] = "region"
        data["capture_mode"] = "area"
        cfg.save(data)
        self.status.set_label(f"Área {spec}")

    def _persist(self) -> None:
        if self._save_lock:
            return
        data = cfg.load()
        data["container"] = enc.value_at(enc.CONTAINERS, self.fmt.get_selected())
        data["codec"] = enc.value_at(enc.CODECS, self.codec.get_selected())
        data["quality"] = enc.value_at(self._quality_rows, self.quality.get_selected())
        data["language"] = i18n.language()
        data["fps"] = enc.value_at(enc.FPS, self.fps.get_selected())
        data["scale"] = enc.value_at(enc.SCALES, self.size.get_selected())
        data["cursor"] = self.cursor.get_active()
        data["wait_for_game"] = self.wait_game.get_active()
        data["hud"] = self.hud.get_active()
        data["minimize_on_record"] = self.min_on_rec.get_active()
        if self.mon_ids:
            data["monitor_name"] = self.mon_ids[min(self.mon_drop.get_selected(), len(self.mon_ids) - 1)]
        if self.cam_ids:
            data["v4l2_device"] = self.cam_ids[min(self.cam_drop.get_selected(), len(self.cam_ids) - 1)]
        mix = min(self.audio_mix.get_selected(), 3)
        data["audio_system"] = AUDIO_MIX[mix][0]
        data["audio_mic"] = AUDIO_MIX[mix][1]
        track = min(self.audio_tracks.get_selected(), len(AUDIO_TRACKS) - 1)
        data["audio_tracks"] = AUDIO_TRACKS[track][0]
        self.sys_audio.set_active(data["audio_system"])
        self.mic_audio.set_active(data["audio_mic"])
        if self.out_ids:
            data["audio_system_device"] = self.out_ids[min(self.out_drop.get_selected(), len(self.out_ids) - 1)]
        if self.in_ids:
            data["audio_mic_device"] = self.in_ids[min(self.in_drop.get_selected(), len(self.in_ids) - 1)]
        data["audio_system_vol"] = int(self.sys_vol.get_value())
        data["audio_mic_vol"] = int(self.mic_vol.get_value())
        data["audio_bitrate"] = enc.value_at(enc.AUDIO_BITRATES, self.abitrate.get_selected())
        data["save_audio_folder"] = self.save_audio_folder.get_active()
        data["audio_denoise_ambient"] = self.denoise_ambient.get_active()
        data["audio_denoise_static"] = self.denoise_static.get_active()
        data["audio_denoise_dirty"] = self.denoise_dirty.get_active()
        data["audio_denoise_system"] = self.denoise_system.get_active()
        data["audio_denoise_mic"] = self.denoise_mic.get_active()
        data["audio_denoise_strength"] = ["suave", "media", "agresiva"][min(self.denoise_str.get_selected(), 2)]
        data["screenshot_format"] = "jpg" if self.shot_fmt.get_selected() == 1 else "png"
        data["replay_secs"] = [0, 15, 30, 60, 120][min(self.replay.get_selected(), 4)]
        data["limit_hours"] = [0, 1, 2, 4][min(self.limit.get_selected(), 3)]
        active_mode = next((k for k, b in self.mode_btns.items() if b.get_active()), "monitor")
        data["capture_mode"] = active_mode
        if active_mode == "area":
            data["mode"] = "region"
        elif active_mode == "device":
            data["mode"] = "device"
        elif active_mode == "game":
            data["mode"] = "game"
        else:
            data["mode"] = "monitor"
        game = self._selected_game()
        if game is not None:
            data["game_key"] = game.key
            data["game_title"] = game.title
        folder_text = self.folder_entry.get_text().strip()
        if folder_text:
            data["output_dir"] = folder_text
        cfg.save(data)

    def _on_sys_vol(self, *_args) -> None:
        self._persist()
        audio_mod.set_volume_percent("sink", int(self.sys_vol.get_value()))

    def _on_mic_vol(self, *_args) -> None:
        self._persist()
        audio_mod.set_volume_percent("source", int(self.mic_vol.get_value()))

    def _refresh_hotkey_chip(self) -> None:
        key = cfg.load().get("hotkey") or "F9"
        self.hotkey_chip.set_text(str(key))

    def _refresh_rec_key(self) -> None:
        key = cfg.load().get("hotkey") or "F9"
        self.rec_key.set_markup(f'Tecla: <span font="monospace"><b>{key}</b></span>')

    def _refresh_shot_key(self) -> None:
        key = cfg.load().get("hotkey_screenshot") or "sin asignar"
        markup = f'Tecla: <span font="monospace"><b>{key}</b></span>'
        if hasattr(self, "shot_key"):
            self.shot_key.set_markup(f'Captura: <span font="monospace"><b>{key}</b></span>')
        if hasattr(self, "shot_key_b"):
            self.shot_key_b.set_markup(markup)

    def _refresh_replay_key(self) -> None:
        key = cfg.load().get("hotkey_replay") or "sin asignar"
        markup = f'Tecla: <span font="monospace"><b>{key}</b></span>'
        if hasattr(self, "replay_key"):
            self.replay_key.set_markup(f'Replay: <span font="monospace"><b>{key}</b></span>')
        if hasattr(self, "replay_key_b"):
            self.replay_key_b.set_markup(markup)

    def _refresh_pause_key(self) -> None:
        key = cfg.load().get("hotkey_pause") or "F8"
        markup = f'Tecla: <span font="monospace"><b>{key}</b></span>'
        if hasattr(self, "pause_key_b"):
            self.pause_key_b.set_markup(markup)

    def _grab(self, which: str) -> None:
        self.grabbing = which
        self.status.set_label("Pulsa la tecla para asignarla…")

    def _on_key(self, _c, keyval: int, _code: int, state: Gdk.ModifierType) -> bool:
        if not self.grabbing:
            return False
        name = Gdk.keyval_name(keyval)
        if not name or name in {"Control_L", "Control_R", "Shift_L", "Shift_R", "Alt_L", "Alt_R", "Super_L", "Super_R"}:
            return True
        parts: list[str] = []
        if state & Gdk.ModifierType.CONTROL_MASK:
            parts.append("<Control>")
        if state & Gdk.ModifierType.ALT_MASK:
            parts.append("<Alt>")
        if state & Gdk.ModifierType.SUPER_MASK:
            parts.append("<Super>")
        letter = gtk_accel(name)
        binding = "".join(parts) + letter
        label = "+".join([p.strip("<>") for p in parts] + [letter])
        data = cfg.load()
        which = self.grabbing
        if which == "screenshot":
            data["hotkey_screenshot"] = label
            data["hotkey_screenshot_binding"] = binding
        elif which == "replay":
            data["hotkey_replay"] = label
            data["hotkey_replay_binding"] = binding
        elif which == "pause":
            data["hotkey_pause"] = label
            data["hotkey_pause_binding"] = binding
            app = self.get_application()
            if app is not None:
                app.set_accels_for_action("app.pause", [binding])
        else:
            data["hotkey"] = label
            data["hotkey_binding"] = binding
            app = self.get_application()
            if app is not None:
                app.set_accels_for_action("app.toggle", [binding])
        cfg.save(data)
        gnome_hotkey.install_all(data)
        self.grabbing = None
        self._refresh_hotkey_chip()
        self._refresh_rec_key()
        self._refresh_shot_key()
        self._refresh_replay_key()
        self._refresh_pause_key()
        self.status.set_label(f"Tecla {label} guardada.")
        return True

    def _do_screenshot(self, *_args) -> None:
        path = take_screenshot()
        if path is None:
            self.status.set_label("No se pudo capturar la pantalla.")
            return
        self.status.set_label(f"Captura {path.name}")
        self.reload_library()

    def recordings(self) -> list[Path]:
        files = library.list_recordings(output_dir(), newest_first=True)
        kind = self.sort_drop.get_selected()
        if kind == 1:
            files.sort(key=lambda p: p.name.lower())
        elif kind == 2:
            files.sort(key=lambda p: p.stat().st_size if p.exists() else 0, reverse=True)
        if self.lib_filter == "shots":
            return [p for p in files if p.suffix.lower() in {".png", ".jpg", ".jpeg"}]
        return [p for p in files if p.suffix.lower() in {".mkv", ".mp4", ".webm"}]

    def _clear_flow(self, flow: Gtk.FlowBox) -> None:
        while True:
            child = flow.get_child_at_index(0)
            if child is None:
                break
            flow.remove(child)

    def reload_library(self) -> None:
        self._clear_flow(self.flow)
        if hasattr(self, "lib_flow"):
            self._clear_flow(self.lib_flow)
        files = self.recordings()
        # Cap UI cards — full folder listing of huge AV1 thumbs OOM'd the app (~3.5 GiB).
        lib_limit = 24
        total = library.total_bytes(files)
        self.lib_summary.set_text(f"{len(files)} archivos · {library.fmt_bytes(total)}")
        self.folder_entry.set_text(str(output_dir()))
        recent = files[:4]
        for path in recent:
            self.flow.append(self._card(path))
        if hasattr(self, "lib_stack"):
            self.lib_stack.set_visible_child_name("list" if recent else "empty")
        if hasattr(self, "lib_flow"):
            for path in files[:lib_limit]:
                self.lib_flow.append(self._card(path))
        self._fill_thumbs(recent)

    def _card(self, path: Path) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        box.add_css_class("thumb-card")
        box.set_size_request(168, -1)
        overlay = Gtk.Overlay()
        pic = Gtk.Picture()
        pic.set_size_request(168, 94)
        pic.set_content_fit(Gtk.ContentFit.COVER)
        overlay.set_child(pic)
        dur = Gtk.Label(label="…")
        dur.add_css_class("dur-badge")
        dur.set_halign(Gtk.Align.END)
        dur.set_valign(Gtk.Align.END)
        dur.set_margin_bottom(6)
        dur.set_margin_end(6)
        overlay.add_overlay(dur)
        # Duration via cache only on UI thread; ffprobe off-thread to avoid RAM/CPU spikes.
        hit = _DURATION_CACHE.get(str(path))
        try:
            mtime = path.stat().st_mtime
        except OSError:
            mtime = 0.0
        if hit is not None and hit[0] == mtime:
            dur.set_label(hit[1])
        else:

            def _probe(p: Path = path, label: Gtk.Label = dur) -> None:
                text = probe_duration(p)
                GLib.idle_add(label.set_label, text)

            threading.Thread(target=_probe, daemon=True).start()
        name = Gtk.Label(label=path.name, xalign=0)
        name.add_css_class("thumb-name")
        name.set_ellipsize(Pango.EllipsizeMode.END)
        name.set_max_width_chars(22)
        meta = Gtk.Label(label=f"{fmt_when(path)}  |  {fmt_size(path)}", xalign=0)
        meta.add_css_class("thumb-meta")
        box.append(overlay)
        box.append(name)
        box.append(meta)
        box.path = path  # type: ignore[attr-defined]
        box.thumb = pic  # type: ignore[attr-defined]
        left = Gtk.GestureClick()
        left.set_button(Gdk.BUTTON_PRIMARY)
        left.connect("released", self._on_card_click, box)
        box.add_controller(left)
        right = Gtk.GestureClick()
        right.set_button(Gdk.BUTTON_SECONDARY)
        right.connect("pressed", self._on_card_menu, box)
        box.add_controller(right)
        return box

    def _fill_thumbs(self, files: list[Path]) -> None:
        if self._thumb_busy:
            return
        # Only the recent strip (4) — library page loads thumbs on demand later if needed.
        pending = files[:4]

        def work(paths: list[Path]) -> None:
            try:
                for path in paths:
                    dest = library.ensure_thumb(path)
                    if dest is None:
                        continue
                    GLib.idle_add(self._set_thumb, str(path), str(dest))
            finally:
                GLib.idle_add(self._thumb_done)

        self._thumb_busy = True
        threading.Thread(target=work, args=(list(pending),), daemon=True).start()

    def _thumb_done(self) -> bool:
        self._thumb_busy = False
        return False

    def _apply_thumb(self, flow: Gtk.FlowBox, src: str, dest: str) -> Path | None:
        idx = 0
        found = None
        while True:
            child = flow.get_child_at_index(idx)
            if child is None:
                break
            card = child.get_child()
            path = getattr(card, "path", None) if card is not None else None
            pic = getattr(card, "thumb", None) if card is not None else None
            if isinstance(path, Path) and str(path) == src and isinstance(pic, Gtk.Picture):
                pic.set_file(Gio.File.new_for_path(dest))
                found = path
                break
            idx += 1
        return found

    def _set_thumb(self, src: str, dest: str) -> bool:
        path = self._apply_thumb(self.flow, src, dest)
        if hasattr(self, "lib_flow"):
            path = self._apply_thumb(self.lib_flow, src, dest) or path
        if self.current is None and isinstance(path, Path) and path.suffix.lower() in {".mkv", ".mp4", ".webm"}:
            self.current = path
        return False

    def _on_card(self, _flow, child: Gtk.FlowBoxChild) -> None:
        card = child.get_child()
        path = getattr(card, "path", None) if card is not None else None
        if isinstance(path, Path):
            self._play_path(path)

    def _on_card_click(self, gesture: Gtk.GestureClick, _n: int, _x: float, _y: float, box: Gtk.Box) -> None:
        path = getattr(box, "path", None)
        if isinstance(path, Path):
            self._play_path(path)
            gesture.set_state(Gtk.EventSequenceState.CLAIMED)

    def _on_card_menu(self, gesture: Gtk.GestureClick, _n: int, x: float, y: float, box: Gtk.Box) -> None:
        path = getattr(box, "path", None)
        if not isinstance(path, Path):
            return
        self.current = path
        pop = Gtk.Popover()
        pop.set_has_arrow(False)
        menu = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        menu.set_margin_top(6)
        menu.set_margin_bottom(6)
        menu.set_margin_start(6)
        menu.set_margin_end(6)
        actions = (
            ("Reproducir", lambda: self._play_path(path), False),
            ("Mostrar en carpeta", self._show_in_folder, False),
            ("Copiar ruta", self._copy_path, False),
            ("Eliminar", self._delete_current, True),
        )
        for label, fn, danger in actions:
            btn = Gtk.Button(label=label)
            btn.add_css_class("menu-item")
            if danger:
                btn.add_css_class("menu-danger")
            btn.set_has_frame(False)
            btn.connect("clicked", lambda _b, func=fn, p=pop: (p.popdown(), func()))
            menu.append(btn)
        pop.set_child(menu)
        pop.set_parent(box)
        rect = Gdk.Rectangle()
        rect.x = int(x)
        rect.y = int(y)
        rect.width = 1
        rect.height = 1
        pop.set_pointing_to(rect)
        pop.popup()
        self._clip_pop = pop
        gesture.set_state(Gtk.EventSequenceState.CLAIMED)

    def _play_path(self, path: Path) -> None:
        self.current = path
        if path.suffix.lower() in {".png", ".jpg", ".jpeg"}:
            self.status.set_label(f"Captura {path.name}")
            subprocess.Popen(["xdg-open", str(path)], start_new_session=True)
            return
        if path.suffix.lower() not in {".mkv", ".mp4", ".webm"}:
            return
        # Never load clips into the capture preview — that area is only the capture target.
        self._open_player(path, fullscreen=False)

    def _play_current(self) -> None:
        if self.current is not None:
            self._play_path(self.current)

    def _show_in_folder(self) -> None:
        if self.current is None:
            return
        folder = self.current.parent
        try:
            subprocess.Popen(["nautilus", "--select", str(self.current)], start_new_session=True)
        except OSError:
            subprocess.Popen(["xdg-open", str(folder)], start_new_session=True)

    def _copy_path(self) -> None:
        if self.current is None:
            return
        display = Gdk.Display.get_default()
        if display is None:
            return
        display.get_clipboard().set(str(self.current))
        self.status.set_label("Ruta copiada.")

    def _delete_current(self) -> None:
        path = self.current
        if path is None:
            return
        body = f"Se borrará {path.name} de forma permanente."
        audio_dir = audio_mod.audio_export_dir(path)
        if audio_dir.is_dir():
            body += f" También se borrará la carpeta {audio_dir.name}."
        dlg = Adw.MessageDialog(
            transient_for=self,
            heading="Eliminar grabación",
            body=body,
        )
        dlg.add_response("cancel", "Cancelar")
        dlg.add_response("delete", "Eliminar")
        dlg.set_default_response("cancel")
        dlg.set_close_response("cancel")
        dlg.set_response_appearance("delete", Adw.ResponseAppearance.DESTRUCTIVE)
        dlg.connect("response", self._on_delete_response, path)
        dlg.present()

    def _on_delete_response(self, dlg: Adw.MessageDialog, response: str, path: Path) -> None:
        dlg.destroy()
        if response != "delete":
            return
        try:
            path.unlink(missing_ok=True)
        except OSError as err:
            self.status.set_label(str(err))
            return
        note = ""
        try:
            if audio_mod.remove_audio_export(path):
                note = " y su carpeta de audio"
            library.thumb_path(path).unlink(missing_ok=True)
        except OSError as err:
            if self.current == path:
                self.current = None
                self._close_player()
            self.reload_library()
            self.status.set_label(f"Vídeo eliminado. No se pudo borrar la carpeta de audio: {err}")
            return
        if self.current == path:
            self.current = None
            self._close_player()
        self.reload_library()
        self.status.set_label(f"Eliminado {path.name}{note}")

    def _on_folder_typed(self, *_args) -> None:
        text = self.folder_entry.get_text().strip()
        if not text:
            return
        path = Path(text).expanduser()
        try:
            path.mkdir(parents=True, exist_ok=True)
        except OSError as err:
            self.status.set_label(str(err))
            return
        data = cfg.load()
        data["output_dir"] = str(path)
        cfg.save(data)
        self.reload_library()
        self.status.set_label(f"Carpeta {path}")

    def _choose_folder(self, *_args) -> None:
        dialog = Gtk.FileDialog()
        dialog.set_title("Carpeta donde se guardan los vídeos")
        dialog.set_initial_folder(Gio.File.new_for_path(str(output_dir())))
        dialog.select_folder(self, None, self._on_folder_chosen)

    def _on_folder_chosen(self, dialog: Gtk.FileDialog, result: Gio.AsyncResult) -> None:
        try:
            folder = dialog.select_folder_finish(result)
        except GLib.Error:
            return
        if folder is None:
            return
        path = Path(folder.get_path() or "")
        if not path:
            return
        self.folder_entry.set_text(str(path))
        self._on_folder_typed()

    def toggle_record(self) -> None:
        self._persist()
        snap = self.controller.toggle()
        self._apply_snapshot(snap, force_status=True)
        if snap.state == RecordingState.RECORDING and cfg.load().get("minimize_on_record", True):
            self.minimize()

    def toggle_pause(self) -> None:
        snap = self.controller.pause_toggle()
        self._apply_snapshot(snap, force_status=True)

    def _apply_snapshot(self, snap, force_status: bool = False) -> None:
        recording = snap.state in {RecordingState.RECORDING, RecordingState.PAUSED}
        stopping = snap.state == RecordingState.STOPPING
        if force_status or snap.state != RecordingState.IDLE:
            self.status.set_label(snap.message)
        elif "Toma guardada" in snap.message or snap.state == RecordingState.ERROR:
            self.status.set_label(snap.message)
        if recording or stopping:
            self.live.add_css_class("live-on")
            self.clock.set_label(fmt_time(snap.elapsed_s))
            fps_s = f"{snap.fps:.0f} FPS" if snap.fps > 0 else "— FPS"
            self.metrics.set_label(f"{fps_s}  ·  {session.monitor_size_label()}  ·  {snap.free_gb:.0f} GB")
            if snap.state == RecordingState.PAUSED:
                self.rec_btn.set_label("PAUSE")
            else:
                self.rec_btn.set_label("STOP")
            self.rec_btn.add_css_class("recording")
        else:
            self.live.remove_css_class("live-on")
            self.clock.set_label("00:00:00")
            self.metrics.set_label(session.monitor_idle_label())
            self.rec_btn.set_label("REC")
            self.rec_btn.remove_css_class("recording")

    def _cpu_pct(self) -> str:
        try:
            line = Path("/proc/stat").read_text(encoding="utf-8").splitlines()[0]
            parts = [int(x) for x in line.split()[1:]]
            idle = parts[3] + (parts[4] if len(parts) > 4 else 0)
            total = sum(parts)
            prev = getattr(self, "_cpu_prev", None)
            self._cpu_prev = (idle, total)
            if prev is None:
                return "CPU —"
            didle = idle - prev[0]
            dtotal = total - prev[1]
            if dtotal <= 0:
                return "CPU —"
            pct = max(0.0, min(100.0, (1 - didle / dtotal) * 100))
            return f"CPU {pct:.0f}%"
        except (OSError, ValueError, IndexError):
            return "CPU —"

    def _sys_stats(self) -> str:
        cpu = self._cpu_pct()
        gpu = "GPU —"
        try:
            out = subprocess.run(
                [
                    "nvidia-smi",
                    "--query-gpu=utilization.gpu,memory.used",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                timeout=1,
            )
            line = (out.stdout or "").splitlines()[0] if out.returncode == 0 else ""
            parts = [p.strip() for p in line.split(",")]
            if len(parts) >= 2:
                gpu = f"GPU {parts[0]}%"
        except (OSError, subprocess.TimeoutExpired, ValueError, IndexError):
            gpu = "GPU —"
        mem = "RAM —"
        try:
            info = Path("/proc/meminfo").read_text(encoding="utf-8")
            total = avail = 0
            for line in info.splitlines():
                if line.startswith("MemTotal:"):
                    total = int(line.split()[1])
                elif line.startswith("MemAvailable:"):
                    avail = int(line.split()[1])
            used = max(total - avail, 0) / 1_048_576
            mem = f"RAM {used:.1f} GB"
        except (OSError, ValueError):
            mem = "RAM —"
        free, _pct = library.free_disk(output_dir())
        return f"{cpu}  |  {gpu}  |  {mem}  |  Disco {free} libre"

    def poll(self) -> bool:
        snap = self.controller.poll()
        recording = snap.state in {RecordingState.RECORDING, RecordingState.PAUSED}
        if self.was_recording and snap.state == RecordingState.IDLE:
            # Defer library rebuild so stop+ffmpeg extract don't pile on UI RAM.
            GLib.timeout_add(1500, self._reload_library_safe)
        self._apply_snapshot(snap)
        self.was_recording = recording or snap.state == RecordingState.STOPPING
        if hasattr(self, "vu") and not recording:
            levels = Path.home() / ".cache" / "gamecap" / "levels.json"
            try:
                voice = float(json.loads(levels.read_text(encoding="utf-8")).get("voice") or 0)
                self.vu.set_value(max(0.0, min(1.0, voice)))
            except (OSError, json.JSONDecodeError, TypeError, ValueError):
                self.vu.set_value(0)
        self._stat_tick += 1
        # System stats are secondary: refresh ~ every 5s.
        if self._stat_tick % 10 == 0 and not recording:
            self.sys_lbl.set_text(self._sys_stats())
        return True

    def _reload_library_safe(self) -> bool:
        try:
            self.reload_library()
        except Exception:
            pass
        return False


class GameCapApp(Adw.Application):
    def __init__(self) -> None:
        super().__init__(
            application_id="dev.kebux.gamecap",
            flags=Gio.ApplicationFlags.NON_UNIQUE,
        )
        Gtk.Window.set_default_icon_name("gamecap")
        self.win: GameCapWindow | None = None
        toggle = Gio.SimpleAction.new("toggle", None)
        toggle.connect("activate", lambda *_: self.win.toggle_record() if self.win else None)
        self.add_action(toggle)
        self.set_accels_for_action("app.toggle", [gtk_accel(str(cfg.load().get("hotkey_binding") or "F9"))])
        pause = Gio.SimpleAction.new("pause", None)
        pause.connect("activate", lambda *_: self.win.toggle_pause() if self.win else None)
        self.add_action(pause)
        self.set_accels_for_action("app.pause", [gtk_accel(str(cfg.load().get("hotkey_pause_binding") or "F8"))])

    def do_activate(self) -> None:  # noqa: N802
        if self.win is None:
            self.win = GameCapWindow(self)
        self.win.present()


def main() -> int:
    enc.warm_codec_cache()
    Adw.init()
    return GameCapApp().run(sys.argv)


if __name__ == "__main__":
    sys.exit(main())
