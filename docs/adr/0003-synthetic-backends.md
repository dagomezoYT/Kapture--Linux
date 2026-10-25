# ADR 0003 — Backends sintéticos y adapters nulos

- Estado: aceptado
- Milestone: M0

## Contexto

Hay que probar el pipeline completo sin WGC/PipeWire/NVENC.

## Decisión

- `FakeCaptureBackend` genera frames CPU pequeños con `frame_id` en los primeros 8 bytes. Modo `Deterministic`: timestamps de media exactos, sin sleep.
- “10 s @ 144 FPS” se interpreta como **duración de media**, no wall-clock: `1440` frames, PTS `n/144` s.
- `FakeAudioBackend` emite bloques de 10 ms a 48 kHz, pistas system + micrófono independientes.
- `NullEncoder` no copia el frame completo; solo el encabezado de identidad si el storage ya es CPU. No descarga GPU.
- `NullMuxer` cuenta paquetes y detecta reorder de PTS. No escribe MKV todavía.
- Extensión menor del doc: `AudioEncoder` + `VideoCaptureBackend::is_complete` para fuentes sintéticas. Los backends reales dejan `is_complete = false`.

## Consecuencias

M1/M2 pueden sustituir fakes por backends reales sin cambiar `RecorderSession` si respetan los traits.
