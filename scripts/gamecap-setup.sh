#!/usr/bin/env bash
# Instalador gráfico de GameCap (sin terminal).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec python3 "$ROOT/apps/gamecap-desktop/setup.py"
