#!/bin/bash
# Run by remote.sh on the host, from its gpui folder, before it copies or builds
# anything: gives disk back when the host is short, and refuses the run when it
# still is. remote.sh's age limit doesn't help when several checkouts are all in
# use, and these folders filled the cloud box on 9 Nov, which runs a runner too:
# its saves started failing (THREADBEAR-1R).
#
#   trim-remote-cache.sh <this checkout's key>
#   trim-remote-cache.sh -
#
# Exit 3: too little disk left to build on. With - (scripts/check-remote.sh,
# finding the host short for a check of its own) there's no checkout of ours
# here: it only gives room back, and never refuses.
set -u
own="${1:?usage: trim-remote-cache.sh <key>|-}"

# Under this, idle checkouts lose their target folders: room for a build from
# nothing (a debug target has reached 7 GiB) with some to spare.
TRIM_GIB=12
# Under these a run isn't started at all: it would take the last of the disk
# from whatever else runs on the host. A first build writes far more than one
# with a target to build on.
REFUSE_GIB=3
REFUSE_COLD_GIB=8
# A target with anything changed this recently is in use. Cargo writes as it
# compiles, and remote.sh touches .in-use inside it every minute of a run, so
# this also covers a run that's only driving a built binary.
BUSY_MINUTES=15
# At most this many checkouts keep a target here, however much room there is.
# Room is only looked at when a run starts, and on 12 Nov seven targets of 5
# to 7 GiB had grown between runs until Tinker Desk had 1.8 GiB left and no
# check could run there. Idle ones past this go, longest unused first.
MAX_TARGETS=4

free_gib() {
  local kb
  kb="$(df -Pk . | awk 'NR==2 {print $4}')"
  case "$kb" in
    # A filesystem past its reserved blocks can report less than nothing.
    -[0123456789]*) echo 0; return ;;
    # A df we can't read says nothing: don't delete or refuse on it.
    ""|*[!0123456789]*) echo 999999; return ;;
  esac
  echo $((kb / 1048576))
}

# Whether anything in a folder changed lately. Look inside it, not at it: a
# checkout's own time only moves when rsync writes into it, and cargo working
# inside a folder doesn't change that folder's. A folder that isn't there isn't.
recent() {
  [ -n "$(find "$1" -mmin -"$BUSY_MINUTES" 2>/dev/null | head -1)" ]
}

# Removes a target folder and its checkout's copy if the host is short and
# nothing's using either. The copy counts too: a run touches it as it starts
# and rsync writes into it, before the run's marker in the target begins, so a
# long copy still looks in use (Codex's review, 12 Nov). With "always" they go
# even with room to spare (one past MAX_TARGETS).
evict() {
  [ "${2:-}" = always ] || [ "$(free_gib)" -lt "$TRIM_GIB" ] || return 0
  [ -d "$1" ] || return 0
  local key="${1#target}"
  key="${key#-}"
  local src=""
  [ -z "$key" ] || src="src-$key"
  recent "$1" && return 0
  [ -n "$src" ] && recent "$src" && return 0
  if [ "${2:-}" = always ]; then
    echo "More than $MAX_TARGETS build folders on this host, so removing $1, which nothing has used lately. Its checkout rebuilds next time it runs." >&2
  else
    echo "Short of disk on this host, so removing $1, which nothing has used lately. Its checkout rebuilds next time it runs." >&2
  fi
  # Both moved aside before either is deleted, as the deleting takes a while
  # and a run that starts in that time should find no folders rather than half
  # of one, and must never have its new copy deleted under it. The names stay
  # target-* and src-*, so ones left by a crash here go another time.
  local gone="target-gone-$$-${key:-shared}"
  mv "$1" "$gone" || return 0
  local src_gone=""
  # Looked at again once the target's gone: a run that started in between has
  # touched its copy (remote.sh does as it starts), and keeps it, building
  # from cold. A lock on the host would close the last instant too; a run that
  # loses that race fails its copy and works the next time (Codex, 12 Nov).
  if [ -n "$src" ] && [ -d "$src" ] && ! recent "$src" && mv "$src" "src-gone-$$-$key"; then src_gone="src-gone-$$-$key"; fi
  rm -rf "$gone" ${src_gone:+"$src_gone"}
  return 0
}

