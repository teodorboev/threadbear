#!/bin/bash
# Runs ./check.sh on another Mac, so test suites and the app's test windows
# don't load the Mac you're working on. check.sh calls this itself when a
# check host is set (THREADBEAR_CHECK_HOST, or "checkHost" in runner.json);
# you don't run it by hand.
#
#   scripts/check-remote.sh <ssh host>[,<ssh host>...] [check.sh options]
#
# With several hosts (Mike's are Tinker Desk and the cloud box), each run
# goes to the first one that's free and can run it, or waits for the first
# that can, and leaves that line for another that frees up before its turn
# comes (leave_line, below). A Linux host only takes what needs no Mac: a --quick run that
# doesn't touch ios/, app/ or dock/ (each brings in Swift), with no --shots.
# Mike asked on 6 Nov, when nine threads' checks queued at once.
#
# Mike asked on 6 Nov 2026, after three threads' unit suites, a swift test
# and three debug apps took his M5 to a load of 75 and a test's window came
# up over his browser. The host he picked (Tinker Desk) is also where he
# tries clean installs of Threadbear, so a run leaves nothing behind that
# looks like an install: no runner, no data dir, no checkout. Each run
# copies this checkout's files (tracked and untracked, as check.sh sees
# them) into ~/.cache/threadbear-check/runs/<id> there, runs check.sh on
# it, and deletes it. What's kept there between runs is only what makes
# the next run quick: node_modules per lockfile, the Swift build folders,
# and a mirror of each checkout that sends to it (mirror/<key>), so a run
# sends only what changed since that checkout's last, compressed: a whole
# copy is 17 MB, and on a congested link (6 Nov) that took minutes. Each
# run's copy is made from the mirror there. Delete ~/.cache/threadbear-check
# to take it all away.
#
# One run at a time on the host (lockf); a second waits and says so. The
# copy always runs from the same path, src/, as SwiftPM's build cache only
# works where it was made. Logs of failed steps, and --shots, come back
# here. --e2e never comes here: it needs the claude login, and on Tinker
# Desk a CLI signed in over SSH broke the desktop's login (2 Nov), so
# check.sh runs those on this Mac.
#
# A host with too little disk left is passed over (MIN_FREE_GIB, below).
#
# A host that stops answering partway is dropped for the next (`watched`).
#
# The check there stops when this does: it runs under hangup-guard.sh, which
# ends it when this Mac's ssh goes.
#
# Exit 255: no host that can run it could be reached (check.sh then runs here).
# Hosts that were reached but have no room are exit 1, with their free space:
# the M5 isn't the place for it either, and clearing the host is the fix.
set -uo pipefail

BASE_REL=".cache/threadbear-check"
# Available KiB where the run's files go, as one number. -P keeps each disk
# on one line and the columns the same on macOS and Linux.
free_cmd='df -Pk . 2>/dev/null | awk "END { print \$4 }"'

