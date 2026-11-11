---
id: projectDecisions
name: Decisions on the Projects page
title: Decisions on the Projects page: every open question from a project's threads in one place, answered in a row
added: 2026-11-10
on: false
reminder: a708aa86
---

## What it does

Every question your threads are waiting on you for, gathered by project. On the Projects page a project's card says how many decisions it has ("3 decisions"), and so does its heading in the sidebar's Projects view. Open the project and its page starts with them: a card for each thread waiting on you, with the thread's title, a line on what it's doing, its questions just as the thread shows them (the Recommended star and Something else too), and Open thread for the whole story. Pick an answer and it goes to that thread exactly as if you'd answered it there: the card says "Sending to" the thread with a spinner, then drops off the list once the thread takes it. A card with several questions sends once each has an answer; with Something else, press Return in its box (⌘Return sends whatever's answered). Nothing new runs and nothing is relayed: each question stays in its own thread. The inbox is as before. Mac app only for now; the phone and the website don't have the page yet.

## Lives in

`app/Sources/ThreadbearCore/Threads/ProjectDecisions.swift` (which threads, the counts, the line on what each is doing), `decisions` in `ProjectCardStatus` (`ProjectCards.swift`); the Decisions section and its cards in `app/Sources/Threadbear/Views/ProjectDecisionsView.swift`, shown from `ProjectPage.swift`, the counts in `ProjectsHome.swift` and `ProjectsList.swift`; `pickDecision`, `sendDecision` and `decisionSends` in `AppModel+ThreadActions.swift` and `AppModel.swift`. The app learns it's on from `hello`'s `experiments`. The runner keeps nothing of its own for it: thread summaries already carry their asks, the line comes from the recap (with the recap box experiment on) or the thread's latest headline, and answers are an ordinary `threads.send`.

## Taking it out

Delete `ProjectDecisions.swift` and `ProjectDecisionsView.swift`, the `projectDecisionsOn` checks in `ProjectPage.swift`, `ProjectsHome.swift`, `ProjectsList.swift` and `Projects.swift`, `decisions` in `ProjectCards.swift`, the Decisions part of `AppModel+ThreadActions.swift` and `decisionSends`, and the `decision` automation command and `projectDecisions` in `dump`. Nothing in the runner or the backend changes.
