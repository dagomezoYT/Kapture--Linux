# Kapture

Linux desktop recorder. The window is Kapture. The capture binary is `gamecap`.
A take is captured and encoded by gpu-screen-recorder on the GPU that owns the
connected display, then muxed to MKV with separate audio tracks.

The synthetic core (bounded queues, rational timestamps, fake 144 fps session)
is still in the workspace and is what `cargo test` exercises. It does not
record the screen. The desktop does.

## Run

From the repository root:

```bash
./ejecutar.sh
```

That opens the GTK window. The same entry point with arguments runs the CLI
(`record`, `toggle`, `pause`, `screenshot`, `replay`). The first CLI build
writes `bin/gamecap`.

Desktop smoke checks, without starting a recording:

```bash
python3 apps/gamecap-desktop/test_smoke.py
```

Workspace tests:

```bash
cargo test --workspace
```

## Language

The interface is Spanish by default. **Idioma** in the left sidebar switches
the window to English: navigation, capture view, library, recording panel, and
status line. The choice is stored as `language` (`es` or `en`) in
`~/.config/gamecap/settings.json` and is restored on the next launch.

An unknown code falls back to Spanish. The encode plan does not change with
the language.

## Ultra

Ultra is a heavier gpu-screen-recorder preset (more detail, more bitrate).
The section is drawn only after the app reads the DRM cards and classifies
the GPU that owns the screen. A stronger card that is present but has no
connected output does not count.

| Display GPU | Ultra in the UI | Stable fps ceiling | Budget |
| --- | --- | ---: | --- |
| AMD or Intel with a panel (`eDP`, `LVDS`, `DSI`, `DPI`), including this laptop’s Radeon 780M | Hidden. `ultra` and `very_high` become `high`. | 60 | `vaapi-safe` |
| Discrete AMD or Intel, external output only | Shown. Ultra can be selected. | 120 | `vaapi-discrete` |
| NVIDIA, and it owns the connected output | Shown. Ultra can be selected. | 240 | `nvenc` |
| Any other vendor | Hidden. H.264 on the CPU, 1280×720 if the scale was native. | 30 | `software` |
| Probe failed, or no connected output | Hidden. | 60 | `vaapi-safe` |

The saved default stays 60 fps and quality `high` on every budget. Opening the
app on a faster GPU does not start a heavier take by itself. On this
development laptop the panel is the 780M and the RTX 5070 has no output, so
Ultra stays hidden and the command line stays AV1, `high`, 60 fps,
1920×1080, with no `-encoder` flag. The RTX stays free for the game.

The same gate is in the CLI: `very_high` and `ultra` are kept only when the
display GPU is allowed to hold them.

## Hardware and performance

- [Recording performance](docs/performance/README.md) — measured cost of the
  stable profile on this host, and the encode budgets.
- [Hardware compatibility](docs/compatibility/README.md) — which GPU records,
  and how a missing codec probe is treated.
- [Architecture](docs/architecture/README.md) — core versus platform backends.
- [ADR 0008](docs/adr/0008-host-encode-budget.md) — why the budget follows the
  display GPU.

The recorder does not change clocks, firmware, or sysfs.
