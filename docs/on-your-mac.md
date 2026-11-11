# What Threadbear does on your Mac

Everything Threadbear installs, runs in the background, reads, and talks to
over the network. It's here so you, or the agent installing it for you, can
check the code against it before running anything. If the code does
something that isn't on this list, treat that as a problem and say so.
File references are where to look.

## What setup and install add

- **Git remotes** (`scripts/setup.mjs`, "Your copy"): Mike's repo becomes
  `upstream`. If `gh` is logged in and you cloned rather than forked, setup
  forks the repo to your GitHub account (`gh repo fork --remote`).
- **Packages:** `npm ci` in the repo, in `runner/`, and in each runner
  release. esbuild and fsevents have install scripts.
- **Convex, in your own account:** a login (`~/.convex/config.json`), a
  project with a production deployment, a deploy key, and one environment
  variable (`RUNNER_KEY`). It deploys `convex/` and the `web/` site there.
  A second Mac set up with `--existing` gets none of these: its runner's
  key comes from your first Mac (`tb runner add`), which also writes the
  deployment's address into its `runner.json`.
  If that login goes missing or expires, the Convex panel's Sign in to
  Convex button runs `convex login` again from the runner, which writes
  the same file; you approve it in your browser on auth.convex.dev.
- **Or a self-hosted backend** (`setup --self-hosted`, docs/self-hosting.md;
  Convex cloud is the one we recommend): no Convex login or deploy key.
  Instead two Docker containers, `threadbear-convex` (Convex's open source
  backend) and `threadbear-convex-dashboard`, from pinned
  `ghcr.io/convex/*` images, with the data in the Docker volume
  `threadbear-convex-data`. They listen on this Mac's loopback only
  (ports 13210, 13211 and 16791), and `tailscale serve` puts them on your
  tailnet as `https://<this Mac>.<tailnet>.ts.net` ports 3210 (the API),
  3211 (the website) and 6791 (the dashboard), so nothing is reachable off
  your tailnet. The backend's admin key is in
  `secrets/convex-self-hosted.env`, and setup sets `PUSH_RELAY=off` on it,
  as the push relay only works with Convex cloud. runner.json gets
  `sync.site`, the website's address. With `--local` there's no
  `tailscale serve` at all: the three stay on 127.0.0.1, the API and the
  website on 13210 and 13211 inside the container as well as out, and
  only this Mac can reach them.
- **Apps:** `~/Applications/Threadbear.app` and
  `~/Applications/Threadbear Runner.app`, both built and signed on your Mac.
  If you turn on the `gpuiApp` experiment and run
  `desktop/scripts/install-app.sh`, also `~/Applications/Threadbear GPUI.app`,
  which keeps its window and panel layout in `desktop-settings.json` and
  `gpui-prefs.json` in the data folder.
- **A LaunchAgent:** `~/Library/LaunchAgents/com.mikerosoft.threadbear.runner.plist`,
  which keeps the runner going while you're logged in. A runner on a Linux
  server (INSTALL.md, section 6) has a systemd user service in its place,
  `~/.config/systemd/user/com.mikerosoft.threadbear.runner.service`, and no
  apps; it converts pictures with ImageMagick and rsvg-convert, and reads
  the account's full name from `getent passwd` rather than `id -F`.
- **A command:** `~/.local/bin/threadbear`.
- **Shells, when you open the Terminal (⌘J):** the app starts your login
  shell (`$SHELL`, else zsh) in a pseudo-terminal, one per thread you open
  it on, in that thread's folder. They're its own child processes: a
  thread's shell stops when the thread is archived or deleted, and all of
  them stop when the app quits (`app/Sources/Threadbear/Views/TerminalPanel.swift`).
