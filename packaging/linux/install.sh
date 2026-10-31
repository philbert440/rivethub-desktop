#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
prefix="${PREFIX:-$HOME/.local}"
target="${CARGO_TARGET_DIR:-$root/target}"

# Plug-ins' web panels run in WebKitGTK 4.1, which the app loads only when one opens; without it
# the app runs and a panel says what's missing.
if ! pkg-config --exists webkit2gtk-4.1; then
  echo "rivethub-desktop needs WebKitGTK 4.1 for plug-ins' web panels:" >&2
  echo "  Arch: pacman -S webkit2gtk-4.1    Debian/Ubuntu: apt install libwebkit2gtk-4.1-dev" >&2
  exit 1
fi

cargo build --release --manifest-path "$root/Cargo.toml" -p ui -p webview-gtk
install -Dm755 "$target/release/rivethub-desktop" "$prefix/bin/rivethub-desktop"
install -Dm755 "$target/release/librivethub_webview.so" "$prefix/lib/rivethub-desktop/librivethub_webview.so"
install -Dm644 "$root/crates/ui/assets/brand/rh.png" "$prefix/share/icons/hicolor/512x512/apps/rivethub-desktop.png"
install -Dm644 "$root/packaging/linux/rivethub-desktop.desktop" "$prefix/share/applications/rivethub-desktop.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
echo "installed rivethub-desktop to $prefix/bin and its web panel engine to $prefix/lib/rivethub-desktop"
