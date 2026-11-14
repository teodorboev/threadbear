#!/bin/bash
# Run by remote.sh on the host, from its gpui folder, before it copies or builds
# anything: gives disk back when the host is short, and refuses the run when it
# still is. remote.sh's age limit doesn't help when several checkouts are all in
# use, and these folders filled the cloud box on 9 Nov, which runs a runner too:
# its saves started failing (THREADBEAR-1R).
#
#   trim-remote-cache.sh <this checkout's key> [active target folder]
#   trim-remote-cache.sh -
#
# Exit 3: too little disk left to build on. With - (scripts/check-remote.sh,
# finding the host short for a check of its own) there's no checkout of ours
# here: it only gives room back, and never refuses.
set -u
own="${1:?usage: trim-remote-cache.sh <key>|-}"
active_target="${2:-target-$own}"

# Under this, idle checkouts lose their target folders: room for a build from
# nothing (a debug target has reached 7 GiB) with some to spare.
TRIM_GIB=12
# Under these a run isn't started at all: it would take the last of the disk
# from whatever else runs on the host. A first build writes far more than one
# with a target to build on.
REFUSE_GIB=5
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
  [ "${2:-}" = always ] || [ "${2:-}" = old ] || [ "$(free_gib)" -lt "$TRIM_GIB" ] || return 0
  [ -d "$1" ] || return 0
  local key="${1#target}"
  key="${key#-}"
  local src=""
  [ -z "$key" ] || src="src-$key"
  # This source is active on perf-target, not its old normal target. Count
  # and evict that old target normally, while keeping the live source.
  [ "$key" != "$own" ] || [ "$active_target" != perf-target ] || src=""
  recent "$1" && return 0
  [ -n "$src" ] && recent "$src" && return 0
  if [ "${2:-}" = old ]; then
    echo "Removing $1, a whole build from before checkouts shared this host's build folders, which nothing has used lately." >&2
  elif [ "${2:-}" = always ]; then
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

# Called by check-remote.sh (-) to make room while it picks a host, outside
# the host's locks: then it gives nothing back while a build, measurement or
# check holds them (scripts/perf-lock.pl's two), as deleting GBs would skew a
# measurement there (9c00a5ed). It holds the ones it takes until it's done, so
# none starts mid-trim. remote.sh's own trim already runs inside them.
if [ "$own" = - ]; then
  fd=5
  for lockfile in "$HOME/.cache/threadbear-perf.lock" "$HOME/.cache/threadbear-check/lock"; do
    [ -e "$lockfile" ] || continue
    eval "exec $fd>>\"\$lockfile\"" || exit 0
    if command -v flock >/dev/null 2>&1; then got=0; flock -n "$fd" && got=1
    else got=0; perl -e 'use Fcntl ":flock"; open(my $f, ">&=", $ARGV[0]) or exit 1; flock($f, LOCK_EX | LOCK_NB) or exit 1' "$fd" && got=1; fi
    if [ "$got" = 0 ]; then
      echo "A build, measurement or check is running on this host, so no build folders are given back now." >&2
      exit 0
    fi
    fd=$((fd + 1))
  done
fi

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
  mkdir -p "src-$own" 2>/dev/null
  touch "src-$own" 2>/dev/null
  if [ "$active_target" != perf-target ]; then
    mkdir -p "target-$own" 2>/dev/null
    touch "target-$own" 2>/dev/null
  fi
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
limit=$MAX_TARGETS
own_target=""
if [ "$own" != - ] && [ "$active_target" != perf-target ]; then
  own_target="target-$own"
  limit=$((MAX_TARGETS - 1))
fi
n=0
for t in $(ls -dt target-* 2>/dev/null); do
  case "$t" in target-gone-*|"$own_target") continue ;; esac
  n=$((n + 1))
  [ "$n" -le "$limit" ] || evict "$t" always
done
# A checkout's target with cargo's own folders in it is a whole build from
# before builds shared this host's few (shared-cargo/cargo), 1 to 9 GB that
# nothing from this remote.sh reads: idle ones go, whatever the room. A
# checkout still on an older remote.sh builds its own again next time.
for t in $(ls -d target-* 2>/dev/null); do
  case "$t" in target-gone-*|"target-$own") continue ;; esac
  [ -z "$(find "$t" -maxdepth 3 -name .fingerprint -type d 2>/dev/null | head -1)" ] || evict "$t" old
done
# Longest since a run first: remote.sh touches each target as a run starts.
# Names are target-<hex>, so the unquoted list is safe.
for t in $(ls -dtr target-* 2>/dev/null); do
  [ "$t" = "$own_target" ] || evict "$t"
done

# The shared targets builds take turns in (shared-cargo/cargo): one goes only
# when the host is too short to build even on a warm target, and no build
# holds it (its lock, taken here without waiting and held while it's moved
# aside). Last, and not just to get back to TRIM_GIB, as each is warm for
# every checkout: losing one makes the next builds cold, and wanting more
# room. Its copy of the source is small and stays.
for lockfile in $(ls shared-*.lock 2>/dev/null); do
  [ "$(free_gib)" -lt "$REFUSE_GIB" ] || break
  slot="${lockfile%.lock}"
  [ -d "$slot/target" ] || continue
  (
    exec 7>>"$lockfile"
    if command -v flock >/dev/null 2>&1; then flock -n 7 || exit 0
    else perl -e 'use Fcntl ":flock"; open(my $f, ">&=", 7) or exit 1; flock($f, LOCK_EX | LOCK_NB) or exit 1' || exit 0; fi
    echo "Short of disk on this host, so removing $slot/target, a shared build folder no build is using. The next build there starts from cold." >&2
    gone="target-gone-$$-$slot"
    mv "$slot/target" "$gone" && rm -rf "$gone"
  )
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
# A shared target counts: ordinary builds go there now, and a checkout's own
# holds only what they hand back. A --perf run's is its own.
warm=0
targets=("$active_target")
[ "$active_target" = perf-target ] || targets+=(shared-*/target)
for t in "${targets[@]}"; do
  [ -d "$t" ] || continue
  kb="$(du -sk "$t" 2>/dev/null | awk '{print $1}')"
  [ "${kb:-0}" -lt 1048576 ] || { warm=1; break; }
done
[ "$warm" = 1 ] || need="$REFUSE_COLD_GIB"
free="$(free_gib)"
if [ "$free" -lt "$need" ]; then
  echo "This host has only $free GiB free and this run wants $need, with every other build folder in use, so it won't start. Free some space there and run it again." >&2
  exit 3
fi
