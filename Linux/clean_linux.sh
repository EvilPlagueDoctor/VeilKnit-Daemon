#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
rm -rf "$ROOT/Source/VeilKnitDaemon_src/target" "$ROOT/dist"
mkdir -p "$ROOT/dist"
echo "Linux build outputs removed. User accounts are not stored in the source target folder."
