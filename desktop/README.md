# Threadbear for Mac, Windows and Linux (GPUI)

One native desktop app, drawn with [GPUI](https://gpui.rs) (Zed's UI
framework) and [gpui-kit](https://gpui-kit.com)'s components. It's growing
to replace the Swift Mac app in `app/`; `docs/gpui-port.md` has the plan and
what's left. The iPhone app stays Swift.

Like the Mac app, it holds no agent state of its own. It talks to a runner
(`runner/`) over the protocol in [`../docs/protocol.md`](../docs/protocol.md).

So far it signs in to the runner, shows the inbox, opens a thread (live, as
it streams), answers a card's asks and approvals, sends messages and stops a
turn, in the Swift app's window: the panels in BearDock with the activity bar
and right rail, the thread's title line and menu, banners, notes, the menu bar
and the keyboard shortcuts. It starts new threads (the new-thread page, on any
runner) and offers the slash command palette. Other runners' threads are listed
and open through this Mac's runner.
The pane beside the thread has Highlights, file previews, Patches and the
folder browser.

## How it's put together

- **`core/`, what the app knows with no window**: the inbox rules, and
  `reducer/`, a port of how the Swift app turns events into rows, checked
  against golden files both languages test (`core/tests/golden`).
- **`dock/`, BearDock for GPUI**: splits, tabs, panel dragging and torn-out
  windows, the shell of the app's window (`gpui/src/shell.rs`). Its layout tree
  (`dock/layout`) has no GPUI in it and reads what the Swift `dock/` saved. See
  `dock/README.md`.
- **`gpui/src/settings.rs`, what the app remembers** (window frame, zoom,
  folds, the Inbox's sort, the panel layout): one JSON file in the runner's data
  folder, `desktop-settings.json`, keyed by the Swift app's UserDefaults names
  so a one-time import is mechanical.
- **`client/`, the runner connection** (`runner.rs`): the challenge sign-in
  (`auth.rs`, the same HMAC as `runner/src/rpc-auth.ts`), hello, requests and
  replies, and reconnecting when the runner restarts. `tests/mock_runner.rs`
  signs in to the mock runner.
- **`gpui/`, the app.** GPUI has no tokio, so the connection runs on its own
  runtime and its events cross on a channel (`main.rs`). With the `automation`
  feature (never in a release) `automation.rs` takes the same scripted commands
  as the Swift app, which is how `features/` drives it.
- **Which runner.** The one in `THREADBEAR_HOME` (default `~/Library/Application
  Support/Threadbear`, which the runner uses on Linux too), on the port from
  its `runner.json` (default 8797), as `tb` finds it.

## Run it

You need Rust (`rustup`) and Node 24. On a Mac you also need Xcode's command
line tools; on Windows, the C++ build tools. On Linux:

```bash
sudo apt install libasound2-dev libfontconfig-dev libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libzstd-dev libvulkan-dev
```

On a flaky connection, crates.io downloads need
`CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10`.

```bash
npm ci --prefix ../app/Tools/mock-runner   # for the tests and the mock
cargo run -p threadbear-gpui
```

To run it against the mock runner instead of your real one, start
`node ../app/Tools/mock-runner/server.mjs --data-dir /tmp/tb-mock` and run
the app with `THREADBEAR_HOME=/tmp/tb-mock`.

## Installing it on a Mac

It's an experiment (`runner/src/experiments/gpuiApp.md`): turn it on in
`runner.json`, then:

```bash
scripts/install-app.sh                      # builds here, about 10 minutes
scripts/install-app.sh --build-on <host>    # builds on another Mac and copies it
```

That puts `Threadbear GPUI.app` in `~/Applications`, beside the Swift app, using
the same runner.

## Tests

```bash
cargo test    # the connection (with the mock) and the app's rules
```

`../features/README.md` has the behaviour specs, which run the real app on the
mock runner. Run Rust builds and suites elsewhere (`scripts/remote.sh`):

```bash
scripts/remote.sh cloud -- bash -c 'cd desktop && cargo test -p threadbear-core'
```

`scripts/spec.sh` is the one-command version for use through `remote.sh`: it
builds the app (Swift or GPUI), keeps its own copy of the GPUI binary so another
checkout's build can't swap it mid-run (the cargo target folder is shared), and
runs specs by tag or takes a picture (`--shot "<thread title>" out.png`).

`desktop/test/auth-vector-*.txt` are fixed sign-in proofs. The Rust tests
check them, and so does `runner/test/unit/desktop-auth-vector.test.ts` with
the runner's own code, so if the runner's auth changes both sides fail until
they agree again.

`.github/workflows/desktop.yml` runs it all on Linux, Mac and Windows and
uploads each platform's binary as the run's artifacts.

## Seeing it on a server

`scripts/shot.sh OUT_DIR [--home DIR] [--click X,Y | --type TEXT | --key KEY]...`
runs the debug build (`cargo build -p threadbear-gpui`) on a virtual display
(Xvfb) with Mesa's software Vulkan, against the mock (or the runner in
`--home`), and saves screenshots: `0.png` before any step, then one after
each.

## Known gaps

- On Windows, path handling (`folderName` splits on `/`) and the runner's
  data dir are still Mac-shaped. The runner itself doesn't run on Windows
  yet.
