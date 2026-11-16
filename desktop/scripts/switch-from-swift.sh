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
# Exit 0 with the app open, 3 if an update build is already running, 4 if
# none of runner.json's update build hosts could build it, 5 if runner.json
# has check hosts but no update build host, 6 if the disk hasn't room for
# it, 1 otherwise.
#
# Tried as a stranger's copy on Tinker Desk (15 Nov 2026): with no Rust and
# nothing built before, Rust and the build took 31 minutes at background
# priority and about 4 GB (Rust 1, the build's clone and target 2.3).
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

# Free space on the home folder's disk, in KB, or nothing if it couldn't be
# read. From statvfs, not df's columns, which a file system name with a
# space in it (an SMB home) shifts. A negative figure (df's, past the disk's
# reserve) is none. THREADBEAR_SWITCH_FREE_KB stands in for it in the tests
# (switch-from-swift.test.ts).
free_kb() {
  local kb
  if [[ -n "${THREADBEAR_SWITCH_FREE_KB:-}" ]]; then kb="$THREADBEAR_SWITCH_FREE_KB"
  else kb="$(/usr/bin/python3 -I -c 'import os,sys; s = os.statvfs(sys.argv[1]); print(s.f_bavail * s.f_frsize // 1024)' "$HOME" 2>/dev/null)" || kb=""
  fi
  if [[ "$kb" =~ ^-[0-9]+$ ]]; then echo 0; elif [[ "$kb" =~ ^[0-9]+$ ]]; then echo "$kb"; fi
}

# Whether stage-update.sh will build somewhere else or not at all: runner.json
# names an update build host it would use, or a check host (it then builds
# nothing and exits 5). The same reading as stage-update.sh's hosts_in, so the
# two agree on a name it would refuse.
builds_elsewhere() {
  /usr/bin/python3 -I -c 'import json,sys
try: c = json.load(open(sys.argv[1]))
except Exception: c = {}
if not isinstance(c, dict): c = {}
def hosts(key):
    h = c.get(key) or []
    if isinstance(h, str): h = [h]
    if not isinstance(h, list): h = []
    return [x for x in h if isinstance(x, str) and x and not x.startswith("-") and x.strip() == x and " " not in x]
print("yes" if hosts("updateBuildHosts") or hosts("checkHosts") or hosts("checkHost") else "")' "${THREADBEAR_HOME:-$HOME/Library/Application Support/Threadbear}/runner.json" 2>/dev/null || true
}

# Room for it before any of it starts, rather than a full disk half an hour
# into the build (a 2.6 GB disk filled up 16 minutes in on Tinker Desk, 15
# Nov). Rust about 1 GB. A build here about 4 all told (2.3 measured, with
# the bundle and room to spare), less whatever an earlier try already left
# in the cache (measured, not guessed from a folder cargo makes at once),
# never under 1. Built elsewhere, or not at all, 1 for the clone and bundle
# and no Rust.
room_for_it() {
  local need_kb=0 gib=$((1024 * 1024)) cache="$HOME/Library/Caches/Threadbear/update-desktop"
  if [[ -n "$elsewhere" ]]; then
    need_kb=$gib
  else
    command -v cargo >/dev/null || need_kb=$gib
    local cached=0 build=$((4 * gib))
    [[ -d "$cache" ]] && cached="$(du -sk "$cache" 2>/dev/null | awk '{ print $1 }')" || true
    [[ "$cached" =~ ^[0-9]+$ ]] || cached=0
    build=$((build - cached))
    (( build >= gib )) || build=$gib
    need_kb=$((need_kb + build))
  fi
  local have
  have="$(free_kb)"
  # Unreadable: the build finds out for itself.
  [[ -n "$have" ]] || return 0
  if (( have < need_kb )); then
    gb() { awk -v k="$1" 'BEGIN { printf "%.1f", k / 1048576 }'; }
    echo "Installing the new app needs about $(gb "$need_kb") GB free on this Mac's disk (Rust and the build's files), and it has $(gb "$have") GB. Free some space and run this again." >&2
    exit 6
  fi
}

# What stage-update.sh's exit means here, where no app is installed yet to
# try again by itself.
build_failed() {
  case "$1" in
    3) echo "Another build of the new app is running. Wait for it to finish, then run this again." >&2 ;;
    4) echo "None of runner.json's updateBuildHosts could build it just now (above says why). Run this again in a while, or take updateBuildHosts, checkHosts and checkHost out of runner.json to build on this Mac." >&2 ;;
    5) echo "runner.json has check hosts (checkHosts or checkHost) but no updateBuildHosts, so nothing built it: this Mac is kept free of heavy builds. Ask which Mac should build it, and name it in updateBuildHosts." >&2 ;;
    *) echo "The build failed (exit $1); its output is above, and in ~/Library/Caches/Threadbear/update-desktop." >&2 ;;
  esac
  exit "$1"
}

# Installed means both its binaries are there and not empty: a folder left
# by a copy that was stopped partway isn't, and is replaced.
installed() {
  local bin
  for bin in threadbear-gpui threadbear-web-helper; do
    [[ -x "$TARGET/Contents/MacOS/$bin" && -s "$TARGET/Contents/MacOS/$bin" ]] || return 1
  done
}

if ! installed; then
  elsewhere="$(builds_elsewhere)"
  room_for_it
  # Rust only for a build here: another Mac builds it, or none does (exit 5).
  if [[ -z "$elsewhere" ]] && ! command -v cargo >/dev/null; then
    echo "STEP Installing Rust"
    # rustup.rs's own script, over https only, as its site says to run it.
    # --no-modify-path: the build finds it in ~/.cargo/bin by itself.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$work/rustup-init.sh"
    sh "$work/rustup-init.sh" -y --profile minimal --no-modify-path
    command -v cargo >/dev/null || { echo "Rust installed, but cargo isn't in ~/.cargo/bin." >&2; exit 1; }
  fi

  echo "STEP Building the new app (a first build takes 20 to 40 minutes, its later updates far less)"
  from_main stage-update.sh
  "$work/stage-update.sh" "$CHECKOUT" --next "$work/Threadbear GPUI.app" || build_failed "$?"

  mkdir -p "$HOME/Applications"
  # Not over one that turned up while it built (installed from a terminal).
  # Copied beside it and moved in whole, so a copy stopped partway never
  # looks installed.
  if ! installed; then
    rm -rf "$TARGET.installing"
    ditto "$work/Threadbear GPUI.app" "$TARGET.installing"
    rm -rf "$TARGET"
    mv "$TARGET.installing" "$TARGET"
  fi
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
