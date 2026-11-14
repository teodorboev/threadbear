#!/bin/bash
# The GPUI app for an agent to look at and drive, as app/Tools/dev-app.sh is
# for the frozen Swift one: a debug build with the scripted control, against
# the mock runner or a dev runner, its window kept behind whatever Mike is
# working in. `desktop/scripts/dev-app.sh --help` says the rest; the code is
# dev-app.ts beside this.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# THREADBEAR_NODE, else the node on PATH, else nvm's newest, else Homebrew's.
. "$here/../../runner/scripts/find-node.sh"
node="$(threadbear_find_node)"
exec "$node" "$here/dev-app.ts" "$@"
