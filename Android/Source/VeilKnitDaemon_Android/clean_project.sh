#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
echo "Cleaning VeilKnit Daemon Android build outputs..."
./gradlew clean >/dev/null 2>&1 || true
rm -rf .gradle .kotlin build app/build native/veilknit-daemon/target \
  app/src/main/jniLibs/arm64-v8a app/src/main/jniLibs/x86_64 dist
echo "Clean complete. SDK settings and source were preserved."
