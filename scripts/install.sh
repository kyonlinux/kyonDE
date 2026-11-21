#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/kyonDE"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"

cd "$ROOT"

cargo build --release -p kyonde
cargo build --release -p kyonshell -p kyonctl

mkdir -p "$BIN_DIR" "$CONFIG_DIR" "$DATA_DIR/applications" "$DATA_DIR/wayland-sessions"
install -m 0755 target/release/kyonde "$BIN_DIR/kyonde"
install -m 0755 target/release/kyonshell "$BIN_DIR/kyonshell"
install -m 0755 target/release/kyonctl "$BIN_DIR/kyonctl"
install -m 0755 scripts/kyon-session "$BIN_DIR/kyon-session"

if [ ! -f "$CONFIG_DIR/config.lua" ]; then
    install -m 0644 examples/config.lua "$CONFIG_DIR/config.lua"
fi

sed "s|@BIN_DIR@|$BIN_DIR|g" data/kyonshell-settings.desktop.in > "$DATA_DIR/applications/kyonshell-settings.desktop"
sed "s|@BIN_DIR@|$BIN_DIR|g" data/kyonshell-launcher.desktop.in > "$DATA_DIR/applications/kyonshell-launcher.desktop"
sed "s|@BIN_DIR@|$BIN_DIR|g" data/kyonde-nested.desktop.in > "$DATA_DIR/applications/kyonde-nested.desktop"
install -m 0644 data/kyonde-session.desktop "$DATA_DIR/wayland-sessions/kyonde.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DATA_DIR/applications" >/dev/null 2>&1 || true
fi

echo "Installed kyonde, kyonshell and kyonctl into $BIN_DIR"
echo "Configuration is at $CONFIG_DIR/config.lua"
echo "Start a nested session with: kyon-session"
