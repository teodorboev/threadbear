#!/bin/bash
# The Mac app's half of the parity run (scripts/parity/run.sh): a dev runner
# on the run's throwaway backend with its owner key, so the seeded threads
# are another runner's to it and reach the app through Convex (relay.ts), as
# a Hetzner or bruce thread does on Mike's Mac. The GPUI app (desktop/), the
# one Mike uses since 12 Nov, built with its automation socket, opens each
# scenario's thread on that runner, checks its rows show the words the
# scenario must show, and saves a picture (gpui-shots.ts). Prints JSON: each
# scenario's file and the words it didn't find. Until 12 Nov this was the
# frozen Swift app (app/Tools/dev-app.sh).
#
#   scripts/parity/mac-shots.sh <convex url> <owner key> <seed.json> <out dir>
set -euo pipefail
URL=$1 KEY=$2 SEED=$3 OUT=$4
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$OUT"
work=$(mktemp -d "${TMPDIR:-/tmp}/tb-parity-mac.XXXXXX")
home="$work/home"
mkdir -p "$home/secrets"
runner=""
cleanup() {
  [ -n "$runner" ] && kill "$runner" 2>/dev/null || true
  [ -n "$runner" ] && wait "$runner" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# The build, in remote.sh's folder on this host with the same Rust, in a
# target of its own ("parity") that trim-remote-cache.sh looks after with the
# others: it gives back room when the disk is short and refuses (exit 3)
# when there still isn't enough to build. A target that's been trimmed
# builds from nothing, which takes a while.
base="$HOME/.cache/threadbear-check/gpui"
mkdir -p "$base"
(cd "$base" && bash "$ROOT/desktop/scripts/trim-remote-cache.sh" parity) >&2 || { echo "not enough disk to build the GPUI app here" >&2; exit 1; }
if [ -x "$base/cargo/bin/cargo" ]; then
  export CARGO_HOME="$base/cargo" RUSTUP_HOME="$base/rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
elif [ -x "$HOME/.cargo/bin/cargo" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
export CARGO_TARGET_DIR="$base/target-parity" CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
# In use for the whole run, so the trim another run starts leaves it alone.
( while kill -0 $$ 2>/dev/null; do touch "$CARGO_TARGET_DIR/.in-use" 2>/dev/null || true; sleep 60; done ) >/dev/null 2>&1 </dev/null &
(cd "$ROOT/desktop" && cargo build -p threadbear-gpui --features automation) > "$work/build.log" 2>&1 \
  || { echo "the GPUI app didn't build:" >&2; tail -30 "$work/build.log" >&2; exit 1; }
cp "$CARGO_TARGET_DIR/debug/threadbear-gpui" "$work/threadbear-gpui"

# The main Mac's runner of a little fleet: the owner key reads other runners' threads.
printf '{ "sync": { "enabled": true, "url": "%s", "name": "Parity Mac" } }\n' "$URL" > "$home/runner.json"
printf '%s\n' "$KEY" > "$home/secrets/convex-key"
chmod 600 "$home/secrets/convex-key"
"$ROOT/runner/scripts/dev-runner.sh" --home "$home" > "$work/runner.log" 2>&1 &
runner=$!
for _ in $(seq 1 60); do [ -x "$home/tb" ] && break; sleep 1; done
[ -x "$home/tb" ] || { echo "the dev runner didn't start:" >&2; tail -20 "$work/runner.log" >&2; exit 1; }

TB_RUNNER_HOME="$home" TB_GPUI_BIN="$work/threadbear-gpui" node "$ROOT/scripts/parity/gpui-shots.ts" "$SEED" "$OUT"
