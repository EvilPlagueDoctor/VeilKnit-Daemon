#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Cleaning VeilKnit Daemon Linux build outputs..."
rm -rf "$ROOT/Source/VeilKnitDaemon_src/target" "$ROOT/dist"
find "$ROOT" -type f \( -name '*.o' -o -name '*.so' -o -name '*.a' -o -name '*.d' \) -delete 2>/dev/null || true
echo "Clean complete. Source and user data were preserved."
