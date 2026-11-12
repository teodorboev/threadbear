#!/bin/bash
# The Swift Mac app (app/, and BearDock in dock/) is frozen: Mike uses the
# GPUI app (desktop/) every day since 12 Nov 2026, and the Swift one is
# retiring in its favour (docs/gpui-port.md, "Rollout"). This keeps new code
# out of it, two ways:
#
#   swift-freeze.sh --commit   git's pre-commit hook (worktree-setup.sh puts
#                              it in): looks at what's staged
#   swift-freeze.sh --tool     Claude Code's PreToolUse hook on Edit and
#                              Write (.claude/settings.json): the tool call
#                              on stdin, so an agent hears it before it
#                              writes the code, not after
#   swift-freeze.sh --gpui-shared
#                              prints which of app/ the GPUI app uses, as a
#                              regex, for check.sh
#
# In Mike's own copy (origin is mikecann/threadbear) it refuses. Only when
# Mike asks for a change to the Swift app itself, in that thread (the last
# Swift build that moves people across at the cutover, say), does an agent
# let it through, for that worktree only:
#   touch "$(git rev-parse --absolute-git-dir)/threadbear-swift-ok"
# (a file, as Claude Code's hooks don't see an agent's environment;
# THREADBEAR_SWIFT_OK=1 on the commit works too). Deleting files is always
# fine.
#
# In anyone else's copy it only warns, and only once WARN_FORKS is on: they
# can't get the GPUI app yet, so steering their agents to it now would send
# them somewhere they can't follow. Turn it on with the release that gives
# them the GPUI app, and the overlap where they have both begins.
WARN_FORKS=0

set -uo pipefail
# No globbing anywhere: inside() splits paths on /, and a part like a[p]p
# would otherwise match app (Codex's review, 12 Nov).
set -f

# What in app/ the GPUI app still uses, so it isn't frozen: the mock runner
# its CI and the features/ specs run against, the Swift test tools its
# pointer, drag and menu specs compile (swiftTool in
# features/tools/real-screen.ts), the golden transcripts and fixtures its
# tests share, and the art and rich-block scripts it builds in (check.sh's
# DESKTOP_NEEDS). check.sh reads this too (--gpui-shared), so a change to
# only these doesn't build and test the Swift app. They move out of app/
# before it goes at the cutover.
GPUI_SHARED='^app/Tools/mock-runner/|^app/Tools/(droptarget|post-gesture|realdrag)\.swift$|^app/Tests/Fixtures/|^app/Sources/Threadbear/Resources/(Art|Rich)/'

mode="${1:-}"
if [ "$mode" = --gpui-shared ]; then
  printf '%s\n' "$GPUI_SHARED"
  exit 0
