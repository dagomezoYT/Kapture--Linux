# ADR 0002 — Colas acotadas y backpressure

- Estado: aceptado
- Milestone: M0

## Contexto

Una cola infinita desplaza el fallo a RAM y latencia. La captura no debe esperar al disco.

## Decisión

- Colas implementadas en `gamecap-core::queue` con `Mutex` + `Condvar` (sin crate externo).
- El hilo de captura solo hace `try_send`. Reintentos cortos (`yield_now`) son configurables; agotados → drop `encode_backpressure`.
- El encoder puede esperar un timeout breve al mux (`send_timeout`); si sigue lleno → `mux_backpressure`.
- Los drops se contabilizan por motivo y aparecen en el sidecar. No se sustituyen por un FPS promedio.

## Consecuencias

Los tests deterministas usan colas holgadas y reintentos altos porque `NullEncoder` es instantáneo y el scheduler puede burstar 1440 frames. Un encoder lento con cola pequeña debe producir drops clasificados.