- **Files:** the data folder `~/Library/Application Support/Threadbear`
  (threads, settings, `secrets/`, runner releases, the pinned release key
  `upstream-signers`, the Mac app's `app-drafts.json`, what's typed but not
  sent in each message box, so a quit or relaunch doesn't lose it, and
  `sync-sent.json`, hashes of what's already in your
  Convex mirror so a restart sends only what changed, and
  `shared-pools.json`, the last pools your Convex backend had, which every
  runner shares, so one starts with them while the backend can't be reached,
  and `shortcuts.json`, the keyboard shortcuts you've changed in Settings >
  Keyboard, which the app reads and writes and never sends anywhere,
  `schedule-runs/`, schedule runs your Convex backend hasn't been told
  of yet, a file each, kept only until it has, and
  with the standing instructions experiment on, `instructions.md`, which every
  agent is given, and `instructions.sig`, which says Settings saved it). In `secrets/`, `asked/` holds the answers you
  type into an agent's secret card, one owner-only file each with a small
  script beside it that prints it for `sudo -A`, removed after 24 hours,
  and `secret-box.key` is the key your phone seals those answers to;
  `made-deploy-keys` lists, by name, the deploy keys this Mac made (setup's
  and the Convex panel's), so `tb runner rotate-owner` can tell them from
  others;
  `convex-key` is this Mac's key for your Convex backend (the deployment's
  `RUNNER_KEY` on your first Mac, a runner key of its own on any other);
  `runners/<id>.token` is another Mac's runner token, if you copied one
  in (only for Settings > Runners > Connect to other machines directly;
  by default another Mac's threads come through your Convex backend, read
  by your first Mac's runner while one is open, and nothing connects to
  that Mac), and `runners/<id>.host` beside it, which you copy in too (`tb pair`
  on that Mac prints both): the only host the app sends that token to.
  The app only reads these, and proves it has the token rather than
  sending it (docs/protocol.md, "Auth"); with no `.host` it dials nowhere;
  `~/Library/Caches/Threadbear/update`;
  `~/Library/Caches/Threadbear/remote` (copies of pictures and files from
  threads on your other Macs, at most 512 MB, unused ones go after 30 days);
  `~/Library/Caches/Threadbear/rich` (pictures of the ```` ```mermaid ```` and
  ```` ```html ```` blocks in replies, and each one's page for the side pane,
  newest 900 files kept). They're drawn on this Mac in web views that load
  nothing from the network: the diagram library, [Pele](https://pele.run/),
  is bundled with the app (`Resources/Rich/pele.js`, MIT), and a block's
  page runs no scripts;
  `~/Library/Logs/threadbear-runner.log` and `threadbear-update.log`;
  `~/Library/Logs/Threadbear/hangs` (the last 10 times the app froze: if its
  main thread is stuck for 4 seconds it runs macOS's `sample` on itself, in
  a sandbox, and notes the main thread's priority, the Mac's load average
  and how long since the last keyboard or mouse input (just how long, never
  what); nothing else ever reads these); and `~/ThreadbearThreads`, the default
  folder for new threads. A folder you open from a link in the side pane
  is listed, and watched for changes only while it's showing, and macOS may
  ask for permission the first time that's somewhere like Documents. The runner also runs `vm_stat` and `sw_vers` on a Mac, or reads `/proc/meminfo` and `/etc/os-release` on Linux, for the memory and system figures in the website's Runners view and the Mac app's runner details (`runner/src/runner-stats.ts`), every 5 minutes and while either is open. It also runs `ps`, and `sample` (in a
  sandbox, through `sandbox-exec`) on a thread's own CLI, when a turn has been quiet for a minute,
  to tell a CLI frozen on a macOS prompt from a long job. The sample is a
  temp file, deleted straight after. Taking over from a draining runner
  older than 5 Nov 2026, it runs `lsof` on that runner's pid once, to check
  the connection it's about to send the token over is really that runner's.
  `mac-screen.json` in the data folder
  remembers what it's said about dialogs on the screen (below).

## What runs in the background

- **Landing work** (`runner/src/worktrees.ts`): `tb land` keeps an empty
  `threadbear-land.lock` in the repo's common git directory. It runs
  macOS's `lockf` (Linux's `flock`) with a small Node child to hold an OS
  lock from before rebasing until the check and fast-forward finish.
  Another landing waits; exiting, including a crash, releases the lock.
  Beside it, `threadbear-land-times.log` records how long each landing's
  check took (branch, commit, step times, no output), cut back at 1 MiB.
