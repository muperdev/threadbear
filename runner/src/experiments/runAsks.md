---
id: runAsks
name: Run on the card
title: "Run on the card: an agent's ask can carry a command, with a Run button that runs it once and tells the agent how it went"
added: 2026-11-10
on: false
reminder: c9e2765c
---

## What it does

When something is yours to run and not the agent's (a production migration, a deploy), the agent can put the command on its card. You see the exact command and the folder it runs in, with a Run button. It runs once, on the machine the thread is on, and the output goes straight back to the agent, so it carries on with no copying or typing "done". Works from the Mac, the phone and the website, except a command the phone or website would show with a secret scrubbed out of it: that one runs only from the Mac app connected straight to the thread's runner. The command can't answer prompts, so one that asks "are you sure?" fails and the agent hears why. It stops after ten minutes.

## Lives in

`armRunAsks`, `pressRun`, `runPressed` and `tellInterruptedRuns` in `runner/src/runner/buttons.ts`; `run` on an ask in `runner/src/needs.ts` and on `end_turn` in `runner/src/threadbear-mcp.ts`; `AskRun` in `runner/src/protocol.ts` and `run` on `asks` in `convex/schema.ts`; the card's command and button in `app/Sources/Threadbear/Transcript/RowRenderer.swift`, `ios/ThreadbearKit` and `web/src/App.tsx`.

## Taking it out

Remove the `runAsks` check in `armRunAsks` and the `run` argument on `end_turn`, then the card's command and button on each app. `run` on the backend's `asks` can stay until the next schema tidy.
