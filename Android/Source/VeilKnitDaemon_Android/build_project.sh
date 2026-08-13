#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
echo "============================================================"
echo " VeilKnit Daemon - Android debug build"
echo "============================================================"
echo "Required software (Ubuntu/Zorin host):"
echo "  sudo apt update && sudo apt install -y openjdk-21-jdk unzip curl"
echo "  Install Android Studio/SDK/NDK from developer.android.com/studio"
echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
echo "  cargo install cargo-ndk"
echo
command -v java >/dev/null || { echo "ERROR: Java was not found."; exit 1; }
command -v cargo >/dev/null || { echo "ERROR: cargo was not found."; exit 1; }
cargo ndk --version >/dev/null 2>&1 || cargo install cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
./gradlew :app:assembleDebug
mkdir -p dist
cp -f app/build/outputs/apk/debug/VeilKnitDaemon-debug.apk dist/
echo "Build complete: dist/VeilKnitDaemon-debug.apk"
