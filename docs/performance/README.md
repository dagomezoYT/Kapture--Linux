# Recording performance

Measured on the development host on 26 October 2026, then checked against the
code paths that actually run. Figures below are either timed on that host or
computed from a measured mode. They are not a generic benchmark.

## Host

| Item | Value | How it was read |
| --- | --- | --- |
| CPU | AMD Ryzen 7 260, 8 cores / 16 threads | `lscpu` |
| Memory | 15.6 GiB total, 8.0 GiB available at probe time | `/proc/meminfo` |
| Panel | eDP-1, 1920×1200 @ 180.001 Hz, current mode | Mutter `GetCurrentState` (`is-current`) |
| Display GPU | AMD HawkPoint1 (Radeon 780M), `/dev/dri/card1`, render node `renderD128`, eDP-1 connected | `/sys/class/drm` + `lspci` |
| Discrete GPU | NVIDIA GB206M (GeForce RTX 5070 Max-Q), `/dev/dri/card0`, render node `renderD129`, no connected output | `/sys/class/drm` + `lspci` |
| Session | Wayland, gpu-screen-recorder 6.1.3 | `gpu-screen-recorder --info` |
| Codecs GSR will use on this display GPU | AV1, AV1 HDR, AV1 10-bit | `gpu-screen-recorder --info` (2.95 s) |

The RTX has an eDP connector in sysfs and it is disconnected. Capture and
encode stay on the 780M. Waking the RTX for encode records black and takes
clocks away from the game. That rule is unchanged.

## What “recording” costs

The production path does not encode inside the UI process.

```
UI / hotkey
  → session.start_recording          (settings, audio graph, power profile)
  → gamecap record                   (supervisor)
  → gpu-screen-recorder              (KMS capture + GPU encode + Opus + MKV)
  → status file every 400 ms
stop
  → SIGINT, wait up to 5 s
  → WAV export, optional timestamp lock (ffprobe + ffmpeg)
```

gpu-screen-recorder owns the frame path. GameCap’s supervisor sleeps 400 ms
between status writes. A full encode queue in the in-process fallback degrades
(drops a frame) instead of aborting the file. Disk, memory, and a stuck mux
still stop the session.

### Pixel rate of the stable profile

The panel clock and the encode budget are different workloads. Both numbers
are `width × height × rate`, using the mode Mutter reported and the settings
the recorder already ships (`1920×1080`, 60 fps, quality `high`, AV1).

| Workload | Geometry | Rate | Pixel/s |
| --- | --- | ---: | ---: |
| Panel, current mode | 1920×1200 | 180.001 Hz | 414.7 million |
| Stable encode | 1920×1080 | 60 fps | 124.4 million |

