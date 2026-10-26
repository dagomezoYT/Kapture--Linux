# ADR 0008 — Encode budget follows the display GPU

- Status: accepted
- Milestone: M1 (desktop recording)

## Context

The stable profile (`high`, 60 fps, AV1, 1920×1080, no CPU encoder flag) was
chosen because `very_high` / `ultra` and an uncapped refresh inflated bitrate
and hitched pans on the integrated AMD GPU that owns this laptop’s panel.
The same constants were applied to every host. A desktop whose monitor is
plugged into NVIDIA, an Intel iGPU, or a VM with no encoder either could not
use a preset the hardware can hold, or was asked for a GPU codec it does not
have.

gpu-screen-recorder already encodes on the GPU that owns the connected output.
The desktop must not second-guess that by waking a discrete NVIDIA card that
has no display.

## Decision

- Classify the display GPU as VA-API-safe (AMD, Intel, or an unreadable
  probe), NVENC (NVIDIA is the only card with a connected output), or software
  (any other vendor).
- VA-API-safe keeps today’s ceiling: quality `medium` or `high`, 60 fps when
  stable mode is on. `very_high` and `ultra` still collapse to `high` in both
  the desktop and the CLI.
- NVENC may keep `very_high` and `ultra`, and the stable ceiling rises to
  240. The saved default remains 60 / `high`, so opening the app on a new GPU
  does not start a heavier take.
- Software forces H.264, `-encoder cpu`, a 30 fps ceiling, and 1280×720 when
  the scale was native. gpu-screen-recorder’s CPU encoder is H.264 only.
- Codec substitution uses `gpu-screen-recorder --info`, cached off the UI
  thread. An empty cache leaves the requested codec unchanged.
- Portal fallback tries NVENC before VA-API only when the display vendor is
  NVIDIA. Every other vendor keeps the previous order, with VA-API AV1 first.
- The VA-API device probe tries the display GPU’s render node first. On a
  single working node the chosen device is the same one as before.

## Consequences

A hybrid laptop that already records on the integrated GPU keeps the same
gpu-screen-recorder arguments. A NVIDIA-only desktop can use the presets the
UI shows for that budget. A host with no supported vendor still produces a
file instead of failing the GPU codec. The core still does not write clocks,
firmware, or sysfs.
