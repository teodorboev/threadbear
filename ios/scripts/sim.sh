#!/bin/zsh
# This checkout's own iOS simulator. Threads must not share one: on 1 Nov one
# thread installed its build over another's repro and shut the simulator down,
# and the first thread's frames were silently wrong until someone noticed.
#
# So each worktree gets "Threadbear <folder>" (slot-3 gets "Threadbear slot-3"),
# cloned on first use. Only one thread holds a worktree at a time, and folders
# are reused, so that's a safe owner and the clones stay few. Those whose
# worktree has gone are deleted when a new one is made. The main checkout can
# be shared by threads, so a thread there is refused rather than given a
# simulator another thread could be using. SIM_NAME picks one by name, for
# anything that isn't a thread's (the App Store screenshots).
#
#   ios/scripts/sim.sh        prints its id, making it if needed
#   ios/scripts/sim.sh boot   the same, booted; refused if MAX_BOOTED others are up
#   ios/scripts/sim.sh stop   shuts it down, which you should once you're done
set -euo pipefail
cd "$(dirname "$0")"
MAX_BOOTED=2

# id <name>: the id of the available simulator with exactly this name.
id_of() {
  xcrun simctl list devices available | grep -F "    $1 (" | grep -oE "[0-9A-F-]{36}" | head -1 || true
}

# Clones of worktrees that have since been removed. Only shut-down ones, and
# only slot names, so nothing anyone made by hand or is running goes.
prune_gone() {
  local kept device name sim
  kept=$(git worktree list --porcelain | sed -n 's|^worktree .*/||p')
  xcrun simctl list devices | grep -E "^ +Threadbear slot-[0-9]+ \(.*\(Shutdown\)" | while IFS= read -r device; do
    name=$(print -r -- "$device" | grep -oE "slot-[0-9]+")
    print -r -- "$kept" | grep -qxF "$name" && continue
    sim=$(print -r -- "$device" | grep -oE "[0-9A-F-]{36}")
    xcrun simctl delete "$sim" && rm -rf "/tmp/threadbear-dd-$sim"
  done
}

if [[ -z "${SIM_NAME:-}" ]]; then
  if [[ "$(git rev-parse --absolute-git-dir)" == "$(cd "$(git rev-parse --git-common-dir)" && pwd -P)" ]]; then
    if [[ -n "${THREADBEAR_THREAD_ID:-}" ]]; then
      echo "This is the main checkout, which threads can share, so it has no simulator for a thread." >&2
      echo "Run this from your thread's worktree (./borrow-worktree.sh lends one), or set SIM_NAME to a name of your own." >&2
      exit 1
    fi
    SIM_NAME="Threadbear main"
  else
    SIM_NAME="Threadbear $(basename "$(git rev-parse --show-toplevel)")"
  fi
fi

SIM=$(id_of "$SIM_NAME")
if [[ -z "$SIM" ]]; then
  prune_gone
  # Cloned from a stock 6.9" iPhone, since `simctl create` refuses some pairs.
  # Pro Max sizes are what the App Store asks for.
  # Any Pro Max, newest first; failing that any iPhone, as a Mac whose
  # runtime only had an iPhone 17 and 17 Pro left this empty and the
  # script quit saying nothing (11 Nov).
  AVAILABLE=$(xcrun simctl list devices available)
  BASE=$(print -r -- "$AVAILABLE" | grep -E "iPhone [0-9]+ Pro Max \(" | sort -t' ' -k2 -rn | grep -oE "[0-9A-F-]{36}" | head -1 || true)
  [[ -z "$BASE" ]] && BASE=$(print -r -- "$AVAILABLE" | grep -E "iPhone [0-9]+" | grep -oE "[0-9A-F-]{36}" | head -1 || true)
  if [[ -z "$BASE" ]]; then
    echo "No iPhone simulator to clone: add one in Xcode (Window > Devices and Simulators)." >&2
    exit 1
  fi
  SIM=$(xcrun simctl clone "$BASE" "$SIM_NAME")
fi

case "${1:-}" in
  "") ;;
  boot)
    if ! xcrun simctl list devices booted | grep -qF "($SIM)"; then
      BOOTED=$(xcrun simctl list devices booted | grep -E "\(Booted\)" || true)
      if [[ $(print -r -- "$BOOTED" | grep -c "Booted") -ge $MAX_BOOTED ]]; then
        echo "$MAX_BOOTED simulators are already booted, and Mike wants no more than that on this Mac:" >&2
        print -r -- "$BOOTED" >&2
        echo "Wait for one to finish. Don't shut down another thread's." >&2
        exit 1
      fi
      xcrun simctl boot "$SIM"
    fi
    xcrun simctl bootstatus "$SIM" -b >/dev/null
    ;;
  stop) xcrun simctl shutdown "$SIM" 2>/dev/null || true ;;
  *) echo "Usage: ios/scripts/sim.sh [boot|stop]" >&2; exit 2 ;;
esac
print -r -- "$SIM"
