#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
prefix="${PREFIX:-$HOME/.local}"
target="${CARGO_TARGET_DIR:-$root/target}"

cargo build --release --manifest-path "$root/Cargo.toml" -p ui
install -Dm755 "$target/release/rivethub-desktop" "$prefix/bin/rivethub-desktop"
install -Dm644 "$root/crates/ui/assets/brand/rh.png" "$prefix/share/icons/hicolor/512x512/apps/rivethub-desktop.png"
install -Dm644 "$root/packaging/linux/rivethub-desktop.desktop" "$prefix/share/applications/rivethub-desktop.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
echo "installed rivethub-desktop to $prefix/bin"
