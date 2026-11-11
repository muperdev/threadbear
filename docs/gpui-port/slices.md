# How a slice of the GPUI port gets done

The plan is [../gpui-port.md](../gpui-port.md). A slice is a small piece of what
the Swift Mac app does, done in the GPUI app and proved to be the same.

## The method

1. **Find what it should do.** The rows in [features.md](features.md) for your
   area say what exists and which Swift files it's in. Read that Swift. The Swift
   app is the definition of right.
2. **Write the scenario first**, in `features/` (`features/README.md` says how:
   one `.feature` per area, its steps in `steps/<name>.steps.ts`, each scenario
   tagged with its features.md id, and `node verify-pairing.mjs` passing). Write
   behaviour someone would notice, not implementation. Scenarios assert on what's
   drawn.
3. **Pass it on the Swift app first**, so you know the scenario is right. If the
   Swift `dump` doesn't show something a scenario needs, add it to AppDump.swift
   (debug-only), keeping the field names plain.
4. **Build it in `desktop/`.** The app's `automation` commands and `dump` keep the
   Swift app's names, so a scenario works unchanged on both. If you need a new
   command, read how the Swift one behaves (`app/README.md`) and match it.
5. **Pass it on the GPUI app**, on Linux (`cloud`) and on a real Mac (Tinker Desk).
6. **Compare the pictures.** `features/tools/shot.ts` saves either app's window,
   and `features/tools/compare.ts` takes the same threads in both apps and puts
   them side by side on one page (`TB_APPEARANCE=dark` or `light`). Match the
   layout: spacing, sizes, colours (from `desktop/gpui/src/style.rs`, which holds
   the Swift values; add to it rather than typing numbers). A GPUI row that only
   reads the same isn't done. Read colours with care: the Swift pictures carry the
   display's colour profile and the GPUI ones none, so the same colour reads a few
   steps apart, and the GPUI shot has no traffic lights (it's drawn in-process).
7. **Log it** in `PARITY.md`: what changed, files, what's left, and a
   `Verification:` line that names a spec run or a screenshot. A slice with no
   runtime check doesn't count.

Commit each slice on its own, with passing specs. Keep `cargo test -p
threadbear-core` and the specs green.

## Where things run

Rust builds and spec runs never go on Mike's M5.

```bash
desktop/scripts/remote.sh cloud -- bash -c 'cd desktop && cargo build -p threadbear-gpui --features automation 2>&1 | tail -5'
desktop/scripts/remote.sh cloud -- bash -c 'cd features && TB_APP=gpui TB_TAGS="@sidebar.search" node_modules/.bin/cucumber-js 2>&1 | tail -30'
desktop/scripts/remote.sh tinker-desk -- bash -c 'cd desktop && cargo build -p threadbear-gpui --features automation 2>&1 | tail -5'
desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd features && TB_APP=gpui node_modules/.bin/cucumber-js 2>&1 | tail -30'
desktop/scripts/remote.sh tinker-desk -- bash -c 'cd app && swift build --disable-keychain --product Threadbear 2>&1 | tail -1'
desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd features && TB_APP=swift node_modules/.bin/cucumber-js 2>&1 | tail -30'
```

- `threadbear-core` is small: `cargo test -p threadbear-core` in `desktop/` is fine
  on this Mac with `taskpolicy -b` (`$HOME/.cargo/bin` on the PATH).
- macOS has no `timeout` command. Never use `npx` for cucumber (it fetches a
  package of that name if ours is missing).
- Never `pkill` or `killall` on these machines: other threads' work runs there.
  Stop what you started by its pid.
- Pictures from Tinker Desk: `TB_APP=gpui node features/tools/shot.ts "<thread
  title>" "$TMPDIR/out.png"` through `--desk`, then `scp` it back (its TMPDIR
  there is `~/.cache/threadbear-check/gpui/tmp`). Dark mode unless told otherwise.
  Delete pictures when done unless they're evidence for the log.

## How it has to look and behave

From AGENTS.md, and Mike's standing rules:

- UI copy: plain words, no eyebrow or kicker labels, no em dashes.
- Every control gets a tooltip.
- Every click shows something at once: an optimistic change, or a spinner with
  the control disabled until the work lands. Never a button that looks idle while
  it waits and can be pressed twice.
- Comments say why, especially for anything unusual. Name the Swift type or
  function a piece of Rust ports.
- The automation commands are compiled only with the `automation` feature, never
  into a release.
- Don't add a dependency without a good reason, and pin it exactly.
