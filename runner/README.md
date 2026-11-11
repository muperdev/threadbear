# Threadbear runner

A small daemon that owns every agent process for Threadbear. It drives the
official Claude Code CLI through `@anthropic-ai/claude-agent-sdk`, keeps
threads on disk, turns SDK messages into the events in
[`docs/protocol.md`](../docs/protocol.md), and serves them to the Mac app
over a local WebSocket JSON-RPC API. Quitting or updating the app never
stops a turn.

TypeScript on Node 24, run directly (Node strips the types, so there's no
build step). Runtime packages: the Agent SDK (pinned to 0.3.310), `ws`, and
`zod` for the threadbear tools' input schemas.
No `node:sqlite`; it truncates text at NUL on Node 24, and plain files are
enough.

## Run it

```sh
cd runner
npm ci
npm start                      # listens on 127.0.0.1:8797, path /rpc
scripts/dev-runner.sh          # the same, on a temp data dir and a free port
```

As a LaunchAgent (`com.mikerosoft.threadbear.runner`, starts at login,
restarted if it exits):

```sh
scripts/install-runner.sh --print       # show the plist, change nothing
scripts/install-runner.sh               # install and start it
scripts/install-runner.sh --uninstall   # stop it and remove the plist
scripts/deploy-runner.sh                # ship this checkout's runner/ to it
```

