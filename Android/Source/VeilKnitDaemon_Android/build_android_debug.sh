#!/usr/bin/env sh
set -eu
command -v cargo >/dev/null 2>&1 || { echo "Rust/Cargo is required." >&2; exit 1; }
cargo ndk --version >/dev/null 2>&1 || cargo install cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
./gradlew :app:assembleDebug :mailer:assembleDebug
mkdir -p dist
cp -f app/build/outputs/apk/debug/VeilKnitDaemon-debug.apk dist/
cp -f mailer/build/outputs/apk/debug/VeilKnitMailer-debug.apk dist/
echo "Built the single daemon and Mailer APKs in dist/"
