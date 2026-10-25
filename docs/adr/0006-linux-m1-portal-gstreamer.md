# ADR 0006 — Linux M1: portal + adaptador GStreamer delimitado

- Estado: aceptado
- Milestone: M1

## Contexto

El core ya orquesta captura → encode → mux. En este host (Fedora/GNOME/Wayland) hay runtime de PipeWire, GStreamer `pipewiresrc` y `libopenh264`, pero **no** `pipewire-devel` ni cabeceras de libav. Enlazar `pipewire-rs` / `ffmpeg-next` exigiría mutar el sistema.

## Decisión

- El crate `gamecap-capture-linux` abre la sesión con **xdg-desktop-portal** (`ashpd`) y obtiene el nodo PipeWire.
- Los buffers se leen con un proceso **GStreamer delimitado** (`gst-launch-1.0 pipewiresrc … ! videoconvert ! BGRx ! fdsink`) hacia un `UnixStream`. No se pega GStreamer al core.
- Audio de sistema: `pulsesrc` del monitor PipeWire/Pulse, mismo patrón. Si no hay monitor, silencio explícito.
- Vídeo: encoder delimitado en `gamecap-encoder-ffmpeg` (FFmpeg NVENC/VAAPI o OpenH264). Ver ADR 0007.
- Contenedor: muxer MKV propio (`A_PCM/FLOAT/IEEE` + `V_MPEG4/ISO/AVC`).
- El core no importa tipos de portal/PipeWire. Los traits públicos no cambian.

## Consecuencias

M1 graba un `.mkv` reproducible en Wayland/GNOME. El siguiente paso de fidelidad es sustituir GStreamer por `libpipewire` (DMA-BUF) y OpenH264 por NVENC/VAAPI vía libav, sin tocar `gamecap-core`.
