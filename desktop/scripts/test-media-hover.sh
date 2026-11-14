#!/bin/bash
# Run on a Mac check host, through desktop/scripts/remote.sh tinker-desk --desk.
# AppKit and Quick Look need that user's desktop session, but no window is shown.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-hover.XXXXXX")"
trap 'rm -rf "$work"' EXIT
sources=("$root/desktop/gpui/native/media_views.m" "$root/desktop/gpui/tests/native/media_hover.m")
frameworks=(-framework AppKit -framework Foundation -framework AVFoundation -framework AVKit -framework PDFKit -framework CoreMedia -framework CoreVideo -framework QuickLookThumbnailing)
xcrun clang -fobjc-arc -Wno-deprecated-declarations "${sources[@]}" "${frameworks[@]}" -o "$work/hover"
"$work/hover"
xcrun clang -fobjc-arc -Wno-deprecated-declarations -DTB_LATE_CALLBACK_TEST '-DTB_VIDEO_WAIT_NS=(5*NSEC_PER_MSEC)' "${sources[@]}" "${frameworks[@]}" -o "$work/late"
"$work/late"
xcrun clang -fobjc-arc -Wno-deprecated-declarations -DTB_LATE_QUICKLOOK_TEST '-DTB_QUICKLOOK_WAIT_NS=(1000*NSEC_PER_MSEC)' "${sources[@]}" "${frameworks[@]}" -o "$work/late-quicklook"
"$work/late-quicklook"

# The pane deliberately has no hover deadline, including during package copies.
xcrun clang -fobjc-arc -Wno-deprecated-declarations '-DTB_QUICKLOOK_WAIT_NS=(10*NSEC_PER_MSEC)' "$root/desktop/gpui/tests/native/thumbnail_job.m" "${frameworks[@]}" -o "$work/jobs"
"$work/jobs"
