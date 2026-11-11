---
id: devin
name: Devin threads
title: Devin threads: run threads on the Devin CLI, on your own Devin login
added: 2026-11-09
on: false
reminder: ce47e0f1
---

## What it does

Your Devin CLI login (`~/.local/share/devin`, made with `devin auth login`) shows up beside your other logins as devin-main, in its own Devin pool, and threads can run on it like any other: pick a Devin model (SWE-2, or any other model your Devin account offers) in the model picker, or move a thread to the Devin login. Devin threads have the threadbear tools, approval cards with Always allow, Devin's own slash commands, its models and thinking levels, the context meter and token counts per turn. The permission modes set Devin's own: Full access is Bypass, Auto is Smart (a fast model decides), Plan first is Plan, and Ask before edits is Code, which asks before commands but lets edits inside the folder run, as Devin offers no mode over ACP that asks before every edit. Devin also keeps its own permission rules (`~/.config/devin/config.json`), which can allow things before Threadbear is asked. It needs the Devin CLI from its own installer (`curl -fsSL https://cli.devin.ai/install.sh | bash`), which Threadbear then keeps up to date like the other CLIs. Only one Devin login for now, and Devin threads can't run in the sandbox.

## Lives in

`runner/src/adapters/devin/` (Devin's dialect of the Grok adapter); `runner/src/adapters/grok/` (the ACP adapter both share); `runner/src/updates/devin.ts` (its updates); the Devin login in `accounts.ts` (`devinOn`); built in `main.ts`; `docs/devin-plan.md`

## Taking it out

Delete `runner/src/adapters/devin/`, the `devin` adapter and `devinOn` in `main.ts`, the Devin lines in `accounts.ts`, `devinToolOf` and `DEVIN_TERMINAL_ONLY` in `adapters/grok/normalize.ts`, and `devin` from `Provider` in `protocol.ts` and `docs/protocol.md` (the compiler then lists the rest: titles, the probe host, config, CLI updates).
