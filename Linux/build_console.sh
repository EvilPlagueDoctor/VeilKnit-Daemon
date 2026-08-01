#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE="$ROOT/Source/VeilKnitDaemon_src"
DIST="$ROOT/dist"
command -v cargo >/dev/null 2>&1 || { echo "Rust/Cargo was not found. Install Rust with rustup first."; exit 1; }
mkdir -p "$DIST"
export CARGO_INCREMENTAL=0
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$SOURCE=/_/veilknit-daemon/linux"$'\x1f'"--remap-path-prefix=$HOME=/_/home"$'\x1f''-C'$'\x1f''debuginfo=0'$'\x1f''-C'$'\x1f''strip=symbols'
cd "$SOURCE"
cargo build --release --locked
install -m 755 target/release/veilid_test_node "$DIST/veilknit-daemon-console"
strip --strip-all "$DIST/veilknit-daemon-console" 2>/dev/null || true
echo "Built: $DIST/veilknit-daemon-console"
