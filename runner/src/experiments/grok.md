---
id: grok
name: Grok threads
title: Grok threads: run threads on Grok Build, xAI's coding CLI, on your own grok.com login
added: 2026-11-08
on: false
reminder: 72f3b3b4
---

## What it does

Your Grok Build login (`~/.grok`, made with `grok login`) shows up beside your Claude and Codex logins, in its own Grok pool, and threads can run on it like any other: pick a Grok model in the model picker, or move a thread to the Grok login. Grok threads have the threadbear tools, the permission modes (Full access, and Ask before edits, which Auto and Plan first behave like for now), approval cards with Always allow, Grok's own slash commands, its models and effort levels, and token counts per turn. More logins go in `grok-1`, `grok-2` and so on under the accounts folder, each signed in with `GROK_HOME=<the folder> grok login`. Editing a message goes back by reversing the thread's diffs, as Grok's own rewind isn't used yet, and Grok threads can't run in the sandbox.

## Lives in

`runner/src/adapters/grok/` (the adapter, its process and normaliser); `runner/src/mcp-http.ts` (the threadbear tools over MCP for it); Grok logins in `accounts.ts` (`grokOn`); built in `main.ts`; `docs/grok-plan.md`

## Taking it out

Delete `runner/src/adapters/grok/` and `runner/src/mcp-http.ts`, the `grok` adapter and `grokOn` in `main.ts`, the Grok lines in `accounts.ts`, and `grok` from `Provider` in `protocol.ts` and `docs/protocol.md` (the compiler then lists the rest: titles, the probe host, config).
