#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export LIQINIR_RUNTIME_DIR="${XDG_RUNTIME_DIR:?}/liqinir-demo"
rm -rf "$LIQINIR_RUNTIME_DIR"
mkdir -p "$LIQINIR_RUNTIME_DIR"
"$ROOT/target/debug/liqinir" daemon --demo --config "$ROOT/config/config.toml" &
daemon_pid=$!
trap 'kill "$daemon_pid" 2>/dev/null || true' EXIT
sleep .3
quickshell -p "$ROOT/shell.qml" &
qs_pid=$!
trap 'kill "$qs_pid" 2>/dev/null || true; kill "$daemon_pid" 2>/dev/null || true' EXIT
wait "$qs_pid"
