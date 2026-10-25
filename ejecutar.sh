#!/usr/bin/env bash
# Lanzador de GameCap: interfaz gráfica, o CLI si pasas argumentos.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

if [[ -x "$ROOT/.toolchain/cargo/bin/cargo" ]]; then
  export RUSTUP_HOME="$ROOT/.toolchain/rustup"
  export CARGO_HOME="$ROOT/.toolchain/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
fi

unset CARGO_TARGET_DIR
export CARGO_TARGET_DIR="$ROOT/target"

BIN="$ROOT/bin/gamecap"

if [[ $# -eq 0 || "${1:-}" == "gui" ]]; then
  if [[ ! -x "$BIN" ]]; then
    exec python3 "$ROOT/apps/gamecap-desktop/setup.py"
  fi
  exec python3 "$ROOT/apps/gamecap-desktop/ui.py"
fi

# Hotkeys must never wait on cargo rebuild (pause/stop need to be instant).
if [[ "${1:-}" == "toggle" ]]; then
  exec python3 "$ROOT/apps/gamecap-desktop/toggle.py"
fi
if [[ "${1:-}" == "pause" ]]; then
  exec python3 "$ROOT/apps/gamecap-desktop/pause.py"
fi
if [[ "${1:-}" == "screenshot" ]]; then
  exec python3 "$ROOT/apps/gamecap-desktop/screenshot.py"
fi
if [[ "${1:-}" == "replay" ]]; then
  exec python3 "$ROOT/apps/gamecap-desktop/replay.py"
fi

need_build=0
if [[ ! -x "$BIN" ]]; then
  need_build=1
elif find "$ROOT/crates" "$ROOT/apps" -name '*.rs' -newer "$BIN" | grep -q .; then
  need_build=1
fi

if [[ "$need_build" -eq 1 ]]; then
  echo "Compilando GameCap..."
  mkdir -p "$ROOT/bin"
  cargo build -p gamecap-cli --release
  cp -f "$ROOT/target/release/gamecap" "$BIN"
  chmod +x "$BIN"
fi

set +e
"$BIN" "$@"
status=$?
set -e

if [[ -t 0 ]]; then
  echo
  read -r -p "Pulsa Enter para cerrar..."
fi
exit "$status"
