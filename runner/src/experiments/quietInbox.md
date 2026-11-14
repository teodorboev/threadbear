---
id: quietInbox
name: A quiet inbox
title: A quiet inbox: a finished thread another thread started archives itself, and one waiting only on threads it started hibernates until they're done
added: 2026-11-12
on: false
reminder: eca1f43e
---

## What it does

Keeps finished threads that have nothing for you out of the inbox, in two ways. A thread that another thread's agent started archives itself when it finishes with nothing left open: nothing asked, no follow-ups suggested, nothing uncommitted, unlanded or unmerged, no background job running and no check-in booked. A note in the thread says why it went, and it shows as archived under "Threads it started" in the Highlights of the thread that started it. It stays in the inbox if you've written in it, starred it or brought it back out of the archive, and so does one that failed or was stopped. Threads you started yourself, and ones a tool or a schedule started, are never archived this way. The other way: a finished thread whose only loose ends are threads it started (still working, or waiting on you) hibernates until they finish, as if its agent had said to wait on them, as long as none of them has less access than it (a lower mode, or a sandbox): what they say would reach it without the OK a message upward needs. Its row says Waiting on and which. It wakes with what they said, runs a turn, and its reply comes to the inbox then. It wakes after a day at the latest.

## Lives in

`runner/src/runner/quiet-inbox.ts` (`settleQuietly`, called at the end of `completeTurn` in `runner.ts`); `keptOut` on the thread's record (`store.ts`, set in `Runner.update`); `gatherLooseEnds` and `childEnds` in `runner/src/runner/loose-ends.ts`, which the done card's check shares

## Taking it out

Delete `quiet-inbox.ts`, its entry in the parts list and the `Runner` interface in `runner.ts`, the `settleQuietly` call in `completeTurn`, and `keptOut` in `store.ts` and `Runner.update`. `gatherLooseEnds` and `childEnds` stay: the loose ends check uses them.
