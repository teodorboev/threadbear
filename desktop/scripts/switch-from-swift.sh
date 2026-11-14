#!/bin/bash
# Installs the GPUI app beside the Swift one and opens it. The Swift app's
# "Start using the new app" starts a thread that runs this (NewApp.swift's
# brief): first with --no-open, fixing whatever stops it, then without once
# the person says Switch now. The Swift app stays and keeps working.
#
#   desktop/scripts/switch-from-swift.sh <checkout> [--no-open]
#
# --no-open stops after installing it, leaving links and the app alone (for
# trying this on a test Mac without taking over its links or its screen).
#
# 1. Rust, if there's no cargo: rustup's own installer, minimal profile,
#    without touching the shell's startup files (docs/on-your-mac.md).
# 2. The app, built from what's committed on the checkout's main, by
#    stage-update.sh: the same build the installed app's updates use, so its
#    clone and target folder are already warm for the first update. Built on
#    an update build host if runner.json names one, else here.
# 3. Moved into ~/Applications/Threadbear GPUI.app, and threadbear:// links
#    pointed at it (thread-links.sh).
# 4. Opened. It brings over the Swift app's settings, drafts and files by
#    itself the first time it starts (desktop/gpui/src/swift_import.rs).
#
# Lines starting "STEP " say where it's got to. The thread runs this from
# main (git show), never the working tree, like the apps' own updates.
# The other scripts it runs come from main the same way.
#
# Exit 0 with the app open, 3 if an update build is already running, 5 if
# runner.json has check hosts but no update build host, 1 otherwise.
set -euo pipefail

CHECKOUT="${1:-}"
OPEN=1
[[ "${2:-}" == --no-open ]] && OPEN=0
[[ -n "$CHECKOUT" && ( -d "$CHECKOUT/.git" || -f "$CHECKOUT/.git" ) ]] || { echo "No checkout to build from (give the repo's path)." >&2; exit 2; }
[ "$(uname -s)" = Darwin ] || { echo "Only for a Mac." >&2; exit 2; }

export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$HOME/.cargo/bin:$PATH"
TARGET="$HOME/Applications/Threadbear GPUI.app"
work="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-switch-XXXX")"
trap 'rm -rf "$work"' EXIT

from_main() {
  git -C "$CHECKOUT" show "refs/heads/main:desktop/scripts/$1" > "$work/$1" || { echo "main has no desktop/scripts/$1." >&2; exit 1; }
  chmod +x "$work/$1"
}

if [[ ! -d "$TARGET" ]]; then
  if ! command -v cargo >/dev/null; then
    echo "STEP Installing Rust"
    # rustup.rs's own script, over https only, as its site says to run it.
    # --no-modify-path: the build finds it in ~/.cargo/bin by itself.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$work/rustup-init.sh"
    sh "$work/rustup-init.sh" -y --profile minimal --no-modify-path
    command -v cargo >/dev/null || { echo "Rust installed, but cargo isn't in ~/.cargo/bin." >&2; exit 1; }
  fi

  echo "STEP Building the new app"
  from_main stage-update.sh
  "$work/stage-update.sh" "$CHECKOUT" --next "$work/Threadbear GPUI.app"

  mkdir -p "$HOME/Applications"
  # Not over one that turned up while it built (installed from a terminal).
  [[ -d "$TARGET" ]] || ditto "$work/Threadbear GPUI.app" "$TARGET"
  echo "Installed $TARGET"
fi

[[ "$OPEN" == 1 ]] || exit 0

echo "STEP Pointing thread links at it"
from_main thread-links.sh
# Not fatal: the app's in, and thread-links.sh gpui moves them later.
"$work/thread-links.sh" gpui || echo "threadbear:// links weren't moved; desktop/scripts/thread-links.sh gpui tries again." >&2

echo "STEP Opening it"
open "$TARGET"
echo "STEP Done"
