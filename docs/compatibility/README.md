# Hardware compatibility

The recorder does not require a specific GPU. It reads the DRM cards, picks
the one with a connected output, and applies the budget in
[Recording performance](../performance/README.md). A failed probe keeps the
historical VA-API budget (`high`, 60 fps, no CPU encoder flag) so a machine
that already records does not change presets because sysfs could not be read.

## Which GPU records

| Connected outputs | GPU used for capture and encode |
| --- | --- |
| Only the integrated GPU (this laptop: 780M on eDP-1, RTX with no output) | Integrated GPU. The discrete NVIDIA card is not woken. |
| Only NVIDIA | NVIDIA. NVENC presets up to `ultra` and a 240 fps ceiling are available. The saved default stays 60 / `high`. |
| Panel GPU and NVIDIA both connected | The non-NVIDIA GPU. Forcing the discrete card records the wrong surface on a hybrid laptop. |
| AMD or Intel with a panel (eDP, LVDS, DSI, DPI) | That GPU, VA-API safe budget. Ultra stays hidden and `ultra` / `very_high` collapse to `high`. |
| Discrete AMD or Intel, external output only | That GPU. Ultra is available. The fps ceiling is 120. The saved default stays 60 / `high`. |
| Vendor is not NVIDIA, AMD, or Intel | H.264 on the CPU at 30 fps, scaled to 1280×720 when the user asked for native resolution. |
| No connected output, or sysfs unreadable | Safe budget, requested codec unchanged. |

The same choice is implemented in `apps/gamecap-desktop/gpu.py` and
`crates/gamecap-hardware`. The portal fallback probes VA-API on the display
GPU’s render node first, then the other nodes in the previous order.

## Codecs

gpu-screen-recorder publishes the codecs the display GPU can encode
(`--info`). The desktop caches that list and will not block the record button
on it.

| Request | Probe says the codec exists | Probe says it does not | Probe missing |
| --- | --- | --- | --- |
| `av1` on this 780M | `av1` | falls through to a listed codec | `av1` (unchanged) |
| `h264` on this 780M | `h264` | `av1`, the first listed family | `h264` (unchanged) |
| `av1_hdr` | kept when listed | same family if a sibling is listed (`av1`) | unchanged |
| any, software budget | forced `h264` | forced `h264` | forced `h264` |

CPU encoding in gpu-screen-recorder is H.264 only (`-encoder cpu`). The
software budget sends that flag. The GPU budgets do not, so the command line
on a working AMD or NVIDIA session does not gain an encoder switch.

## What still is not a hardware path

| Environment | Behavior |
| --- | --- |
| Linux, KMS, gpu-screen-recorder installed | Production path. |
| Linux, KMS unavailable | Portal share, then VA-API or NVENC, then OpenH264. |
| Linux, no GPU encoder | CPU H.264, 30 fps ceiling, 720p if the scale was native. |
| Windows `gamecap record` | Refuses. The capture crates are stubs. The desktop UI is the Linux session. |
| macOS | Not a target of this tree. |

Vulkan encode flags and Intel Quick Sync / AMD AMF are recognized by FFmpeg on
some installs and are not selected. VA-API (AMD and Intel) and NVENC (NVIDIA)
are the hardware encoders the portal fallback knows how to open.

## Checks that must keep passing

- Hybrid fixture: NVIDIA with no outputs plus AMD with `eDP-1` chooses AMD and does not allow Ultra.
- AMD with only `HDMI-A-1`, and NVIDIA with a display output, allow Ultra. An Intel panel does not.
- NVIDIA with only `HDMI-A-1` chooses NVIDIA.
- VA-API plan for `ultra` at 240 fps, scale `1920×1080`, codec list
  `{av1, av1_hdr, av1_10bit}` stays `high` / 60 / `av1` / `1920×1080` and does
  not add `-encoder`.
- Software plan with native scale becomes H.264, 30 fps, 1280×720, `-encoder cpu`.
- A missing codec probe does not rewrite the requested codec.