- **The runner** listens on `127.0.0.1:8797` only, and needs a token
  (`runner/src/config.ts`). It starts the official `claude` and `codex`
  CLIs for your threads. Each thread's CLI gets a key of its own in its
  environment (`THREADBEAR_THREAD_KEY`), never the token. When something
  connects, the runner runs `lsof` and `ps` on that one connection to see
  which process it is and whether it's under a thread's CLI
  (`runner/src/peer.ts`, docs/protocol.md "Who's really calling").
- **With the `grok` experiment on** (off by default) it also starts xAI's
  `grok` CLI (`grok agent --no-leader stdio`, from `~/.local/bin/grok` or
  `grokPath`) for Grok threads, and while one runs serves that thread's
  threadbear tools to it over MCP on a free port on 127.0.0.1, at a path
  with a random secret that goes when the thread's CLI stops
  (`runner/src/mcp-http.ts`). With the `devin` experiment on (off by
  default) it starts the Devin CLI the same way (`devin acp`, from
  `~/.local/bin/devin` or `devinPath`) for Devin threads; Devin also loads
  the MCP servers in its own config.
- **With the `acpAgents` experiment on** (off by default) it also starts the
  `opencode acp` and Cursor's `agent acp` CLIs for threads on them, runs each
  with `--version` when the runner starts, and runs its own updater
  (`opencode upgrade`, `agent update`) only when you press Update in the app, and each
  live thread on one has a listener on `127.0.0.1` on a port the system picks,
  serving that thread's Threadbear tools, needing a random token made for it.
- **New installs start threads on Auto.** On Auto, Claude runs with its
  safety check approving or blocking each action, and Codex asks before
  edits. An install set up before 9 Nov 2026 keeps starting threads on
  full access (Claude with `bypassPermissions`, Codex with
  `danger-full-access`). `"defaults": { "permissionMode": ... }` in
  `runner.json` (`"full"`, `"auto"` or `"ask"`) sets it, and each thread
  can pick its own. The threads the runner starts by itself (bringing in
  Mike's releases and fixing a failed update to main, both below, and a
  note about this Mac) start on that default too
  (`runner/src/own-threads.ts`); on Ask, or Codex's Auto, what they can't
  do without a yes waits on a card for you.
- **Your phone and browser can drive the Mac.** A paired iPhone or a
  signed-in browser can start threads in any folder, send messages and
  answer approvals, through your Convex deployment (`runner/src/sync.ts`).
  Those threads start on your default mode like any other (full access on
  an older install), and can be switched to full access from there, so
  treat a pairing like a login to the Mac, and so is admin access to your
  Convex deployment.
  Pairings last 90 days unless set otherwise, at most 365
  (`convex/tokens.ts`); Revoke in Settings › Phone & browser ends one.
- **Following your main** (`runner/src/update.ts`, `app/Sources/Threadbear/AppUpdate.swift`):
  the runner and the app watch your local `main`, and look again every 5
  minutes. Within seconds of it changing, they rebuild themselves from it
  and redeploy your backend, so whatever reaches your `main` runs, with no
  signature check; only Mike's release tags are signed (below). Turn it
  off for both with `followMain: false` in `runner.json` (ask any thread
  to; it left Settings on 2 Nov). Around each step of the runner's build
  it reads macOS's sleep count (`pmset -g stats`, which changes nothing),
  so a step that failed while the Mac slept is tried again later (up to
  three times for the same commit) rather than taken as a broken main.
  When the runner can't build your main or deploy your backend, it starts
  a thread by itself, on your default mode with your own Claude or Codex
  login, in the release's checkout (`runner/src/fix-update.ts`). That
  agent reads the output, fixes the cause, lands the fix on your `main`,
  watches the next build and archives itself once it goes through, so you
  only hear about it when it needs you (including a step its mode asks
  you about). A later failure goes to the same thread.
  `followMain: false` turns this off with the rest.
