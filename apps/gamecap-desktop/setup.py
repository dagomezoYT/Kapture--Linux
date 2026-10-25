#!/usr/bin/env python3
"""Instalador gráfico de GameCap: menú de aplicaciones, icono y acceso de escritorio."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import threading
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, GLib, Gtk

ROOT = Path(__file__).resolve().parents[2]
BIN = ROOT / "bin" / "gamecap"
BRAND = Path(__file__).resolve().parent / "branding"
ICON_SRC = Path(__file__).resolve().parent / "icons" / "gamecap.svg"
ICON_PNG = BRAND / "app-icon-256.png"
LAUNCH = ROOT / "scripts" / "gamecap-launch.sh"
APP_ID = "dev.kebux.gamecap"

CSS = b"""
window.gamecap-setup { background: #1b1d21; color: #ececec; }
.brand { font-weight: 800; font-size: 22px; letter-spacing: 0.4px; }
.brand-accent { color: #e10600; }
.hint { color: #8b9098; font-size: 13px; }
.check-ok { color: #6ee7a8; }
.check-bad { color: #e10600; }
.primary {
  min-height: 42px;
  background: #e10600;
  color: #fff;
  font-weight: 700;
}
.statusline {
  background: #121417;
  color: #8b9098;
  padding: 8px 14px;
  font-size: 12px;
}
"""


def which(name: str) -> bool:
    return shutil.which(name) is not None


def desktop_dir() -> Path:
    try:
        out = subprocess.run(
            ["xdg-user-dir", "DESKTOP"],
            capture_output=True,
            text=True,
            timeout=3,
        )
        path = Path((out.stdout or "").strip() or str(Path.home() / "Escritorio"))
    except (OSError, subprocess.TimeoutExpired):
        path = Path.home() / "Escritorio"
    return path if path.is_dir() else Path.home() / "Desktop"


def icon_dest() -> Path:
    return Path.home() / ".local" / "share" / "icons" / "hicolor" / "scalable" / "apps" / "gamecap.svg"


def install_icons() -> None:
    """Install PNG app icon into the user hicolor theme (plus legacy SVG if present)."""
    base = Path.home() / ".local" / "share" / "icons" / "hicolor"
    for size in (32, 48, 64, 128, 256, 512):
        src = BRAND / f"app-icon-{size}.png"
        if not src.is_file() and ICON_PNG.is_file() and size == 256:
            src = ICON_PNG
        if not src.is_file():
            continue
        dest = base / f"{size}x{size}" / "apps" / "gamecap.png"
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dest)
    if ICON_SRC.is_file():
        # Prefer PNG icons; keep SVG only as fallback if no PNG sizes exist.
        if not any((BRAND / f"app-icon-{s}.png").is_file() for s in (32, 48, 64, 128, 256, 512)):
            dest_svg = icon_dest()
            dest_svg.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ICON_SRC, dest_svg)


def app_desktop_path() -> Path:
    return Path.home() / ".local" / "share" / "applications" / f"{APP_ID}.desktop"


def desktop_entry() -> str:
    return (
        "[Desktop Entry]\n"
        "Type=Application\n"
        "Version=1.0\n"
        "Name=Kapture\n"
        "Comment=Record what matters — grabadora de juegos\n"
        f"Exec={LAUNCH}\n"
        f"Path={ROOT}\n"
        "Icon=gamecap\n"
        "Terminal=false\n"
        "Categories=AudioVideo;Recorder;Game;\n"
        "StartupNotify=true\n"
        f"StartupWMClass={APP_ID}\n"
        "Keywords=grabar;juego;replay;minecraft;kapture;vyro;\n"
    )


def checks() -> list[tuple[str, bool]]:
    return [
        ("Python 3", which("python3")),
        ("GTK / interfaz", _gtk_ok()),
        ("FFmpeg", which("ffmpeg")),
        ("GPU Screen Recorder", which("gpu-screen-recorder") and which("gsr-cli")),
        ("Reproductor mpv", which("mpv")),
        ("Binario de grabación", BIN.is_file() and os.access(BIN, os.X_OK)),
        ("Acceso en el menú", app_desktop_path().is_file()),
    ]


def _gtk_ok() -> bool:
    try:
        import gi as gi_mod

        gi_mod.require_version("Gtk", "4.0")
        gi_mod.require_version("Adw", "1")
        from gi.repository import Adw as _adw  # noqa: F401
        from gi.repository import Gtk as _gtk  # noqa: F401

        return True
    except (ImportError, ValueError):
        return False


def _trust(path: Path) -> None:
    try:
        subprocess.run(
            ["gio", "set", str(path), "metadata::trusted", "true"],
            check=False,
            capture_output=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def _refresh_menus() -> None:
    apps = Path.home() / ".local" / "share" / "applications"
    icons = Path.home() / ".local" / "share" / "icons" / "hicolor"
    try:
        subprocess.run(
            ["update-desktop-database", str(apps)],
            check=False,
            capture_output=True,
            timeout=8,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass
    try:
        subprocess.run(
            ["gtk-update-icon-cache", "-f", str(icons)],
            check=False,
            capture_output=True,
            timeout=8,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def build_binary() -> str:
    if BIN.is_file() and os.access(BIN, os.X_OK):
        return ""
    cargo = ROOT / ".toolchain" / "cargo" / "bin" / "cargo"
    cmd = [str(cargo) if cargo.is_file() else "cargo", "build", "-p", "gamecap-cli", "--release"]
    env = os.environ.copy()
    if cargo.is_file():
        env["RUSTUP_HOME"] = str(ROOT / ".toolchain" / "rustup")
        env["CARGO_HOME"] = str(ROOT / ".toolchain" / "cargo")
        env["PATH"] = f"{cargo.parent}:{env.get('PATH', '')}"
    env["CARGO_TARGET_DIR"] = str(ROOT / "target")
    proc = subprocess.run(
        cmd,
        cwd=str(ROOT),
        env=env,
        capture_output=True,
        text=True,
        timeout=900,
    )
    if proc.returncode != 0:
        err = (proc.stderr or proc.stdout or "No se pudo compilar GameCap.").strip()
        return err[-1200:]
    built = ROOT / "target" / "release" / "gamecap"
    if not built.is_file():
        return "La compilación terminó, pero no apareció bin/gamecap."
    BIN.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(built, BIN)
    BIN.chmod(0o755)
    return ""


def install_launcher() -> str:
    if not LAUNCH.is_file():
        return "Falta scripts/gamecap-launch.sh"
    LAUNCH.chmod(0o755)
    (ROOT / "scripts" / "gamecap-setup.sh").chmod(0o755)
    install_icons()
    apps = app_desktop_path()
    apps.parent.mkdir(parents=True, exist_ok=True)
    text = desktop_entry()
    apps.write_text(text, encoding="utf-8")
    apps.chmod(0o755)
    _trust(apps)
    shortcut = desktop_dir() / "Kapture.desktop"
    shortcut.write_text(text, encoding="utf-8")
    shortcut.chmod(0o755)
    _trust(shortcut)
    # Keep legacy shortcut name for users who already pinned GameCap.
    legacy = desktop_dir() / "GameCap.desktop"
    if legacy != shortcut:
        legacy.write_text(text, encoding="utf-8")
        legacy.chmod(0o755)
        _trust(legacy)
    repo = ROOT / "GameCap.desktop"
    repo.write_text(text, encoding="utf-8")
    repo.chmod(0o755)
    installer = ROOT / "Instalar GameCap.desktop"
    installer.write_text(
        "[Desktop Entry]\n"
        "Type=Application\n"
        "Name=Instalar Kapture\n"
        "Comment=Instala Kapture en el menú de aplicaciones\n"
        f"Exec={ROOT / 'scripts' / 'gamecap-setup.sh'}\n"
        f"Path={ROOT}\n"
        "Icon=gamecap\n"
        "Terminal=false\n"
        "Categories=Settings;\n"
        "StartupNotify=true\n",
        encoding="utf-8",
    )
    installer.chmod(0o755)
    _trust(installer)
    _refresh_menus()
    return ""


def install_all() -> str:
    err = build_binary()
    if err:
        return err
    return install_launcher()


class SetupWindow(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application) -> None:
        super().__init__(application=app, title="Instalar GameCap")
        self.add_css_class("gamecap-setup")
        self.set_default_size(460, 520)
        self.set_resizable(False)
        self._busy = False

        provider = Gtk.CssProvider()
        provider.load_from_data(CSS)
        display = self.get_display()
        if display is not None:
            Gtk.StyleContext.add_provider_for_display(
                display, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        body.set_margin_top(22)
        body.set_margin_bottom(18)
        body.set_margin_start(24)
        body.set_margin_end(24)

        brand_path = BRAND / "window-logo-ui.png"
        if brand_path.is_file():
            pic = Gtk.Picture.new_for_filename(str(brand_path))
            pic.set_content_fit(Gtk.ContentFit.CONTAIN)
            pic.set_size_request(-1, 40)
            pic.set_halign(Gtk.Align.START)
            body.append(pic)
        else:
            brand = Gtk.Box(spacing=0)
            a = Gtk.Label(label="KAP")
            a.add_css_class("brand")
            b = Gtk.Label(label="TURE")
            b.add_css_class("brand")
            b.add_css_class("brand-accent")
            brand.append(a)
            brand.append(b)
            body.append(brand)

        hint = Gtk.Label(
            label="Instálalo una vez. Después ábrelo desde Actividades o el icono del escritorio, sin terminal.",
            wrap=True,
            xalign=0,
        )
        hint.add_css_class("hint")
        body.append(hint)

        self.list_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        body.append(self.list_box)

        self.install_btn = Gtk.Button(label="Instalar")
        self.install_btn.add_css_class("primary")
        self.install_btn.connect("clicked", self._on_install)
        body.append(self.install_btn)

        self.open_btn = Gtk.Button(label="Abrir Kapture")
        self.open_btn.connect("clicked", self._on_open)
        body.append(self.open_btn)

        root.append(body)
        self.status = Gtk.Label(label="Listo para instalar.", xalign=0)
        self.status.add_css_class("statusline")
        root.append(self.status)
        self.set_content(root)
        self.refresh()

    def refresh(self) -> None:
        child = self.list_box.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self.list_box.remove(child)
            child = nxt
        for name, ok in checks():
            row = Gtk.Box(spacing=10)
            mark = Gtk.Label(label="●" if ok else "○")
            mark.add_css_class("check-ok" if ok else "check-bad")
            lab = Gtk.Label(label=name, xalign=0)
            lab.set_hexpand(True)
            row.append(mark)
            row.append(lab)
            self.list_box.append(row)
        installed = app_desktop_path().is_file() and BIN.is_file()
        self.install_btn.set_label("Reinstalar" if installed else "Instalar")
        self.open_btn.set_sensitive(BIN.is_file())
        if installed:
            self.status.set_label("GameCap ya está en el menú de aplicaciones.")

    def _on_install(self, *_args) -> None:
        if self._busy:
            return
        self._busy = True
        self.install_btn.set_sensitive(False)
        self.status.set_label("Instalando…")

        def work() -> None:
            err = install_all()
            GLib.idle_add(self._done, err)

        threading.Thread(target=work, daemon=True).start()

    def _done(self, err: str) -> bool:
        self._busy = False
        self.install_btn.set_sensitive(True)
        self.refresh()
        if err:
            self.status.set_label(err.splitlines()[-1][:180])
        else:
            self.status.set_label("Listo. Ábrelo desde Actividades o el escritorio.")
        return False

    def _on_open(self, *_args) -> None:
        if not BIN.is_file():
            self.status.set_label("Instálalo primero.")
            return
        subprocess.Popen(
            [str(LAUNCH)],
            cwd=str(ROOT),
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.close()


class SetupApp(Adw.Application):
    def __init__(self) -> None:
        super().__init__(application_id="dev.kebux.gamecap.setup")

    def do_activate(self) -> None:  # noqa: N802
        win = SetupWindow(self)
        win.present()


def main() -> int:
    if "--cli" in sys.argv:
        err = install_all()
        if err:
            print(err, file=sys.stderr)
            return 1
        print(f"Instalado: {app_desktop_path()}")
        print(f"Escritorio: {desktop_dir() / 'GameCap.desktop'}")
        return 0
    Adw.init()
    return SetupApp().run([sys.argv[0]])


if __name__ == "__main__":
    sys.exit(main())