# ---- On the check host: scripts/check-remote.sh --on-host <id> [options]
if [ "${1:-}" = "--on-host" ]; then
  id="$2"
  shift 2
  # --pidfile: the run is under Desk Runner (--shots), away from the
  # hangup-guard.sh that would stop it if this Mac's ssh went, so it says
  # where it is, and doesn't start if that ssh has already gone.
  if [ "${1:-}" = "--pidfile" ]; then
    case "$2" in /*) pidfile="$2" ;; *) pidfile="$HOME/$2" ;; esac
    shift 2
    echo $$ >"$pidfile"
    [ ! -e "$pidfile.gone" ] || exit 129
  fi
  base="$HOME/$BASE_REL"
  src="$base/src"
  # An ssh session's PATH has no nvm, so find-node.sh looks there too.
  . "${BASH_SOURCE[0]%/*}/../runner/scripts/find-node.sh"
  NODE="$(threadbear_find_node)" || exit 1
  export PATH="${NODE%/*}:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"
  # Everything the tests make in a temp folder goes here and is deleted
  # with the run, so nothing piles up in /var/folders.
  export TMPDIR="$base/tmp-$id"
  rm -rf "$src" "$TMPDIR"
  # Anything a run cut short left (a copy that never got the lock, logs
  # nobody fetched) goes after half a day.
  find "$base/runs" "$base" -mindepth 1 -maxdepth 1 \( -name '[0-9]*' -o -name 'tmp-*' -o -name 'kept-*' -o -name 'shots-*' -o -name 'check-*.pid*' \) \
    -mmin +720 -exec rm -rf {} + 2>/dev/null
  # A checkout's mirror that hasn't sent a run for a week (a removed worktree).
  find "$base/mirror" -mindepth 1 -maxdepth 1 -type d -mtime +7 -exec rm -rf {} + 2>/dev/null
  # Folders this run makes are private whatever the host's umask (see probe).
  umask 022
  mkdir -p "$TMPDIR"
  mv "$base/runs/$id" "$src" || { echo "The copy for run $id isn't on $(hostname -s)." >&2; exit 1; }
  # Processes still working in this run's temp folder or its copy of the
  # checkout when it ends (runs here take turns, under the lock): a test
  # that timed out or failed partway can leave a child behind, and with its
  # parent gone it goes to launchd and stays. Six `tb land`s from landing
  # tests sat on Tinker Desk for days, waiting on a check nobody released
  # (7 to 14 Nov), and kept it from ever looking idle. Found by working
  # directory: /proc on Linux, lsof on macOS.
  left_in_run() {
    local dirs="" d
    for d in "$TMPDIR" "$src"; do
      d="$(cd "$d" 2>/dev/null && pwd -P)" && dirs+="$d"$'\n'
    done
    [ -n "$dirs" ] || return 0
    if [ -d /proc/self ]; then
      local p c
      for p in /proc/[0-9]*; do
        c="$(readlink "$p/cwd" 2>/dev/null)" || continue
        while IFS= read -r d; do
          [ -n "$d" ] || continue
          case "$c" in "$d" | "$d"/*) echo "${p#/proc/}"; break ;; esac
        done <<<"$dirs"
      done
    else
      # Through the environment: awk -v won't take a newline on macOS.
      lsof -nP -a -u "$(id -u)" -d cwd -Fpn 2>/dev/null | LEFT_DIRS="$dirs" awk '
        BEGIN { n_d = split(ENVIRON["LEFT_DIRS"], d, "\n") }
        /^p/ { pid = substr($0, 2) }
        /^n/ { n = substr($0, 2); for (i = 1; i <= n_d; i++) if (d[i] != "" && (n == d[i] || index(n, d[i] "/") == 1)) { print pid; break } }'
    fi | grep -vx "$$"
  }
  stop_left_in_run() {
    local pids
    pids="$(left_in_run | tr '\n' ' ')"
    [ -n "${pids// /}" ] || return 0
    kill $pids 2>/dev/null
    sleep 1
    pids="$(left_in_run | tr '\n' ' ')"
    [ -z "${pids// /}" ] || kill -9 $pids 2>/dev/null
  }

  # A failed check keeps its logs in $TMPDIR; they move aside for this
  # Mac to fetch, and everything else goes.
  keep_logs() {
    local rc=$?
    cd "$base" || return
    stop_left_in_run
    # A failed run's free space, before its temp files go: if they're what
    # filled the disk, it'd look fine by the time this Mac asks (Codex's
    # review). The Mac falls back to asking if a full disk stops this write.
    if [ "$rc" != 0 ] && mkdir -p "$base/kept-$id" 2>/dev/null; then
      eval "$free_cmd" >"$base/kept-$id/free-kb" 2>/dev/null
    fi
    for d in "$TMPDIR"/threadbear-check.*; do
      [ -d "$d" ] && mkdir -p "$base/kept-$id" && mv "$d" "$base/kept-$id/"
    done
    rm -rf "$src" "$TMPDIR"
  }
  trap keep_logs EXIT

  # A git repo of its own, on main, as some tests run git in the repo
  # (deploy-backend.mjs reads HEAD). Its one commit is the copy as sent.
  (cd "$src" && git init -q -b main && git add -A && git -c user.name=check -c user.email=check@localhost commit -qm "check copy") \
    || { echo "Couldn't make the copy a git repo." >&2; exit 1; }

  # node_modules per package-lock, kept between runs (the newest three of
  # each). npm ci only when the lockfile is new.
  # --runner (the runner's self-update) needs only the runner's: a broken
  # lockfile elsewhere mustn't hold the runner back (Codex, 9 Nov).
  dep_dirs=". runner app/Tools/mock-runner"
  case " $* " in *" --runner "*) dep_dirs="runner" ;; esac
  for dir in $dep_dirs; do
    [ -f "$src/$dir/package-lock.json" ] || continue
    name="$(printf '%s' "$dir" | tr -c 'a-zA-Z0-9' '-')"
    hash="$(shasum -a 256 "$src/$dir/package-lock.json" | cut -c1-16)"
    # The folder itself has to be called node_modules: Node finds a
    # package's own dependencies from its real path, by looking for
    # node_modules folders above it.
    cache="$base/deps/$name-$hash/node_modules"
    if [ ! -d "$cache" ]; then
      echo "npm ci in $dir (its lockfile is new here)"
      (cd "$src/$dir" && "$NODE" "$src/scripts/limit-step.mjs" --silence 1800 --total 1800 -- npm ci --no-audit --no-fund --loglevel=error >/dev/null) || { echo "npm ci failed in $dir" >&2; exit 1; }
      mkdir -p "$(dirname "$cache")"
      mv "$src/$dir/node_modules" "$cache"
    fi
    touch "$(dirname "$cache")"
    rm -rf "$src/$dir/node_modules"
    ln -s "$cache" "$src/$dir/node_modules"
    ls -td "$base/deps/$name-"* 2>/dev/null | tail -n +4 | xargs rm -rf
  done
  # The Swift build folders, kept between runs.
  for dir in app dock ios/ThreadbearKit; do
    [ -d "$src/$dir" ] || continue
    name="$(printf '%s' "$dir" | tr -c 'a-zA-Z0-9' '-')"
    mkdir -p "$base/build/$name"
    rm -rf "$src/$dir/.build"
    ln -s "$base/build/$name" "$src/$dir/.build"
  done
  # Cargo for the desktop app's Rust (desktop/): the one desktop/scripts/remote.sh
  # installed under gpui/ if there is one, else rustup's. Its target folder is
  # kept between runs too, one for every checkout: this host's lock means only
  # one run uses it at a time, and the copy is always at src/ with the Mac's
  # file times (rsync -a), so cargo rebuilds only the crates whose files
  # changed. It's dropped when the disk runs short, as a GPUI build fills
  # several GiB and the cloud box runs a runner of its own (THREADBEAR-1R).
  if [ -x "$base/gpui/cargo/bin/cargo" ]; then
    export CARGO_HOME="$base/gpui/cargo" RUSTUP_HOME="$base/gpui/rustup"
    PATH="$CARGO_HOME/bin:$PATH"
  elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  fi
  # THREADBEAR_CHECK_SHARED_TARGET has check.sh make sure cargo sees every
  # change (cargo_step there says why).
  export CARGO_TARGET_DIR="$base/build/desktop-target" THREADBEAR_CHECK_SHARED_TARGET=1 \
    CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
  free_kb="$(cd "$base" && eval "$free_cmd")"
  case "$free_kb" in
    ""|*[!0123456789]*) ;;
    *) if [ "$free_kb" -lt $((12 * 1048576)) ] && [ -d "$CARGO_TARGET_DIR" ]; then
         echo "Short of disk here, so the desktop app's cargo target goes; it builds from cold this time."
         rm -rf "$CARGO_TARGET_DIR"
       fi ;;
  esac

  cd "$src" || exit 1
  # Under a limit of its own as well: this Mac giving up on it (a dropped
  # connection, its MAX_SECS) doesn't stop it here, and it holds this host's
  # lock while it runs (Codex's review). Its Swift steps have shorter ones.
  THREADBEAR_CHECK_CHANGED_FILE="$src/.check-changed" THREADBEAR_NODE="$NODE" \
    "$NODE" "$src/scripts/limit-step.mjs" --silence 3600 --total "${THREADBEAR_CHECK_HOST_MAX_SECS:-7200}" -- ./check.sh --local "$@"
  code=$?
  # limit-step's 124 is what this Mac takes for a host that dropped out, which
  # would run the whole check again elsewhere: it's a failed check (1).
  [ "$code" != 124 ] || code=1
  exit "$code"
fi

# ---- On this Mac
HOST_LIST="${1:?usage: check-remote.sh <ssh host>[,<ssh host>...] [check.sh options]}"
shift
read -ra HOSTS <<<"$(printf '%s' "$HOST_LIST" | tr ',' ' ')"
# THREADBEAR_CHECK_PREFER: hosts to try first, the rest of the list still
# behind them. tb land sets it for a group's check from what each landing in
# it asked for (THREADBEAR_CHECK_HOST), as the check runs with the holder's
# environment (runner/src/worktrees.ts, checkPreference). A host that can't
# run this check is passed over as ever, so a group that needs a Mac still
# goes to one.
if [ -n "${THREADBEAR_CHECK_PREFER:-}" ]; then
  ordered=()
  for h in $(printf '%s' "$THREADBEAR_CHECK_PREFER" | tr ',' ' ') ${HOSTS[@]+"${HOSTS[@]}"}; do
    [[ "$h" =~ ^[A-Za-z0-9][A-Za-z0-9._@-]*$ ]] || continue
    case " ${ordered[*]+"${ordered[*]}"} " in *" $h "*) continue ;; esac
    ordered+=("$h")
  done
  if [ "${#ordered[@]}" -gt 0 ]; then
    HOSTS=("${ordered[@]}")
    HOST_LIST="$(IFS=,; printf '%s' "${HOSTS[*]}")"
  fi
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/scripts/remote-guard-lib.sh"
alive_stdin 7 || { echo "Couldn't make the pipe that tells a check host this check has gone (alive_stdin)." >&2; exit 1; }
# A folder only this account can use: it exists (made here if not, mode
# 0700), isn't a symlink, is owned by this user and has mode 0700 exactly.
# One that's anything else is never repaired or used (Daybreak, 7 Nov: a
# name in a shared folder like /tmp can be taken by another account first).
MY_UID="$(id -u)"
owner_mode() {
  if [ "$(uname -s)" = Darwin ]; then stat -f '%u %Lp' "$1" 2>/dev/null; else stat -c '%u %a' "$1" 2>/dev/null; fi
}
private_dir() {
  [ -e "$1" ] || [ -L "$1" ] || mkdir -m 700 "$1" 2>/dev/null
  [ -d "$1" ] && [ ! -L "$1" ] && [ "$(owner_mode "$1")" = "$MY_UID 700" ]
}
# Only these characters in a path this script builds commands from: ssh
# and rsync's -e split on spaces, and the host's commands quote with '.
plain_path() {
  case "$1" in /*) ;; *) return 1 ;; esac
  # Byte by byte, in the C locale: a bracket range in a UTF-8 locale
  # took é for a letter on Linux. Counted, not captured, as $() drops a
  # newline at the end (Codex's review).
  local odd
  odd="$(printf '%s' "$1" | LC_ALL=C tr -d 'A-Za-z0-9._/-' | wc -c)"
  [ $((odd)) -eq 0 ] || return 1
  case "/$1/" in */../*|*/./*) return 1 ;; esac
}

# One connection per host, shared by every ssh and rsync this run makes:
# a run makes half a dozen, and on a congested link (6 Nov, 2 s pings) each
# new one took 20 s to set up. Its socket is in ~/.ssh/tbc, a folder of this
# user's own (above): it was /tmp/tbc-<uid>-%C, a name another account on
# the Mac could make first and so answer for the host (Daybreak, 7 Nov).
# A socket's path can't be long: ssh binds <path>.<16 random> first, %C is
# 40 characters, and macOS allows 103 (Linux 107). Too long, or no private
# folder, and each ssh makes its own connection instead.
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=30 -o ServerAliveCountMax=4)
sock_dir="${HOME:-}/.ssh/tbc"
if plain_path "$sock_dir" && [ $(( ${#sock_dir} + 1 + 40 + 17 )) -le 103 ] \
  && { [ -d "$HOME/.ssh" ] || mkdir -m 700 "$HOME/.ssh" 2>/dev/null; } && private_dir "$sock_dir"; then
  SSH+=(-o ControlMaster=auto -o "ControlPath=$sock_dir/%C" -o ControlPersist=60)
else
  echo "Not sharing one ssh connection per host: $sock_dir isn't a short folder of this user's own (mode 700)." >&2
  SSH+=(-o ControlMaster=no -o ControlPath=none)
fi

# A host that stops answering mustn't hold the check, and with it the landing
# lock, forever. On 8 Nov Tinker Desk stopped taking ssh connections, and a
# check sat in its ssh for 79 minutes with no output: neither ServerAlive nor
# the shared connection gave up. So while anything here talks to a host,
# `watched` pings it every PING_SECS on a connection of its own (no shared
# socket, which may be the thing that's stuck), and after PING_MISSES pings in
# a row that can't even log in, it kills what was talking to the host and
# says the host dropped out (exit 124). The run then goes to the next host,
# or to this Mac like any host that can't be reached. MAX_SECS caps the
# whole run, waiting in line included, as a last resort: each Swift step
# already has a limit of its own (check.sh, limit-step.mjs). The THREADBEAR_
# names are for the tests.
PING_SECS="${THREADBEAR_CHECK_PING_SECS:-30}"
PING_MISSES="${THREADBEAR_CHECK_PING_MISSES:-4}"
MAX_SECS="${THREADBEAR_CHECK_MAX_SECS:-10800}"
DEADLINE=$(( $(date +%s) + MAX_SECS ))
PING=(ssh -o BatchMode=yes -o ConnectTimeout=20 -o ControlMaster=no -o ControlPath=none)

# kill_tree <pid>: it and everything under it, all found before any is
# signalled, so none is handed to launchd and missed (one of those could keep
# the run's output open). Asked first, then made to, after 5 s, if any ignored
# that (Codex's review).
tree_of() {
  local kid
  echo "$1"
  # ps, not pgrep -P: Homebrew's proctools pgrep takes -P with no pattern as
  # every process on the machine, and the walk never ends.
  for kid in $(ps -A -o pid= -o ppid= 2>/dev/null | awk -v p="$1" '$2 == p { print $1 }'); do tree_of "$kid"; done
}
kill_tree() {
  local pids p left n=0
  pids="$(tree_of "$1")"
  kill $pids 2>/dev/null
  while [ "$n" -lt 25 ]; do
    left=""
    for p in $pids; do kill -0 "$p" 2>/dev/null && left+=" $p"; done
    [ -n "$left" ] || return 0
    sleep 0.2
    n=$((n + 1))
  done
  kill -9 $left 2>/dev/null
}

# timed <secs> <command...>: the command, killed after secs (exit 124).
timed() {
  local secs="$1" pid ticks=0
  shift
  "$@" &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$ticks" -ge $((secs * 5)) ]; then kill_tree "$pid"; wait "$pid" 2>/dev/null; return 124; fi
    sleep 0.2
    ticks=$((ticks + 1))
  done
  wait "$pid"
}

# watched <host> <command...>: the command's own exit, or 124 when the host
# stopped answering and it was killed, or 125 past MAX_SECS. A command that
# finishes before the first ping never sees one.
watched() {
  local host="$1" pid watcher why
  shift
  "$@" &
  pid=$!
  (
    misses=0
    while :; do
      for _ in $(seq 1 $((PING_SECS * 5))); do
        kill -0 "$pid" 2>/dev/null || exit 0
        [ "$(date +%s)" -lt "$DEADLINE" ] || { kill_tree "$pid"; exit 2; }
        sleep 0.2
      done
      if timed $((PING_SECS + 20)) "${PING[@]}" "$host" true </dev/null >/dev/null 2>&1; then misses=0
      else misses=$((misses + 1)); fi
      [ "$misses" -lt "$PING_MISSES" ] || { kill_tree "$pid"; exit 1; }
    done
  ) &
  watcher=$!
  wait "$pid"
  local code=$?
  wait "$watcher"
  why=$?
  case "$why" in 1) return 124 ;; 2) return 125 ;; esac
  return "$code"
}

# What to do about a host that dropped out: say so, never try it again this
# run, and close its shared connection (in case that's what's stuck).
dropped() {
  echo "$1 stopped answering over ssh (${PING_MISSES} tries in a row, ${PING_SECS} s apart, couldn't log in); trying the others." >&2
  SKIP+=" $1"
  timed 10 "${SSH[@]}" -O exit "$1" >/dev/null 2>&1
}
# Past MAX_SECS: not 255, as running it all again here wouldn't be quicker.
# Before the check started there (waiting for a host or copying: over_time
# busy), BUSY_EXIT, as with no host having room: the runner's self-update
# sets THREADBEAR_CHECK_BUSY_EXIT so it can tell the hosts' trouble from a
# failed check and try again later (deploy-steps.mjs, remoteChecks).
BUSY_EXIT="${THREADBEAR_CHECK_BUSY_EXIT:-1}"
case "$BUSY_EXIT" in [1-9]|[1-9][0-9]|1[01][0-9]|12[0-3]) ;; *) BUSY_EXIT=1 ;; esac
over_time() {
  echo "This check has been going for $((MAX_SECS / 60)) minutes, waiting in line included, so it was stopped." >&2
  rm -f "$files" "$changed" "$left_out"
  if [ "${1:-}" = busy ]; then exit "$BUSY_EXIT"; fi
  exit 1
}

ARGS=()
SHOTS=""
QUICK=0
RUNNER_ONLY=0
ALL=0
while [ $# -gt 0 ]; do
  case "$1" in
    --shots)
      case "$2" in /*) SHOTS="$2" ;; *) SHOTS="$PWD/$2" ;; esac
      shift ;;
    --quick) QUICK=1; ARGS+=("$1") ;;
    --runner) QUICK=1; RUNNER_ONLY=1; ARGS+=("$1") ;;
    --all) ALL=1; ARGS+=("$1") ;;
    *) ARGS+=("$1") ;;
  esac
  shift
done

# What this checkout changes from main, worked out here where the history
# is (the copy's git has just one commit), as check.sh skips steps by it
# (see `needed` there).
changed="$(mktemp)"
{
  base="$(git -C "$ROOT" merge-base HEAD main 2>/dev/null)" && git -C "$ROOT" diff --name-only "$base"
  git -C "$ROOT" ls-files --others --exclude-standard
} 2>/dev/null | sort -u >"$changed"

# Whether a Linux host can run this: only the runner's, Convex's and the
# website's checks, so --quick, nothing changed under ios/ (ThreadbearKit's
# tests), app/ or dock/ (the Mac app's release build) beyond what the GPUI
# app shares (scripts/swift-freeze.sh --gpui-shared: the mock runner,
# fixtures, art, which the Swift app's build no longer runs for, but not
# the mock runner, whose Swift tests still run), and
# something changed (with nothing, check.sh itself or --all, it runs
# everything), no shots.
linux_ok=0
GPUI_SHARED="$("$ROOT/scripts/swift-freeze.sh" --gpui-shared 2>/dev/null)"
# Without it, nothing counts as shared: an empty pattern would match every
# line, and every change would look like none.
[ -n "$GPUI_SHARED" ] || GPUI_SHARED='^$'
# The mock runner, though shared, has Swift tests check.sh runs whenever it
# or they change (swift-freeze.sh --mock-tests), and those need a Mac.
MOCK_TESTS="$("$ROOT/scripts/swift-freeze.sh" --mock-tests 2>/dev/null | head -1)"
[ -n "$MOCK_TESTS" ] || MOCK_TESTS='^$'
if [ "$QUICK" = 1 ] && [ "$ALL" = 0 ] && [ -z "$SHOTS" ] && [ -s "$changed" ] \
  && ! grep -qE '^ios/|^check\.sh$' "$changed" && ! grep -vE "$GPUI_SHARED" "$changed" | grep -E '^(app|dock)/' >/dev/null \
  && ! grep -E "$MOCK_TESTS" "$changed" >/dev/null; then linux_ok=1; fi
# (grep to /dev/null, not -q: -q closing the pipe early fails grep -v under
# pipefail, and a big change would have looked like none.)
# --runner is only Node, whatever changed: the runner's self-update sends
# it from a clone on main, where nothing has, and with Tinker Desk away a
# Linux host should still take it rather than this Mac (Codex, 9 Nov).
if [ "$RUNNER_ONLY" = 1 ] && [ -z "$SHOTS" ]; then linux_ok=1; fi
# Why it needs a Mac, for the message when only Linux hosts answered.
MAC_WHY="it isn't --quick"
if [ "$QUICK" = 1 ]; then
  if [ -n "$SHOTS" ]; then MAC_WHY="--shots"
  elif [ "$ALL" = 1 ]; then MAC_WHY="--all"
  elif [ ! -s "$changed" ]; then MAC_WHY="nothing changed from main, so it runs everything"
  else MAC_WHY="it changes ios/, app/, dock/ or check.sh"; fi
fi

# The disk a run wants free on its host. On 7 Nov Tinker Desk fell under 1 GiB
# and an --app check died with a CodeSign error that said nothing about space,
# which cost a landing. Sized for a host starting cold, from Tinker Desk that
# day: anything building Swift can write 2.4 GiB of build folders, 2.3 of
# SwiftPM's cache and 0.6 of node_modules, about 5.5 GiB, and 8 leaves macOS
# room to work. A run with no Swift (what a Linux host can take, so linux_ok
# says which) writes only the node_modules and its temp files.
MIN_FREE_GIB=8
[ "$linux_ok" = 0 ] || MIN_FREE_GIB=2

# Whether a df answer is under the limit: 0 yes, and FREE_GIB is it to one
# place, rounded down; 1 no; 2 it isn't a number we can use. It's the host's
# word, so it's only compared, and the digits are spelled out because a
# bracket range in a UTF-8 locale can take in other characters. Rounded down,
# as "8.0 free, needs 8" would make no sense.
FREE_GIB=""
low_on_room() {
  case "$1" in ""|*[!0123456789]*) return 2 ;; esac
  # More digits than any disk has would wrap around in bash's arithmetic.
  [ "${#1}" -le 15 ] || return 2
  local kb=$((10#$1))
  [ "$kb" -lt $((MIN_FREE_GIB * 1048576)) ] || return 1
  FREE_GIB="$((kb / 1048576)).$(((kb % 1048576) * 10 / 1048576))"
}
# The host just used, asked again after something failed there: true, with
# FREE_GIB set, if it's low. Not if it's fine or can't be asked.
host_is_low() {
  local kb
  kb="$(watched "$HOST" "${SSH[@]}" "$HOST" "cd '$remote_base' && $free_cmd" 2>/dev/null)" && low_on_room "$kb"
}

# Each host: reachable, its system, whether its lock is free now, and how much
# disk it has left. The first free one that can run it wins; otherwise the
# first that can, which then waits for its lock. Two checks can both see a host
# free, so a run there takes the lock without waiting (exit 75 if it can't),
# and the one that lost chooses again; after three goes it waits in line.
# The copy's folders are made private: a host whose umask is 002 (the cloud
# box) left them group-writable, and the private-dir tests (ios/scripts/
# private-dir.mjs) refuse a folder under one of those (9 Nov).
probe='mkdir -p ~/'"$BASE_REL"'/runs && chmod go-w ~/'"$BASE_REL"' ~/'"$BASE_REL"'/runs && cd ~/'"$BASE_REL"' && pwd && uname -s && if [ "$(uname -s)" = Darwin ]; then /usr/bin/lockf -t 0 -k lock true 2>/dev/null && echo free || echo busy; else flock -n lock true 2>/dev/null && echo free || echo busy; fi && '"$free_cmd"
# Hosts with no room, as "<host> has <n> GiB free": those this choosing passed
# over, and those whose copy failed for lack of it, which aren't asked again.
LOW="" LOW_COPY=""
# A host short of room may only be short because of the GPUI build folders
# desktop/scripts/remote.sh keeps there (63 GB of them on Tinker Desk, 12 Nov,
# with 1.8 GiB free). Those idle get given back by the same script remote.sh
# uses, and the disk read again: true if it now has room. $2 is the host's
# folder: as its probe gave it, already checked as a plain path, or \$HOME's
# when the probe couldn't run, for the host to expand. Sent in the command,
# not on stdin, as watched runs ssh in the background, where stdin is empty.
# Under a time limit of its own, so a host stuck deleting doesn't keep the
# other hosts from being tried.
TRIM="$ROOT/desktop/scripts/trim-remote-cache.sh"
make_room() {
  [ -f "$TRIM" ] || return 1
  local kb
  timed 600 "${SSH[@]}" "$1" "cd \"$2/gpui\" 2>/dev/null || exit 0; bash -c \"\$(printf %s '$(base64 <"$TRIM" | tr -d '\n')' | base64 -d)\" trim -" </dev/null || true
  kb="$(watched "$1" "${SSH[@]}" "$1" "cd \"$2\" && $free_cmd" 2>/dev/null)" || return 1
  low_on_room "$kb"
  [ $? = 1 ]
}
pass_over() {
  echo "Passing over $1: it has $FREE_GIB GiB free, and this check wants $MIN_FREE_GIB GiB." >&2
  LOW+="${LOW:+, }$1 has $FREE_GIB GiB free"
}
# Hosts that answered but can't run this kind of check, with why. They're
# reachable, so a run none can take isn't this Mac's either (exit 1, each
# host's reason said): only hosts nobody can reach send it here (255). On
# 13 Nov a landing that asked only for the cloud box, with Swift in its
# group, ran the runner's suites and the app's release build on Mike's M5.
CANT="" CANT_COPY=""
cant_run() { CANT+="${CANT:+; }$1: $2"; }
choose() {
  HOST="" HOST_OS="" remote_base="" FREE=0 LOW="$LOW_COPY" CANT="$CANT_COPY"
  local first="" first_os="" first_base="" h out rc b os state kb
  for h in "${HOSTS[@]}"; do
    # An ssh host, never something ssh would take for an option.
    case "$h" in -*|"") continue ;; esac
    # One that dropped out during a copy this run isn't tried again.
    case " $SKIP " in *" $h "*) continue ;; esac
    # Probed again once if the first failed for want of room and giving back
    # build folders made some.
    tried_room=0
    while :; do
      out="$(watched "$h" "${SSH[@]}" "$h" "$probe" 2>/dev/null)"
      rc=$?
      [ "$rc" != 125 ] || over_time busy
      [ "$rc" != 0 ] && [ "$rc" != 124 ] && [ "$rc" != 255 ] && [ "$tried_room" = 0 ] || break
      tried_room=1
      kb="$(watched "$h" "${SSH[@]}" "$h" "cd ~ && $free_cmd" 2>/dev/null)" && low_on_room "$kb" || break
      make_room "$h" '$HOME/'"$BASE_REL" || break
    done
    if [ "$rc" = 124 ]; then
      dropped "$h"
      continue
    fi
    if [ "$rc" != 0 ]; then
      # It answered, but a disk too full for the probe's mkdir fails it too,
      # and calling that unreachable would run the check on this Mac (Codex's
      # review). 255 is ssh itself: nobody there to ask.
      # One that answers the disk question has answered: whatever stopped
      # the probe (a folder it can't make or lock), it's no reason to run the
      # check here (Codex's review, 14 Nov).
      if [ "$rc" != 255 ] && kb="$(watched "$h" "${SSH[@]}" "$h" "cd ~ && $free_cmd" 2>/dev/null)"; then
        if low_on_room "$kb"; then pass_over "$h"
        else cant_run "$h" "it answered, but couldn't make or lock ~/$BASE_REL there (exit $rc)"; fi
      elif [ "$rc" != 255 ]; then
        # Not ssh's own 255: it logged in and its commands failed (a home it
        # can't open, say). Answered all the same (Codex, 14 Nov).
        cant_run "$h" "it answered, but its commands there failed (exit $rc)"
      else
        echo "Couldn't reach $h over ssh." >&2
      fi
      continue
    fi
    { read -r b; read -r os; read -r state; read -r kb; } <<<"$out"
    # Every rm -rf over there is under this, so never let it be empty. It's
    # the host's word, so it's only ever data: an absolute path of plain
    # characters ending in the folder asked for, else the host is passed
    # over (Daybreak, 7 Nov: it went into a sed program here).
    if ! plain_path "$b"; then echo "Unexpected folder on $h (not a plain absolute path); passing it over." >&2; cant_run "$h" "its check folder isn't a plain absolute path"; continue; fi
    case "$b" in */$BASE_REL) ;; *) echo "Unexpected folder on $h: '$b'" >&2; cant_run "$h" "its check folder isn't ~/$BASE_REL"; continue ;; esac
    case "$os" in
      Darwin) ;;
      Linux) [ "$linux_ok" = 1 ] || { cant_run "$h" "it's Linux, and this check needs a Mac ($MAC_WHY)"; continue; } ;;
      *) cant_run "$h" "it runs $os, not macOS or Linux"; continue ;;
    esac
    # After the system, so a host that couldn't run this anyway isn't named.
    # Busy or free alike: waiting for a full disk gets nobody anywhere.
    low_on_room "$kb"
    case $? in
      0)
        make_room "$h" "$b" && echo "$h was short of disk; idle GPUI build folders there were given back, and it has room now." >&2 || {
          pass_over "$h"
          continue
        } ;;
      # Not knowing is no reason to stop checks; a failure there still says.
      2) echo "Couldn't read how much disk $h has free; trying it anyway." >&2 ;;
    esac
    if [ "$state" = free ]; then HOST="$h" HOST_OS="$os" remote_base="$b" FREE=1; return 0; fi
    [ -n "$first" ] || { first="$h" first_os="$os" first_base="$b"; }
  done
  [ -n "$first" ] || return 1
  HOST="$first" HOST_OS="$first_os" remote_base="$first_base"
}

