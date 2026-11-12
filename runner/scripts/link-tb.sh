#!/bin/bash
# Puts `tb` in ~/.local/bin, next to the `threadbear` launcher, so the tb
# commands setup and the docs print (tb runner add, tb restart, tb status)
# run in a plain terminal. Setup only linked `threadbear`, so on Wayne's
# second Mac every one of them was "command not found" (issue #39).
#
#   link-tb.sh <data dir> <node> <checkout's runner dir> [--from-source]
#                                                            write it
#   link-tb.sh --remove                                      take it out
#
# What it writes is a tiny script, not a link: it runs the tb of the runner
# release this Mac runs (<data dir>/runner/current/bin/tb, so it always
# matches the runner), or the checkout's when there's no release yet; with
# --from-source (install-runner.sh's) always the checkout's, as the runner
# runs from there and an older release may still be lying about. Under the Node the runner is pinned to,
# since the Node on PATH may be too old. It never writes over a tb that
# isn't one of these (MARK says which are). install-runner.sh calls it for
# the real data dir only, never for a sandbox's.
#
# THREADBEAR_BIN_DIR replaces ~/.local/bin (the tests use it).
set -euo pipefail

BIN="${THREADBEAR_BIN_DIR:-$HOME/.local/bin}"
DEST="$BIN/tb"
MARK='# Written by Threadbear (runner/scripts/link-tb.sh).'

ours() { [ -f "$DEST" ] && [ ! -L "$DEST" ] && grep -qF "$MARK" "$DEST"; }

if [ "${1:-}" = --remove ]; then
  if ours; then rm -f "$DEST" && echo "Removed $DEST."; fi
  exit 0
fi

if [ $# -lt 3 ] || [ $# -gt 4 ] || { [ $# = 4 ] && [ "$4" != --from-source ]; }; then
  echo "Usage: link-tb.sh <data dir> <node> <runner dir> [--from-source] | --remove" >&2
  exit 2
fi
DATA_HOME="$1" NODE="$2" RUNNER_DIR="$3" FROM_SOURCE="${4:-}"

if { [ -e "$DEST" ] || [ -L "$DEST" ]; } && ! ours; then
  echo "$DEST is already there and isn't Threadbear's, so it's left alone. Threadbear's tb is $DATA_HOME/runner/current/bin/tb." >&2
  exit 0
fi

q() { printf '%q' "$1"; }
mkdir -p "$BIN"
tmp="$DEST.$$.new"
if [ -n "$FROM_SOURCE" ]; then
  pick="t=$(q "$RUNNER_DIR/bin/tb")"
else
  pick="t=$(q "$DATA_HOME/runner/current/bin/tb")
[ -f \"\$t\" ] || t=$(q "$RUNNER_DIR/bin/tb")"
fi
cat > "$tmp" <<EOF
#!/bin/bash
$MARK
# The tb of the runner this Mac runs, so it matches the runner.
$pick
exec $(q "$NODE") "\$t" "\$@"
EOF
chmod 755 "$tmp"
if ours; then
  # Something replacing our own tb in the moment between the check and the
  # mv would be overwritten; nothing short of a lock shared with every other
  # installer closes that, and it's our file a moment before.
  mv -f "$tmp" "$DEST"
# A hard link never replaces a file, so a tb something else put there since
# the check above stays.
elif ln "$tmp" "$DEST" 2>/dev/null; then
  rm -f "$tmp"
else
  rm -f "$tmp"
  echo "$DEST appeared while this was writing it, so it's left alone." >&2
  exit 0
fi
echo "tb: $DEST"
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) echo "Add $BIN to PATH in your shell profile to run tb by name." ;;
esac
