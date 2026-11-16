---
id: selfReinstall
name: The runner updates its launcher itself
title: The runner updates its launcher itself: when launch.mjs or Threadbear Runner.app is behind main, the runner stages the new ones by itself, to take over at the next login
added: 2026-11-14
on: false
reminder: d80d9406
---

## What it does

A few parts of the runner (launch.mjs, the LaunchAgent and Threadbear Runner.app) aren't updated by its releases. When one of them is behind main, the Updates panel has an Update it button for it. With this on, the runner doesn't wait for the button: it puts the new ones in place by itself, as a separate background job, without stopping anything, and they take over the next time your Mac restarts or you log in again. (Restarting them straight away would stop every thread at once, and a busy Mac never has a moment with nothing running.) If it doesn't work, the Updates panel shows why, with a Try again button. A failed one isn't tried again by itself until main's launcher changes.

## Lives in

`runner/src/reinstall.ts` (`auto` reads this experiment), wired in `runner/src/main.ts`; `runner/scripts/install-runner.sh --stage` (and `THREADBEAR_INSTALL_QUIET`) serves Update it too.

## Taking it out

Have `auto` in main.ts's `Reinstaller` return false. Update it in the Updates panel still works without it.
