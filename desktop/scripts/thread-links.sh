#!/bin/bash
# Which app opens threadbear://thread/<id> links (BrowserBear's Open thread pill
# on an agent's tab runs `open` on one) while the Swift app and the GPUI app are
# both installed. Both claim the scheme in their Info.plist, and with no default
# set macOS picks one by itself, which can change as either is rebuilt. So
# install-app.sh makes the GPUI app the default, as it's the one being moved to,
# and this gives the links back to the Swift app when you want them there.
#
#   desktop/scripts/thread-links.sh          # which app has them now
#   desktop/scripts/thread-links.sh gpui     # ~/Applications/Threadbear GPUI.app
#   desktop/scripts/thread-links.sh swift    # ~/Applications/Threadbear.app
#
# It only changes which app macOS starts for these links (LaunchServices'
# default for the scheme), nothing in either app.
set -euo pipefail

[ "$(uname -s)" = Darwin ] || { echo "Only for a Mac." >&2; exit 2; }
case "${1:-}" in
  "") app="" ;;
  gpui) app="$HOME/Applications/Threadbear GPUI.app" ;;
  swift) app="$HOME/Applications/Threadbear.app" ;;
  *) echo "usage: thread-links.sh [gpui|swift]" >&2; exit 2 ;;
esac
command -v swift >/dev/null || { echo "This needs swift (Xcode's command line tools) to talk to LaunchServices." >&2; exit 1; }

work="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-links-XXXX")"
trap 'rm -rf "$work"' EXIT
cat > "$work/links.swift" <<'SWIFT'
import AppKit

let probe = URL(string: "threadbear://thread/x")!
func say() {
    let now = NSWorkspace.shared.urlForApplication(toOpen: probe)
    print("threadbear:// links open \(now?.path ?? "nothing")")
}
guard CommandLine.arguments.count > 1 else { say(); exit(0) }
let app = URL(fileURLWithPath: CommandLine.arguments[1])
guard FileManager.default.fileExists(atPath: app.path) else {
    FileHandle.standardError.write("\(app.path) isn't installed.\n".data(using: .utf8)!)
    exit(1)
}
// Asked of LaunchServices, which answers on its own queue.
var failure: Error?
let done = DispatchSemaphore(value: 0)
NSWorkspace.shared.setDefaultApplication(at: app, toOpenURLsWithScheme: "threadbear") { error in
    failure = error
    done.signal()
}
done.wait()
if let failure {
    FileHandle.standardError.write("macOS didn't take it: \(failure.localizedDescription)\n".data(using: .utf8)!)
    exit(1)
}
say()
SWIFT

if [ -n "$app" ]; then
  # So LaunchServices has read the app's Info.plist (its URL types) before
  # it's made the default.
  lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  [ -d "$app" ] && [ -x "$lsregister" ] && "$lsregister" -f "$app" || true
  swift "$work/links.swift" "$app"
else
  swift "$work/links.swift"
fi
