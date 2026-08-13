#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Cleaning all VeilKnit Daemon projects available on Linux..."
"$ROOT/Linux/clean_project.sh"
"$ROOT/Android/Source/VeilKnitDaemon_Android/clean_project.sh"
echo "All Linux-hosted daemon projects cleaned."
