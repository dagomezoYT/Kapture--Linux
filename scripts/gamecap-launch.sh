#!/usr/bin/env bash
# Lanza GameCap sin terminal. Si falta el binario, abre el instalador gráfico.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -x "$ROOT/.toolchain/cargo/bin/cargo" ]]; then
  export RUSTUP_HOME="$ROOT/.toolchain/rustup"
  export CARGO_HOME="$ROOT/.toolchain/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
fi

BIN="$ROOT/bin/gamecap"
UI="$ROOT/apps/gamecap-desktop/ui.py"
SETUP="$ROOT/apps/gamecap-desktop/setup.py"

if [[ ! -x "$BIN" ]]; then
  exec python3 "$SETUP"
fi
exec python3 "$UI"
