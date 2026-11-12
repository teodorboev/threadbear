#!/usr/bin/env bash
# Gradle for this project, kept small enough for the 8 GB Hetzner box:
# a memory cap on the whole build, no daemon left behind, one build at a time.
#   ./build.sh :kit:test
#   ./build.sh :app:assembleDebug
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
cd "$(dirname "$self")"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
lock="${TMPDIR:-/tmp}/threadbear-android-build.lock"
if command -v flock >/dev/null; then
  exec 9>"$lock"
  if ! flock -n 9; then
    echo "Waiting for another Android build to finish..." >&2
    flock 9
  fi
elif [ -z "${THREADBEAR_ANDROID_LOCK_HELD:-}" ] && [ -x /usr/bin/lockf ]; then
  # macOS has no flock(1); its lockf(1) wraps the command instead, so run
  # this script again inside it (the env var stops it wrapping twice).
  THREADBEAR_ANDROID_LOCK_HELD=1 exec /usr/bin/lockf -k "$lock" "$self" "$@"
fi
# A Mac keeps the SDK where Android Studio puts it, not ~/android-sdk.
[ -d "$ANDROID_HOME" ] || [ ! -d "$HOME/Library/Android/sdk" ] || export ANDROID_HOME="$HOME/Library/Android/sdk"
if command -v systemd-run >/dev/null && systemctl --user show-environment >/dev/null 2>&1; then
  exec systemd-run --user --scope -q -p MemoryMax=5.5G -- ./gradlew --no-daemon "$@"
fi
exec ./gradlew --no-daemon "$@"
