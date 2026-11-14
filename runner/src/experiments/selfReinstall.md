---
id: selfReinstall
name: The runner reinstalls itself
title: The runner reinstalls itself: when launch.mjs or Threadbear Runner.app is behind main, the runner runs install-runner.sh by itself once no thread is busy
added: 2026-11-14
on: false
reminder: d80d9406
---

## What it does

A few parts of the runner (launch.mjs, the LaunchAgent and Threadbear Runner.app) aren't updated by its releases, only by reinstalling it. When one of them is behind main, the Updates panel has a Reinstall now button for it. With this on, the runner doesn't wait for the button: once no thread is busy, it runs the reinstall itself, as a separate background job, so stopping the runner doesn't stop the reinstall too. If a thread starts while the reinstall is building, it waits for that thread to finish before trying again. If the new runner doesn't come up, the old one is put back and the Updates panel shows why, with a Try again button. A failed reinstall isn't tried again by itself until main's launcher changes.

## Lives in

`runner/src/reinstall.ts` (`auto` reads this experiment), wired in `runner/src/main.ts`; `runner/scripts/install-runner.sh` (the busy exit and `THREADBEAR_INSTALL_QUIET`) serves Reinstall now too.

## Taking it out

Have `auto` in main.ts's `Reinstaller` return false. Reinstall now in the Updates panel still works without it.
