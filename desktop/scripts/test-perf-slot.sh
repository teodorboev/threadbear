#!/bin/bash
# Checks with real cargo that a --perf build through shared-cargo's perf-slot
# rebuilds exactly what differs between checkouts, and never runs another
# checkout's code. Run it by hand after changing how shared-cargo copies:
#
#   desktop/scripts/test-perf-slot.sh <ssh host>
#
# Two throwaway checkouts (two keys on the host), each a tiny workspace: an
# app crate that include_str!s a file outside desktop/, and a vendored path
# crate standing in for the GPUI kits. Their runs go through remote.sh like
# any other, with a perf build key set by hand rather than --perf, and their
# own slot base and target: the host's real perf-target and its owner marker
# are never touched. 9c00a5ed asked for a real cargo here (14 Nov), as a fake
# that decides freshness by times alone can miss what cargo really tracks.
set -euo pipefail
host="${1:?usage: test-perf-slot.sh <ssh host>}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/perf-slot.XXXXXX")"
base_rel=".cache/threadbear-check/gpui"
test_base="$base_rel/perf-slot-test"
keys=()
cleanup() {
  rm -rf "$tmp"
  local k rm_list="$test_base"
  for k in ${keys[@]+"${keys[@]}"}; do rm_list="$rm_list $base_rel/src-$k $base_rel/target-$k"; done
  ssh "$host" "rm -rf $rm_list" || true
}
trap cleanup EXIT

checkout() {
  local w="$tmp/$1"
  mkdir -p "$w/desktop/scripts" "$w/scripts" "$w/runner/scripts" "$w/desktop/app/src" "$w/desktop/vendor/kit/src"
  cp "$repo/desktop/scripts/remote.sh" "$repo/desktop/scripts/trim-remote-cache.sh" "$w/desktop/scripts/"
  cp -R "$repo/desktop/scripts/shared-cargo" "$w/desktop/scripts/"
  cp "$repo/scripts/"{remote-guard-lib.sh,hangup-guard.sh,perf-lock.pl,perf-children.pl,build-priority.sh,perf-build.sh} "$w/scripts/"
  cp "$repo/runner/scripts/find-node.sh" "$w/runner/scripts/"
  cat >"$w/desktop/Cargo.toml" <<'EOF'
[workspace]
resolver = "2"
members = ["app"]
exclude = ["vendor"]
EOF
  cat >"$w/desktop/app/Cargo.toml" <<'EOF'
[package]
name = "app"
version = "0.1.0"
edition = "2021"
[dependencies]
kit = { path = "../vendor/kit" }
EOF
  cat >"$w/desktop/vendor/kit/Cargo.toml" <<'EOF'
[package]
name = "kit"
version = "0.1.0"
edition = "2021"
EOF
  printf 'pub const KIT: &str = "K";\n' >"$w/desktop/vendor/kit/src/lib.rs"
  printf 'fn main() { println!("{} {} {}", include_str!("../../../ext.txt").trim(), VALUE, kit::KIT); }\nconst VALUE: u32 = 1;\n' >"$w/desktop/app/src/main.rs"
  printf 'a\n' >"$w/ext.txt"
  # Old times everywhere, as a copy made earlier would have.
  find "$w" -type f -exec touch -t 202001010000 {} +
  keys+=("$(printf '%s' "$(cd "$w" && pwd)" | shasum | cut -c1-10)")
}
checkout one
checkout two
# Builds and runs the app from a checkout, through the perf slot. Prints the
# app's line, then which crates compiled.
run() {
  "$tmp/$1/desktop/scripts/remote.sh" "$host" -- bash -c "
    export THREADBEAR_SHARED_CARGO_BASE=\"\$HOME/$test_base\" CARGO_TARGET_DIR=\"\$HOME/$test_base/perf-target\"
    export THREADBEAR_PERF_BUILD_KEY=$1 THREADBEAR_PERF_BUILD_SOURCE=\"\$PWD\"
    cd desktop
    out=\"\$(cargo build 2>&1)\" || { echo BUILD-FAILED; exit 0; }
    \"\$CARGO_TARGET_DIR/debug/app\"
    printf '%s\n' \"\$out\" | sed -n 's/^ *Compiling \([a-z]*\) .*/compiled \1/p'
  " 2>/dev/null
}
fail() { echo "FAIL: $*"; exit 1; }
expect() { # <what> <output> <first line> <compiled crates, space separated>
  local got first compiled
  got="$2"
  first="$(head -1 <<<"$got")"
  compiled="$(sed -n 's/^compiled //p' <<<"$got" | sort | tr '\n' ' ' | sed 's/ $//')"
  [ "$first" = "$3" ] || fail "$1: ran '$first', wanted '$3'"
  [ "$compiled" = "$4" ] || fail "$1: compiled '$compiled', wanted '$4'"
  echo "ok: $1"
}

