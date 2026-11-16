---
id: nestedThreads
name: Nested threads
title: Nested threads: a thread's own threads sit under it in the sidebar, and it hears from them once, when they've finished
added: 2026-11-15
on: false
reminder: 27cedfad
---

## What it does

Threads another thread started sit indented under it in the sidebar, like sub-agents, instead of having rows of their own across the inbox. The group sits where its most urgent member would: in the inbox if any of them needs you, in Running if any is working, otherwise where the starting thread is. It opens by itself when one of them has a question for you and folds otherwise, saying how many are working. Each one opens with a line saying which thread started it and why. An agent can start a thread that isn't part of its work as one of its own, and you can move a thread out from under its starter, or back, from its menu. The starting thread no longer wakes for each report its threads send it: they're kept until all of them have finished (or one fails), then come to it together. Mac app only for now.

## Lives in

`runner/src/runner/nested.ts` (`holdReport`, `settleHeld`, `takeHeld`, called from `send`, `completeTurn`, `update`, `delete` in `runner.ts`, `deliverWaits` in `runner/hibernation.ts` and the start in `main.ts`); `detached` and `heldReports` on the thread's record (`store.ts`), `detached` on `threads.create` and `threads.update` (`rpc.ts`) and `create_thread`'s `nested`; in the GPUI app, the grouping in `desktop/core` and the sidebar and the "Started by" line in `desktop/gpui`

## Taking it out

Delete `nested.ts`, its entry in the parts list and the `Runner` interface in `runner.ts`, the `holdReport` and `deliverHeld` lines in `send`, the `settleHeldSoon` calls, `takeHeld` in `deliverWaits`, the `held reports` start step, `heldReports` in `store.ts`, and the GPUI app's grouping and "Started by" line. `detached` can stay: it's harmless without the sidebar reading it.
