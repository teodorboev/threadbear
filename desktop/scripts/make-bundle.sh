#!/bin/bash
# Puts the GPUI app's two binaries into a signed .app bundle, with the
# build.json that its crash reports and its self-update go by (the Swift
# app's build-app.sh writes the same file). install-app.sh uses it, so an
# install by hand and an update come out the same.
#
#   desktop/scripts/make-bundle.sh <binaries dir> <bundle> [<checkout to follow>]
#
# <binaries dir> has threadbear-gpui and threadbear-web-helper, built from this
# script's own tree: its commit and its desktop/ tree go in build.json.
# <checkout to follow> is the repo whose main the installed app follows
# (default: this script's own), and whose origin says whether it's Mike's
# (only a build of his repo reports to his Sentry, never someone's fork).
# Nothing outside <bundle> is touched.
set -euo pipefail
bins="${1:?usage: make-bundle.sh <binaries dir> <bundle> [<checkout to follow>]}"
bundle="${2:?usage: make-bundle.sh <binaries dir> <bundle> [<checkout to follow>]}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
checkout="${3:-$root}"
bundle_id="com.mikerosoft.threadbear.desktop"
# Started from the app, PATH is the bare system one; git, codesign, sips and
# the rest are all under /usr/bin.
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

[ -x "$bins/threadbear-gpui" ] && [ -x "$bins/threadbear-web-helper" ] || { echo "No threadbear-gpui and threadbear-web-helper in $bins." >&2; exit 2; }

# Every caller says where the binaries were built (THREADBEAR_BUILT_ON: a host,
# or "" for this Mac) except an installed app's older stage-update.sh, which
# runs from that app's bundle but takes this script from main, and built on
# the first of runner.json's checkHosts: a Mac trusted only to run tests
# (Daybreak 108). Its binaries go in only if they're the build it made here
# (stage-update.sh's cache keeps that beside them, in target/release); ones
# from a check host are refused, and the app stays as it is until it's
# installed again (install-app.sh), which says where it builds.
if [ -z "${THREADBEAR_BUILT_ON+set}" ]; then
  built_here="$(dirname "$bins")/target/release"
  for bin in threadbear-gpui threadbear-web-helper; do
    cmp -s "$bins/$bin" "$built_here/$bin" || {
      echo "Not making a bundle of binaries that weren't built on this Mac by an updater that doesn't say where it built them: check hosts aren't trusted with what this Mac runs. Install the app again (desktop/scripts/install-app.sh) to get the updater that asks for updateBuildHosts." >&2
      exit 6
    }
  done
fi

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$bins/threadbear-gpui" "$bundle/Contents/MacOS/threadbear-gpui"
# Beside the app, where it looks for it: the web runtime's helper processes
# are copies of it, made in the data folder when Chromium first starts
# (desktop/gpui/src/web_runtime.rs). Signed first, so the bundle's own
# signature takes it in as signed code.
cp "$bins/threadbear-web-helper" "$bundle/Contents/MacOS/threadbear-web-helper"
codesign --force --sign - "$bundle/Contents/MacOS/threadbear-web-helper" 2>/dev/null
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>threadbear-gpui</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleName</key><string>Threadbear GPUI</string>
    <key>CFBundleDisplayName</key><string>Threadbear GPUI</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <!-- threadbear://thread/<id> opens a thread (desktop/gpui/src/deep_links.rs),
         as the Swift app's app/Resources/Info.plist claims it too. -->
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>com.mikerosoft.threadbear.desktop.thread-link</string>
            <key>CFBundleURLSchemes</key>
            <array><string>threadbear</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST
# The Swift app's bear, green, half machine, with GPUI across its front, so
# the two apps can be told apart in the Dock and the app switcher while both
# are installed (Mike picked it, 12 Nov). Drawn from app/icons/threadbear.png.
icon="$root/desktop/gpui/icons/threadbear-gpui.png"
iconset="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-icon-XXXX")/threadbear.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$icon" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$icon" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/threadbear.icns"
rm -rf "$(dirname "$iconset")"
/usr/libexec/PlistBuddy -c 'Add :CFBundleIconFile string threadbear' "$bundle/Contents/Info.plist"

# What this bundle was built from: the commit, desktop/'s tree at it (only a
# change under desktop/ changes what runs, so the updater compares that), the
# checkout whose main to follow, and that checkout's origin.
commit="$(git -C "$root" rev-parse HEAD 2>/dev/null || true)"
short="$(git -C "$root" rev-parse --short HEAD 2>/dev/null || true)"
tree="$(git -C "$root" rev-parse HEAD:desktop 2>/dev/null || true)"
origin="$(git -C "$checkout" remote get-url origin 2>/dev/null || true)"
# builtOn: the host the binaries came from ("" for this Mac), when the caller
# says (stage-update.sh does), so the log can tell which Mac made what runs.
/usr/bin/python3 -I - "$bundle/Contents/Resources/build.json" "$commit" "$short" "$tree" "$checkout" "$origin" "${THREADBEAR_BUILT_ON-}" "${THREADBEAR_BUILT_ON+set}" <<'PY'
import datetime, json, sys
path, commit, short, tree, checkout, origin, built_on, said = sys.argv[1:]
info = {
    "commit": commit, "short": short, "appTree": tree, "checkout": checkout,
    "configuration": "release", "origin": origin,
    "builtAt": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
}
if said:
    info["builtOn"] = built_on
with open(path, "w") as f:
    json.dump(info, f, indent=2)
PY

# The update script, so the app runs the copy it was built with
# (desktop/gpui/src/app_update.rs).
cp "$root/desktop/scripts/stage-update.sh" "$bundle/Contents/Resources/stage-update.sh"
chmod +x "$bundle/Contents/Resources/stage-update.sh"

# Signed with the first Apple Development identity, as the Swift app is, so
# the app keeps the same identity from one build to the next and macOS's
# privacy permissions stick. Ad hoc otherwise, with the designated
# requirement pinned to the bundle id, as an ad hoc one is the binary's hash
# and changes on every build.
identity="${THREADBEAR_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | awk '/"Apple Development:/ { print $2; exit }' || true)}"
requirements=()
if [ -z "$identity" ]; then
  identity="-"
  requirements=(--requirements "=designated => identifier \"$bundle_id\"")
fi
codesign --force --timestamp=none --sign "$identity" ${requirements[@]+"${requirements[@]}"} "$bundle" >/dev/null 2>&1
echo "Made $bundle ($short, signed: $identity)"
