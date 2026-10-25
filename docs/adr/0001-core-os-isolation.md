# ADR 0001 — Aislamiento del core respecto al SO

- Estado: aceptado
- Milestone: M0

## Contexto

El documento de arquitectura exige que `gamecap-core` no llame APIs de plataforma y que los tipos nativos no crucen esa frontera.

## Decisión

- `gamecap-types` modela GPU storage con `NativeHandle` y `DmaBufPlane.fd_id` opacos. No se usa `RawFd`, `HWND` ni `ID3D11Texture2D` en types/core.
- Los traits viven en `gamecap-capture`, `gamecap-audio`, `gamecap-encoder` y `gamecap-mux`.
- Las crates `*-windows` / `*-linux` / `*-nvenc` existen en el workspace pero no tienen FFI en M0.
- El core no contiene `cfg(windows)` / `cfg(unix)` en lógica de dominio.

## Consecuencias

Los backends reales (M1/M2) implementarán los mismos traits. Cambiar una interfaz pública del core exigirá un ADR nuevo.
