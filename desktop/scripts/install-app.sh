#!/bin/bash
# Installs the GPUI app as ~/Applications/Threadbear GPUI.app, beside the Swift
# app, talking to the same runner. Mac only for now. It's an experiment
# (runner/src/experiments/gpuiApp.md): this refuses unless runner.json says
# "experiments": { "gpuiApp": true }.
#
#   desktop/scripts/install-app.sh [--build-on <ssh host>] [--keep-links]
#
# With --build-on it builds there (desktop/scripts/remote.sh) and copies the
# result here, for a Mac that shouldn't do a 10-minute Rust build. Otherwise it
# builds on this machine, at low priority.
#
# threadbear://thread/<id> links (BrowserBear's Open thread pill) open this app
# afterwards: both apps claim the scheme, so it's made the default for it
# (desktop/scripts/thread-links.sh, which gives them back to the Swift app).
# --keep-links leaves them with whichever app has them.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
build_on=""
links=gpui
while [ $# -gt 0 ]; do
  case "$1" in
    --build-on) build_on="${2:?--build-on needs an ssh host}"; shift 2 ;;
    --keep-links) links=""; shift ;;
    *) echo "usage: install-app.sh [--build-on <ssh host>] [--keep-links]" >&2; exit 2 ;;
  esac
done

[ "$(uname -s)" = Darwin ] || { echo "Only for a Mac so far: on Linux, run target/release/threadbear-gpui." >&2; exit 2; }

config="${THREADBEAR_HOME:-$HOME/Library/Application Support/Threadbear}/runner.json"
if ! /usr/bin/python3 -I -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1])).get("experiments",{}).get("gpuiApp") else 1)' "$config" 2>/dev/null; then
  echo "The GPUI app is an experiment: add \"experiments\": { \"gpuiApp\": true } to $config first." >&2
  exit 1
fi

# View > Show Frame Rate (desktop/gpui/src/frame_stats.rs), for seeing how
# smoothly it draws on this Mac.
features=frame-stats

work="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-gpui-XXXX")"
trap 'rm -rf "$work"' EXIT

if [ -n "$build_on" ]; then
  # This Mac's own folders on the host (remote.sh), as the checkout's path
  # can be the same on two Macs that share one.
  THREADBEAR_REMOTE_KEY="$(ioreg -rd1 -c IOPlatformExpertDevice 2>/dev/null | awk -F'"' '/IOPlatformUUID/ { print $4; exit }')"
  export THREADBEAR_REMOTE_KEY="${THREADBEAR_REMOTE_KEY:-$(hostname)}"
  # pipefail, so a build that fails stops here rather than passing on tail's
  # success and copying the last good build's binaries under this commit's
  # name; and the binaries come from where the build says it built
  # (remote.sh sets CARGO_TARGET_DIR), not a key worked out here (Codex's
  # reviews, 12 Nov).
  "$root/desktop/scripts/remote.sh" "$build_on" -- bash -c "set -o pipefail; cd desktop && nice cargo build --release -p threadbear-gpui --features $features 2>&1 | tail -2 && echo \"BUILT_IN=\$CARGO_TARGET_DIR\"" > "$work/remote.out" 2>&1 || { cat "$work/remote.out" >&2; exit 1; }
  cat "$work/remote.out"
  built_in="$(sed -n 's/^BUILT_IN=//p' "$work/remote.out" | tail -1)"
  [[ "$built_in" == /* ]] || { echo "The build on $build_on didn't say where it built." >&2; exit 1; }
  # The app and its web helper, which the same build makes (desktop/gpui/Cargo.toml).
  for bin in threadbear-gpui threadbear-web-helper; do
    scp -q "$build_on:$built_in/release/$bin" "$work/$bin"
  done
else
  export CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
  (cd "$root/desktop" && taskpolicy -b "$root/scripts/one-build.sh" cargo build --release -p threadbear-gpui --features "$features")
  cp "$root/desktop/target/release/threadbear-gpui" "$root/desktop/target/release/threadbear-web-helper" "$work/"
fi

bundle="$work/Threadbear GPUI.app"
# The bundle, its icon, signature and build.json (make-bundle.sh), from this
# checkout, which the installed app then follows.
"$root/desktop/scripts/make-bundle.sh" "$work" "$bundle" "$root"

mkdir -p "$HOME/Applications"
target="$HOME/Applications/Threadbear GPUI.app"
rm -rf "$target"
ditto "$bundle" "$target"
echo "Installed $target"
if [ -n "$links" ]; then
  # Not fatal: the app's in, and the links can be moved later.
  if "$root/desktop/scripts/thread-links.sh" gpui; then
    echo "desktop/scripts/thread-links.sh swift gives them back to the Swift app."
  else
    echo "threadbear:// links weren't moved to this app; desktop/scripts/thread-links.sh gpui tries again." >&2
  fi
fi
