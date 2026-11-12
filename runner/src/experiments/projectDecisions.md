---
id: projectDecisions
name: Decisions on the Projects page
title: Decisions on the Projects page: every open question from a project's threads in one place, answered in a row
added: 2026-11-10
on: false
reminder: a708aa86
---

## What it does

Every question your threads are waiting on you for, gathered by project. On the Projects page a project's card says how many decisions it has ("3 decisions"). The sidebar's Projects view becomes one row a project: its icon and name, its decisions in orange when it has any (else how many threads it has), and a chevron that shows its threads under it or hides them again. Clicking the rest of the row opens the project's page. Every project starts folded, and each remembers whether you left it open; Not in a project folds the same way. Open the project and its page is in two tabs at the top, Decisions (with how many are waiting, its parts' too) and Project (its parts, goals, state, notes, tasks, folders and threads). It opens on Decisions when something's waiting and on Project otherwise, keeps the tab you last picked for each project while the app runs, and if the decisions all get answered while you're on that tab it says "Nothing waiting" there rather than jumping away. ⇧⌘[ and ⇧⌘] (Go menu) move between the tabs. The Decisions tab has a card for each thread waiting on you, with the thread's title, a line on what it's doing, its questions just as the thread shows them (the Recommended star and Something else too), and Open thread for the whole story. Pick an answer and it goes to that thread exactly as if you'd answered it there: the card says "Sending to" the thread with a spinner, then drops off the list once the thread takes it. A card with several questions sends once each has an answer; with Something else, press Return in its box (⌘Return sends whatever's answered). Nothing new runs and nothing is relayed: each question stays in its own thread. The inbox is as before. Mac app only for now; the phone and the website don't have the page yet.

## Lives in

`app/Sources/ThreadbearCore/Threads/ProjectDecisions.swift` (which threads, the counts, the line on what each is doing), `decisions` in `ProjectCardStatus` (`ProjectCards.swift`); the Decisions section and its cards in `app/Sources/Threadbear/Views/ProjectDecisionsView.swift`, shown from `ProjectPage.swift`, the counts in `ProjectsHome.swift`, the folded sidebar rows in `ProjectsList.swift` (`ProjectFoldRow`) with `app/Sources/ThreadbearCore/Threads/ProjectFolds.swift` and `openProjectRows` in `AppModel.swift`; the page's tabs in `ProjectPage.swift` (`ProjectTabBar`) with `app/Sources/ThreadbearCore/Threads/ProjectTabs.swift`, `projectTabsPicked` in `AppModel.swift` and the Go menu's Show Previous Tab and Show Next Tab (`previousTab`, `nextTab` in `Shortcuts.swift`); `pickDecision`, `sendDecision` and `decisionSends` in `AppModel+ThreadActions.swift` and `AppModel.swift`. The app learns it's on from `hello`'s `experiments`. The runner keeps nothing of its own for it: thread summaries already carry their asks, the line is the thread's latest headline, and answers are an ordinary `threads.send`.

## Taking it out

Delete `ProjectDecisions.swift`, `ProjectFolds.swift`, `ProjectTabs.swift` and `ProjectDecisionsView.swift`, `ProjectTabBar` and the two tab commands, `ProjectFoldRow` and `openProjectRows`, the `projectRow` automation key, the `projectDecisionsOn` checks in `ProjectPage.swift`, `ProjectsHome.swift`, `ProjectsList.swift` and `Projects.swift`, `decisions` in `ProjectCards.swift`, the Decisions part of `AppModel+ThreadActions.swift` and `decisionSends`, and the `decision` automation command and `projectDecisions` in `dump`. Nothing in the runner or the backend changes.
