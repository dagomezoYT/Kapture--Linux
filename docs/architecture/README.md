# Arquitectura

GameCap separa el dominio (`gamecap-core`, `gamecap-types`) de los backends de plataforma. El core nunca importa APIs de Windows ni Linux.

Flujo M1 (Linux, captura real):

```
Portal+PipeWire (GStreamer SHM) -> VideoQueue -> OpenH264 -> PacketQueue -+
Pulse monitor / silencio        -> AudioQueue -> PCM pass  --------------+--> MKV -> archivo + JSON
```

`run-fake` sigue disponible para el pipeline sintético de M0.

La captura no espera al disco. Si la cola de encode está llena, el frame se descarta con `encode_backpressure`.

El gobernador observa espacio libre, bytes en cola, saturación y telemetría de solo lectura. Si el host se satura, degrada o **para** la grabación; nunca muta clocks, ventiladores ni firmware.

El costo del modo de grabación y el presupuesto por tipo de GPU están en `docs/performance/README.md` (inglés). La matriz de hardware está en `docs/compatibility/README.md`.
