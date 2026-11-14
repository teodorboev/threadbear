#!/bin/bash
# A shared Cargo target can call another checkout's path crates fresh. Force
# inputs newer when ownership changes, and invalidate before any build starts
# so a failed/interrupted build cannot leave a previous owner's valid marker.
set -euo pipefail
key="${THREADBEAR_PERF_BUILD_KEY:-}"
source="${THREADBEAR_PERF_BUILD_SOURCE:-}"
if [ -z "$key" ] || [ -z "$source" ]; then exec "$@"; fi
marker="$CARGO_TARGET_DIR/.perf-built-by"
mkdir -p "$CARGO_TARGET_DIR"
if [ "$(cat "$marker" 2>/dev/null || true)" != "$key" ]; then
  inputs=("$source/desktop")
  for d in Art Rich; do
    [ ! -d "$source/app/Sources/Threadbear/Resources/$d" ] || inputs+=("$source/app/Sources/Threadbear/Resources/$d")
  done
  find "${inputs[@]}" \( -type f -o -type l \) -not -path "$source/desktop/target/*" -exec touch {} +
  rm -f "$marker"
fi
# Preparing an unchanged owner hasn't built anything yet. Preserve its valid
# ownership until Cargo really starts, so the PATH wrapper keeps a warm build.
[ "${1:-}" != --prepare ] || exit 0
rm -f "$marker"
"$@"
case "${THREADBEAR_PERF_BUILD_ACTION:-build}" in
  build|check|clippy|rustc|test|run|bench)
    printf '%s\n' "$key" >"$marker.$$.tmp"
    mv "$marker.$$.tmp" "$marker" ;;
esac
