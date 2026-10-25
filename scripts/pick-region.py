#!/usr/bin/env python3
"""GNOME/Wayland region picker. Prints WxH+X+Y (GSR/slurp format). Esc cancels."""

from __future__ import annotations

import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Graphene, Gtk


class Picker(Gtk.Application):
    def __init__(self) -> None:
        super().__init__(application_id="dev.kebux.gamecap.pick-region")
        self.result: str | None = None
        self.start: tuple[float, float] | None = None
        self.cur: tuple[float, float] | None = None

    def do_activate(self) -> None:  # noqa: N802
        win = Gtk.ApplicationWindow(application=self)
        win.set_decorated(False)
        win.set_title("GameCap")
        win.fullscreen()

        area = Gtk.DrawingArea()
        area.set_hexpand(True)
        area.set_vexpand(True)

        def draw(_widget, snapshot, width, height) -> None:
            snapshot.append_color(
                Gdk.RGBA(red=0, green=0, blue=0, alpha=0.42),
                Graphene.Rect().init(0, 0, width, height),
            )
            if self.start is None or self.cur is None:
                return
            x = min(self.start[0], self.cur[0])
            y = min(self.start[1], self.cur[1])
            w = abs(self.cur[0] - self.start[0])
            h = abs(self.cur[1] - self.start[1])
            snapshot.append_color(
                Gdk.RGBA(red=0.25, green=0.55, blue=1.0, alpha=0.22),
                Graphene.Rect().init(x, y, w, h),
            )

        area.set_draw_func(draw)

        click = Gtk.GestureClick()
        click.set_button(1)

        def pressed(_g, _n, x, y) -> None:
            self.start = (x, y)
            self.cur = (x, y)
            area.queue_draw()

        def released(_g, _n, x, y) -> None:
            if self.start is None:
                return
            x0, y0 = self.start
            rx, ry = min(x0, x), min(y0, y)
            rw, rh = abs(x - x0), abs(y - y0)
            scale = max(int(area.get_scale_factor()), 1)
            x_i = int(rx * scale) & ~1
            y_i = int(ry * scale) & ~1
            w_i = int(rw * scale) & ~1
            h_i = int(rh * scale) & ~1
            if w_i >= 2 and h_i >= 2:
                self.result = f"{w_i}x{h_i}+{x_i}+{y_i}"
            self.quit()

        click.connect("pressed", pressed)
        click.connect("released", released)
        area.add_controller(click)

        motion = Gtk.EventControllerMotion()

        def moved(_c, x, y) -> None:
            if self.start is not None:
                self.cur = (x, y)
                area.queue_draw()

        motion.connect("motion", moved)
        area.add_controller(motion)

        key = Gtk.EventControllerKey()

        def on_key(_c, keyval, _code, _state) -> bool:
            if keyval == Gdk.KEY_Escape:
                self.result = None
                self.quit()
                return True
            return False

        key.connect("key-released", on_key)

        overlay = Gtk.Overlay()
        overlay.set_child(area)
        label = Gtk.Label(
            label="Clic y arrastra sobre la ventana. Esc = monitor entero."
        )
        label.set_halign(Gtk.Align.CENTER)
        label.set_valign(Gtk.Align.START)
        label.set_margin_top(40)
        overlay.add_overlay(label)

        css = Gtk.CssProvider()
        css.load_from_data(
            b"label { color: #fff; font-size: 18px; text-shadow: 0 1px 2px #000; }"
        )
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(),
            css,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )

        win.set_child(overlay)
        win.add_controller(key)
        win.present()


def main() -> int:
    app = Picker()
    app.run()
    if app.result:
        print(app.result)
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