# One trim at a time on a host, holding a lock (fd 8, so it goes when this
# does, however it ends) from before it decides a folder is idle until it's
# gone, and this checkout's own folders are marked used under the same lock.
# That closes the instant the second look in evict leaves: a run starting
# between another trim's look and its move. A remote.sh from before this lock
# still has that instant. The lock is on this folder itself, not a file in it,
# so a disk too full to make a file still takes turns. perl for macOS, which
# has no flock command.
exec 8<.
if command -v flock >/dev/null 2>&1; then flock 8
else perl -e 'use Fcntl ":flock"; open(my $f, "<&=", 8) or die "trim: no lock: $!\n"; flock($f, LOCK_EX) or die "trim: lock: $!\n"' || exit 1; fi
if [ "$own" != - ]; then
  mkdir -p "src-$own" "target-$own" 2>/dev/null
  touch "src-$own" "target-$own" 2>/dev/null
fi

# Runs' temp files (TMPDIR for every remote.sh run) that nothing has touched for
# a couple of days: 19,000 of them on Tinker Desk, 12 Nov. SwiftPM's lock files
# are kept while the copy they're named for is here: one is reused by every
# Swift build in that copy, and only holding it open shows it's in use, not its
# time, so deleting it would let two builds each take a lock of their own.
if [ -d tmp ]; then
  find tmp -mindepth 1 -maxdepth 1 -mtime +2 2>/dev/null | while IFS= read -r f; do
    case "$f" in
      *.lock)
        k="${f#*_gpui_src-}"
        if [ "$k" != "$f" ]; then [ -e "src-${k%%_*}" ] && continue
        else case "$f" in *_gpui_src_*) [ -e src ] && continue ;; esac; fi ;;
    esac
    rm -rf "$f"
  done
fi

# The one folder every checkout shared before they each got their own.
evict target
# Past the cap first, whatever the room: the newest MAX_TARGETS stay (this
# checkout's always among them), and of the rest, those idle go.
# This checkout's own counts as one of them whether or not it's built yet.
limit=$MAX_TARGETS; [ "$own" = - ] || limit=$((MAX_TARGETS - 1))
n=0
for t in $(ls -dt target-* 2>/dev/null); do
  case "$t" in target-gone-*|"target-$own") continue ;; esac
  n=$((n + 1))
  [ "$n" -le "$limit" ] || evict "$t" always
done
# Longest since a run first: remote.sh touches each target as a run starts.
# Names are target-<hex>, so the unquoted list is safe.
for t in $(ls -dtr target-* 2>/dev/null); do
  [ "$t" = "target-$own" ] || evict "$t"
done

# A copy whose target has gone (deleted by hand, or by an older trim that left
# copies) is up to 2.4 GiB of node_modules nothing uses: a run makes its target
# before it copies anything. Not one with anything changed lately, in case
# it's mid-start.
for c in $(ls -d src-* 2>/dev/null); do
  [ "$c" != "src-$own" ] && [ ! -e "target-${c#src-}" ] || continue
  recent "$c" || rm -rf "$c"
done

[ "$own" != - ] || exit 0
need="$REFUSE_GIB"
# By size, not whether it's empty: a first build that died early leaves a
# folder with next to nothing to build on.
kb="$(du -sk "target-$own" 2>/dev/null | awk '{print $1}')"
[ "${kb:-0}" -ge 1048576 ] || need="$REFUSE_COLD_GIB"
free="$(free_gib)"
if [ "$free" -lt "$need" ]; then
  echo "This host has only $free GiB free and this run wants $need, with every other build folder in use, so it won't start. Free some space there and run it again." >&2
  exit 3
fi
