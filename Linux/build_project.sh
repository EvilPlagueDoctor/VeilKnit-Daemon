#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE="$ROOT/Source/VeilKnitDaemon_src"
GUI="$ROOT/Source/VeilKnitDaemon_GTK"
DIST="$ROOT/dist"
echo "============================================================"
echo " VeilKnit Daemon - Linux console and GTK builds"
echo "============================================================"
echo "Required software (Ubuntu/Zorin):"
echo "  sudo apt update"
echo "  sudo apt install -y build-essential pkg-config libgtk-3-dev curl"
echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
echo
command -v cargo >/dev/null || { echo "ERROR: cargo was not found."; exit 1; }
command -v g++ >/dev/null || { echo "ERROR: g++ was not found."; exit 1; }
pkg-config --exists gtk+-3.0 || { echo "ERROR: GTK 3 development files were not found."; exit 1; }
mkdir -p "$DIST"
export CARGO_INCREMENTAL=0
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$SOURCE=/_/veilknit-daemon/linux"$'\x1f'"--remap-path-prefix=$HOME=/_/home"$'\x1f''-C'$'\x1f''debuginfo=0'$'\x1f''-C'$'\x1f''strip=symbols'
(cd "$SOURCE" && cargo build --release --locked)
install -m 755 "$SOURCE/target/release/veilid_test_node" "$DIST/veilknit-daemon-console"
install -m 755 "$SOURCE/target/release/veilid_test_node" "$DIST/veilknit-daemon"
g++ -std=c++17 -O3 -DNDEBUG -Wall -Wextra -Wpedantic -pthread \
  -ffile-prefix-map="$ROOT=/_/veilknit-daemon" \
  -fdebug-prefix-map="$ROOT=/_/veilknit-daemon" \
  -fmacro-prefix-map="$ROOT=/_/veilknit-daemon" \
  "$GUI/src/main.cpp" -o "$DIST/veilknit-daemon-gui" \
  $(pkg-config --cflags --libs gtk+-3.0)
strip --strip-all "$DIST"/* 2>/dev/null || true
echo "Build complete in: $DIST"
