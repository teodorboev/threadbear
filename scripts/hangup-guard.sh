# hangup-guard.sh <pidfile|-> <command...>
#
# Runs on a check host, as `bash -c "<this file>" hangup-guard ...` in an ssh
# command (guarded_cmd in scripts/remote-guard-lib.sh puts it together), so it needn't
# be there already. It runs the command, and when the ssh that started it goes
# (killed, Ctrl-C, the connection dropped), stops the command and everything
# under it.
#
# Without it they ran on: ssh without a terminal gives the remote side no
# hangup, so a command under it only finds out its caller's gone when it next
# writes, and a build or a recording may not. On 13 Nov a stopped remote.sh
# --desk run on Tinker Desk went on to build and record its clips over another
# thread's (27401630).
#
# How it knows: its stdin is the ssh channel, and the local side never writes
# to it (alive_stdin in remote-guard-lib.sh), so a read
# that ends means the ssh client is gone. The command gets /dev/null instead.
#
# The command runs in a process group of its own (perl's setpgrp, as macOS has
# no setsid command), so the group catches anything that left the tree, and the
# tree catches anything that made a group of its own.
#
# Desk Runner (Tinker Desk's ~/.local/bin/desk) runs its command in the GUI
# session, not under this, so the desk client dying wouldn't touch it. For that,
# the command run there writes its pid to <pidfile> as it starts (remote.sh's
# run script does), and exits at once if <pidfile>.gone is there, which is
# written before the pidfile is read, in case the ssh went before it started.
# Its process group is only signalled when it leads one, never Desk Runner's.
# After a hangup both files stay, for a run that starts late to find.

pidfile="$1"
shift
[ "$pidfile" != - ] || pidfile=""
exec 3<&0 </dev/null

tree_of() {
  local kid
  echo "$1"
  for kid in $(pgrep -P "$1" 2>/dev/null); do tree_of "$kid"; done
}
# stop <pid>...: each one's group (where it leads one) and tree, asked first and
# made to after 5 s. Everything is found before anything is signalled, so none
# is handed to init and missed. A group is waited on as well as the pids, so
# anything forked into it as it was being stopped goes with the KILL.
stop() {
  local p pids="" groups="" left n=0
  for p in "$@"; do
    [ -n "$p" ] || continue
    # A group numbered as this pid is one it made, though it may have ended
    # since; never Desk Runner's or the guard's.
    ! kill -0 -- "-$p" 2>/dev/null || groups+=" -$p"
    pids+=" $(tree_of "$p")"
  done
  [ -n "$pids$groups" ] || return 0
  for p in $groups; do kill -TERM -- "$p" 2>/dev/null; done
  kill -TERM $pids 2>/dev/null
  while [ "$n" -lt 25 ]; do
    left=""
    for p in $pids; do kill -0 "$p" 2>/dev/null && left+=" $p"; done
    for p in $groups; do kill -0 -- "$p" 2>/dev/null && left+=" $p"; done
    [ -n "$left" ] || return 0
    sleep 0.2
    n=$((n + 1))
  done
  for p in $groups; do kill -KILL -- "$p" 2>/dev/null; done
  for p in $pids; do kill -KILL "$p" 2>/dev/null; done
}

perl -e 'setpgrp(0, 0); exec { $ARGV[0] } @ARGV or die "hangup-guard: $ARGV[0]: $!\n"' "$@" 3<&- &
child=$!
(
  # Nothing ever comes; this returns when the ssh client's gone. Nothing is
  # written to say so: stdout and stderr went with it, and a write would end
  # this subshell (SIGPIPE) before it stopped anything.
  while read -r -u 3 _; do :; done
  # From here it finishes, though the command may end under it and the guard
  # below try to stop this as it would a watcher still waiting.
  trap '' TERM
  desk=""
  if [ -n "$pidfile" ]; then
    # Left there afterwards, not cleaned up: a run Desk Runner starts late
    # must still find it (Codex's review). Its name is the run's own, and the
    # callers clear old ones.
    : >"$pidfile.gone"
    desk="$(cat "$pidfile" 2>/dev/null)"
    case "$desk" in ""|*[!0-9]*) desk="" ;; esac
  fi
  stop "$child" $desk
) &
watcher=$!
# The guard's own stderr goes too, the command keeping its own: bash tells
# stderr when a job it waits for was killed, and that write, on a channel
# that's gone, ended the guard (SIGPIPE) before it cleaned up.
exec 2>/dev/null
wait "$child"
rc=$?
# The command can end because the ssh went (the desk client, writing to it),
# before the watcher has seen that. So a look at the channel first, a second
# at most: ended, the watcher is left to stop the rest; still there, the
# command just finished and the watcher goes. perl, not read -t: bash 3.2's
# gives 1 for a timeout as well as for the end, and only times out on a
# terminal or a pipe, not ssh's socket.
gone=0
perl -e 'use IO::Select; exit(IO::Select->new(\*STDIN)->can_read(1) && !sysread(STDIN, my $b, 1) ? 1 : 0)' <&3 || gone=1
if [ "$gone" = 1 ]; then
  wait "$watcher" 2>/dev/null
  exit "$rc"
fi
kill "$watcher" 2>/dev/null
wait "$watcher" 2>/dev/null
[ -z "$pidfile" ] || rm -f "$pidfile" "$pidfile.gone"
exit "$rc"
