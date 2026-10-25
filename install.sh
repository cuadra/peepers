#!/usr/bin/env bash
# Install peepers for the current user (no root needed).
#   ./install.sh            use the prebuilt binary in bin/
#   ./install.sh --build    build from source with cargo first
#   ./install.sh --uninstall
set -euo pipefail
cd "$(dirname "$0")"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
CFG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/peepers"

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$BIN_DIR/peepers" "$APP_DIR/peepers.desktop"
  echo "Removed peepers (config left in $CFG_DIR)."
  exit 0
fi

if [[ "${1:-}" == "--build" || ! -f bin/peepers ]]; then
  command -v cargo >/dev/null || { echo "cargo not found: sudo pacman -S rust"; exit 1; }
  echo "Building (release, first build takes a few minutes)…"
  cargo build --release --locked
  mkdir -p bin && cp target/release/peepers bin/peepers
fi

# Migrate from the app's earlier name, "still".
if [[ -e "$BIN_DIR/still" || -e "$APP_DIR/still.desktop" ]]; then
  rm -f "$BIN_DIR/still" "$APP_DIR/still.desktop"
  echo "Removed the old 'still' install."
fi
OLD_CFG="${XDG_CONFIG_HOME:-$HOME/.config}/still/config.toml"
if [[ -f "$OLD_CFG" && ! -f "$CFG_DIR/config.toml" ]]; then
  install -Dm644 "$OLD_CFG" "$CFG_DIR/config.toml"
  echo "Copied your config from ~/.config/still."
fi

install -Dm755 bin/peepers "$BIN_DIR/peepers"
install -Dm644 assets/peepers.desktop "$APP_DIR/peepers.desktop"
sed -i "s|^Exec=peepers|Exec=$BIN_DIR/peepers|; s|^TryExec=peepers|TryExec=$BIN_DIR/peepers|" "$APP_DIR/peepers.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$APP_DIR" 2>/dev/null || true

if [[ ! -f "$CFG_DIR/config.toml" ]]; then
  install -Dm644 extras/config.toml "$CFG_DIR/config.toml"
fi

echo "Installed: $BIN_DIR/peepers"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "Note: add $BIN_DIR to your PATH.";; esac
echo
echo "Try:   peepers ~/Images"
echo "Keys:  ? shortcuts · i info · h/l prev/next · j/k zoom in/out · HJKL pan · Esc reset · q quit"
echo "Hyprland rules: extras/hyprland.conf   Yazi opener: extras/yazi.toml"
