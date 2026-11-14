---
id: sameWork
name: Threads already on it
title: Threads already on it: a new thread's first turn is told which open threads may be doing the same work, so its agent checks before starting
added: 2026-11-13
on: false
reminder: ddfd3a91
---

## What it does

When you start a thread, Threadbear compares your first message with the threads you have open (not archived), weighting those in the same repo or project, and if any look like the same work, tells the new thread's agent about them before it starts: their titles, ids and what they were last asked. The agent reads any that look the same, and if one already covers it, says so and passes anything new to that thread instead of doing the work twice, or asks you which should own it. A line in the new thread says which threads it found. Nothing is added when nothing matches. Only threads with at least the new thread's access (its mode, and outside the sandbox if it is) are named to it, checked again as its first session starts, as their titles and words are their own agents' and send_message would ask you before passing them up. It's plain word matching on your machine, with no model and nothing sent anywhere, and it only looks at this Mac's threads, not another runner's.

## Lives in

`runner/src/same-work.ts` (the matching and the note); `findSameWork` in `runner/src/runner/new-threads.ts`, called from `create`; `ThreadRecord.sameWork` and `sameWorkIds` (`store.ts`), added to the first session's instructions in `briefFor` (`runner.ts`)

## Taking it out

Self-contained: delete `same-work.ts`, `findSameWork` and its call in `create`, the `sameWork` and `sameWorkIds` fields and their lines in `briefFor`, and `mayTellOf` in `hibernation.ts` if nothing else uses it.
