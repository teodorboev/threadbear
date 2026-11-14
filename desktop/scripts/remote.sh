#!/bin/bash
# Runs a command from this checkout on another machine, so the Rust builds and
# the spec runs stay off the Mac you're working on (AGENTS.md).
#
#   desktop/scripts/remote.sh <ssh host> [--desk] [--perf] [--try] -- <command...>
#
# --try is for automatic updates: exit 75 instead of queueing behind this
# checkout's run or either host lease, leaving queued measurements ahead.
#
# It copies the checkout, committed or not, less its build folders, to a folder
# of its own under ~/.cache/threadbear-check/gpui there (so two checkouts at once
# don't overwrite each other), and runs the command from that folder, with Rust
# and Node on the PATH, npm packages installed the first time, and cargo's target
# folder kept between runs. --desk runs it on Tinker Desk's desktop (Desk Runner),
# which is what a window needs there.
#
# Everything lives under ~/.cache/threadbear-check, as scripts/check-remote.sh
# has it, so the host still looks like one with no Threadbear on it. Tinker
# Desk is also where clean installs get tried.
#
# Runs from one checkout to one host take turns: each copies over the folder
# the last one is building from, so an overlap tested whichever copy landed
# last, or files deleted mid-build. A script run there that edits the copy and
# puts a file back must touch it afterwards: put back, it's older than the build
# of the edit, and cargo calls that build fresh.
set -euo pipefail
host="${1:?usage: remote.sh <ssh host> [--desk] [--perf] -- <command>}"; shift
desk=0
perf=0
try_flag=""
# An older remote.sh ignores this request and still builds normally. The
# updater lives in the app bundle but this script comes from main, which can
# be rolled back to a version from before --try existed.
[ "${THREADBEAR_REMOTE_TRY:-}" != 1 ] || try_flag="--try "
while [ $# -gt 0 ]; do
  case "$1" in
    --desk) desk=1; shift ;;
    --perf) perf=1; shift ;;
    --try) try_flag="--try "; shift ;;
    --) shift; break ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] || { echo "no command given" >&2; exit 2; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
base=".cache/threadbear-check/gpui"
# Each checkout's folders there are its own, by its path here. A caller whose
# path is the same on every Mac (the app's update clone, under the same cache
# folder for anyone with the same user name) adds THREADBEAR_REMOTE_KEY, this
# Mac's own id, so two Macs sharing a host never build in each other's
# folders (Codex's review, 12 Nov). stage-update.sh works out the same key.
key="$(printf '%s%s' "$root" "${THREADBEAR_REMOTE_KEY:+@$THREADBEAR_REMOTE_KEY}" | shasum | cut -c1-10)"
src="$base/src-$key"
tag="$key-$$-$RANDOM$RANDOM"
target="$base/target-$key"
# Performance work shares a warm target, under the whole-run host lock. Keep
# updater/dev targets per checkout: their post-run scp reads those binaries.
[ "$perf" = 0 ] || target="$base/perf-target"

# The turn is a lock on a file this script holds open (fd 9), so it goes when
# the script does, however it ends. Not lockf or flock running this script as
# their child: kill that and the turn was free while this went on. ssh drops fd
# 9 as it starts. The setup steps before the run are short, and a TERM or HUP
# then is taken once the step ends. The run itself is a background ssh this
# waits for, so a signal stops it at once: the trap kills that ssh, and
# hangup-guard.sh on the host stops the command when its ssh goes (13 Nov: a
# stopped --desk run went on building and recording on Tinker Desk). A KILL
# can't be trapped, but it takes the ssh too when it's sent to the group, as
# Ctrl-C and Claude Code's stop are, and the guard does the rest.
# The folder is fixed, not TMPDIR, so every run from the checkout finds
# the same file. perl for macOS, which has no flock command.
. "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/remote-guard-lib.sh"
# Every ssh here (rsync's too) sends a keep-alive every 30 s and gives up after
# 4 unanswered, so a connection that dies quietly fails in about 2 minutes. On
# 14 Nov a network drop to Tinker Desk left a --desk --perf run sitting on a
# dead connection after "Waiting for another build or measurement...", twice,
# for over half an hour: the host had already ended its side, and with no
# traffic this ssh never found out. scripts/check-remote.sh has the same.
# An array, not a function: a background function is a subshell, and its $!
# isn't the ssh the signal trap has to kill.
SSH=(ssh -o ServerAliveInterval=30 -o ServerAliveCountMax=4)
# wait_run: waits for the background ssh, and says when it was ssh itself that
# failed (255: the connection dropped, or never came up), so a drop doesn't
# read as the lock wait or build step that was last printed.
wait_run() {
  local rc=0
  wait "$ssh_pid" || rc=$?
  ssh_pid=""
  [ "$rc" != 255 ] || echo "remote.sh: lost the ssh connection to $host (ssh exited 255; a dropped connection fails within about 2 minutes). Whatever it last said it was doing, including waiting for a lock, didn't finish. The host stops its side when it sees the connection go. Run it again once $host answers." >&2
  return "$rc"
}
ssh_pid=""
lock_pid=""
# The turn is kept until the host says the run has gone (30 s at most): the
# guard there takes up to 5 s to stop a command that ignores TERM, and the
# next run mustn't copy over its folder while it's still going (Codex's
# review). [r] so the pgrep's own shell, whose command line has the pattern
# in it, doesn't count. A host that can't be reached says nothing either way.
# Every child of this script goes first, the turn's waiter among them even in
# the moment before lock_pid is set. || true throughout: under set -e a kill
# of one that has just exited would end the trap before its exit (Codex).
on_signal() {
  [ -z "$lock_pid" ] || kill "$lock_pid" 2>/dev/null || true
  pkill -TERM -P $$ 2>/dev/null || true
  if [ -n "$ssh_pid" ]; then
    kill "$ssh_pid" 2>/dev/null || true
    "${SSH[@]}" -o BatchMode=yes -o ConnectTimeout=5 "$host" \
      "i=0; while pgrep -f '[r]un-$tag' >/dev/null && [ \$i -lt 60 ]; do sleep 0.5; i=\$((i + 1)); done" </dev/null >/dev/null 2>&1 || true
  fi
  exit "$1"
}
trap 'on_signal 143' TERM
trap 'on_signal 129' HUP
trap 'on_signal 130' INT
if [ "$(uname -s)" = Darwin ]; then turns="$HOME/Library/Caches/Threadbear"; else turns="${XDG_CACHE_HOME:-$HOME/.cache}/threadbear"; fi
mkdir -p "$turns/remote"
exec 9>>"$turns/remote/$key-$(printf '%s' "$host" | tr -c 'A-Za-z0-9.-' _).lock"
waiting="Waiting for another remote.sh run from this checkout on $host to finish..."
# The wait for the turn is a background child this waits for, like the run's
# ssh: bash holds a trap until a foreground child ends, so a TERM while
# another run held the turn went unanswered until that run finished (14 Nov,
# d7aca215). The lock is the open file's, so the child taking it on fd 9
# takes it for this script too, and it stays after the child exits.
# turn_cmd <block>: the command that takes the turn, blocking or not (exit 1
# if it's taken). Run directly, never through a function, so a background one's
# $! is the flock or perl itself, not a subshell above it that a kill would
# leave it behind (Codex's review).
if command -v flock >/dev/null 2>&1; then
  turn_cmd() { if [ "$1" = 0 ]; then turn=(flock -n 9); else turn=(flock 9); fi; }
else
  turn_cmd() { turn=(perl -e 'use Fcntl ":flock"; open(my $f, ">&=", 9) or die "remote.sh: no lock file: $!\n"; flock($f, $ARGV[0] ? LOCK_EX : LOCK_EX | LOCK_NB) or exit 1' "$1"); }
fi
turn_cmd 0
if ! "${turn[@]}"; then
  if [ -n "$try_flag" ]; then
    echo "Another remote.sh run from this checkout is busy; deferring the update build." >&2
    exit 75
  fi
  echo "$waiting" >&2
  turn_cmd 1
  "${turn[@]}" &
  lock_pid=$!
  wait "$lock_pid" || { echo "remote.sh: couldn't take the turn lock." >&2; exit 1; }
  lock_pid=""
fi

# A checkout that hasn't been run for a few days gives its folders back: a
# target is many GB and the machines are short of disk. The bare src and target
# are from before each checkout had its own. The target is touched as well as
# the src because cargo working inside it doesn't change its time, and a folder
# in daily use would otherwise look a few days old.
#
# A target with no .synced-by-content was built from copies made with rsync -a
# (below), and may hold a build cargo wrongly calls fresh that a copy by content
# would never fix, as nothing in it changes. Its sources are touched once, so
# its own crates build again, and a run that can't do that doesn't go on.
# Exit 9 is that touch failing; any other failure here is let go, as before (a
# marker not written, on a full disk, just means touching them again next time).
# Cache eviction can remove GB, so it gets a turn too. Bootstrap the small
# guard files first: the worktree copy may not exist yet, or be queued for
# eviction. No build or capture starts until the final whole-command turn.
boot="$base/run-$tag-tools"
# The helpers are written here and then run, so every folder down to them must
# be this account's own and closed to others, or the run stops: a group with
# write on the cache could otherwise swap one in between (Codex's review of
# Daybreak 114/116, 13 Nov). A fresh boot folder each run, never one found there.
"${SSH[@]}" "$host" "umask 077 && mkdir -p $base && for d in .cache ${base%/*} $base; do [ -d \$d ] && [ ! -L \$d ] && [ -O \$d ] && chmod go-w \$d || { echo \"remote.sh: ~/\$d on $host isn't a folder of this account's own\" >&2; exit 1; }; done && rm -rf $boot && mkdir -m 700 $boot"
for helper in perf-lock.pl perf-children.pl hangup-guard.sh; do
  "${SSH[@]}" "$host" "cat > $boot/$helper" <"$root/scripts/$helper"
done
"${SSH[@]}" "$host" "cat > $boot/trim-remote-cache.sh" <"$root/desktop/scripts/trim-remote-cache.sh"
# The age limit goes by the folder's own time, which a long run inside it
# doesn't move: one with anything changed inside in the last 15 minutes (a
# run's .in-use marker, perf-lock.pl's heartbeat) stays, as the trim's do
# (9c00a5ed, 13 Nov).
#
# A checkout's target from before the shared ones (cargo's own folders in it)
# is a whole build, many GB, that nothing reads any more: it goes, and the
# folder's left for the programs builds hand back. Not on a --perf run, whose
# own target is perf-target and whose old one the perf threads may still want.
# Found with find, not a glob: Tinker Desk's login shell is zsh, where a glob
# that matches nothing ends the whole command.
old_target=""
[ "$perf" = 1 ] || old_target="[ -z \"\$(find $base/target-$key -maxdepth 3 -name .fingerprint -type d 2>/dev/null | head -1)\" ] || rm -rf $base/target-$key; "
precopy="$old_target""mkdir -p $src $target; [ -e $target/.synced-by-content ] || { if [ -d $src ]; then find $src -type f -exec touch -c {} + || exit 9; fi; touch $target/.synced-by-content 2>/dev/null; }; touch $src $target; find $base -mindepth 1 -maxdepth 1 \\( -name 'src-*' -o -name 'target-*' -o -name src -o -name target \\) -mtime +3 ! -name 'src-$key' ! -name '${target##*/}' 2>/dev/null | while IFS= read -r d; do [ -n \"\$(find \"\$d\" -mmin -15 2>/dev/null | head -1)\" ] || rm -rf \"\$d\"; done; find $base -mindepth 1 -maxdepth 1 -name 'run-*' -mmin +720 -exec rm -rf {} + 2>/dev/null; true
cd \"\$HOME/$base\"
bash \"\$HOME/$boot/trim-remote-cache.sh\" $key ${target##*/}"
alive_stdin 7
remote="env THREADBEAR_REMOTE_SOURCE=\"\$HOME/$src\" THREADBEAR_REMOTE_TARGET=\"\$HOME/$target\" THREADBEAR_REMOTE_TOOLS=\"\$HOME/$boot\" perl $boot/perf-lock.pl ${try_flag}- bash -c $(single_quoted "$precopy")"
"${SSH[@]}" "$host" "$remote" <&7 &
ssh_pid=$!
rc=0
wait_run || rc=$?
if [ "$rc" = 9 ]; then
  echo "Couldn't mark this checkout's copy on $host to build again, so it may hold a stale build. Not running." >&2
  exit 1
fi
if [ "$rc" != 0 ]; then
  # A deferred attempt has only uploaded its own helpers. Give those back
  # now rather than accumulate a tools folder at every five-minute retry.
  [ "$rc" != 75 ] || "${SSH[@]}" "$host" "rm -rf $boot" </dev/null || true
  exit "$rc"
fi
# Named by hand, not read from .gitignore: rsync takes those rules from the
# receiving side when it decides what to delete, which removed part of a
# Swift build and a node_modules here on 9 Nov.
#
# Files are compared by content (-c) and written with the host's own time, not
# this Mac's (no -t, so not -a). Cargo calls a crate fresh when its files are
# older than its last build, and a Mac time can be: a file changed here before a
# build there that compiled the old copy (an overlapping run, a script that
# edited the copy) arrived looking older than that build, and the next run
# passed on stale code (11 Nov, thread 699f31d2). Unchanged files keep their
# time, so they don't rebuild. desktop/scripts/test-remote-freshness.sh <host>
# plays that case.
rsync -rlpDcz -e "${SSH[*]}" --delete --exclude .git --exclude node_modules --exclude .build --exclude target --exclude reports \
  --exclude .fixtures --exclude .swiftpm --exclude dist --exclude DerivedData --exclude .DS_Store \
  "$root/" "$host:$src/"

printf -v cmd '%q ' "$@"
# The run's first argument, with --desk, is its pidfile (hangup-guard.sh):
# Desk Runner starts it away from the guard, so it says where it is.
read -r -d '' prelude <<PRELUDE || true
if [ -n "\${1:-}" ]; then
  echo \$\$ >"\$1"
  [ ! -e "\$1.gone" ] || exit 129
fi
set -e
if [ "$desk" = 1 ]; then caffeinate -u -t 2 >/dev/null 2>&1 || true; fi
cd "\$HOME/$src"
# rsync changes this folder's time only when it writes into it, and the age limit above reads it.
touch .
# So trim-remote-cache.sh can tell this target's in use for the whole run. Its
# output goes nowhere, or ssh would wait for it after the command ends.
# The mkdir is for a host so full the one above failed, until the trim made room.
# || true because this inherits set -e, and a touch on a full disk mustn't end it.
mkdir -p "\$HOME/$target"
( while kill -0 \$\$ 2>/dev/null; do touch "\$HOME/$target/.in-use" || true; sleep 60; done ) >/dev/null 2>&1 </dev/null &
. runner/scripts/find-node.sh
NODE="\$(threadbear_find_node)"
export PATH="\${NODE%/*}:\$PATH"
if [ -x "\$HOME/$base/cargo/bin/cargo" ]; then
  export CARGO_HOME="\$HOME/$base/cargo" RUSTUP_HOME="\$HOME/$base/rustup"
  export PATH="\$CARGO_HOME/bin:\$PATH"
elif [ -x "\$HOME/.cargo/bin/cargo" ]; then
  export PATH="\$HOME/.cargo/bin:\$PATH"
fi
# Where a run finds what it built: this checkout's own folder, or perf-target.
export CARGO_TARGET_DIR="\$HOME/$target" CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
if [ "$perf" = 1 ]; then
  export THREADBEAR_PERF_BUILD_KEY="$key" THREADBEAR_PERF_BUILD_SOURCE="\$PWD"
  bash scripts/perf-build.sh --prepare
else
  unset THREADBEAR_PERF_BUILD_KEY THREADBEAR_PERF_BUILD_SOURCE
  # Ordinary builds share a few warm targets on the host, taking turns
  # (desktop/scripts/shared-cargo/cargo, first on the PATH, says how): one
  # target per checkout, up to 8 GB each, filled the cloud box on 13 Nov. The
  # checkout's own target-<key> holds only the programs each build hands back,
  # so CARGO_TARGET_DIR still names where a run finds what it built. A --perf
  # run keeps its own stable target.
fi
# A --perf run goes through it too, from a source copy of its own into perf-target.
export THREADBEAR_SHARED_CARGO_BASE="\$HOME/$base" THREADBEAR_SHARED_CARGO_SRC="\$HOME/$src" THREADBEAR_SHARED_CARGO_KEY="$key"
# Debug builds keep line numbers (enough for a panic's backtrace) but not the
# rest of the debug info, which was most of a target: one checkout's reached
# 15 GB in an hour on the 75 GB cloud box, the threadbear-gpui test binary
# alone 1.1 GB, and four of them filled it (13 Nov). Nothing reads the rest
# there: no debugger runs on these hosts.
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export TMPDIR="\$HOME/$base/tmp"
mkdir -p "\$TMPDIR"
. scripts/build-priority.sh
# The shared targets' cargo goes in front of build-priority's, which wrote the
# real cargo's path into its own as it was made: so the slot's wait and copy
# come first, then the priority, then cargo. In front of it before that, the
# priority cargo would have called this one, and this one it, for ever.
export PATH="\$HOME/$src/desktop/scripts/shared-cargo:\$PATH"
# What the specs need, the first time this folder's used.
for dir in features app/Tools/mock-runner runner; do
  [ -d "\$dir/node_modules" ] || npm ci --silent --prefix "\$dir" >/dev/null 2>&1 || true
done
PRELUDE

# A disk that fills during the run fails it in ways that can look like a pass:
# on 13 Nov every spec scenario failed with ENOSPC on the cloud box, cargo said
# nothing, and the run ended 0. So a run that leaves the host under 1 GiB free
# fails, whatever its command said, and says why.
read -r -d '' postlude <<POSTLUDE || true
kb="\$(df -Pk "\$HOME/$base" 2>/dev/null | awk 'END { print \$4 }')"
case "\$kb" in
  ""|*[!0123456789-]*) ;;
  *) if [ "\$kb" -lt 1048576 ]; then
       echo "remote.sh: $host has only \$((kb / 1024)) MiB free after this run, so whatever it wrote may have failed for want of space, even if it said it passed. Treating it as failed. Run it again: it gives back idle build folders there first, or says why it can't." >&2
       [ "\$rc" != 0 ] || rc=1
     fi ;;
