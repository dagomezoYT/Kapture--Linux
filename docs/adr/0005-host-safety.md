# ADR 0005 — El motor no muta el hardware y falla hacia la parada

- Estado: aceptado
- Milestone: M0 (extensión de seguridad)

## Contexto

Una grabadora no debe overclockear, tocar firmware/BIOS, escribir en `/sys` sensible ni en dispositivos raw, ni exigir root. Tampoco puede llenar el SSD ni la RAM si el encoder o el disco se atrasan.

## Decisión

- No existe API de control de voltaje, clocks, power limits, ventiladores u overclock. `deny_hardware_mutation` siempre devuelve error.
- Las escrituras de sesión pasan `assert_user_write_path`. `/sys`, `/proc`, `/dev`, `/boot` y dispositivos raw están prohibidos.
- El motor no requiere privilegios elevados. Si se ejecuta como root, solo se registra `elevated_privileges`; no se usa para abrir dispositivos raw.
- Colas acotadas **y** `max_pipeline_bytes`. Si el presupuesto de RAM no alcanza, drop `memory_limit`.
- `StorageProbe` consulta espacio libre antes de `start` y en el hilo gobernador. Si baja de `min_free_bytes`, se **para** la sesión y se finaliza el mux.
- `max_output_bytes` corta la escritura para no saturar el disco por un buffer descontrolado.
- Saturación de encoder/GPU/mux: primero `Degrade` (drops clasificados), después `Stop`. El watchdog corta si el mux no avanza con la cola de paquetes llena.
- Temperaturas y carga se leen en solo lectura (`/proc`, hwmon). Nunca se escribe sysfs.
- Ante error de captura/audio se activa `stop` de inmediato: se abandona la grabación antes que bloquear el equipo.

## Consecuencias

`gamecap-hardware` puede usar `statvfs` (unsafe mínimo, documentado). El core sigue sin tipos de SO. Los backends reales no deben añadir escrituras a cpufreq, nvidia-smi -lgc, ni `/dev/sd*`.
