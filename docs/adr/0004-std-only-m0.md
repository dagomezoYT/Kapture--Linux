# ADR 0004 — Cero dependencias externas en librerías M0

- Estado: aceptado
- Milestone: M0

## Contexto

El primer milestone debe compilar en Windows y Linux con el menor acoplamiento posible.

## Decisión

Las crates de librería usan solo la biblioteca estándar. El sidecar JSON se serializa a mano. El logging de producción (tracing) se añadirá cuando existan backends reales.

## Consecuencias

Menos superficie de supply-chain y builds reproducibles. Cuando se añada `tracing` o `serde`, no deben entrar en `gamecap-core` tipos de OS.
