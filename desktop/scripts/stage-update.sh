#!/bin/bash
# Builds the checkout's main into the GPUI app's staged update, which the
# installed app swaps in when it next quits or launches
# (desktop/gpui/src/app_update.rs). The app runs the copy in its bundle a few
# moments after desktop/ changes on main; it's fine to run by hand too:
#
#   desktop/scripts/stage-update.sh <checkout> [--next <dir>]
#
# <checkout> is the repo whose main to build; --next is where the bundle goes
# (default ~/Applications/.Threadbear GPUI.app.next). Only what's committed on
# main is built, from a clone of its own under
# ~/Library/Caches/Threadbear/update-desktop, so nothing an agent has in the
# checkout gets into the app, and an agent's own builds there aren't waited on.
#
# Where it's built: the release build takes every core for minutes, so it goes
# to the first of runner.json's checkHosts that's a Mac and answers
# (remote.sh, as install-app.sh --build-on does), and only the two binaries
# come back; the bundle's made here, signed with this Mac's identity. With
# check hosts set but none answering it builds nothing and exits 4, and the app
# tries again at its next look: Mike's MacBook isn't for builds like this. With
# none set at all (anyone else's copy) it builds here, at the background
# priority the app runs it at, as the Swift app's update does.
#
# Exit 0 with the bundle staged, 3 if another run holds the lock, 4 if no Mac
# could build it (none answered, or the one building it went away or ran short
# of disk), 1 on a failed build (an earlier staged bundle is left as it was).
set -euo pipefail

CHECKOUT=""
NEXT_DIR="$HOME/Applications/.Threadbear GPUI.app.next"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --next) NEXT_DIR="$2"; shift ;;
    -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    -*) echo "Unknown option: $1" >&2; exit 2 ;;
    *) CHECKOUT="$1" ;;
  esac
  shift
done
[[ -n "$CHECKOUT" && ( -d "$CHECKOUT/.git" || -f "$CHECKOUT/.git" ) ]] || { echo "No checkout to build from (give the repo's path)." >&2; exit 2; }

# Started from the app, PATH is the bare system one: git, ssh, rsync and
# codesign are in /usr/bin, and cargo, for a build here, in ~/.cargo/bin.
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$HOME/.cargo/bin:$PATH"
CACHE="$HOME/Library/Caches/Threadbear/update-desktop"
SRC="$CACHE/src"
LOCK="$CACHE/lock"
mkdir -p "$CACHE"

# One build at a time. The lock holds the pid of the run that has it, so one
# left by a run that died (the app killed mid-build) is taken over at once
# rather than holding up the next for hours.
if ! mkdir "$LOCK" 2>/dev/null; then
  holder="$(cat "$LOCK/pid" 2>/dev/null || true)"
  if [[ -n "$holder" ]] && kill -0 "$holder" 2>/dev/null; then
    echo "Another update build is running (pid $holder)." >&2
    exit 3
  fi
  rm -rf "$LOCK"
  mkdir "$LOCK" 2>/dev/null || { echo "Another update build has just started." >&2; exit 3; }
fi
echo $$ >"$LOCK/pid"
trap 'rm -rf "$LOCK"' EXIT

MAIN="$(git -C "$CHECKOUT" rev-parse --verify -q refs/heads/main)" || { echo "$CHECKOUT has no main branch." >&2; exit 2; }
echo "Staging main ($(git -C "$CHECKOUT" rev-parse --short "$MAIN")) from $CHECKOUT into $NEXT_DIR"

# A clone of the checkout, moved to its main: only the files that changed get
# new times, so the build is incremental wherever it happens.
if [[ ! -d "$SRC/.git" ]]; then
  rm -rf "$SRC"
  git clone -q --no-checkout "$CHECKOUT" "$SRC"
fi
git -C "$SRC" fetch -q "$CHECKOUT" "refs/heads/main"
git -C "$SRC" checkout -q --detach FETCH_HEAD
git -C "$SRC" clean -qfd -e target

# runner.json's checkHosts, from the data folder the app uses.
CONFIG="${THREADBEAR_HOME:-$HOME/Library/Application Support/Threadbear}/runner.json"
HOSTS="$(/usr/bin/python3 -I -c 'import json,sys
try: hosts = json.load(open(sys.argv[1])).get("checkHosts") or []
except Exception: hosts = []
print("\n".join(h for h in hosts if isinstance(h, str) and h))' "$CONFIG" 2>/dev/null || true)"

