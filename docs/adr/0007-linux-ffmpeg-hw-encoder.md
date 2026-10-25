# ADR 0007 — Linux M1: encoder FFmpeg delimitado + cola de encode

- Estado: aceptado
- Milestone: M1

## Contexto

OpenH264 por software no sostiene 1080p a ~60 FPS en este host: la cola de
vídeo se llena, el gobernador abortaba con `encoder_saturation` y el `.mkv`
quedaba corto. FFmpeg del sistema ya lista `h264_nvenc` y `h264_vaapi`, pero
no hay `ffmpeg-devel` para enlazar libav.

## Decisión

- `gamecap-encoder-ffmpeg` puede spawnar un `ffmpeg` dedicado: BGRA por stdin,
  Annex-B H.264 por stdout. Orden: `h264_nvenc` → `h264_vaapi` → OpenH264.
- OpenH264 queda como fallback; `UsageType::CameraVideoRealTime` (ScreenContent
  dispara avisos no soportados).
- Cola de encode llena = `Degrade` (drops acotan RAM). No `Stop` por
  `EncoderSaturation`. Disco, mux atascado y memoria siguen parando.

## Consecuencias

M1 puede usar HW encode sin pegar libav al core. El siguiente paso de
fidelidad es NVENC/VAAPI vía libav y DMA-BUF, sin tocar traits públicos.
