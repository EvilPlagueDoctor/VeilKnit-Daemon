#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_SOURCE="$ROOT/Source/VeilKnitDaemon_src"
GUI_SOURCE="$ROOT/Source/VeilKnitDaemon_GTK"
DIST="$ROOT/dist"
command -v cargo >/dev/null 2>&1 || { echo "Rust/Cargo was not found. Install Rust with rustup first."; exit 1; }
command -v g++ >/dev/null 2>&1 || { echo "g++ was not found. Install build-essential."; exit 1; }
pkg-config --exists gtk+-3.0 || { echo "GTK 3 development files were not found. Install libgtk-3-dev and pkg-config."; exit 1; }
mkdir -p "$DIST"
export CARGO_INCREMENTAL=0
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$BACKEND_SOURCE=/_/veilknit-daemon/linux"$'\x1f'"--remap-path-prefix=$HOME=/_/home"$'\x1f''-C'$'\x1f''debuginfo=0'$'\x1f''-C'$'\x1f''strip=symbols'
cd "$BACKEND_SOURCE"
cargo build --release --locked
install -m 755 target/release/veilid_test_node "$DIST/veilknit-daemon"
strip --strip-all "$DIST/veilknit-daemon" 2>/dev/null || true
g++ -std=c++17 -O3 -DNDEBUG -Wall -Wextra -Wpedantic -pthread \
    -ffile-prefix-map="$ROOT=/_/veilknit-daemon" \
    -fdebug-prefix-map="$ROOT=/_/veilknit-daemon" \
    -fmacro-prefix-map="$ROOT=/_/veilknit-daemon" \
    "$GUI_SOURCE/src/main.cpp" -o "$DIST/veilknit-daemon-gui" \
    $(pkg-config --cflags --libs gtk+-3.0)
strip --strip-all "$DIST/veilknit-daemon-gui" 2>/dev/null || true
echo "Built: $DIST/veilknit-daemon-gui"
echo "Backend: $DIST/veilknit-daemon"