esac
exit "\$rc"
POSTLUDE
run="$prelude
rc=0
$cmd || rc=\$?
$postlude"

# The run goes up as a file, as ssh's stdin is the guard's: nothing is sent on
# it, and it ending is how the host knows this has gone.
# Named by this run alone: a stopped --desk run's .gone file stays there (the
# guard's comment says why) until the clear-out above, half a day on.
script="$base/run-$tag.sh"
"${SSH[@]}" "$host" "cat > $script" <<<"$run"
alive_stdin 7
# What a run waiting for the host's perf lock is told the holder is running
# (perf-lock.pl's holder note): the checkout's folder, which names the thread's
# slot, and the command.
label="remote.sh from $(basename "$root")$([ "$desk" = 1 ] && printf ' --desk' || true): $*"
label="THREADBEAR_PERF_LOCK_LABEL=$(single_quoted "${label:0:200}")"
if [ "$desk" = 1 ]; then
  pidfile="$base/run-$tag.pid"
  remote="env $label THREADBEAR_REMOTE_SOURCE=\"\$HOME/$src\" THREADBEAR_REMOTE_TARGET=\"\$HOME/$target\" perl $src/scripts/perf-lock.pl ${try_flag}--desk $pidfile bash $script $pidfile"
else
  remote="env $label THREADBEAR_REMOTE_SOURCE=\"\$HOME/$src\" THREADBEAR_REMOTE_TARGET=\"\$HOME/$target\" perl $src/scripts/perf-lock.pl ${try_flag}- bash $script"
fi
"${SSH[@]}" "$host" "$remote; rc=\$?; rm -f $script; rm -rf $boot; exit \$rc" <&7 &
ssh_pid=$!
rc=0
wait_run || rc=$?
exit "$rc"