# Waiting in line on a busy host, the others are looked at every POLL_SECS,
# and one that's free and can run this check is taken instead, so long as the
# check hasn't started where it waits. On 13 Nov a landing's quick check found
# both hosts busy, took the first (Tinker Desk) and waited there over half an
# hour behind GPUI release builds, which hold the same lock (perf-lock.pl),
# while the cloud box was free soon after; every landing behind it waited
# too. free_elsewhere: true if a host other than the one waited on is free
# and could run it, by the same rules as choose(), less giving back build
# folders (that can take minutes, and the line is still there).
POLL_SECS="${THREADBEAR_CHECK_POLL_SECS:-30}"
free_elsewhere() {
  local h out b os state kb
  for h in "${HOSTS[@]}"; do
    case "$h" in -*|""|"$HOST") continue ;; esac
    case " $SKIP " in *" $h "*) continue ;; esac
    out="$(timed 30 "${SSH[@]}" "$h" "$probe" 2>/dev/null </dev/null)" || continue
    { read -r b; read -r os; read -r state; read -r kb; } <<<"$out"
    plain_path "$b" || continue
    case "$b" in */$BASE_REL) ;; *) continue ;; esac
    case "$os" in Darwin) ;; Linux) [ "$linux_ok" = 1 ] || continue ;; *) continue ;; esac
    low_on_room "$kb"
    [ $? != 0 ] || continue
    [ "$state" = free ] && { FREE_HOST="$h"; return 0; }
  done
  return 1
}
# leave_line <waiter>: while the check waits in line on $HOST (the waiter
# being its ssh), look elsewhere; true once it has left that line for a free
# host. Leaving is a rename of the run's copy there (runs/<id> to
# runs/<id>.withdrawn), which is atomic, as is --on-host's first step once it
# has the lock (runs/<id> to src): whichever renames first wins, so a check
# that has started is never pulled, and one that's withdrawn finds no copy
# when its turn comes and starts nothing. The lock it would have held is
# never taken from anything running. The rename is the claim, and the host
# is asked where the copy is to learn who won, so a reply lost on the way
# back (a dropped ssh) is never taken for a lost race (Codex's review): the
# next look finds runs/<id>.withdrawn and leaves then. The copy's own name is
# asked about first: a rename landing between the two tests then reads as
# waiting (looked at again) rather than started (Codex's second review).
# ASK_SECS is for the tests.
ASK_SECS="${THREADBEAR_CHECK_ASK_SECS:-15}"
where_is_copy() {
  timed "$ASK_SECS" "${SSH[@]}" "$HOST" "if [ -d '$remote_run' ]; then echo waiting; elif [ -d '$remote_run.withdrawn' ]; then echo withdrawn; else echo started; fi" 2>/dev/null </dev/null
}
leave_line() {
  local waiter="$1" next=$((SECONDS + POLL_SECS)) where="" claimed=0
  while kill -0 "$waiter" 2>/dev/null; do
    if [ "$SECONDS" -lt "$next" ]; then sleep 1; continue; fi
    next=$((SECONDS + POLL_SECS))
    where="$(where_is_copy)"
    # Started there: it stays, however long it takes.
    [ "$where" != started ] || return 1
    if [ "$where" = withdrawn ]; then break; fi
    # Not answering: the waiter's own watch deals with that.
    [ "$where" = waiting ] || continue
    # Still there after a claim: it never reached the host, so claim again.
    free_elsewhere || continue
    claimed=1
    timed "$ASK_SECS" "${SSH[@]}" "$HOST" "mv '$remote_run' '$remote_run.withdrawn'" >/dev/null 2>&1 </dev/null
    where="$(where_is_copy)"
    [ "$where" != started ] || return 1
    [ "$where" != withdrawn ] || break
  done
  # The waiter may have ended by itself after an unanswered claim (its turn
  # came and found no copy): one more look says whether that was this.
  [ "$where" = withdrawn ] || { [ "$claimed" = 1 ] && where="$(where_is_copy)" && [ "$where" = withdrawn ]; } || return 1
  kill_tree "$waiter"
  wait "$waiter" 2>/dev/null
  # Best effort: what's left goes with the host's own clean-up (half a day).
  timed "$ASK_SECS" "${SSH[@]}" "$HOST" "rm -rf '$remote_run.withdrawn'" >/dev/null 2>&1 </dev/null
  echo "${FREE_HOST:-Another host} is free now; leaving the line on $HOST, where this check hadn't started."
  return 0
}

