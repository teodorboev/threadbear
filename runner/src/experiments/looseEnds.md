---
id: looseEnds
name: Loose ends on the done card
title: Loose ends on the done card: the runner checks a finished thread's branch, worktree, the threads it started and its check-ins, so Archive is safe without asking
added: 2026-11-07
on: false
reminder: c7330a39
---

## What it does

When a thread finishes with nothing for you to do, Threadbear checks what's still open before you archive it: commits on its worktree's branch that aren't on main, files in its worktree that aren't committed (or, for a thread with no worktree of its own, files it changed that aren't committed and commits it made that aren't pushed), work it pushed that isn't on main yet (naming the PR when there's an open one on GitHub; a merged or closed one doesn't count), threads it started that are still working or waiting for you, and check-ins it booked. The agent also says what's still open that only it knows, like a reply worth sending or a decision waiting somewhere else. If there's nothing, the done card shows just Done, so Archive is safe without asking the agent first. If there is, it lists them under "Still open" (a thread opens from its line; Land is the chip above the message box). A check-in alone shows under "Coming back", as the thread comes back by itself when it runs. It checks again when you open the thread, after an action like Land, and when a thread it started finishes. The phone's and the website's done cards show the same list, though not for a thread on another Mac.

## Lives in

`runner/src/loose-ends.ts`, `runner/src/runner/loose-ends.ts`; end_turn's `open` (`looseEnds` in `threadbear-mcp.ts`, `done` in `needs.ts`, kept on `rec.needs`); the calls in `completeTurn` and `update` (`runner.ts`), `runAction` (`runner/patches.ts`) and `threads.subscribe` (`rpc.ts`); `schedulesFor` set in `main.ts`; app `looseEndsText` in `RowRenderer.swift`, `setLooseEnds` in `TranscriptReducer.swift`

## Taking it out

Self-contained: delete the two files, end_turn's `open` and those calls; the app draws nothing without the field.