BINS="$CACHE/bins"
rm -rf "$BINS"
mkdir -p "$BINS"
FEATURES=frame-stats
if [[ -n "$HOSTS" ]]; then
  HOST=""
  while IFS= read -r h; do
    # A Mac that answers within a few seconds, with no prompt to wait on.
    if [[ "$(ssh -o BatchMode=yes -o ConnectTimeout=8 "$h" uname -s 2>/dev/null || true)" == Darwin ]]; then HOST="$h"; break; fi
  done <<<"$HOSTS"
  if [[ -z "$HOST" ]]; then
    echo "None of the check hosts ($(tr '\n' ' ' <<<"$HOSTS")) is a Mac that answered, so nothing was built; the app tries again later." >&2
    exit 4
  fi
  echo "Building on $HOST"
  # A run the host couldn't finish (it went away, ssh's own 255, or remote.sh's
  # disk check said no with 3) is tried again later, as when none answered. One
  # it finished and failed is a broken main: not tried again until main moves.
  remote_failed() {
    local rc="$1"
    if [[ "$rc" == 3 || "$rc" == 255 || "$(ssh -o BatchMode=yes -o ConnectTimeout=8 "$HOST" uname -s 2>/dev/null || true)" != Darwin ]]; then
      echo "$HOST couldn't finish the build (exit $rc: it went away or is short of disk); the app tries again later." >&2
      exit 4
    fi
    echo "The build on $HOST failed (exit $rc)." >&2
    exit 1
  }
  # pipefail, so a failed build fails here rather than handing back the last
  # good build's binaries.
  rc=0
  # This Mac's own id in the folders' key on the host (remote.sh): the clone's
  # path is the same on every Mac with the same user name.
  THREADBEAR_REMOTE_KEY="$(ioreg -rd1 -c IOPlatformExpertDevice 2>/dev/null | awk -F'"' '/IOPlatformUUID/ { print $4; exit }')"
  export THREADBEAR_REMOTE_KEY="${THREADBEAR_REMOTE_KEY:-$(hostname)}"
  # The build says where it built (remote.sh sets CARGO_TARGET_DIR), and the
  # binaries come from there: not a key worked out here, which an older
  # remote.sh in main would disagree with (Codex's second look, 12 Nov).
  out="$CACHE/remote.out"
  "$SRC/desktop/scripts/remote.sh" "$HOST" -- bash -c "set -o pipefail; cd desktop && nice cargo build --release -p threadbear-gpui --features $FEATURES 2>&1 | tail -3 && echo \"BUILT_IN=\$CARGO_TARGET_DIR\"" > "$out" 2>&1 || rc=$?
  cat "$out"
  [[ "$rc" == 0 ]] || remote_failed "$rc"
  built_in="$(sed -n 's/^BUILT_IN=//p' "$out" | tail -1)"
  [[ "$built_in" == /* ]] || { echo "The build on $HOST didn't say where it built." >&2; exit 1; }
  for bin in threadbear-gpui threadbear-web-helper; do
    scp -q "$HOST:$built_in/release/$bin" "$BINS/$bin" || remote_failed "$?"
  done
else
  echo "No check hosts in $CONFIG: building here."
  (cd "$SRC/desktop" && CARGO_TARGET_DIR="$CACHE/target" CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10 cargo build --release -p threadbear-gpui --features "$FEATURES")
  cp "$CACHE/target/release/threadbear-gpui" "$CACHE/target/release/threadbear-web-helper" "$BINS/"
fi

# Made beside where it goes and moved in whole, so the app never finds half a
# bundle staged.
"$SRC/desktop/scripts/make-bundle.sh" "$BINS" "$NEXT_DIR.making" "$CHECKOUT"
# The bundle has its own copies of the binaries (55 MB).
rm -rf "$BINS"
rm -rf "$NEXT_DIR"
mv "$NEXT_DIR.making" "$NEXT_DIR"
echo "Staged $NEXT_DIR; the app swaps it in when it next quits or launches."
