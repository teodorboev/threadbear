# Sourced by the scripts that run commands on check hosts over ssh
# (desktop/scripts/remote.sh, scripts/check-remote.sh), so a command there
# stops when the ssh that started it does (scripts/hangup-guard.sh says why).

# guarded_cmd <pidfile|-> <command...>: an ssh command line that runs the
# command under hangup-guard.sh. The guard goes inline, so it needn't be on the
# host, less its comments, which would otherwise fill every ps and pgrep -f
# line there. Paths are best relative: ssh and Desk Runner both start in $HOME.
# Single quotes rather than printf %q, which writes a command's spaces as
# backslashes: the command stays as it was written in ps there, and in the
# tests' fake ssh, which reads it.
guarded_cmd() {
  local here a out
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  out="/bin/bash -c $(single_quoted "$(sed -e '/^[[:space:]]*#/d' -e '/^[[:space:]]*$/d' "$here/hangup-guard.sh")") hangup-guard"
  for a in "$@"; do out+=" $(single_quoted "$a")"; done
  printf '%s' "$out"
}
single_quoted() {
  local q="'\\''"
  printf "'%s'" "${1//\'/$q}"
}

# alive_stdin <fd>: opens <fd> on a pipe nothing writes to and nothing closes
# while this shell or the ssh given it lives, to be that ssh's stdin. The guard
# reads its end of the channel and takes it ending as the ssh going. Not
# /dev/null, which ends at once, nor this script's own stdin, which a
# background job hasn't got. A fifo opened read and write doesn't wait for a
# writer, and is gone from the disk as soon as it's open.
alive_stdin() {
  local dir
  dir="$(mktemp -d "${TMPDIR:-/tmp}/tb-alive.XXXXXX")" || return 1
  mkfifo "$dir/alive" || { rm -rf "$dir"; return 1; }
  eval "exec $1<>\"\$dir/alive\""
  rm -rf "$dir"
}