fi
# The checkout: for a commit, git's own, never CLAUDE_PROJECT_DIR, which can
# name another folder when an agent commits in a worktree it cd'd into.
if [ "$mode" = --tool ] && [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then
  top="$CLAUDE_PROJECT_DIR"
else
  top="$(git rev-parse --show-toplevel 2>/dev/null)"
fi
[ -n "$top" ] || exit 0

# And what the iPhone app (which stays Swift) builds from app/: the files
# ThreadbearKit links to (SlashCommands.swift and the like, found from its
# links as check.sh's IOS_NEEDS does) and Sentry's framework.
IPHONE_SHARED=(app/SentryBinary/)
top_real="$(cd "$top" && pwd -P)"
# macOS's other name for the same disk, which pwd -P keeps: both sides drop
# it, so /Users/... and /System/Volumes/Data/Users/... compare as one.
undata() {
  case "$1" in
    /System/Volumes/Data/?*)
      # Only when the top folder it names is there without the prefix too
      # (/Users), as the rest of the path may not exist yet.
      local r="${1#/System/Volumes/Data}" first
      first="${r#/}"; first="/${first%%/*}"
      [ -d "$first" ] && { printf '%s\n' "$r"; return; } ;;
  esac
  printf '%s\n' "$1"
}
top_real="$(undata "$top_real")"
while IFS= read -r link; do
  to="$(readlink "$link")"
  dir="$(cd "$(dirname "$link")" && cd "$(dirname "$to")" 2>/dev/null && pwd -P)" || continue
  case "$dir" in "$top_real"/app/*) IPHONE_SHARED+=("${dir#"$top_real"/}/$(basename "$to")") ;; esac
done < <(find "$top_real/ios" -type l -not -path '*/.build/*' 2>/dev/null)

shared() {
  [[ "$1" =~ $GPUI_SHARED ]] && return 0
  local s
  for s in "${IPHONE_SHARED[@]}"; do
    case "$s" in
      */) [[ "$1" == "$s"* ]] && return 0 ;;
      *) [ "$1" = "$s" ] && return 0 ;;
    esac
  done
  return 1
}
# inside <path>: the path from the checkout's top, with links, . and ..
# resolved as far as the folders exist (a file being written may not yet),
# so ./app/x, desktop/../app/x and /System/Volumes/Data/... all count as
# app/x; fails for a path outside the checkout.
inside() {
  local p="$1" rest=""
  case "$p" in /*) ;; *) p="$top/$p" ;; esac
  while [ ! -d "$p" ]; do
    rest="/$(basename "$p")$rest"
    p="$(dirname "$p")"
  done
  p="$(cd "$p" && pwd -P)$rest"
  p="$(undata "$p")"
  # The part below the last real folder can still say . or ..
  local out=() part
  local IFS=/
  for part in $p; do
    case "$part" in
      ''|.) ;;
      ..) [ "${#out[@]}" -gt 0 ] && unset "out[$((${#out[@]} - 1))]" ;;
      *) out+=("$part") ;;
    esac
  done
  p="/${out[*]}"
  case "$p" in "$top_real"/*) printf '%s\n' "${p#"$top_real"/}" ;; *) return 1 ;; esac
}
frozen() {
  case "$1" in
    app/*|dock/*) shared "$1" && return 1; return 0 ;;
  esac
  return 1
}

# Mike's copy, or someone else's. THREADBEAR_SWIFT_FREEZE_COPY is for the
# tests (runner/test/unit/swift-freeze.test.ts).
copy="${THREADBEAR_SWIFT_FREEZE_COPY:-}"
if [ -z "$copy" ]; then
  if git -C "$top" remote get-url origin 2>/dev/null | grep -qE '[:/]mikecann/threadbear(\.git)?/?$'; then
    copy=mike
  else
    copy=fork
  fi
fi
[ "$copy" = mike ] || [ "$WARN_FORKS" = 1 ] || exit 0
[ "${THREADBEAR_SWIFT_OK:-}" = 1 ] && exit 0
gitdir="$(git -C "$top" rev-parse --absolute-git-dir 2>/dev/null)"
[ -n "$gitdir" ] && [ -e "$gitdir/threadbear-swift-ok" ] && exit 0

hits=()
case "$mode" in
  --commit)
    # Finishing a merge, cherry-pick or revert brings in commits that are
    # already made (a fork bringing in Mike's releases, mostly), so it isn't
    # new code from here.
    for h in MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD; do
      git rev-parse -q --verify "$h" >/dev/null 2>&1 && exit 0
    done
    # Added, copied, modified, renamed or changed type (a file swapped for a
    # link): deletions go through. -z so a name git would quote (an accent,
    # a quote mark) comes through as it is.
    while IFS= read -r -d '' f; do
      frozen "$f" && hits+=("$f")
    done < <(git -C "$top" diff --cached --name-only -z --diff-filter=ACMRT)
    ;;
  --tool)
    input="$(cat)"
    # file_path for Edit and Write, notebook_path for NotebookEdit. sed rather
    # than a JSON parser, so the hook needs nothing installed.
    f="$(printf '%s' "$input" | sed -nE 's/.*"(file_path|notebook_path)"[[:space:]]*:[[:space:]]*"([^"]*)".*/\2/p' | head -1)"
    [ -n "$f" ] || exit 0
    f="$(inside "$f")" || exit 0  # outside this checkout
    frozen "$f" && hits+=("$f")
    ;;
  *)
    echo "usage: swift-freeze.sh --commit | --tool | --gpui-shared" >&2
    exit 64
    ;;
esac
[ "${#hits[@]}" -gt 0 ] || exit 0

list="$(printf '  %s\n' "${hits[@]}")"
if [ "$copy" = mike ]; then
  cat >&2 <<EOF
The Swift Mac app (app/, dock/) is frozen: Mike uses the GPUI app now, and the
Swift one is retiring. Make this change in the GPUI app in desktop/ instead
(desktop/README.md, docs/gpui-port.md), with a spec in features/ if it changes
what the app does:
$list
Only if Mike asked in this thread for a change to the Swift app itself:
  touch "\$(git rev-parse --absolute-git-dir)/threadbear-swift-ok"
lets edits and commits to it through in this worktree.
EOF
  # git stops on any nonzero exit; Claude Code needs 2 to refuse the tool call
  # and pass this text back to the agent.
  exit 2
fi

msg="Heads up: app/ and dock/ are the Swift Mac app, which is being retired in favour of the GPUI app in desktop/ (Mac, Windows and Linux). A change here won't carry over when the Swift app goes, so unless the user wants it in the Swift app specifically, make it in desktop/ instead and tell them why. Files:
$list"
if [ "$mode" = --tool ]; then
  # A warning, not a refusal: the edit goes ahead, and the agent reads this.
  node -e 'process.stdout.write(JSON.stringify({hookSpecificOutput:{hookEventName:"PreToolUse",additionalContext:process.argv[1]}}))' "$msg" 2>/dev/null \
    || printf '%s\n' "$msg" >&2
else
  printf '%s\n' "$msg" >&2
fi
exit 0
