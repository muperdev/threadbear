---
id: gpuiApp
name: GPUI desktop app
title: GPUI desktop app: the Rust app that's replacing the Swift one, beside it for now
added: 2026-11-09
on: false
reminder: 19f8de18
---

## What it does

The app in `desktop/`, the one that will run on Mac, Windows and Linux, installed as `~/Applications/Threadbear GPUI.app` beside the Swift app and talking to the same runner. It doesn't do everything the Swift app does yet (`docs/gpui-port/PARITY.md` says what it does), so it's only for someone who wants to try it. Turning this on lets `desktop/scripts/install-app.sh` install it, and makes the runner count it as the app being open when it works out whether you're at your Mac.

## Lives in

`desktop/` and `features/`; `appConnected` in `runner/src/rpc.ts`; `desktop/scripts/install-app.sh`

## Taking it out

Delete `desktop/`, `features/`, `.github/workflows/desktop.yml` and the desktop line in `appConnected`. Until it replaces the Swift app, nothing depends on it.
