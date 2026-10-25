"""Lucide icons (ISC) recolored for the GameCap panel."""

from __future__ import annotations

from pathlib import Path

from gi.repository import GdkPixbuf, Gtk

DIR = Path(__file__).resolve().parent / "icons"


def pixbuf(name: str, size: int = 22, color: str = "#e8eaed") -> GdkPixbuf.Pixbuf:
    path = DIR / f"{name}.svg"
    raw = path.read_text(encoding="utf-8").replace("currentColor", color)
    loader = GdkPixbuf.PixbufLoader.new_with_mime_type("image/svg+xml")
    loader.set_size(size, size)
    loader.write(raw.encode("utf-8"))
    loader.close()
    loaded = loader.get_pixbuf()
    if loaded is None:
        raise RuntimeError(f"no se pudo cargar el icono {name}")
    return loaded


def image(name: str, size: int = 22, color: str = "#e8eaed") -> Gtk.Image:
    img = Gtk.Image.new_from_pixbuf(pixbuf(name, size, color))
    img.set_pixel_size(size)
    return img


def paint(img: Gtk.Image, name: str, size: int, color: str) -> None:
    img.set_from_pixbuf(pixbuf(name, size, color))
