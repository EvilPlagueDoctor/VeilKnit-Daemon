#!/usr/bin/env bash
set -euo pipefail
if [[ $# -lt 1 ]]; then echo "Usage: $0 <artifact-or-directory> [private-token ...]"; exit 2; fi
TARGET="$1"; shift
TOKENS=("${USER:-}" "${HOME:-}" "/Users/" "C:\\Users\\" "Desktop\\" "/home/")
TOKENS+=("$@")
FAIL=0
while IFS= read -r -d '' file; do
  for token in "${TOKENS[@]}"; do
    [[ -z "$token" ]] && continue
    if strings -a -el "$file" 2>/dev/null | grep -F -i -q -- "$token" || strings -a "$file" 2>/dev/null | grep -F -i -q -- "$token"; then
      echo "POTENTIAL METADATA LEAK: $file contains: $token"
      FAIL=1
    fi
  done
done < <(find "$TARGET" -type f -print0 2>/dev/null || printf '%s\0' "$TARGET")
exit "$FAIL"