The stable encode is 30% of the panel’s pixel clock. The HUD strip (96 px) is
cropped before encode, so a full-screen take is `1920×1104` and then scaled to
1920×1080 by gpu-screen-recorder. Recording the cropped region at the panel
rate would be about 382 million pixels/s on the same GPU that is drawing the
game. That is the load the 60 fps cap exists to avoid. The cap is a property
of the VA-API budget, not of every GPU. See [Hardware budgets](#hardware-budgets).

### Idle UI path

`apps/gamecap-desktop/test_smoke.py`, after the budget change, no window and
no recording. The monitor list is already cached, so the region helper does
not spawn gpu-screen-recorder.

| Call | Samples | Average | p95 | Budget |
| --- | ---: | ---: | ---: | ---: |
| `controller.poll()` | 80 | 0.03 ms | 0.05 ms | 5 ms |
| `session._region_without_hud(None)` | 80 | 0.00 ms | 0.00 ms | 2 ms |
| `library.list_recordings()` | 20 | 0.11 ms | 0.12 ms | 30 ms |
| `gsr_audio_tracks(both)` | 200 | 0.00 ms | 0.00 ms | 1 ms |

`lspci -mm`, used only to name the GPU in the settings hint, averaged 87 ms
over five calls (first call 129 ms) and is cached for 30 s. It is not on the
capture path.

`gpu-screen-recorder --info` took 2.95 s. The UI starts that probe on a
background thread and caches the codec list for 12 hours under
`~/.cache/gamecap/gsr-codecs.json`. A missing cache does not change the codec
that was requested, so the first record is not blocked on the probe.

## Where time goes after Stop

Stop is the expensive GameCap-owned work, and it runs after the game is no
longer being captured.

1. The supervisor sends `SIGINT` and waits up to 5 s, then `SIGKILL`.
2. If “save audio folder” is on, ffmpeg extracts the Opus masters to 24-bit
   48 kHz WAV before any remux. That order is deliberate: remuxing to AAC
   first is what made the editor WAVs hiss.
3. `ffprobe` reads every video packet timestamp (timeout 180 s) and the audio
   duration. Dropped frames are gaps larger than 1.75 frame periods.
4. The timestamp lock runs only when restamping onto `1/fps` still matches the
   soundtrack within 3% (or the measured rate is within 8% when there is no
   audio duration). gpu-screen-recorder often tags a 60 fps picture as 16 ms
   (62.5 fps). Restamping a take that actually dropped frames would pull the
   picture ahead of the sound, so that take is left alone.

None of this shares a thread with capture. The UI poll stays on the timings
above while a recording is in progress; the HUD is a separate process.

## In-process fallback

If KMS capture cannot start, `gamecap record` asks for a portal share and
encodes in-process. That path copies BGRA through a pipe. It is the slow path.

| Piece | Production fallback | Synthetic `run-fake` |
| --- | --- | --- |
| Video / audio / packet queues | 64 / 32 / 128 | 256 / 256 / 512 |
| Pipeline RAM cap | 384 MiB | 256 MiB default |
| Encode-full retries before a drop | 16 | 50 000 (test only) |
| Encoder order, AMD / Intel / unknown | VA-API AV1, NVENC, VA-API H.264, OpenH264 | null encoder |
| Encoder order, NVIDIA driving the display | NVENC, then VA-API, then OpenH264 | null encoder |

OpenH264 is capped at 60 fps in its encoder config and uploads from the CPU.
It is the last resort, not the profile a working GPU uses. Portal selection
still prefers VA-API AV1 when the display GPU is not NVIDIA, which is this
host: a failed KMS session keeps the previous encoder order.

## Hardware budgets

The desktop asks the display GPU which budget applies, then builds the
gpu-screen-recorder arguments from that budget. The user’s saved 60 fps and
`high` are not rewritten upward.

| Display GPU | Budget | Stable fps ceiling | Quality | Codec if the probe has run | Encoder flag |
| --- | --- | ---: | --- | --- | --- |
| AMD or Intel (this 780M) | `vaapi-safe` | 60 | `medium`, `high` | keep the request when it is listed; otherwise AV1, then HEVC, then H.264 | none (GPU) |
| NVIDIA, and it owns the only connected output | `nvenc` | 240 | adds `very_high`, `ultra` | same fallback | none (GPU) |
| Other PCI vendor (virtio, VMware, …) | `software` | 30 | `medium`, `high` | forced H.264 | `cpu` |
| Probe failed, or no connected output | `vaapi-safe` | 60 | same as AMD/Intel | request kept | none |

`very_high` and `ultra` stay blocked on VA-API. On this 780M they inflated
bitrate and hitched pans. Discrete AMD and Intel cards stay on that safe
budget too: the recorder does not guess that a bigger chip can absorb the
preset. NVIDIA is the exception because the hitch was a VA-API behavior and
NVENC is a different encoder.

Codec fallback uses the list from `gpu-screen-recorder --info`. On this host
that list is AV1 / AV1 HDR / AV1 10-bit. A saved `av1` therefore stays `av1`.
A saved `h264` is rewritten to `av1` only after the cache exists, because H.264
is not in the list this GPU published. If the cache is empty, the requested
codec is sent as-is.

### This host’s plan, checked after the change

`plan_encode` for the saved profile (`av1`, `high`, 60, scale `1920×1080`)
against the live budget and the codec list above:

| Field | Value |
| --- | --- |
| Budget | `vaapi-safe` |
| fps | 60 |
| codec | av1 |
| quality | high |
| scale | 1920×1080 |
| extra encoder flag | none |

Those are the same arguments the recorder already sent. The Rust supervisor
also maps `very_high` / `ultra` back to `high` unless the display vendor is
NVIDIA, so a hand-written CLI command on this laptop cannot bypass the cap.

## What was left alone

- Capture still follows the GPU that owns the connected output. Hybrid
  graphics keep the RTX free for the game.
- Queue sizes, the governor, the HUD crop, the audio export order, and the
  timestamp lock rules are unchanged.
- The in-process core still does not write clocks, firmware, or sysfs.
- No new encoder library was linked. Intel Quick Sync and AMD AMF show up in
  this machine’s `ffmpeg -encoders` list, but Linux capture goes through
  VA-API or NVENC inside gpu-screen-recorder. Quick Sync and AMF are not wired
  into the portal fallback; VA-API covers Intel and AMD there, and OpenH264
  covers a host with neither.
