#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${HOME}/.local"
CONFIG="${XDG_CONFIG_HOME:-${HOME}/.config}"
STATE_DIR="${HOME}/.local/state/liqinir"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/liqinir"
NOCTALIA_PLUGIN_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/noctalia/plugins/liqinir-island"

command -v cargo >/dev/null || { echo "需要 cargo" >&2; exit 1; }
command -v quickshell >/dev/null || { echo "需要 quickshell 0.3+" >&2; exit 1; }
install -d -m700 "$STATE_DIR" "$RUNTIME_DIR"
cargo build --release --manifest-path "$ROOT/Cargo.toml"
install -Dm755 "$ROOT/target/release/liqinir" "$PREFIX/bin/liqinir"
install -Dm644 "$ROOT/integrations/dito/island-adapter.mjs" "$PREFIX/share/liqinir/dito/island-adapter.mjs"
install -Dm644 "$ROOT/shell.qml" "$CONFIG/quickshell/liqinir/shell.qml"
install -Dm644 "$ROOT/niri/liqinir-glass.kdl" "$CONFIG/niri/liqinir-glass.kdl"
install -Dm644 "$ROOT/systemd/liqinir.service" "${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user/liqinir.service"
install -Dm644 "$ROOT/systemd/liqinir-island.service" "${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user/liqinir-island.service"
install -Dm644 "$ROOT/config/config.toml" "$CONFIG/liqinir/config.toml"
mkdir -p "$CONFIG/DankMaterialShell/plugins"
cp -a "$ROOT/integrations/dms/LiqinirIslandBridge" "$CONFIG/DankMaterialShell/plugins/"
rm -rf "$NOCTALIA_PLUGIN_DIR"
install -d "$NOCTALIA_PLUGIN_DIR"
cp -a "$ROOT/integrations/noctalia/LiqinirIsland/." "$NOCTALIA_PLUGIN_DIR/"
install -d "$CONFIG/noctalia"
cat > "$CONFIG/noctalia/liqinir.toml" <<'EOF'
[widget.liqinir-island]
type = "liqinir/island:island"
enabled = true

[bar.main]
center = ["liqinir-island"]
EOF

NIRI_CONFIG="${CONFIG}/niri/config.kdl"
if [[ -f "$NIRI_CONFIG" ]] && ! grep -Fq 'liqinir-glass.kdl' "$NIRI_CONFIG"; then
  cp -a "$NIRI_CONFIG" "${NIRI_CONFIG}.bak-liqinir-$(date +%Y%m%d%H%M%S)"
  printf '\n// Liqinir liquid glass effects\ninclude "liqinir-glass.kdl"\n' >> "$NIRI_CONFIG"
fi

systemctl --user daemon-reload
systemctl --user disable --now liqinir-island.service 2>/dev/null || true
systemctl --user enable --now liqinir.service
if command -v noctalia >/dev/null; then
  NOCTALIA_STATE="${XDG_STATE_HOME:-${HOME}/.local/state}/noctalia/settings.toml"
  if [[ -f "$NOCTALIA_STATE" ]] && ! grep -Fq '"liqinir-island"' "$NOCTALIA_STATE"; then
    cp -a "$NOCTALIA_STATE" "${NOCTALIA_STATE}.bak-liqinir-$(date +%Y%m%d%H%M%S)"
    python - "$NOCTALIA_STATE" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
lines = path.read_text().splitlines()
section = None
for index, line in enumerate(lines):
    stripped = line.strip()
    if stripped.startswith("[") and stripped.endswith("]"):
        section = stripped[1:-1]
    if section == "bar.main" and stripped.startswith("center ="):
        lines[index] = 'center = [ "liqinir-island" ]'
        break
else:
    raise SystemExit(0)
path.write_text("\n".join(lines) + "\n")
PY
  fi
  noctalia msg plugins enable liqinir/island >/dev/null 2>&1 || true
  noctalia msg config-reload >/dev/null 2>&1 || true
fi
echo "已安装。Liqinir 已嵌入 Noctalia 顶栏；DMS 插件将在下次 DMS 重启后转发通知：dms restart"