- **Following GitHub on a second machine** (`runner/src/follow-origin.ts`):
  only with `followOrigin: true` in that machine's `runner.json`, which
  nothing sets for you. Every 5 minutes the runner runs `git fetch origin
  main` in its checkout (the only network call it adds, to whatever
  `origin` is), fast-forwards that checkout's `main` when it can, and builds
  and runs `origin/main`, with no signature check. It never resets or
  rewrites anything: a commit made only there stays, and `tb status` says so.
  `tb land` refuses in that repo. If `origin` is Mike's repo rather than
  your own, leave this off.
- **Schedule checks** (`runner/src/schedule-check.ts`, experiment
  `scheduleChecks`, off by default): a schedule's check command runs in
  `/bin/sh` before each run, as you, for at most a minute (or up to five,
  if the schedule says). Each keeps a
  state file (and a note that a held check was told about, `<id>.held`) in `schedule-checks/` in Threadbear's data folder, and
  beside it what the check last printed (`<id>.<hash>.kept.json`, readable
  only by you) until a run carrying it has been sent, and, while a check
  runs, which runner process is running it (`<id>.running.json`).
- **Keeping the Mac awake** (`runner/src/keep-awake.ts`, on unless
  Settings > General > Sleep or `"keepAwake": false` in `runner.json`
  turns it off): while a thread is working and for 3
  minutes after, the runner runs macOS's `/usr/bin/caffeinate -i -w
  <runner pid>`, which stops idle sleep only (the display still sleeps
  and the screen still locks). While it does, it reads `pmset -g batt`
  every 2 minutes and lets go on battery below 15%.
- **Mike's releases** (`runner/src/upstream.ts`): every 6 hours it lists
  `upstream`'s `release/*` tags, fetches the newest few into a folder of
  their own (`<home>/runner/upstream-fetch`, deleted after each check),
  checks each with `ssh-keygen -Y verify` against the pinned key (the
  signature goes in a temp file under `$TMPDIR` for a moment), fetches
  upstream's `main` with them to check the tag's commit is on it, and
  copies the newest that passes into your checkout, as
  `refs/threadbear/upstream/`. A release's `scripts/release-signers` and
  `scripts/release-keys-revoked` can change the pinned keys
  (`<home>/upstream-signers`, `<home>/upstream-revoked`, and what's waiting
  in `<home>/upstream-keys.json`, changed by one runner at a time under
  `<home>/upstream-keys.lock`): a new key after a week, a revoked one at
  once (`runner/src/release-keys.ts`). It never changes your code by
  itself. Every 5 days, when there's one your copy doesn't have, it starts
  one thread (on your own Claude or Codex login, on your default mode) that
  works out the merge without checking it out (`scripts/merge-preview.sh`,
  in a throwaway repo under `$TMPDIR` it deletes), tells you what it brings
  and asks; it runs nothing from the release (no checks, installs or builds)
  until you say go, then merges, checks and lands
  (`runner/src/bring-in.ts`). A release Mike marks important starts one
  straight away. Settings › General › Updates from Mike, or
  `"releases": { "bringIn": 14 }` (or `"important"`, or `"off"`) in
  `runner.json`, changes it. It keeps what it last did in
  `runner/bring-in.json` in the data folder.
- **CLI updates** (`runner/src/updates/`): every hour it checks for new
  Claude Code and Codex versions and installs them. If you don't point it
  at your own Codex, it installs one into the data folder. Once Grok
  Build or the Devin CLI is installed, it checks those too: Grok with
  `grok update --check --json` and `grok update`; Devin by reading
  `https://static.devin.ai/cli/current/manifest.json`, downloading the
  bundle it names with `curl`, checking its sha256, unpacking it into
  Devin's own `~/.local/share/devin/cli/_versions/<version>` and pointing
  `current` at it, as Devin's installer does (`devin update` needs a
  terminal). It keeps the previous Devin version and removes older ones
  nothing runs. Each CLI's Updates setting (Automatic, Ask me, Off) is in
  the Updates panel. It also runs
  any update commands you list under `cliUpdates.extra` in `runner.json`
  (none unless you add them), once a day each, and keeps when each last
  ran in `extra-updates.json` in the data folder.
  `THREADBEAR_CLI_UPDATES=off` stops all of this.
- **Small model calls** (`runner/src/models/`): thread titles, filing
  threads into projects (the librarian, every 5 minutes) and similar jobs
  send short excerpts of your threads to `claude -p` on your own logins.
  When a project's notes pass 12,000 characters, the librarian sends them
  the same way to be rewritten shorter, and keeps the old notes in the
  data folder's `project-notes/` (the last 5 for each project).
  Mike's publishing runner also sends commit messages to Opus through its
  Claude pool when preparing a daily release, to write the plain-language release notes.
  The input includes subjects and bodies of merged commits, including pull requests.
  If that call fails, the signed release still goes out without notes.
- **Housekeeping** (`runner/src/housekeeping.ts`): trims logs, deletes
  week-old copies of pictures pasted into drafts (a sent message keeps its
  own copy) and deleted threads after 30 days in the trash. Archived
  threads keep their pictures unless you set `"archivedMediaDays": <days>`
  in `runner.json`.
- **Moving threads** (the `moveToRunner` experiment, off by default;
  `runner/src/moves.ts`): watches your Convex deployment for threads you've
  moved to or from this machine. One moving away is packed (its folder,
  its Claude transcript, its worktree's git work), sealed to the other
  machine's key, uploaded in pieces and deleted from the deployment once
  it lands; this machine keeps its old copy in `moved/` in the data folder
  for a week. One arriving is unpacked in `moves/` first, and may make a
  worktree for it, write its transcript into a login's Claude folder, and
  `git clone` its repo from `origin` if this machine has no copy.
- **Presence** (`runner/src/presence.ts`): reads how long the keyboard and
  mouse have been idle (`ioreg`), so your phone knows whether you're at
  the Mac.
- **Dialogs on the screen** (`runner/src/mac-screen.ts`): every 2 minutes it
  runs `osascript` to count the windows of macOS's UserNotificationCenter, so
  it can tell you when a dialog has been left waiting. It sees how many, not
  what they say. `THREADBEAR_MAC_SCREEN=off` stops it.
- **Patches** (`.threadbear/patches/`): code from a folder runs only after
  you trust that folder (`runner/src/patch-trust.ts`). A folder's Claude
  Code hooks, other commands, `env` settings and (in Ask and Auto threads)
  permission rules in its `.claude/settings*.json`, its `.mcp.json`
  servers, and hooks in its `.claude/agents`, `skills` and `commands`, are
  held back the same way (`runner/src/claude-trust.ts`): the runner reads
  those files and the scripts they reach to hash them, runs `git ls-files`
  in the folder to find skills in folders under it, and keeps what you
  trusted in the same `patch-trust.json`. Beside it,
  `patch-trust-unvouched.json` lists worktrees a thread moved from another
  Mac changed the patches or Claude settings of, which Threadbear's own
  checkout's standing trust doesn't reach.
- **Menus on macOS 27.2** (`app/Sources/Threadbear/DragSpace.swift`): every
  5 seconds, and before a button opens a menu, the app asks the window
  server which Spaces its own windows are on, and takes them off the drag
  Space a window drag can leave them on, which hides their menus. A window
  filed under another display's Space than the one it's on (after a
  restart in the background) is put on its own display's. These are
  private window server calls, and they only touch the app's own windows.

Dictation (the mic beside Send) keeps this Mac's Collabute sign-in in
the login Keychain, as one item named "Threadbear Collabute dictation"
(the OAuth tokens Collabute gave it for `voice:transcribe` only), and the
server and the mic's switch in the app's preferences
(`app/Sources/Threadbear/Dictation/`). Settings > Dictation > Sign Out
deletes the item. The microphone is only on while the mic is red.


- From your Claude and Codex logins: which account each is (the email in
  `~/.claude.json`), a few settings, and the token counts in your session
  transcripts, so it can show how much room each login has
  (`runner/src/accounts.ts`, `runner/src/token-tally.ts`;
  `THREADBEAR_TOKEN_TALLY=off` stops the counting). With the `grok`
  experiment on, whether `~/.grok/auth.json` (or a `grok-N` folder's)
  exists, to list a Grok login; it's never opened. With the `devin`
  experiment on, whether `~/.local/share/devin/credentials.toml` exists,
  to list a Devin login; it's never opened either. It never reads or
  copies login tokens or the Keychain.
- With the `acpAgents` experiment on: whether `~/.opencode/bin/opencode`, an
  `opencode` or `cursor-agent` on your PATH, `~/.local/bin/cursor-agent` and
  `~/.local/bin/agent` exist and where they really lead, and whether
  `~/.cursor/cli-config.json` exists (never opened). OpenCode itself reads
  `~/.config/opencode` and, as it does anywhere, your `~/.claude/skills`; a
  folder's own OpenCode config and plugins are turned off.
- Token Stats' files, if you have that app, and any logins you've added
  under `accounts` in its data folder (`claude-1`, `codex-1`...).
- Your first name, from your Mac account's full name (`id -F`), so its
  agents know what to call you (`runner/src/user-name.ts`). It stays in
  the instructions your own threads get.
- Whether it has Full Disk Access, by opening macOS's permissions database
  (`~/Library/Application Support/com.apple.TCC/TCC.db`) and closing it
  again. It never reads it. Without the access macOS refuses, with no prompt.
- Whether `Threadbear.app` is in `/Applications` or `~/Applications`, as a Mac
  without it is probably one nobody sits at.
- The `.env*` files in the folders your threads work in (the folder, the
  folders directly inside it, and up to its repo's root; not `.env.example`
  and the like, your home folder or external drives), and everything in its
  own `secrets/` folder, to know what to take out of text before it goes to
  your Convex deployment (`runner/src/secret-values.ts`). Looked at when a
  folder first turns up, when a thread has just touched a file of secrets
  (including a `.env` in a folder of its choosing), and otherwise at most
  every 5 minutes. Kept in memory, never logged, written down or sent anywhere.

## Who it talks to

| Service | Whose | What goes there |
|---|---|---|
| Your Convex deployment | yours | Thread titles and status, all message text (each message cut at 8,000 characters), one line per tool call, projects, schedules, your logins' emails and usage, whether you're at the Mac, whether this machine is low on memory or disk, and the pictures you paste into a message, up to 10 MB each, so your own website can show them. Those go up as they are, in your deployment's file storage: unlike text, nothing can scrub a secret out of a picture, so a screenshot of something sensitive goes too. `"sync": { "pastedPictures": false }` in `runner.json` keeps them on this Mac. While someone has the website's Runners view open, this machine's CPU, memory and disk use, its processor model, operating system and time since it started, every 10 seconds; with nobody looking, none of that is sent. Photos you send from the phone, and pictures you send from the Mac app to a thread on another of your Macs, deleted once that Mac has fetched them. Pictures and videos agents put in their replies or pin, at most 10 MB each, for the phone to show. Other images only for threads you share by link. Other tool output and files stay on the Mac, unless you move a thread to another of your machines (the `moveToRunner` experiment): then the whole thread, sealed so only that machine can open it, goes through for as long as the move takes. Text has known secrets taken out first: see below. |
| Convex (the Convex panel) | yours | Only for a thread in a Convex project, when its Convex panel opens: the runner's Convex CLI asks api.convex.dev for the project's deployments and a deploy key, the runner asks the deployment whether a kept key still works (`/api/check_admin_key`), and the panel loads Convex's embedded dashboard (dashboard-embedded.convex.dev) with that key. Sign in to Convex goes to auth.convex.dev. (`runner/src/convex-keys.ts`) |
| Your other Macs, over ssh | yours | Only when you run `tb runner add <name> --ssh <host>` or `tb runner check --ssh <host>` on your first Mac: it reads that Mac's Threadbear `runner.json`, writes its id and the deployment's address there if it hasn't them, writes its new key into that Mac's `secrets/convex-key`, and looks whether that Mac has a deploy key, Convex panel keys or a Convex login (names only, never a key), through your own ssh setup with agent and port forwarding off (`runner/src/runner-keys.ts`). `tb runner audit` asks this Mac's runner for its threads' last seqs and your deployment for what the main key made. `tb runner rotate-owner` sets env vars on your deployment with your deploy key, and, if you give it a Convex team access token for that run (`CONVEX_TEAM_TOKEN`, never kept), asks api.convex.dev for the deployment's deploy keys (their names and dates) before it rotates. |
| GitHub | Mike's repo | Release tags fetched every 6 hours; the fork at setup |
| downloads.claude.ai, registry.npmjs.org | Anthropic, npm | CLI version checks and installs; `npm ci` |
| Anthropic, OpenAI | yours | Your threads, through the official CLIs on your own logins. With the `cardReviewer` experiment on (off by default), an approval card in a thread on Auto, with the thread's first and latest messages, goes to Opus through the same CLI so it can decide in your place. Unless you turn off Settings > General > Code review (`"labReview": false`), a thread's code change, with your message and the agent's reply, goes to your other provider's CLI as a review thread, as if you'd started one (or a fresh session of the same provider, with no other login). After a turn fails because the API couldn't be reached, the runner itself opens (and straight away closes) a connection to api.anthropic.com or chatgpt.com on port 443, at most once a minute, until one opens and the thread carries on. Nothing is sent over it (`runner/src/net-probe.ts`). |
| OpenCode's Zen (opencode.ai), models.dev, and whichever providers you've logged it into | theirs | Only with the `acpAgents` experiment on and a thread on OpenCode: that thread's prompts and tool output, through the official CLI, and small jobs' text when a pool of only OpenCode logins is picked for them in Settings |
| Cursor (api2.cursor.sh) | theirs | Only with the `acpAgents` experiment on and a thread on Cursor: the same, on your own login |
| Mike's push relay | Mike's | Only when a paired iPhone or Android phone gets a notification (an Android phone's goes on to Google's Firebase Cloud Messaging, from the relay). It's sent from your Convex deployment, not the Mac. The text is encrypted for the phone; the relay sees the device token and thread id (`convex/push.ts`, `docs/push-relay.md`). The first time, your deployment registers with it, giving its own address; the relay keeps that address and checks it by posting a one-time code back to your deployment's `/relay/verify` (`convex/http.ts`). |
| A Connectors gateway | whoever runs it | Only if one is set in `runner.json` or `~/.claude.json`. Its `/health` every few minutes (and once more at the end of a turn where a call to one of its services failed, to see if it needs signing in), and for a project with a Notion page, a Notion read or two every 5 minutes for its task board (`runner/src/notion-tasks.ts`), signed with the header the `headersHelper` command in that `connectors` entry prints, which it runs for each read |
| Collabute | Collabute's, or the server set in Settings > Dictation | Only once you sign in for dictation: the sign-in (Collabute's OAuth device flow, its approval page in your browser), a token refresh when the last one has run out, and for each dictation a short-lived speech-to-text token from Collabute (`/api/mcp/voice-token`), then your voice, while the mic is red, straight to the speech-to-text service that token is for (Vercel's AI Gateway), which sends back the words. Nothing else of yours is sent, and nothing is recorded or kept (`app/Sources/Threadbear/Dictation/`). |
| Sentry | Mike's | Nothing from your copy. Only a copy whose `origin` is Mike's own repo reports errors or uploads the app's debug symbols (`runner/src/sentry.ts`, `app/Sources/Threadbear/CrashReporting.swift`, `app/build-app.sh`). The App Store iPhone app sends anonymous crash reports. |

An HTML file opened in the side pane (a mockup or report an agent made,
a page in a project) talks to nothing. It loads through the app's own
`threadbear-preview:` scheme, which serves only that page and the
styles, pictures, fonts and media its markup names in its folder:
never another page, a script file, SVG, a dotfile, a symlink or a file
outside it. Its web view
blocks every other load (a WebKit content rule list, which WebKit keeps
in its own store under `~/Library/WebKit`), sends any connection it would
still make to a closed port on this Mac (127.0.0.1:9) so no name is
looked up, runs none of the page's scripts (unless you choose Run them
over that page, which turns them on for that version of the file only,
remembered by its SHA-256 in the app's settings), has WebRTC off, and doesn't
follow links away from it, submit
forms, open windows or download
(`app/Sources/Threadbear/Views/LocalPreviewSchemeHandler.swift`,
`app/Sources/ThreadbearCore/Support/LocalPreviewSite.swift`).

On the Mac itself, a web link you click in the app first goes to
BrowserBear's socket (`~/Library/Application Support/BrowserBear/browserbear.sock`),
if you have BrowserBear, to ask whether an agent's Chrome tab is already
on that page (`app/Sources/Threadbear/BrowserBear.swift`). Only the link
goes there.

The Browser panel also uses that private socket to list the pages agents
associate with threads, ask for a small still of each and bring a page's real
window to the front (`app/Sources/Threadbear/Views/AgentBrowserPanel.swift`).
Threadbear does not read Chrome's cookie database or copy credentials, and
stills stay on this Mac, in memory, not sent to Convex. BrowserBear stores
thread/tab/profile associations in `panel-tabs.json` in its own folder (0600).

The agents' own browser (`browserbear ... --profile agent`) is Helium where it's
installed, else Google Chrome, with its own profile folder; the panel reads
`agent/tabs.json` in BrowserBear's folder for its pages. Any program on this Mac
could reach its local debugging port, so it holds only what you sign it in to.

Nothing else: no analytics, no ads, no tracking.

## Secrets and your Convex deployment

Before any text goes to your deployment, the runner takes out what it can
recognise (`runner/src/scrub.ts`): shapes of keys and tokens
(`runner/src/redaction.ts`) and the actual values in `.env` files and its own
`secrets/` folder. Your deployment does the shape check again before storing it
(`convex/redaction.ts`), so an older runner can't put a known shape there. Your
own copy on the runner keeps every word. If the check itself fails, that text
isn't sent, and `tb status` says so.

**It isn't a guarantee.** It catches shapes and values it knows, so a secret in
an unfamiliar format can get through, and it can't read pictures at all: any
picture that goes up (ones agents post or pin, and anything in a thread you
share) goes as it is. Scrubbing also can't undo a leak. A key that shows up in
a thread has already been to the AI provider, so rotate it.

**Full sync** is off unless a runner's `runner.json` has `"sync": { "full": true }`.
With it off, tool output, diffs and tool screenshots stay on that runner, and
nothing below happens. With it on, from then on (older tools aren't sent), that
runner also sends each finished tool's output, its full input, its diffs and the
reasoning before it to your deployment, after the same scrubbing, where they're
kept 30 days. This lets you read them from any device while the runner's off. It's
the same not-a-guarantee as above, over much more text. A tool whose command or file
can hold secrets (`.env` files, `env list`, key files: `runner/src/withhold.ts`) sends
nothing of itself, and a runner without an on-device model relies on these checks
alone, which `tb status` says. A tool's screenshots stay on the runner unless you tap
one on the website, which uploads that one for a day.

`runner/scripts/scrub-stored.mjs` runs today's rules over what's already stored,
and only when you run it. Convex backups and snapshot exports keep their copies.

There's also a silent on-device check, off unless `runner.json` has
`secretCheck: { enabled: true }`, on a Mac with Apple Intelligence
(`runner/src/secret-check.ts`). It's an experiment: it builds a small helper the
first time (`<data>/tools/secret-check/`), and on mains power, at background
priority, it passes a sample of tool output and pictures to Apple's on-device
model, and Apple's on-device text recognition for a picture. Nothing leaves the
Mac and it changes nothing that uploads. It keeps counts, and the thread and
sequence number of what it flagged, in `<data>/secret-check.json`. Never the text.
