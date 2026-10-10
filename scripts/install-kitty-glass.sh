#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG="${XDG_CONFIG_HOME:-${HOME}/.config}"
KITTY_DIR="$CONFIG/kitty"
NIRI_DIR="$CONFIG/niri"
KITTY_CONF="$KITTY_DIR/kitty.conf"
NIRI_CONF="$NIRI_DIR/config.kdl"
BACKUP_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/liqinir/backups/kitty-glass-$(date +%Y%m%d-%H%M%S)"

[[ -f "$KITTY_CONF" ]] || { echo "找不到 Kitty 配置：$KITTY_CONF" >&2; exit 1; }
[[ -f "$NIRI_CONF" ]] || { echo "找不到 niri 配置：$NIRI_CONF" >&2; exit 1; }

install -d -m700 "$KITTY_DIR" "$NIRI_DIR" "$BACKUP_DIR"
cp -a "$KITTY_CONF" "$BACKUP_DIR/kitty.conf"
cp -a "$NIRI_CONF" "$BACKUP_DIR/niri-config.kdl"

install -Dm644 "$ROOT/kitty/liqinir-glass.conf" "$KITTY_DIR/liqinir-glass.conf"
install -Dm644 "$ROOT/niri/kitty-glass.kdl" "$NIRI_DIR/kitty-glass.kdl"

if ! grep -Fqx 'include liqinir-glass.conf' "$KITTY_CONF"; then
    printf '\n# Liqinir liquid-glass profile\ninclude liqinir-glass.conf\n' >> "$KITTY_CONF"
fi

if ! grep -Fqx 'include "kitty-glass.kdl"' "$NIRI_CONF"; then
    printf '\n// Liqinir Kitty liquid-glass window surface\ninclude "kitty-glass.kdl"\n' >> "$NIRI_CONF"
fi

echo "Kitty 液态玻璃已安装。备份：$BACKUP_DIR"
echo "重启 Kitty 窗口即可看到效果；niri 会自动重新加载配置。"
