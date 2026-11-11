---
id: recapBox
name: Recap box on quiet threads
title: Recap box: threads quiet for 12 hours open with what they're for and where they got to
added: 2026-11-10
on: false
reminder: fb8fb00c
---

## What it does

When you open a thread that hasn't done anything for 12 hours or more, a box above the message box says what the thread is for and where it stands, so you don't have to ask. The librarian writes it ahead of time, after each turn, as part of the look it already takes, so it costs no extra model calls. Press × to put it away; sending a message does too. It shows on the Mac, the phone and the website.

## Lives in

`recapShowing` and `recapText` in `runner/src/runner.ts`, `setBrief` in `runner/src/runner/titles.ts`, the `purpose` field in `runner/src/librarian.ts`; `recap` on a thread's row (`runner/src/sync.ts`, `runner/src/thread-index.ts`, `convex/schema.ts`, `convex/sync.ts`); the box itself in `app/Sources/Threadbear/Views/RecapBox.swift`, `ios/Threadbear/RecapBox.swift` and `web/src/App.tsx`; the shared rule in `app/Sources/ThreadbearCore/Threads/Recap.swift`, `ios/ThreadbearKit/Sources/ThreadbearKit/Recap.swift` and `web/src/view-logic.ts`.

## Taking it out

Remove the `recapBox` check in `summary` and `recapShowing` in `runner/src/runner.ts` (`msg.recapped` goes with it), then delete the three boxes and their rule. `recap` on the row can stay until the next backend change.
