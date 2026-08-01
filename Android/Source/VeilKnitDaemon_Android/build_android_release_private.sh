#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
./gradlew --no-daemon clean assembleRelease
echo "Release APKs are under app/build/outputs/apk/release and mailer/build/outputs/apk/release."
