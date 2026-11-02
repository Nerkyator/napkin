#!/bin/sh
# Installs Napkin for the current user: binary in ~/.local/bin, launcher entry and icon.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cargo build --release --locked --manifest-path "$root/Cargo.toml" -p napkin
install -Dm755 "$root/target/release/napkin" "$HOME/.local/bin/napkin"
install -Dm644 "$root/packaging/napkin.desktop" \
  "$HOME/.local/share/applications/napkin.desktop"
install -Dm644 "$root/packaging/napkin.svg" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/napkin.svg"
# Entries from before Napkin took its short name.
rm -f "$HOME/.local/share/applications/io.github.nerkyator.Napkin.desktop" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.nerkyator.Napkin.svg"
gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$HOME/.local/share/applications" 2>/dev/null || true
echo "Napkin installed in $HOME/.local/bin/napkin"
