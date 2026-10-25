# GameCap

Grabadora de juegos multiplataforma (Windows + Linux) orientada a fidelidad temporal, visual y sonora.

Este repositorio implementa la arquitectura descrita en el documento técnico. **Milestone M0** construye el esqueleto verificable: workspace, tipos, traits, máquina de estados, colas acotadas, métricas y backends sintéticos. No incluye WGC, PipeWire, NVENC ni UI.

## Requisitos

- Rust 1.80+ (`rustc`, `cargo`)
- No hay dependencias de crates.io en las librerías del M0 (solo `std`)

## Ejecutar tests

Desde la raíz del workspace:

```bash
cargo test --workspace
```

Solo el contrato M0 (10 s @ 144 FPS sintéticos):

```bash
cargo test -p gamecap-integration-tests ten_seconds_at_144_fps_is_exact_and_drop_free -- --nocapture
```

Tests unitarios de políticas y tipos:

```bash
cargo test -p gamecap-types
cargo test -p gamecap-core
cargo test -p gamecap-encoder
```

Clippy (el workspace deniega `unwrap`/`expect` en código de librería):

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

## Cómo ejecutarlo

Doble clic en `ejecutar.sh` o en `GameCap.desktop`, o desde la carpeta del proyecto:

```bash
./ejecutar.sh
```

La primera vez compila y deja el binario en `bin/gamecap`. Después solo lanza la prueba (10 s @ 144 FPS sintéticos). **No graba la pantalla todavía.**

Si ya compilaste, también vale:

```bash
./bin/gamecap
```

## Qué cubre M0

- Reloj monotónico común y timestamps racionales (sin float en PTS)
- Traits `VideoCaptureBackend`, `AudioCaptureBackend`, `VideoEncoder`, `Muxer`
- `RecorderSession` con máquina de estados y colas acotadas
- `FakeCaptureBackend` (60/144/240 FPS) y `FakeAudioBackend` (48 kHz, pistas separadas)
- `NullEncoder` / `NullMuxer` para medir timing sin codec
- Drops clasificados (`source_gap`, `encode_backpressure`, `memory_limit`, …); nunca se ocultan en un FPS promedio
- SDR/HDR como pipelines explícitos; el core no convierte GPU→CPU automáticamente
- Límites de RAM y disco; parada o degradación si encoder/GPU/almacenamiento se saturan
- Sin overclock, sin escritura a firmware/BIOS/`/sys`/dispositivos raw, sin exigir root

## Qué no cubre M0

Backends reales (WGC/DXGI, portal/PipeWire, WASAPI), encoders de hardware, contenedor MKV real y la UI de escritorio. Las crates correspondientes existen como stubs compilables para no acoplar el core más adelante.

## Decisiones M0

Ver `docs/adr/`.
