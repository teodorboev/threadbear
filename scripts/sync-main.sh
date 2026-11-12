#!/bin/bash
# Brings GitHub's main and this Mac's together, and pushes, with tb land's
# lock held the whole time (docs/thread-worktrees.md, "Syncing main with
# GitHub"). Main here moves by tb land; GitHub's by pushes and merged PRs.
#
#   scripts/sync-main.sh     from any checkout of this repo
#
# - Only GitHub has moved: main fast-forwards to it.
# - Only this Mac has: main is pushed.
# - Both have: this Mac's commits are rebased onto GitHub's in a borrowed
#   worktree, ./check.sh --quick runs on that (a combination nobody has
#   checked yet), and only if it passes does main move to it and get
#   pushed. A conflict or a failed check leaves main as it was.
#
# Never rebase main in its own checkout instead: a rebase there leaves
# HEAD detached until it ends, and a landing in the meantime would move
# that rather than main (tb land now refuses while one's in progress, but
# it then has to wait for you).
set -euo pipefail
die() { echo "$*" >&2; exit 1; }
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
MAIN="$(git -C "$(dirname "$SELF")" worktree list --porcelain | awk '
  /^worktree / { path = substr($0, 10) }
  /^branch refs\/heads\/main$/ && !found { print path; found = 1 }
')"
[ -n "$MAIN" ] || die "No checkout of this repo has main checked out."
LOCK="$(git -C "$MAIN" rev-parse --path-format=absolute --git-common-dir)/threadbear-land.lock"

# Everything below runs under the landing lock, in one process.
if [ "${THREADBEAR_SYNC_LOCKED:-}" != 1 ]; then
  echo "Taking the landing lock (a landing in progress finishes first)..."
  if [ "$(uname)" = Darwin ]; then
    exec /usr/bin/lockf -k "$LOCK" /usr/bin/env THREADBEAR_SYNC_LOCKED=1 "$SELF" "$@"
  else
    exec flock -x "$LOCK" env THREADBEAR_SYNC_LOCKED=1 "$SELF" "$@"
  fi
fi

cd "$MAIN"
[ "$(git symbolic-ref -q HEAD || true)" = refs/heads/main ] || die "$MAIN isn't on main; nothing synced."
GITDIR="$(git rev-parse --absolute-git-dir)"
for f in rebase-merge rebase-apply MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD sequencer BISECT_LOG; do
  [ -e "$GITDIR/$f" ] && die "$MAIN has something half done ($GITDIR/$f); finish or abort it first. Nothing synced."
done

git fetch -q origin main
here="$(git rev-parse main)"
there="$(git rev-parse origin/main)"
if [ "$here" = "$there" ]; then
  echo "main is the same here and on GitHub."
  exit 0
fi
if git merge-base --is-ancestor main origin/main; then
  git merge -q --ff-only origin/main
  echo "main fast-forwarded to GitHub's ($(git log --oneline "$here..main" | wc -l | tr -d ' ') commits)."
  exit 0
fi
if git merge-base --is-ancestor origin/main main; then
  git push -q origin main:main
  echo "Pushed main to GitHub ($(git log --oneline "$there..main" | wc -l | tr -d ' ') commits)."
  exit 0
fi

echo "main and GitHub's have both moved; checking this Mac's commits on top of GitHub's in a borrowed worktree..."
wt="$("$MAIN/borrow-worktree.sh" | tail -1)"
[ -d "$wt" ] || die "Couldn't borrow a worktree; nothing synced."
give_back() {
  git -C "$wt" rebase --abort >/dev/null 2>&1 || true
  git -C "$wt" reset -q --hard main >/dev/null 2>&1 || true
  "$MAIN/borrow-worktree.sh" --give-back "$wt" >/dev/null 2>&1 || echo "Couldn't give $wt back; run $MAIN/borrow-worktree.sh --give-back $wt" >&2
}
trap give_back EXIT
git -C "$wt" reset -q --hard main
# A merge on this Mac's side (a release brought in) would be flattened by a
# rebase, losing what only the merge holds, so GitHub's is merged in then.
if [ -n "$(git rev-list --merges origin/main..main)" ]; then
  join=(merge -q --no-edit origin/main)
else
  join=(rebase -q origin/main)
fi
if ! git -C "$wt" "${join[@]}"; then
  files="$(git -C "$wt" diff --name-only --diff-filter=U)"
  die "This Mac's commits conflict with GitHub's in:
$files
Nothing synced. Bring them together by hand in a worktree of your own, check, then land that."
fi
(cd "$wt" && ./check.sh --quick) || die "The check failed on this Mac's commits over GitHub's, so main wasn't touched."
new="$(git -C "$wt" rev-parse HEAD)"
# Under the lock no landing moves main, but something by hand could have
# switched the checkout or started a rebase during the check: asked again.
[ "$(git symbolic-ref -q HEAD || true)" = refs/heads/main ] || die "$MAIN isn't on main any more; nothing synced."
for f in rebase-merge rebase-apply MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD sequencer BISECT_LOG; do
  [ -e "$GITDIR/$f" ] && die "$MAIN has something half done ($GITDIR/$f); nothing synced."
done
[ "$(git rev-parse main)" = "$here" ] || die "main moved while this ran; nothing synced."
# reset --keep keeps unstaged changes but not what's staged: refuse those.
git diff --cached --quiet || die "$MAIN has staged changes, which moving main would unstage; commit or unstage them. Nothing synced."
git reset -q --keep "$new" || die "main's checkout has changes in files this would change; nothing synced."
[ "$(git rev-parse main)" = "$new" ] && [ "$(git reflog -1 --format=%H refs/heads/main)" = "$new" ] &&
  [ "$(git symbolic-ref -q HEAD || true)" = refs/heads/main ] || die "main didn't end up at the checked $new; look at git reflog in $MAIN. Not pushed."
git push -q origin main:main
echo "main is now GitHub's with this Mac's $(git log --oneline "$there..main" | wc -l | tr -d ' ') commits on top, checked and pushed."
