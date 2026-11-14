#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/tb-native-text.XXXXXX")"
trap 'rm -rf "$work"' EXIT
clang -fobjc-arc -fobjc-arc-exceptions -Wno-deprecated-declarations -framework AppKit "$root/gpui/tests/native_text.m" -o "$work/test"
"$work/test"
