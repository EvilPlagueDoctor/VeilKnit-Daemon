#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Required: Rust, build-essential, GTK 3 development files, Android SDK/NDK, and JDK 21."
echo "Install commands (Ubuntu/Zorin):"
echo "  sudo apt update && sudo apt install -y build-essential pkg-config libgtk-3-dev openjdk-21-jdk unzip curl"
echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
echo "  cargo install cargo-ndk"
echo "  Install Android Studio/SDK/NDK from developer.android.com/studio"
echo
echo "Building all VeilKnit Daemon projects available on Linux..."
"$ROOT/Linux/build_project.sh"
"$ROOT/Android/Source/VeilKnitDaemon_Android/build_project.sh"
echo "All Linux-hosted daemon projects built successfully."
