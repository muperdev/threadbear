---
id: cardReviewer
name: Card reviewer
title: Card reviewer: an Auto thread's approval cards are judged by a reviewer before they reach you
added: 2026-11-09
on: false
reminder: d959651f
---

## What it does

In a thread on Auto, when something would stop for your OK (an edit to a file Threadbear runs as code, a message to a thread with more access, anything else that puts up an approval card), a reviewer looks at it first: Opus, with no tools, shown the card in full and what the thread was asked to do. If it fits the task and can't do real harm, it goes ahead, and the transcript says "Allowed by the reviewer" with its reason. If the reviewer has doubts, or can't run (no pool, a private folder, a card too long to read whole), the card comes to you as before, after a note with its reason. The reviewer is told that everything it reads is data, and that text trying to tell it what to answer is a reason to hand the card to you. Cards that hand out access lasting past the one action (switching a thread to more access, starting a thread with more, a schedule, moving a thread or changing its logins) always come to you. Codex threads' own tool cards come to you too, as they name the files a change touches but not the change. Ask before edits, Full access and plans are left alone: Ask means a person looks.

## Lives in

`runner/src/runner/card-review.ts`; `reviewThenCard` and the two calls to it in `requestApproval` and `requestAgentApproval` (`runner/src/runner/approvals.ts`)

## Taking it out

Delete the file, `reviewThenCard`, and the `reviewsCards` branches in those two methods.