# The files check.sh would see, less what no check reads: the pictures in
# docs/ and site/ (about 50 MB of the repo). TourTests reads site's JSON.
# The site's build (site/scripts/build.mjs --check) checks every link lands
# on a file, so it's told what was left out (.check-left-out), and takes
# those as there.
files="$(mktemp)"
left_out="$(mktemp)"
(cd "$ROOT" && git ls-files -co --exclude-standard -z) | tr '\0' '\n' \
  | grep -vE '^(docs|site)/.*\.(png|gif|jpe?g|mp4|mov|webp)$' | tr '\n' '\0' >"$files"
(cd "$ROOT" && git ls-files -co --exclude-standard) \
  | grep -E '^(docs|site)/.*\.(png|gif|jpe?g|mp4|mov|webp)$' >"$left_out"
BASE_ARGS=(${ARGS[@]+"${ARGS[@]}"})
SKIP=""
tries=0
# The copy lock: a kernel lock (lockf, or flock on Linux) held by a small
# helper until this run lets go or ends, so one a killed run held frees
# itself. The helper looks every second for its release file and for
# this script.
#
# The lock file is shared by every run from this checkout, so it lives in a
# folder of this user's own (private_dir); the helper's ready and release
# files are this run's alone, in a folder mktemp -d makes (0700, atomically).
# Both were names in $TMPDIR that mktemp -u only made up, not made, which
# another account could have put a link at first (Daybreak, 7 Nov).
holder="" release="" sentinels=""
lock_dir="${XDG_CACHE_HOME:-${HOME:-}/.cache}/threadbear-check-locks"
take_copylock() {
  local ready
  if [ -z "$sentinels" ]; then
    sentinels="$(mktemp -d "${TMPDIR:-/tmp}/tb-check-lock.XXXXXX")" || { sentinels=""; return 1; }
  fi
  ready="$sentinels/ready"
  release="$sentinels/release"
  rm -f "$ready" "$release"
  local hold=': >"$1"; while kill -0 "$2" 2>/dev/null && [ ! -e "$3" ]; do sleep 1; done'
  if command -v flock >/dev/null 2>&1; then flock "$1" /bin/sh -c "$hold" sh "$ready" "$$" "$release" &
  else /usr/bin/lockf -k "$1" /bin/sh -c "$hold" sh "$ready" "$$" "$release" & fi
  holder=$!
  # Waiting for another check from this checkout counts towards MAX_SECS too.
  until [ -e "$ready" ] || ! kill -0 "$holder" 2>/dev/null; do
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then kill_tree "$holder"; wait "$holder" 2>/dev/null; holder="" release=""; over_time busy; fi
    sleep 0.2
  done
  # The helper gone with no ready file never had the lock (a lock file it
  # can't open): no copy without it.
  if [ ! -e "$ready" ]; then
    wait "$holder" 2>/dev/null
    rm -f "$release"
    holder="" release=""
    return 1
  fi
  rm -f "$ready"
}
drop_copylock() {
  [ -n "$holder" ] || return 0
  : >"$release"
  wait "$holder" 2>/dev/null
  rm -f "$release"
  holder="" release=""
}
finish() {
  drop_copylock
  [ -z "$sentinels" ] || rm -rf "$sentinels"
}
trap finish EXIT
# A signal stops everything this started, the ssh to the check host among
# them, so the check there stops too (hangup-guard.sh). Without it the
# ssh lived on, and the check with it.
on_signal() {
  local kid
  for kid in $(ps -A -o pid= -o ppid= 2>/dev/null | awk -v p="$$" '$2 == p { print $1 }'); do kill_tree "$kid"; done
  exit "$1"
}
trap 'on_signal 143' TERM
trap 'on_signal 129' HUP
trap 'on_signal 130' INT
# Each line with `from` and the letters and digits after it (a log folder's
# mktemp suffix) changed to `to`, as the check runs, line by line. Both are
# plain text, read from the environment so neither is ever part of a
# program: this was a sed program with the host's folder spliced into it
# (Daybreak, 7 Nov). ENVIRON, not -v, which would read backslashes.
rewrite_logs() {
  LC_ALL=C REWRITE_FROM="$1" REWRITE_TO="$2" awk '
    BEGIN { from = ENVIRON["REWRITE_FROM"]; to = ENVIRON["REWRITE_TO"]; n = length(from) }
    {
      line = $0; out = ""
      while (n > 0 && (i = index(line, from)) > 0) {
        out = out substr(line, 1, i - 1) to
        line = substr(line, i + n)
        match(line, /^[A-Za-z0-9]*/)
        line = substr(line, RLENGTH + 1)
      }
      print out line
      fflush()
    }'
}
# The host that has this run's copy, and how long it took. A run that loses
# the race for a free host keeps its copy: copying again is what costs, as
# over a slow uplink a first copy took 22 minutes (6 Nov), and one thread's
# app check copied, lost, deleted and copied again. After a slow copy it
# waits in line on that host; after a quick one it chooses again, reusing
# the copy if that's the same host. THREADBEAR_CHECK_STAY_SECS is for the
# tests.
copied_host="" copy_secs=0 moves=""
stay_secs="${THREADBEAR_CHECK_STAY_SECS:-60}"
while :; do
  # A disk that's filled up since (the check that won may have done it) is no
  # place to wait: choosing again passes it over and deletes the copy.
  if [ -n "$copied_host" ] && [ "$copy_secs" -ge "$stay_secs" ] && ! host_is_low; then
    # HOST, HOST_OS and remote_base are still the copy's.
    FREE=0
  elif ! choose; then
    [ -z "$copied_host" ] || watched "$copied_host" "${SSH[@]}" "$copied_host" "rm -rf '$remote_run'" 2>/dev/null
    rm -f "$files" "$changed" "$left_out"
    if [ -n "$LOW" ]; then
      # Not 255: a disk to clear isn't a reason to run it all on this Mac.
      echo "No check host has room for this check: $LOW, and it wants $MIN_FREE_GIB GiB. Free some space there and run it again." >&2
      [ -z "$CANT" ] || echo "The others can't run it: $CANT." >&2
      exit "$BUSY_EXIT"
    fi
    if [ -n "$CANT" ]; then
      echo "None of $HOST_LIST can run this check ($CANT), and it doesn't run on this Mac instead. Add a host that can to THREADBEAR_CHECK_HOST, or leave it unset for runner.json's." >&2
      exit "$BUSY_EXIT"
    fi
    # The last word before this Mac takes the check: every host, on a fresh
    # connection of its own. Any that logs in at all (anything but ssh's own
    # 255, or no answer) is reachable, whatever went wrong on the way, and
    # the M5 isn't the place for a check a reachable host couldn't take.
    # Codex's reviews kept finding paths that took one for gone (a broken
    # shared connection, a host back since it dropped, a login whose shell
    # failed); asking here covers them all.
    answering=""
    for h in "${HOSTS[@]}"; do
      case "$h" in -*|"") continue ;; esac
      # A marker that came back proves a login, whatever the exit (a login
      # shell can exit 124 itself; Codex's fourth review).
      reply="$(timed 20 "${PING[@]}" "$h" echo tb-answers </dev/null 2>/dev/null)"
      rc=$?
      case "$reply:$rc" in *tb-answers*) answering+="${answering:+, }$h" ;; *:255|*:124) ;; *) answering+="${answering:+, }$h" ;; esac
    done
    if [ -n "$answering" ]; then
      echo "None of $HOST_LIST could take this check${CANT:+ ($CANT)}, but $answering still answers, so it doesn't run on this Mac instead. Run it again when that host is back to normal." >&2
      exit "$BUSY_EXIT"
    fi
    echo "None of $HOST_LIST can be reached, so this check can't run on any of them now." >&2
    exit 255
  fi
  if [ "$HOST" = "$copied_host" ]; then
    echo "Checking on $HOST (its copy is already there)"
  else
    [ -z "$copied_host" ] || watched "$copied_host" "${SSH[@]}" "$copied_host" "rm -rf '$remote_run'" 2>/dev/null
    copied_host=""
    # $moves (digits, empty until this check has left a line) keeps a new
    # copy's id from ever being one it withdrew, whose waiter may still be
    # in line there and would take the new copy for its own.
    id="$(date +%H%M%S)$moves-$$-$tries"
    remote_run="$remote_base/runs/$id"
    start=$SECONDS
    # This checkout's mirror on the host: one per Mac and checkout, so two
    # worktrees never write the same one. Only what changed goes over the
    # link; the run's copy is made from it there, of exactly this run's files
    # (a file deleted here stays in the mirror but isn't copied).
    key="$(printf '%s' "$(hostname -s):$ROOT" | shasum | cut -c1-16)"
    mirror="$remote_base/mirror/$key"
    # Two checks from this checkout at once take turns at the mirror, from
    # the upload until the run's copy is made, so neither copies the other's
    # files.
    copylock="$lock_dir/copy-$key-$(printf '%s' "$HOST" | tr -c 'A-Za-z0-9._-' '_').lock"
    if ! { [ -d "$(dirname "$lock_dir")" ] || mkdir -p "$(dirname "$lock_dir")"; } || ! private_dir "$lock_dir" || ! take_copylock "$copylock"; then
      rm -f "$files" "$changed" "$left_out"
      # Not 255: running it here instead isn't the answer to a broken lock.
      echo "Couldn't take the lock on this checkout's copy ($copylock; its folder must be this user's own, mode 700)." >&2
      exit 1
    fi
    # By content (-c), not size and time: an edit that keeps both would
    # otherwise leave the mirror's old copy in the run.
    copy_it() {
      "${SSH[@]}" "$HOST" "umask 022 && mkdir -p '$mirror' '$remote_run' && touch '$mirror' && for d in '$remote_base/mirror' '$mirror' '$remote_run'; do [ -d \"\$d\" ] && [ ! -L \"\$d\" ] && [ -O \"\$d\" ] && chmod go-w \"\$d\" || { echo \"\$d on the check host isn't this account's own\" >&2; exit 1; }; done" \
        && rsync -azc --from0 --files-from="$files" -e "${SSH[*]}" "$ROOT/" "$HOST:$mirror/" \
        && rsync -a -e "${SSH[*]}" "$files" "$HOST:$remote_run.files" \
        && "${SSH[@]}" "$HOST" "rsync -a --from0 --files-from='$remote_run.files' '$mirror/' '$remote_run/' && rm -f '$remote_run.files'" \
        && rsync -a -e "${SSH[*]}" "$changed" "$HOST:$remote_run/.check-changed" \
        && rsync -a -e "${SSH[*]}" "$left_out" "$HOST:$remote_run/.check-left-out"
    }
    watched "$HOST" copy_it
    rc=$?
    if [ "$rc" = 124 ] || [ "$rc" = 125 ]; then
      drop_copylock
      [ "$rc" != 125 ] || over_time busy
      dropped "$HOST"
      continue
    elif [ "$rc" != 0 ]; then
      drop_copylock
      watched "$HOST" "${SSH[@]}" "$HOST" "rm -rf '$remote_run' '$remote_run.files'" 2>/dev/null
      # The next host that can run it, before this Mac (6 Nov: Tinker Desk
      # dropped off mid-copy and the check ran on the M5).
      if host_is_low; then
        echo "Couldn't copy this checkout to $HOST (it has $FREE_GIB GiB free, and this check wants $MIN_FREE_GIB GiB); trying the others." >&2
        LOW_COPY+="${LOW_COPY:+, }$HOST has $FREE_GIB GiB free"
      else
        echo "Couldn't copy this checkout to $HOST; trying the others." >&2
        # Still answering: the copy failed there, not the link (a link that
        # went is what sends a check to this Mac; Codex's review, 14 Nov).
        if timed 20 "${PING[@]}" "$HOST" true </dev/null >/dev/null 2>&1; then
          CANT_COPY+="${CANT_COPY:+; }$HOST: it answers, but the copy to it failed"
        fi
      fi
      SKIP+=" $HOST"
      continue
    fi
    drop_copylock
    copied_host="$HOST" copy_secs=$((SECONDS - start))
    echo "Checking on $HOST (copied in ${copy_secs}s; THREADBEAR_CHECK_HOST= ./check.sh --local runs here)"
  fi

  ARGS=(${BASE_ARGS[@]+"${BASE_ARGS[@]}"})
  remote_shots=""
  [ -z "$SHOTS" ] || { remote_shots="$remote_base/shots-$id"; ARGS+=(--shots "$remote_shots"); }

  quoted=""
  for a in ${ARGS[@]+"${ARGS[@]}"}; do quoted+=" $(printf '%q' "$a")"; done
  inner="/bin/bash $(printf '%q' "$remote_run/scripts/check-remote.sh") --on-host $id$quoted"
  # The design screenshots go through Desk Runner, which has Screen Recording
  # there; over plain ssh they'd be drawn in-process, with the sidebar's
  # glass a flat fill.
  guard_pid=-
  if [ -n "$SHOTS" ]; then
    guard_pid="$BASE_REL/check-$id.pid"
    inner="\$HOME/.local/bin/desk $(printf '%q' "perl -e 'setpgrp(0, 0); exec @ARGV' /bin/bash $(printf '%q' "$remote_run/scripts/check-remote.sh") --on-host $id --pidfile $guard_pid$quoted")"
  fi
  # Under hangup-guard.sh, so the check stops there when this does, or when
  # `watched` gives up on the host, rather than running on with the host's
  # lock (13 Nov, remote.sh's run that went on recording on Tinker Desk). Its
  # stdin is the ssh's, which lockf and flock pass on: see alive_stdin. Inside
  # the lock, so a Desk Runner check has stopped before the next one can
  # start (Codex's review). One that loses its ssh while waiting for the lock
  # stops as soon as it gets it.
  guarded="$(guarded_cmd "$guard_pid" /bin/bash -c "$inner")"
  lock="$remote_base/lock"
  # A free host is taken without waiting (75 if another check got there
  # first); otherwise this waits for its turn.
  now=0
  [ "$FREE" = 1 ] && [ "$tries" -lt 3 ] && now=1
  if [ "$HOST_OS" = Linux ]; then
    # A server that runs its own runner and threads: the check goes in under them.
    run="nice -n 10 $guarded"
    if [ "$now" = 1 ]; then remote_cmd="flock -n -E 75 $lock $run"
    else remote_cmd="flock -n $lock true 2>/dev/null || echo 'Waiting for another check on $HOST to finish...'; flock $lock $run"; fi
  else
    run="$guarded"
    if [ "$now" = 1 ]; then remote_cmd="/usr/bin/lockf -t 0 -k $lock $run"
    else remote_cmd="/usr/bin/lockf -t 0 -k $lock true 2>/dev/null || echo 'Waiting for another check on $HOST to finish...'; /usr/bin/lockf -k $lock $run"; fi
  fi

  # A failed step's log path is on the host: its logs come back here, and the
  # path in the output is changed to match.
  local_logs="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-check-remote.XXXXXX")"
  check_ssh() { "${SSH[@]}" "$HOST" "$remote_cmd" <&7; }
  # In the background with a wait, so a TERM, HUP or INT to this script is
  # taken at once (on_signal) rather than once the check ends: bash holds a
  # trap until a pipeline it's waiting on has finished (Codex's review).
  ( watched "$HOST" check_ssh 2>&1 | rewrite_logs "$remote_base/tmp-$id/threadbear-check." "$local_logs"; exit "${PIPESTATUS[0]}" ) &
  waiter=$!
  # Waiting in line with other hosts to look at, and a copy quick enough to
  # make again elsewhere (a slow one waits, as above).
  if [ "$now" = 0 ] && [ "${#HOSTS[@]}" -gt 1 ] && [ "$copy_secs" -lt "$stay_secs" ] && leave_line "$waiter"; then
    rm -rf "$local_logs"
    copied_host=""
    moves=$((${moves:-0} + 1))
    continue
  fi
  wait "$waiter"
  code=$?
  if [ "$code" = 125 ]; then
    rmdir "$local_logs" 2>/dev/null
    # Still waiting for the host's lock, the check never started: its copy is
    # still under runs/, which --on-host moves out once it has the lock. A
    # host that doesn't answer is taken as still waiting too; a run that did
    # start (a hung step) is the check's own failure (Codex, 9 Nov).
    if ! timed 15 "${SSH[@]}" "$HOST" "test ! -d '$remote_run'" >/dev/null 2>&1; then over_time busy; fi
    over_time
  fi
  # 255 is ssh's own: the connection went before the check ended. Like a host
  # that stops answering, it's the next host's (or, with none, this Mac's): a
  # run that can't finish anywhere would hold the landing for nothing.
  if [ "$code" = 124 ] || [ "$code" = 255 ]; then
    # Its copy over there goes with that host's own clean-up (half a day).
    rmdir "$local_logs" 2>/dev/null
    if [ "$code" = 255 ]; then
      echo "Lost the connection to $HOST during the check; trying the others." >&2
      SKIP+=" $HOST"
      timed 10 "${SSH[@]}" -O exit "$HOST" >/dev/null 2>&1
      # A new connection still gets in: the check's channel broke, not the
      # host, which is no reason to run it here (Codex, 14 Nov).
      if timed 20 "${PING[@]}" "$HOST" true </dev/null >/dev/null 2>&1; then
        CANT_COPY+="${CANT_COPY:+; }$HOST: the check's connection broke, though it still answers"
      fi
    else
      dropped "$HOST"
    fi
    copied_host=""
    continue
  fi
  if [ "$now" = 1 ] && [ "$code" = 75 ]; then
    # Never started. Its copy stays for the next go (above).
    rmdir "$local_logs" 2>/dev/null
    if [ "$copy_secs" -ge "$stay_secs" ]; then echo "Another check took $HOST first; waiting for it there, as the copy took ${copy_secs}s."
    else echo "Another check took $HOST first; choosing again."; fi
    tries=$((tries + 1))
    continue
  fi
  break