The LaunchAgent runs a release (a snapshot, below), never the checkout, so
merging to main changes nothing until a deploy. `--from-source` installs the
old way, running `src/main.ts` from the checkout, for debugging (and moves
`launch.mjs` aside, so the deploy script won't ship to it).

`install-runner.sh` refuses before it changes anything when the checkout's
`launch.mjs` doesn't load (the deploy script imports it too), when HEAD isn't
main's tip (`--allow-branch`), or when a thread is busy, since replacing a
running copy stops it (`--force` doesn't wait). It keeps the release
`current` names only if it was built from HEAD, and otherwise builds one that
was. It keeps the old plist, `launch.mjs` and `current` until the new runner
has come up and stayed up long enough to be marked good (about a minute),
and puts all three back if launchd won't load the new plist (bootstrap gets
a few tries first) or the runner doesn't get that far. The plist
pins a Node 24 or newer: `--node` or `THREADBEAR_NODE` if you give one, else
the `node` on your PATH, else the newest Node 24 or newer nvm installed, or Homebrew's
(`scripts/find-node.sh`, which every script uses) and the PATH your login shell
has, so agents can run the same commands a terminal can. Pass `--path` to set
it yourself. Logs go to `~/Library/Logs/threadbear-runner.log`.

A sandbox install next to the real one takes its own `--home` and `--label`
(the script refuses a sandbox under the real label); its port defaults to 0,
a free one, and its log goes to `<home>/runner.log`.

## Releases and deploys

```
<home>/runner/
  launch.mjs          the LaunchAgent's program (copied in by install-runner.sh)
  releases/<id>/      src/, bin/, package files, production node_modules, release.json
  current -> releases/<id>
  state.json          { current, previous, lastGood, boots, rolledBack?, activated? }
```

A release id is `<UTC yyyymmddThhmmssZ>-<short commit>`, plus `-dirty` when
built from uncommitted files. `release.json` records the commit and the
branch. `deploy-runner.sh`:

0. refuses a LaunchAgent that doesn't run releases (for the real runner the
   one-time switch is Mike's, from a terminal), a HEAD that isn't main's tip
   (`--allow-branch`: a feature branch's work goes live unmerged, and the
   next deploy from main drops it), a release that is still waiting for its
   own restart (`--force` replaces it, from a terminal), and `--now` while
   other threads are busy (below),
1. runs the typecheck and unit tests on the pinned Node (`--skip-checks`),
2. snapshots `src/`, `bin/` and the package files at HEAD, refusing
   uncommitted changes to them (`--allow-dirty` builds from the files as they
   are), then installs production packages: cloned from an earlier release
   with the same `package-lock.json` (APFS copy-on-write, so seconds and next
   to no disk), else `npm ci --omit=dev`,
3. smoke tests it: starts it on a temp data dir and port 0, says hello, and
   checks the version names the new release,
4. points `current` at it,
5. asks the runner to restart once no thread is running (`runner.restart`;
   background shells and subagents hold it for `--wait`, 30 minutes, at
   most),
6. checks the runner comes back on the new release within 30 s and stays
   up (20 s, or until it's marked good): one that died a few seconds after
   it started listening used to pass. If it doesn't, it points `current`
   back at the release that was running, restarts, prints the end of the
   log and exits 1,
7. keeps the newest 5 releases plus the last good one (and anything
   current, previous or still running). Releases that crashed at startup
   go first and don't count towards the 5; the newest of them stays, for
   `--rollback --force`.

Deploying a commit whose release crashed at startup gets a note saying so
before anything is built. Inside a thread, if the runner says it won't
carry the thread on afterwards (it has been resumed 3 times this hour; a
crash rollback's resume replaces the crashed release's rather than counting
twice), the script says so and that Mike has to message the thread, rather
than promising the resume. For a sandbox `--home`, the `tb` commands it
prints carry `THREADBEAR_HOME=<home>`.

Inside a Threadbear thread the restart waits for that thread's turn to end,
so the script says so and exits: end the turn, and the runner restarts and
sends the thread "The runner restarted on release <id>. Check that your
change works, then carry on." (`--resume "text"` changes it). If the thread
has background shells or subagents of its own (a dev runner, say), the
script lists them: ending the turn doesn't stop them, so stop them first.
If the runner comes back on another release (it kept crashing, say), that
message starts by saying so. `--now` restarts at once (through
`runner.restart`, so the thread is still carried on). It is refused inside
a thread unless `--force`, and while any other thread has a turn,
background shells or subagents going it is refused from a thread even with
`--force` (Mike can run it from a terminal with `--force`). None of the
messages an agent sees offer `--force`. A runner from before
`runner.restart` is polled until idle (`--wait 30m`), then kickstarted;
inside a thread the script prints the command for Mike instead. A runner
that is listening but doesn't answer in time is never kickstarted: the
script fails and prints the command. If the health check fails and the
runner is up on some other release, it is asked to restart onto the one it
should be on once no thread is busy, rather than kickstarted.

Outside a thread the script waits for the restart (up to `--wait`). If it is
withdrawn meanwhile (`tb restart --cancel`), the script says what `current`
points at and exits 1.

The script counts as inside a thread when `THREADBEAR_THREAD_ID` is set and
`THREADBEAR_RUNNER_HOME` (which the runner sets, and agents don't change,
unlike `THREADBEAR_HOME`) names the data dir it deploys to, or when that
runner's pid (`run/runner.pid`) is one of its ancestors, which also catches
runners too old to set the variables. The real runner's label only goes
with its own data dir, and the other way round.

```sh
scripts/deploy-runner.sh --list       # releases, branches, which is current, previous, last good, running
scripts/deploy-runner.sh --rollback   # back to the previous release
scripts/deploy-runner.sh --dry-run    # what it would do
```

`--rollback` won't go back to a release that crashed at startup (after an
automatic rollback, `previous` names it; `--force` tries it again, with
fresh tries: `state.json` records when each release was `activated`, and
`launch.mjs` only counts crashes since then). Its default resume says the
change is no longer live. When the release it goes back to is the one
running (`current` named one that never started), it only moves the link.
`--list` marks a current release the runner isn't on yet as "not running
yet".

`launch.mjs` records every start in `state.json`. When a release starts 3
times in 10 minutes without reaching 60 s of listening (when `main.ts` marks
it `lastGood`), the third start runs `lastGood` instead, records
`rolledBack`, and the runner warns every client about it. If it can't move
the `current` link (a full disk), it still starts `lastGood` from its
folder. A start that
listened and then exited 0 (a planned restart, `launchctl kickstart`, a
reboot) is marked `clean` and doesn't count, and a start that isn't
listening after 90 s is stopped and does, so a release that hangs at
startup is rolled back too. The runner adds `rolledBack.reportedAt` once it
has reported it, so only the first start after a rollback does; `tb status`
(`runner.status`'s `rolledBack`) says so for as long as that release runs.
When a release exits uncleanly, `launch.mjs` logs which, with its exit code
and how long it ran, since a release that calls `process.exit` leaves
nothing else in the log. A change to
`launch.mjs` itself only reaches the LaunchAgent through
`install-runner.sh`; the deploy script says when the installed copy is out
of date.

## Restarts

`runner.restart` (`tb restart`, and the deploy script's restart step)
restarts the runner without cutting a turn short; `docs/protocol.md` has
the details.

- It waits until no thread is starting, running or waiting on an approval
  or a question, and no CLI is running background shells or subagents
  (`tb status` lists both), then stops the way it does on SIGTERM and exits
  0, and launchd starts it again. Background work only holds it for
  `backgroundWaitMs` when that's given (the deploy script's `--wait`,
  `tb restart --max-wait`, both 30 minutes by default), since a dev server
  never finishes; turns always do. When background work ends during a
  turn, the thread stays busy for up to 15 s after it, in case the CLI
  starts a turn of its own to read the result. `--now` (`when: "now"`)
  doesn't wait; any background work it stops gets a note in its thread.
  From the moment it begins, new sends are refused with "send again in a
  moment".
- The request is saved in `restart.json` in the data dir until the next
  start handles it, so a crash meanwhile doesn't lose it. `tb restart
  --cancel` withdraws one that hasn't begun. If it was a deploy's, `current`
  goes back to the release that is running, so the withdrawn one doesn't
  start on the next restart of any kind.
- A thread that asked to carry on gets a note ("Runner restarted: <old> to
  <new>") and its resume text as a new turn from the runner, if the old
  runner stopped less than 15 minutes ago, the thread isn't snoozed or
  archived, nobody has written in it since it asked, and it hasn't been
  resumed 3 times in the last hour (`run/resumes.json` keeps count).
  Otherwise it only gets the note, which says why.
- If it asked for a release (the deploy script does) and the runner came
  back on another, the note and the message both say so.
- The handled request stays as `restart.done.json` until the new runner is
  marked good or stops cleanly. The same release starting again leaves it
  alone (`launch.mjs` gives a broken release two starts), so when
  `launch.mjs` then goes back to the last good one, the thread is carried on
  once more there.
- Snoozed and archived threads keep their queued messages through a
  restart and send them when they come back, even when the snooze ended
  while the runner was down.

## A dev runner

`scripts/dev-runner.sh [--home DIR] [--port N] [--keep]` runs the working
tree's runner in the foreground on a temp data dir (it refuses the real one)
and prints what to point `tb` and the app at:

```
  pid:  41234
For tb:      /tmp/threadbear-dev.Ab12Cd/tb status
For the app: ~/dev/me/threadbear/app/Tools/dev-app.sh start --home /tmp/threadbear-dev.Ab12Cd
Stop it:     kill 41234   (never pkill: the real runner runs src/main.ts too)
```

`<home>/tb` is `bin/tb` with that data dir built in, since an exported
`THREADBEAR_HOME` lasts one command in an agent's shell and a bare `tb`
then reaches the real runner. It also sets `THREADBEAR_TB` to itself, so
the commands tb suggests (`restart --cancel`, `events`) name it too. A temp
data dir the script made is deleted when it stops (Ctrl-C or `kill`;
`--keep` keeps it), and kept, with a note saying where, if the runner
failed. Like launchd, the script starts the runner
again when it exits 0 (`tb restart`), and stops when it exits with an
error. It uses your real logins, so threads in it cost like any other. On a
new data dir it writes a `runner.json` whose threads default to the folder
`<home>/work`, account `claude-main` (or `TB_E2E_ACCOUNT`) and model `claude-haiku-5-5`, so a thread
a debug app's composer starts is a cheap one too (an account's own default
can be Fable with a 1M context), and it prints a `tb new` line. Ctrl-C or the `kill` stops it and the CLIs it
started. Run in the background from a thread, it is background work that
holds the live runner's restart, so stop it before a deploy.

## Configuration

`runner.json` in the data dir:

```json
{
  "port": 8797,
  "defaults": { "cwd": "~/ThreadbearThreads", "accountId": "claude-main", "model": "claude-haiku-5-5" },
  "claudePath": "~/.local/bin/claude",
  "idleMinutes": 30,
  "maxLiveCLIs": 6,
  "followMain": true,
  "models": { "pool": "claude", "privateFolders": ["~/dev/client-work"] }
}
```

The runner writes `port` (the one it is actually listening on, so the app
can find it) and `defaults.cwd` on start, and `defaults.accountId` whenever
a thread is created on, or moved to, an account, so new threads start on
the last one used. `defaults.model` is optional: a new thread that names no
model gets it, rather than the account's default (`hello` reports it). The
installed runner leaves it out; `scripts/dev-runner.sh` sets it.
`followMain` (default true) lets a runner that runs from a release build
and restart onto main by itself (`docs/dev-tools.md`, "The runner updating
itself"); `THREADBEAR_FOLLOW_MAIN=0` does the same for one run.
`models` is for the runner's own small-model calls
(`src/models/`, docs/protocol.md "Settings and models"): `pool`, the pool
whose Claude logins answer them through the CLI (default `"claude"`,
`null` for none, which turns titles and the librarian off), and folders
whose threads are never sent. The app's Settings window and `tb settings`
show and change both.

Environment variables, mainly for tests:

| Variable | What it does |
|---|---|
| `THREADBEAR_HOME` | Data dir (default `~/Library/Application Support/Threadbear`). `THREADBEAR_DATA_DIR`, the app's name for it, works too. |
| `THREADBEAR_PORT` | Port to listen on. `0` picks a free one; it's written to `runner.json`. |
| `THREADBEAR_LOG` | Log file (default `~/Library/Logs/threadbear-runner.log`). |
| `THREADBEAR_IDLE_MS` | Idle release delay in ms (default 30 minutes). |
| `THREADBEAR_MAX_LIVE` | Most CLIs kept live at once (default 6, or `maxLiveCLIs` in `runner.json`). |
| `THREADBEAR_CLAUDE_PATH` | The `claude` binary (default `~/.local/bin/claude`, so the CLI keeps updating itself). |
| `THREADBEAR_ACCOUNTS_DIR` | Where `claude-N` logins live (default Token Stats' `Accounts` folder). |
| `THREADBEAR_CLAUDE_MAIN_DIR`, `THREADBEAR_CLAUDE_MAIN_JSON` | The main login (default `~/.claude` and `~/.claude.json`). |
| `THREADBEAR_DEBUG=1` | Log CLI stderr and other detail. |
| `THREADBEAR_LOG_STDERR=1` | Also print the log to stderr. |
| `THREADBEAR_NODE` | The Node binary the scripts use. The runner sets it in every thread to the Node it runs on. Unset, they take the `node` on PATH, else nvm's newest Node 24 or newer, else Homebrew's (`scripts/find-node.sh`). |
| `THREADBEAR_GOOD_AFTER_MS` | How long a release has to stay up before it is marked `lastGood` (default 60 s; the tests shorten it). |
| `THREADBEAR_START_TIMEOUT_MS` | How long `launch.mjs` gives a release to start listening before stopping it (default 90 s). |
| `THREADBEAR_LAUNCHCTL`, `THREADBEAR_LAUNCH_AGENTS_DIR` | Stand-ins for `launchctl` and `~/Library/LaunchAgents` in the install and deploy scripts (the deploy tests use both). |

What the runner gives the CLIs it starts is under Tools for agents below.

## Data layout

```
~/Library/Application Support/Threadbear/
  runner.json
  runner/                       releases, launch.mjs and state.json (above)
  restart.json                  a restart that's waiting for threads to finish
  restart.done.json             the last one handled, until this release is marked good
  secrets/runner-token          random RPC token, 0600 (folder 0700); clients prove they have it, never send it
  models/usage.ndjson           one line per model call: feature, model, login, tokens (no text)
  inbox/                        images the app imports before sending
  run/children.json             CLI process groups this runner started, and the
                                groups their tool commands ran in
  run/runner.pid                the lock: pid and start time of the runner that
                                owns this data dir
  run/resumes.json              when each thread was last resumed after a restart
  trash/<threadId>-<time>/      deleted threads, kept until removed by hand
  threads/<threadId>/
    thread.json                 summary plus internal state, written atomically
    events.ndjson               persisted events, line N is seq N
    media/                      images sent with messages or returned by tools
    blobs/                      full tool output when it's over 64 KB
```

`events.ndjson` is append-only. On load the runner indexes line offsets so
a page of a long thread is one read; a half-written last line from a crash
is dropped so `seq` stays gapless. A `thread.json` that can't be read is
moved aside (`thread.json.unreadable-<time>`) and rebuilt from the event log:
title, preview and seqs come back, while the folder, account and permission
mode reset to the defaults and `ask`, and the next message starts a new
session with a handoff note. Temp files from interrupted atomic writes are
removed at startup.

## The tb CLI

`bin/tb` talks to a running runner. It reads `THREADBEAR_HOME` and the port
from `runner.json` like the runner does (`THREADBEAR_PORT`, unless it's 0,
or `THREADBEAR_URL` override it). Thread ids can be shortened to a unique
prefix.

```sh
bin/tb new "Fix the flaky test" [--cwd DIR] [--account claude-3] [--model M] [--ask] [--image FILE] [--wait]
bin/tb send <id> "and commit it" [--image FILE] [--wait]
bin/tb list [--snoozed | --archived | --all] [--json]
bin/tb search "words"
bin/tb tail <id> [--from SEQ] [--json] [--until-done]
bin/tb events <id> [--from SEQ] [--to SEQ] [--json]   # each line starts with its seq
bin/tb interrupt <id>
bin/tb approve <id> <approvalId> [allow|allowAlways|deny]   # the id's start will do; never a thread's own
bin/tb answer <id> <approvalId> "Tea"
bin/tb snooze <id> 2h          # or an ISO time
bin/tb archive <id>            bin/tb unarchive <id>
bin/tb move <id> <folder>      # from the next message
bin/tb delete <id>             # to trash/
bin/tb accounts
bin/tb settings                # what the Settings window shows
bin/tb models try summarize "some text"   # one call by hand; also classify --labels a,b, extract --field name=desc, generate
```

For working on Threadbear itself:

```sh
bin/tb status [--json]          # version, live CLIs, busy threads, background work, a pending restart
bin/tb self [--json]            # the thread this runs in (THREADBEAR_THREAD_ID)
bin/tb restart [--now [--force]] [--resume "text" | --no-resume] [--reason "text"] [--max-wait 30m]
bin/tb restart --cancel         # withdraw a pending restart (--force: someone else's, from a terminal)
bin/tb show <id> [--last N] [--tools]   # the thread as plain text, approvals with their ids
bin/tb logs [-f] [-n 200] [--thread ID]
bin/tb export <id> [--out DIR]  # a mock-runner fixture in app/.fixtures/threads/<id>
bin/tb rpc <method> ['{"json": "params"}']
```

`tb restart` is for dev runners and sandboxes. `--force` is Mike's, from a
terminal: `tb --help` and the flag errors never mention it. It refuses the live runner
(the default data dir) from that runner's own threads (the deploy script
ships and restarts it), and a live runner run from a checkout from anywhere,
since that would come back on whatever is in the checkout, untested; from a
thread that holds even with `--force`. `tb rpc runner.restart` is always
refused in favour of `tb restart`, which has those guards. From a thread
of the runner it would cancel, `tb restart --cancel` only withdraws that
thread's own restart: anyone else's is almost always a deploy waiting for
this very turn, and withdrawing it would undo the deploy and leave its
thread waiting (`--force`, from a terminal, does it anyway). When the
runner says it won't send the resume (3 this hour), `tb restart` says so
instead of promising it. Inside a thread, `tb restart` carries that thread on
afterwards by default and says to end the turn, since the restart waits for
it, and lists any background work of the thread's own that would outlast
the turn. `--now` is refused inside a thread, and while other threads are
busy. `tb status` marks the calling thread. Against a runner from before
`runner.status` it shows what `hello` and `threads.list` give. `tb self`
says so when the thread is on another runner than the one tb talks to.
`tb show` puts each call's diff, approval and result straight under it, so
parallel calls' results don't land under the wrong call; `--tools` prints
Bash commands whole. `tb events --from --to` prints a seq range (the
"events: 5, 8-9" in the app's Copy debug info, which also gives the
command). Inside a thread, `tb approve` and `tb answer` refuse that
thread's own cards, as `tb rpc approvals.respond` does: they're Mike's,
and `tb rpc threads.update` won't snooze or archive the thread it runs in,
as `tb snooze` and `tb archive` won't. Each
command refuses flags it doesn't read (`tb events --last` used to be
ignored), naming the ones it takes (never `--force`), and `tb <command> --help` shows just that command. `tb logs` says which file it
reads; with the runner down it reads `<home>/runner.log` for a dev runner or
sandbox, and `--thread` includes the CLI's spawn and exit lines. `tb export`
writes the thread as idle (a mid-turn export would otherwise look stuck)
and prints the `app/Tools/dev-app.sh start --mock --fixtures` command for
it (a debug app kept behind Mike's windows, with its own mock runner, which
an agent can dump, screenshot and stop) and the `tb events` one; the mock no longer loads
`app/.fixtures/threads/` by default, so exports never reach design
screenshots. A mistyped flag gets a message, not a stack trace. Messages
print `tb` with its full path, and the one that was run: a dev runner's
`<home>/tb`, or `THREADBEAR_HOME=<home>` in front when it was pointed away
from the thread's own runner.

## Tools for agents

Every CLI the runner starts knows which thread it's in, two ways, and both
name the same thread:

- **The `threadbear` MCP server.** Each Claude session gets its own,
  in-process (`src/threadbear-mcp.ts`) and built with its thread's id:
  `list_threads`, `read_thread`, `list_accounts`, `create_thread`,
  `send_message`, `update_thread`, `move_thread`, `interrupt_thread` and
  `delete_thread`, which call the runner directly. Messages an agent sends
  start with a line naming the thread that sent them. `move_thread` with
  `then` queues that text as the thread's next message, so it carries on
  by itself in the new folder. `delete_thread` asks Mike every time, even
  with full access, offers no "Always allow", and its card names the
  thread.
- **The environment.** `THREADBEAR_THREAD_ID`, `THREADBEAR_HOME`,
  `THREADBEAR_RUNNER_HOME` and `THREADBEAR_RUNNER_VERSION` (what each is
  for is in `docs/protocol.md`), with the runner's own `bin/` at the front
  of `PATH`, so `tb self`, `tb restart` and the deploy script know which
  thread and runner they're in.

Neither the tools nor `tb` will delete, interrupt, snooze or archive the
thread they're called from, since that would end the turn asking. There's
no tool for restarting the runner: from a thread, runner changes go out
through the deploy script (Releases and deploys, above).

## How it behaves

- **Accounts.** `~/.claude` is `claude-main`; each Token Stats
  `Accounts/claude-N` folder is `claude-N`. The title is
  `oauthAccount.emailAddress` from that login's `.claude.json`, and nothing
  else is read from it. Tokens stay in the Keychain. The CLI gets
  `CLAUDE_CONFIG_DIR` for `claude-N` logins and has it removed for
  `claude-main`.
- **One runner per data dir.** `run/runner.pid` is taken with an exclusive
  create before anything else happens. A second runner (say `npm start`
  while the LaunchAgent runs) exits with "Another runner (pid N) is already
  using ..." without sweeping or recovering anything. A lock whose owner has
  exited, or whose pid now belongs to a process that started at another
  time, is taken over. A port that's in use is reported the same way.
- **One `query()` per live thread** in streaming-input mode. New threads get
  a pre-assigned session id; later starts use `resume`. Two live CLIs never
  share a session: a new one waits for a releasing one to exit.
- **Idle release.** 30 minutes after a turn ends with nothing pending, or
  straight away on snooze or archive, the runner ends the CLI's input so it
  exits. The next message resumes the session and adds a "Resumed session"
  note. Snoozing or archiving mid-turn interrupts the turn first. A CLI
  with background work (a `run_in_background` shell, an async subagent) is
  not released at the idle time; the countdown starts when the work ends.
  Snoozing, archiving or moving a thread with background work stops the CLI
  at once rather than waiting for it, and a system note lists what was
  stopped.
- **Live CLI cap.** Each live CLI is a `claude` process plus its own copy of
  the account's MCP servers, about 400 MB on these logins. When starting
  one would pass `maxLiveCLIs` (default 6), the one idle the longest is
  released first. CLIs with a turn, an approval, queued messages or
  background work are never picked; if every one is busy the cap is passed
  rather than stopping work.
- **Turns the CLI starts by itself.** When a background shell or async
  subagent finishes, the CLI runs a turn on its own. The runner gives it a
  turn in the thread: `turn.started` with no `user.message`, a system note
  with the CLI's summary of what finished, the output, then
  `turn.completed`, so the thread shows running and `needsYou` is raised
  when it ends. A message sent meanwhile is queued behind it. Each message
  the runner sends carries its turnId as the SDK message uuid; the CLI's
  `command_lifecycle` frames then say which turn answers which message, so
  a CLI turn that started just before a send never ends that send's turn.
  A subagent that asks for approval after the turn that started it has
  ended gets a turn the same way, and the card is shown.
- **Permission modes.** `full` runs the CLI with `bypassPermissions`. `ask`
  uses `default` mode and turns `canUseTool` into `approval.requested`; for
  Edit, MultiEdit and Write the `edit.diff` comes first, computed from the
  file as it is. `AskUserQuestion` always becomes `question.requested`, in
  both modes. Switching mode or model mid-session goes to the live CLI.
  A subagent's tool requests have no `itemId` (its tool calls aren't in the
  thread) and no diff.
- **Always allow** applies to the thread. The CLI's suggested allow rules
  (for example `Bash(touch second.txt)`) are applied to the live session
  only, kept in `thread.json`, and passed back as `allowedTools` whenever
  the thread's CLI starts again, so they outlast idle release and restarts.
  Suggestions that would switch the session's permission mode (to
  `acceptEdits`) or save rules in the project's `.claude/settings.local.json`
  are dropped; when only a mode switch was offered, the tool itself is
  allowed for the thread.
- **Interrupt.** If the CLI hasn't ended an interrupted turn after 10
  seconds, the runner ends it as `interrupted` anyway.
- **Queued sends.** A message sent during a turn is queued and starts when
  the turn ends. A snoozed or archived thread keeps its queue until it's back.
- **Folder or account change.** Takes effect on the next turn. A folder
  move on the same account copies the session's transcript (and its
  subagents' folder) into the new folder's project dir in the account and
  resumes it there, so the agent keeps the whole conversation. A new
  account, a rebuilt `thread.json`, or a copy or resume that fails starts a
  fresh native session instead, whose first message carries a short handoff
  note with recent messages. Either way the transcript shows only what was
  typed, plus a system note.
- **Delete.** Stops the thread's CLI, moves its folder to `trash/` and
  sends `thread.deleted`; whatever the old CLI still reports is dropped.
- **Deltas** are coalesced to at most about 30 a second per thread, carry the
  seq of the last persisted event, and are never persisted.
  `threads.events` and `threads.subscribe` return `inProgress` with the text
  streamed so far; a client takes the subscribe reply's as the base for the
  deltas that follow, since deltas sent before it subscribed never reach it.
  `tool.output.delta` is never sent for Claude: the SDK reports nothing from
  a running command but elapsed time (`tool_progress`).
- **Images.** Images sent with a message are copied into the thread's
  `media/` (so cleaning `inbox/` never breaks a transcript) and sent to the
  model as base64; ones over about 3.7 MB or 7900 px are shrunk with `sips`
  first. Images in tool results are saved to `media/` with their pixel size.
  Only regular files up to 100 MB are accepted, so a named pipe can't block
  the runner.
- **Big tool output.** Over 64 KB, `tool.result` text is cut (at a
  character boundary) and the whole text saved to `blobs/`. The CLI itself
  saves Bash output over about 30 KB to a file in the account's folder and
  gives the model a 2 KB preview; the runner copies that file into `blobs/`
  and points `blobPath` at the copy.
- **Stopping and startup recovery.** When the runner stops cleanly, turns in
  flight end there and then: streamed text is kept, pending approvals
  expire, and the turn ends `interrupted` with the thread `stopped`. If it
  was killed instead, the next start does the same for threads it finds
  mid-turn (unless the turn's `turn.completed` reached the log before the
  crash, when only the status is put right). Either way, queued messages
  that never ran are kept in a system note, and the next message resumes
  the session.
- **Notices.** `runner.notice` goes to every client. Notices from startup
  (threads stopped mid-turn, leftover processes stopped) are also sent to
  any client that says `hello` in the next 10 minutes, and a missing
  `claude` binary is reported after every `hello`.
- **Clients.** A socket that hasn't authenticated within 5 seconds, or
  sends a binary frame first, is closed with 4401. A client that stops
  reading has deltas skipped once 1 MB is waiting for it, and is closed
  with 4408 past 32 MB; it reconnects and replays from its last seq.
- **Process groups.** Each CLI is spawned in its own process group and
  recorded in `run/children.json` with its start time. The CLI runs each
  Bash command in a group of its own, so every 5 seconds while CLIs are
  live the runner also records the groups their descendants are in. (Tagging
  them through the environment doesn't work: `ps -E` shows no environment
  for other processes on this macOS.) On SIGTERM, SIGINT or SIGHUP the
  runner stops every group (SIGTERM, then SIGKILL after 3 s). A CLI that
  exits with anything but 0 has the groups its tool commands ran in stopped
  too. If the runner is killed outright, the next start stops the groups it
  left and waits until they're gone before it loads threads or listens, so
  a resume never runs beside the old CLI. A group is only touched if its
  leader started at the recorded time, or its leader has exited and the
  group lives on, so a recycled pid (a terminal's `claude`, say) is never
  hit.
- **Disk errors.** A send whose events can't be written fails with the
  error and leaves the thread as it was. A queued message that can't start
  stays at the front of the queue, and output that can't be saved is
  reported as a `runner.notice` rather than looking like a CLI crash.
- **needsYou** is true while waiting on an approval or question, after a
  turn finishes past `lastSeenSeq`, and when a snooze ends.

## Code map

| File | What it holds |
|---|---|
| `launch.mjs` | The LaunchAgent's program: records boots, rolls a crash-looping release back to `lastGood`, imports the release's `src/main.ts`. |
| `src/main.ts` | Entry point: config, logger, orphan sweep, runner, server, signals. |
| `src/release.ts`, `src/restart.ts` | The release this runner is, `lastGood` and rollback notices; `runner.restart`, `runner.status` and the resume after a restart. |
| `src/runner.ts` | The Runner: startup, threads, turns, the queue, the CLI's output, update and delete, events, summaries. |
| `src/runner/*.ts` | The rest of the Runner's methods, one area a file (pools, worktrees, patches, approvals, reviews, handovers, hibernation and so on). Each is an object of methods whose `this` is the Runner; runner.ts merges their types into the class and puts them on its prototype. A new method for one of those areas goes in its file. |
| `src/rpc.ts` | WebSocket JSON-RPC server, auth, subscriptions. |
| `src/adapters/types.ts` | The adapter interface. Codex will be a second implementation. |
| `src/adapters/claude/adapter.ts` | Claude Agent SDK session: options, input queue, `canUseTool`, interrupt, release. |
| `src/adapters/claude/normalize.ts` | SDK messages to protocol events. Pure; tested with recorded sequences. |
| `src/tools.ts`, `src/diff.ts` | Tool kinds and titles, input truncation, edit diffs, plan tracking. |
| `src/threadbear-mcp.ts`, `src/transcripts.ts` | The threadbear tools each session gets; copying a session's transcript when its thread moves folder. |
| `src/store.ts` | `thread.json` and `events.ndjson`. |
| `src/proc.ts` | Process groups, tool command groups and orphan cleanup. |
| `src/lock.ts` | The one-runner-per-data-dir lock. |
| `src/accounts.ts`, `src/media.ts`, `src/config.ts` | Login discovery, image handling, config and data dir. |
| `src/cli.ts`, `src/tb-dev.ts`, `src/client.ts` | `tb` (its working-on-Threadbear commands in `tb-dev.ts`) and the small client it and the tests use. |
| `scripts/deploy-runner.sh`, `scripts/deploy.mjs` | Building, smoke testing, switching to and pruning releases. `scripts/rpc-client.mjs` is their dependency-free client, and `scripts/deploy-steps.mjs` the typecheck and unit test steps a build runs. |
| `scripts/install-runner.sh`, `scripts/dev-runner.sh` | The LaunchAgent, and a foreground runner on a temp data dir. |

## Tests

```sh
npm test               # unit tests, no network, about 5 seconds
npm run typecheck
npm run test:deploy    # the scripts and launch.mjs, about two minutes
npm run test:e2e       # real Claude CLI, about 2.5 minutes
../check.sh            # all of the above but e2e, plus the app's build and tests (about 2.5 minutes)
```

The deploy tests build releases of a throwaway git repo whose runner is
`test/deploy/fake-runner.mjs` (no packages, so no `npm` downloads), with
`test/deploy/fake-launchctl.mjs` standing in for launchd: it reads the plist
`install-runner.sh` wrote and keeps its program running. They cover a first
install, deploys through `runner.restart`, a deploy from inside a thread,
the real label refused for a sandbox, a thread spotted from the process
tree, `--now --force` through `runner.restart`, `--now` refused while
other threads are busy, a thread's own background work, a deploy while
another's restart is pending, a withdrawn restart and `--rollback` onto the
running release, a runner too slow to answer (no kickstart), the main
branch check, an older runner (wait for idle, then kickstart),
`--rollback` (and its refusal after a crash rollback), a smoke test
failure, uncommitted changes, a release that crash-loops back to
`lastGood` (also when the link can't be moved), one that dies a second
after it starts listening, a resume the runner won't send, pruning (crashed
releases first), `--list`,
`--dry-run`, LaunchAgents that don't run releases, the install guards, a
reinstall (busy, a branch, a `launch.mjs` that doesn't load, a stale
release, a bootstrap that fails, a `launch.mjs` that can't start a runner)
and uninstalling.

The unit tests cover normalisation (recorded SDK sequences in
`test/fixtures/`), persistence, seq and replay, idle release and snooze with
a fake clock, approvals, queueing, handoff, recovery, CLI-started turns,
background work, the live CLI cap, disk errors, the RPC server (auth
timeout, backpressure), process groups and the lock (with real processes),
two runners starting on one data dir, restarts (background work, its time
limit and work ending mid-turn, held queues, resumes on the wrong release,
a release that crashes twice before the rollback and its resume counted
once, cancelling and putting `current` back, a deleted thread it was
waiting for, a resume it says it won't send), planned restarts
under `launch.mjs` and its startup watchdog, the threadbear tools (moves
that carry the transcript and their fallbacks, delete, and the refusals
for the calling thread), and `tb` (including its guards on the live runner
and on the thread it runs in).
`test/helpers/protocol-check.ts` checks events and summaries against
`docs/protocol.md` and is shared by both suites.

The end-to-end suite starts the runner on a free port with a temp data dir
and drives account `claude-main` (`TB_E2E_ACCOUNT`) on `claude-haiku-5-5` in a temp folder:
streaming, a Read tool call, resume after idle release, interrupt, an
ask-mode Write approval, AskUserQuestion, an image in and an image out, a
folder change handoff, `tb`, archive, a background shell whose finish
starts a turn (with a send queued behind it), snoozing with a background
job, "Always allow" across a resume, an edit to a file with no final
newline plus big Bash output, a background subagent's approval, a SIGKILLed
runner and a clean stop. `test/e2e/tools.e2e.test.ts` has an agent move
its own thread with `move_thread` and carry on there with its memory, then
start and delete another thread.
`TB_E2E_ACCOUNT` and `TB_E2E_MODEL` change the account and model;
`TB_E2E_KEEP=1` keeps the temp folder for a look afterwards.

To record a new fixture, run a throwaway `query()` script that writes every
SDK message as a line to a file in `.fixtures/` (gitignored; raw recordings
are private), then sanitise it:

```sh
node scripts/sanitize-recording.ts .fixtures/rec.ndjson test/fixtures/name.ndjson /abs/folder/it/ran/in
```

## SDK quirks we hit (0.3.310, CLI 2.1.310)

- **`canUseTool` in bypass mode.** The SDK warns
  `CLAUDE_SDK_CAN_USE_TOOL_SHADOWED` ("canUseTool will not be invoked") when
  both are set, but `AskUserQuestion` still goes through `canUseTool`. The
  runner relies on that and hides the warning.
- **`canUseTool` can beat the tool call.** It is sometimes called before the
  assistant frame carrying that `tool_use` comes out of the message
  iterator. The adapter waits up to 2 s for the frame so events stay in the
  order `tool.call`, `edit.diff`, `approval.requested`.
- **One assistant frame per content block.** With partial messages on, the
  CLI sends a separate `assistant` message for each finished block, all with
  the same message id. The Nth frame for an id is stream index N, which is
  how item ids (`<messageId>:<index>`) line up with deltas.
- **Late deltas.** A complete frame can arrive before its last
  `content_block_delta`; deltas for a finished item are dropped.
- **Interrupt.** `query.interrupt()` returns `{ still_queued: [] }`. The turn
  ends with a `result` of subtype `error_during_execution`, `is_error: true`,
  `terminal_reason: "aborted_streaming"` (or `"aborted_tools"`), an `errors`
  list of `[ede_diagnostic] ...` lines, and no complete frame for the text
  that was cut off. The runner keeps the streamed text and reports
  `interrupted`, not `failed`. Before the CLI has started, `interrupt()`
  throws; the runner kills the CLI instead.
- **Cost is a running total.** `total_cost_usd` counts the whole native
  session and carries on across `resume`, so a turn's cost is the
  difference from the previous result.
- **Resume needs a transcript.** If a CLI dies before writing its first
  message, `resume` fails with "No conversation found". The runner starts a
  fresh session once and resends the message.
- **Inherited session variables.** Started from inside a Claude Code session
  (a terminal in T3, these tests), the CLI would see `CLAUDECODE`,
  `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_SESSION_ID` and friends and think
  it was nested. The adapter removes them.
- **Mode switching.** `setPermissionMode('bypassPermissions')` only works if
  the session started with `allowDangerouslySkipPermissions`, so every
  session sets it, which lets "Ask before edits" be turned off mid-thread.
- **Message origin.** User messages without `origin` count as unattributed
  and fail closed at the CLI's `isHuman()` checks; the adapter stamps
  `{ kind: "human" }`.
- **Resolved paths.** Tool results report real paths (`/private/tmp/...`,
  `/private/var/folders/...`) even when the thread's folder was given
  through a symlink, so titles try both forms when making paths relative.
- **Write diffs.** A Write that creates a file has an empty
  `structuredPatch`; the diff is rebuilt from the content. Edit results
  carry a usable `structuredPatch`.
- **Images from Read.** Reading an image returns a base64 `image` block in
  the `tool_result` content, which is saved to `media/`.
- **Cost of interrupted turns.** The `result` for an interrupted turn
  usually leaves `total_cost_usd` where it was, so those turns show a cost
  of 0.
- **stderr noise.** Every start prints "claude.ai MCP server blocked by
  enterprise policy" on these logins; it's left out of error messages.
- **Turns the CLI starts itself.** After a `run_in_background` shell or an
  async subagent (the Agent tool is async by default) finishes while
  nothing is running, the CLI sends `system/task_notification` (with a
  `summary`), then `system/init` and a whole turn ending in a `result`,
  with nothing sent to it. Mid-turn the notification is folded into the
  running turn instead. `background_tasks_changed` gives the full list of
  running tasks each time it changes. `session_state_changed` is not sent
  by default.
- **Message uuids and `command_lifecycle`.** A user message sent with a
  `uuid` gets `command_lifecycle` frames (`queued`, `started`, `completed`,
  with `command_uuid`; not in the SDK's types), and the turn's first
  assistant frame and stream event carry `user_message_uuid`. A message
  sent while the CLI runs its own turn is `queued` and only `started`
  after that turn's `result`. Turns the CLI starts itself have no
  lifecycle frames.
- **Subagent approvals.** `canUseTool` for a subagent's tool has `agentID`
  set, and its `tool_use` frame never reaches the runner (subagent frames
  have `parent_tool_use_id`). A background subagent can ask after the turn
  that started it has ended.
- **"Always allow" suggestions** can include `setMode: acceptEdits` and an
  `addRules` with destination `localSettings`, which writes
  `<cwd>/.claude/settings.local.json`.
- **No final newline.** `structuredPatch` hunks include `\ No newline at
  end of file` lines (not counted in the line totals).
- **Big Bash output** comes back as a `<persisted-output>` preview, with
  `tool_use_result.persistedOutputPath` naming the file holding all of it.
- **Re-sent command output.** After `/compact`, the CLI re-sends the
  assistant frames of earlier local commands (`/cost`) it kept, with new
  message ids and `local_command_source`; they're skipped.