expect "first build" "$(run one)" "a 1 K" "app kit"
expect "another checkout with the same files builds nothing" "$(run two)" "a 1 K" ""

# A change outside desktop/, its file as old as the rest.
printf 'b\n' >"$tmp/two/ext.txt"; touch -t 202001010000 "$tmp/two/ext.txt"
expect "an old-timed change outside desktop/ rebuilds the app, not the kit" "$(run two)" "b 1 K" "app"

# A broken build in between, then the first checkout again.
printf 'fn main() { broken }\n' >"$tmp/two/desktop/app/src/main.rs"; touch -t 202001010000 "$tmp/two/desktop/app/src/main.rs"
[ "$(run two | head -1)" = BUILD-FAILED ] || fail "the broken checkout built"
expect "the first checkout after a failed build runs its own code" "$(run one)" "a 1 K" "app"

# A build that went round the shim (cargo by its full path, straight from the
# checkout's copy into the same target), then one through it.
key_one="${keys[0]}"
ssh "$host" "cd $base_rel/src-$key_one/desktop && sed -i.bak 's/VALUE: u32 = 1/VALUE: u32 = 9/' app/src/main.rs && sed -i.bak 's/\"K\"/\"Z\"/' vendor/kit/src/lib.rs && rm -f app/src/main.rs.bak vendor/kit/src/lib.rs.bak && CARGO=\$(PATH=\"\$HOME/$base_rel/cargo/bin:\$HOME/.cargo/bin:\$PATH\" command -v cargo) && CARGO_HOME=\$( [ -d \"\$HOME/$base_rel/cargo\" ] && echo \"\$HOME/$base_rel/cargo\" || echo \"\$HOME/.cargo\") RUSTUP_HOME=\$( [ -d \"\$HOME/$base_rel/rustup\" ] && echo \"\$HOME/$base_rel/rustup\" || echo \"\$HOME/.rustup\") CARGO_TARGET_DIR=\"\$HOME/$test_base/perf-target\" \"\$CARGO\" build -q && \"\$HOME/$test_base/perf-target/debug/app\"" 2>/dev/null | tail -1 >"$tmp/bypass" \
  || fail "the build round the shim failed"
# It really did put its code in the target (a legacy --perf run, or cargo by
# its full path, would): a change to the app and to the vendored kit.
[ "$(cat "$tmp/bypass")" = "a 9 Z" ] || fail "the build round the shim ran '$(cat "$tmp/bypass")', wanted 'a 9 Z'"
got="$(run one)"
[ "$(head -1 <<<"$got")" = "a 1 K" ] || fail "after a build round the shim, ran '$(head -1 <<<"$got")', wanted 'a 1 K'"
echo "  (compiled: $(sed -n 's/^compiled //p' <<<"$got" | tr '\n' ' '); cargo keeps a crate built from another folder apart, so nothing here was reused)"
echo "ok: a build round the shim doesn't leave its code in the next one through it"
echo "all ok"
