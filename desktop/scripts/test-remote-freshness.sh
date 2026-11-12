#!/bin/bash
# Checks that remote.sh's copy never leaves cargo on a host thinking a changed
# file is unchanged. Run it by hand after changing how remote.sh copies:
#
#   desktop/scripts/test-remote-freshness.sh <ssh host>
#
# It copies remote.sh into a throwaway checkout holding one tiny crate, so it
# builds in seconds and its folders on the host are its own (and go after a few
# days, like any checkout's). Then it plays what happened on 11 Nov (thread
# 699f31d2): a file's changed here, a build on the host compiles the old copy
# after that change was made (another run still going), and the next run syncs
# the change. And a target left stale by copies made the old way. When
# rsync kept this Mac's times, the synced file was older than that build, so
# cargo called the crate fresh and ran the old code: a green run of what you'd
# changed nothing in.
set -euo pipefail
host="${1:?usage: test-remote-freshness.sh <ssh host>}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/remote-freshness.XXXXXX")"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/desktop/scripts" "$work/runner/scripts" "$work/desktop/src"
cp "$repo/desktop/scripts/remote.sh" "$repo/desktop/scripts/trim-remote-cache.sh" "$work/desktop/scripts/"
cp "$repo/runner/scripts/find-node.sh" "$work/runner/scripts/"
cat >"$work/desktop/Cargo.toml" <<'EOF'
[package]
name = "freshness"
version = "0.1.0"
edition = "2021"
EOF
answer() { printf 'fn main() { println!("answer {}", %s); }\n' "$1" >"$work/desktop/src/main.rs"; }
run() { "$work/desktop/scripts/remote.sh" "$host" -- bash -c "cd desktop && $1"; }
# The copy's folder on the host, as remote.sh names it.
key="$(printf '%s' "$(cd "$work" && pwd)" | shasum | cut -c1-10)"
remote_src=".cache/threadbear-check/gpui/src-$key"
trap 'rm -rf "$work"; ssh "$host" "rm -rf .cache/threadbear-check/gpui/src-$key .cache/threadbear-check/gpui/target-$key"' EXIT

answer 1
run 'cargo run -q' | grep -qx 'answer 1' || { echo "FAIL: the first run didn't build"; exit 1; }

answer 2
sleep 2
# Runs a command in the copy on the host, with its cargo and target.
on_host() { ssh "$host" "cd $remote_src/desktop && export PATH=\"\$HOME/.cache/threadbear-check/gpui/cargo/bin:\$HOME/.cargo/bin:\$PATH\" CARGO_TARGET_DIR=\"\$HOME/.cache/threadbear-check/gpui/target-$key\" && $1"; }
# A build on the host after the change was made here, of the copy it still has.
on_host 'touch src/main.rs && cargo build -q'

got="$(run 'cargo run -q')"
# And a run with nothing changed shouldn't rebuild anything.
again="$(run 'cargo build 2>&1')"

# A copy made before remote.sh copied by content: the file there already
# matches this Mac's but its build is of something else, with a later time.
# Copying by content changes nothing there, so remote.sh rebuilds such a
# target's crates once (.synced-by-content).
on_host 'cp -p src/main.rs /tmp/freshness-$$ && sed -i.x s/2/9/ src/main.rs && cargo build -q && cp -p /tmp/freshness-$$ src/main.rs && rm -f /tmp/freshness-$$ src/main.rs.x && rm -f "$CARGO_TARGET_DIR/.synced-by-content"'
old="$(run 'cargo run -q')"

if [ "$got" != "answer 2" ]; then
  echo "FAIL: after the change, the host ran the old build ($got)"
  exit 1
fi
if grep -q Compiling <<<"$again"; then
  echo "FAIL: a run with nothing changed rebuilt:"; echo "$again"
  exit 1
fi
if [ "$old" != "answer 2" ]; then
  echo "FAIL: a target from before copying by content kept its stale build ($old)"
  exit 1
fi
echo "ok: the host built what was synced, nothing when nothing changed, and a stale target from before once"