done
rm -f "$files" "$changed" "$left_out"

# The disk as the failure left it (keep_logs), or as it is now if that couldn't
# be written. -s: a full disk can leave the file empty.
fail_kb=""
if [ "$code" != 0 ]; then
  fail_kb="$(watched "$HOST" "${SSH[@]}" "$HOST" "cd '$remote_base' && if [ -s 'kept-$id/free-kb' ]; then cat 'kept-$id/free-kb'; else $free_cmd; fi" 2>/dev/null)"
fi

# find, not a glob: the host's login shell may be zsh, which complains
# out loud when a glob matches nothing.
kept="$(watched "$HOST" "${SSH[@]}" "$HOST" "find '$remote_base/kept-$id' -maxdepth 1 -name 'threadbear-check.*' 2>/dev/null | head -1")"
if [ -n "$kept" ]; then
  watched "$HOST" rsync -a -e "${SSH[*]}" "$HOST:$kept/" "$local_logs/" >/dev/null
fi
watched "$HOST" "${SSH[@]}" "$HOST" "rm -rf '$remote_base/kept-$id' '$remote_run'" 2>/dev/null
[ -n "$(ls -A "$local_logs")" ] || rmdir "$local_logs"

if [ -n "$SHOTS" ]; then
  mkdir -p "$SHOTS"
  watched "$HOST" rsync -a -e "${SSH[*]}" "$HOST:$remote_shots/" "$SHOTS/" >/dev/null && watched "$HOST" "${SSH[@]}" "$HOST" "rm -rf '$remote_shots'"
fi
# Last, so it's the line read: what a full disk breaks often looks nothing like
# one (a CodeSign error, 7 Nov).
if low_on_room "$fail_kb"; then
  echo "$HOST had only $FREE_GIB GiB free when this failed, and this check wants $MIN_FREE_GIB GiB, which may be why. Free some space there and run it again." >&2
fi
exit "$code"
