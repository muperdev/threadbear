# Running the Threadbear runner natively on Windows: audit

**Scope.** Everything here comes from reading and grepping the code on commit `b80ddef4`. Nothing was run on Windows and no files were changed. Docs were checked on 9 Nov 2026. Items I could not confirm are marked **verify**.

`src/` means `runner/src/`, and `scripts/` and `launch.mjs` are under `runner/` unless they start with `root:`.

**Sizes:** S is under a day, M is a few days, L is one to two weeks or more.

## Main conclusions

1. **Most failures are silent, not crashes.** Almost every Unix call is inside a `try/catch` that treats failure as "nothing there".
   - `process.kill(-pid)` fails, so `killGroup` and `groupAlive` return false and CLIs leak.
   - `/bin/ps` fails, so `processInfo` returns null and the runner lock always looks stale (`src/lock.ts:56`).
   - `lsof` fails, so `threadOfPort` returns undefined and the presented credential decides.
   - `process.getuid?.()` is undefined on both sides at `src/handover.ts:434`, so the ownership check passes.
   - Port these deliberately. Do not wait for errors to show the problem.
2. **Require Git for Windows for v1.** The runner needs `git.exe` anyway, and it brings `bash.exe`. That keeps `check.sh`, the worktree scripts and about 6,900 lines of POSIX-shell trust code valid (`shell-allowlist`, `script-trust`, `claude-trust`, `patch-trust`). It also matches what Claude's Bash tool uses.
3. **Build one small native helper** (Rust, or C# compiled to a single exe; `koffi` FFI is the alternative). It should provide:
   - Job Objects
   - a process table with creation times
   - the TCP owner table
   - keep-awake and idle time
   
   Roughly 15 sites depend on these and Node has no API for them. A GUI-subsystem launcher in the same binary replaces Runner.app.
4. **Good news.**
   - No PTY, no node-pty, no Unix sockets (TCP on 127.0.0.1 only: `src/rpc.ts:203`, `src/adapters/acp/mcp-http.ts:45`), no `shell: true`.
   - Hooks and MCP tools are in-process.
   - `oxc-parser` is the only native module used at runtime, and it has win32 builds.
   - The IPC handover in `launch.mjs` is portable.

## What the official CLIs need

| | Claude Code | Codex |
|---|---|---|
| Native, no WSL | Yes. Windows 10 1809+ or Server 2019+. [Setup docs](https://code.claude.com/docs/en/setup) | Yes. Win 11 recommended, Win 10 1809+ "best effort". [Windows docs](https://learn.chatgpt.com/docs/windows/windows-sandbox) |
| Git Bash | Optional. Needed for the Bash and Monitor tools. Without it, PowerShell is the shell tool. Git Bash was once required; the [v2.1.120 change is noted here](https://github.com/anthropics/claude-code/issues/53068). Override with `CLAUDE_CODE_GIT_BASH_PATH`. | Not mentioned. Runs in PowerShell. |
| Binary | `%USERPROFILE%\.local\bin\claude.exe`, versions under `.local\share\claude` | npm packages `@openai/codex-win32-x64` and `-win32-arm64` exist (`npm view`, 0.167.0). **Verify** the path inside the vendor folder and `.exe`. |
| Sandbox | **"Not supported"** on native Windows | `mxc` recommended, `elevated` (one admin setup), `unelevated`. Set under `[windows] sandbox`. |
| Credentials | `%USERPROFILE%\.claude\.credentials.json`, or under `CLAUDE_CONFIG_DIR`, relying on profile ACLs. [Auth docs](https://code.claude.com/docs/en/authentication.md) | `auth.json` in `CODEX_HOME`, or the keyring. [Auth docs](https://developers.openai.com/codex/auth). Windows Credential Manager is from a third-party source only. |

**PowerShell trap.** With Git Bash installed, the PowerShell tool is still [on by default](https://code.claude.com/docs/en/tools-reference.md) for claude.ai logins. Threadbear gates on the literal name `Bash`:

- `src/secrets-guard.ts:43` (a security check)
- `src/adapters/claude/adapter.ts:336, 1099, 1112`
- `src/relay.ts:1127`, `src/tb-dev.ts:691, 708`

For v1, set `CLAUDE_CODE_USE_POWERSHELL_TOOL=0` in `cliEnv`.

**Not confirmed:** the exact command wrapper Codex's app-server uses on Windows.

## Inventory

### Process control

| Assumption | Examples and count | Windows replacement | Size |
|---|---|---|---|
| Process groups (`detached`, negative pids) | `process.kill(-pid)` in 6 places: `src/proc.ts:216,225`, `src/main.ts:1168`, `src/upstream.ts:261`, `src/schedule-check.ts:195`, `scripts/deploy.mjs:866`. `detached: true` in 6 direct sites: `src/proc.ts:283`, `src/upstream.ts:250`, `src/patches.ts:1296`, `src/schedule-check.ts:179`, `src/models/claude-pool.ts:85`, `scripts/deploy.mjs:844`. Codex and ACP go through `groupSpawner`. | Named Job Objects (`Local\tb-<id>`), with the helper as a spawn wrapper: create suspended, assign to job, resume. The named job maps onto `children.json`, so `sweepOrphans` becomes open-and-terminate. Interim: `taskkill /T /F`, which misses orphaned chains. | L |
| Signals | Handlers at `src/main.ts:1152-54`, `launch.mjs:417`, `src/tb-dev.ts:944`, `scripts/deploy.mjs:104`. Sends at `launch.mjs:308,423`, `src/handover.ts:292`. | [Node docs](https://nodejs.org/api/child_process.html): `SIGTERM` and `SIGKILL` both kill abruptly, so there is no grace. Add `{type:'stop'}` over the existing IPC channel. Graceful CLI stop is close stdin, wait, then terminate the job. | M |
| Console windows | `windowsHide` appears at 0 of about 60 real spawn sites. `detached` gives each child [its own console](https://nodejs.org/api/child_process.html). | Route every spawn through one wrapper that sets `windowsHide: true`. | S-M |
| `.cmd` shims and npm | `src/updates/manager.ts:98`, `src/updates/codex.ts:159`, `scripts/deploy.mjs:219`, `scripts/deploy-backend.mjs:288-89`, `src/adapters/codex/appserver.ts:49` | `.cmd` [can't be launched by execFile](https://nodejs.org/api/child_process.html). Run `node <dir>/node_modules/npm/bin/npm-cli.js` instead. | S |
| `ps` | 11 sites: `src/proc.ts:247,258`, `src/handover.ts:431`, `src/frozen-cli.ts:224`, `src/updates/manager.ts:456`, `src/tb-connect.ts:50`, `src/stuck-jobs.ts:316`, `src/tb-dev.ts:323`, `src/update.ts:706`, `scripts/deploy.mjs:236`, `root:scripts/limit-step.mjs:73` | Helper: `CreateToolhelp32Snapshot` plus `GetProcessTimes`. `wmic` is being retired and spawning PowerShell every 5 s is too slow. Windows ppids aren't reparented and can be recycled, so compare creation times. | M |
| `lsof`, peer identity | `src/peer.ts:35`, `src/handover.ts:429,444`, `src/stuck-jobs.ts:329` | Helper: `GetExtendedTcpTable` for the owning pid, then job membership instead of a ppid walk. Until built, limited threads can read the token and act as owner. | M |
| `tb` "under a thread" check | `src/tb-connect.ts:50` reads another process's environment with `ps eww`. | Job membership. Windows has no simple equivalent of this read. | S after helper |

### Shells and external tools

| Assumption | Examples and count | Windows replacement | Size |
|---|---|---|---|
| `/bin/sh`, zsh, login shell | `src/system-tools.ts:34-39`, `src/worktrees.ts:552`, `src/schedule-check.ts:174`, `src/notion-tasks.ts:94`, `src/runner-keys.ts:223` | Resolve Git's `bash.exe` explicitly and **never** `System32\bash.exe`, which is the WSL stub. **Verify** that `tb` stays on PATH after `-l` runs `/etc/profile`. | S |
| Root bash scripts | `root:check.sh`, `root:worktree-setup.sh`, `root:borrow-worktree.sh`, `scripts/deploy-runner.sh`, `dev-runner.sh`, `find-node.sh` | Run under Git Bash. Add `*.sh text eol=lf` to `.gitattributes` (absent today; autocrlf would break them). `check-remote.sh` needs rsync, which Git Bash lacks, so run local or `--runner` only. | S |
| Lock tools | `src/worktrees.ts:355` (no flock, so `tb land` fails), `scripts/deploy-backend.mjs:117-24`, `check-remote.sh`, `root:scripts/one-build.sh` (fails open) | Named pipe as a mutex: `net.listen('\\\\.\\pipe\\tb-land-<hash>')`. The OS releases it on death and `EADDRINUSE` means held. Pure Node. | S |
| Open a URL | `src/runner.ts:3851` hard-codes `/usr/bin/open`. | Argument-safe `rundll32 url.dll,FileProtocolHandler <url>`. Avoid `cmd /c start`, which re-parses the URL. | S |
| Mac-only tools, already gated | `caffeinate` and `pmset` (`src/keep-awake.ts:109,122`), `ioreg` (`src/presence.ts:16`), `osascript`, `system_profiler`, `sips`/`qlmanage`, `swiftc` secret check, `taskpolicy` (`src/background.ts:21`), `vm_stat`/`sw_vers` | Graceful no-ops today. Later: `SetThreadExecutionState`, `GetLastInputInfo`, `GetSystemPowerStatus`, `IDLE_PRIORITY_CLASS`. | S each |
| Not gated | `src/frozen-cli.ts:224,230` (`ps`, `sandbox-exec`, `sample`), `src/stuck-jobs.ts:275` (`/tmp/claude-<uid>`), `src/user-name.ts:23` (`getent`, already wrapped) | Frozen-CLI detection targets macOS privacy dialogs, so turn it off on Windows. Stuck-jobs needs Claude's Windows task-output path (**verify**). | S / M |
| `tar`, `cp -c` | `scripts/deploy.mjs:751,792`, `scripts/deploy-backend.mjs:282` | Use absolute `System32\tar.exe`, since GNU tar reads `C:` as a remote host. `cp -c` already falls back to `npm ci`. | S |

### Paths and file system

| Assumption | Examples and count | Windows replacement | Size |
|---|---|---|---|
| Data and log folders | `src/config.ts:217,375,390,406`, `scripts/deploy.mjs:69,214`, `root:scripts/setup.mjs:69`, `root:check.sh:102`, `src/pair.ts:26,33`, `src/runner-keys.ts:123,167,367` | `%LOCALAPPDATA%\Threadbear` plus a single `platformPaths` module. Keep it out of Roaming. | S-M |
| PATH delimiter `:` | `src/restart.ts:156`, `scripts/deploy.mjs:168`, `src/adapters/codex/approvals.ts` (`split(':')`) | `path.delimiter`. Today this corrupts the first PATH entry in every thread. | S |
| `~/` expansion | 14 sites: `src/config.ts:222`, `src/usage.ts:69`, `src/patches.ts:761`, `src/share-images.ts:55`, `src/tools.ts:61`, `src/librarian.ts:1167`, `src/highlights.ts:403`, `src/pools.ts:110`, and others | One `expandHome` that also accepts `~\`. | S |
| "Absolute means starts with `/`" | `src/share-images.ts:56,282`, `src/highlights.ts:283`, `src/secret-values.ts:62`, `src/runner/patches.ts:86`, `src/threadbear-mcp.ts:997`, `src/cli.ts:1107`, `src/runner/moves.ts:44`, `src/cross-runner.ts:113` | `path.isAbsolute`, and check `path.sep` rather than `/`. About 117 grep hits, roughly 45 real. | M |
| Case-sensitive containment | About 13 `startsWith(x + path.sep)` checks, including security ones: `src/secrets-guard.ts:37`, `src/sandbox.ts:47`, `src/cross-runner.ts:112`, `src/project-icons.ts:188,192` | `isInside()` that folds case and normalises the drive letter on win32. | S-M |
| Secrets guard text matching | `src/secrets-guard.ts:44-60` strips backslashes, so `C:\Users\...` forms never match. `Application Support/Threadbear` is the wrong name. | Rewrite the forms for Windows and include PowerShell. Plan a second-lab review. | S |
| Symlinks | Create: `launch.mjs:79`, `src/release.ts:100`, `src/updates/codex.ts:230`, `root:worktree-setup.sh:77`. Read: `launch.mjs:66`, `src/release.ts:82`, `src/updates/codex.ts:82`. | Symlinks need Developer Mode or admin, and rename can't replace a directory link. Replace `current` with a text file on win32. Use a junction for `node_modules` (no privilege needed). | M |
| Atomic rename and deleting in-use files | 24 rename sites, mainly `src/fsutil.ts:20`. Release deletion at `scripts/deploy.mjs:626,1445`. Spare `git clean`. | Retry on `EPERM`/`EBUSY`/`EACCES`, since antivirus and indexers hold files. Treat a busy old release as "delete later": a draining runner has `oxc` loaded. | S-M |
| File permissions | About 28 sites: `src/config.ts:552-58`, `src/secrets.ts:49-59,115-17`, `src/runner-keys.ts:213,353`, `src/convex-keys.ts:226-46` | `chmod` only touches read-only here. `%LOCALAPPDATA%` is already private by profile ACL, as Claude's docs rely on. Add `icacls /inheritance:r /grant:r %USERNAME%:F` on `secrets\`, or DPAPI later. | S-M |
| Askpass helper | `src/secrets.ts:59`, instructions at `src/threadbear-mcp.ts:713` | Write `.cmd` that echoes the file. Drop `sudo -A` text on Windows. | S |
| uid assumptions | `scripts/deploy.mjs:206,1178`, `root:scripts/setup.mjs:238` (these **throw**), `src/handover.ts:434`, `src/shell-allowlist.ts:1877`, `src/sandbox.ts:122-28` | Guard on platform. | S |
| Git worktrees and hooks path | Git prints `C:/...` from `--show-toplevel` and worktree lists. `core.hooksPath=/dev/null` at `src/safe-git.ts:39`, `src/shell-allowlist.ts:2069` | Normalise every path from git. Point hooks at a runner-owned empty folder: `/dev/null` may resolve to `C:\dev\null`, which any user could create (**verify**). Set `core.longpaths`; a child's `cwd` is still limited to about 260 characters. | M |
| Line endings and watchers | Git output parsing is fine. `fs.watch` at `src/usage.ts:204`, `src/update.ts:170` | No change. PowerShell output would need `\r?\n`. | none |

### Finding CLIs, accounts and `tb`

| Assumption | Examples | Windows replacement | Size |
|---|---|---|---|
| Claude binary | `src/config.ts:376` defaults to `~/.local/bin/claude`. `src/updates/claude.ts:20` reads a link but falls back to `--version`. | Default to `claude.exe`. The update path otherwise works. | S |
| Codex binary | `src/adapters/codex/appserver.ts:28-33` has no win32 entry. Line 108 assumes `bin/codex`. Line 49 is the fallback. | Add `win32-x64` and `win32-arm64`, triples `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, and `codex.exe` (**verify** layout). | S |
| Account discovery | `CLAUDE_CONFIG_DIR` and `CODEX_HOME` at `src/adapters/claude/adapter.ts:48-49`, `src/adapters/codex/appserver.ts:148`, `src/accounts.ts:95-134` | Works as is. Token Stats is Mac-only but `<home>/accounts` already covers it. Project folders use the same non-alphanumeric rule (**verify** `C--Users-x`). | S |
| `tb` | `bin/tb` is shebang only. PATH injection at `src/restart.ts:156`. Quoting at `src/runner/worktrees.ts:322-23`. Dev shim at `scripts/dev-runner.sh:126`. | Add `bin/tb.cmd` and keep `bin/tb` for Git Bash. `bin` already ships in releases. | S |
| Agent hooks | SDK callbacks at `src/adapters/claude/adapter.ts:465-84`. In-process MCP at `src/threadbear-mcp.ts:2036`. | Nothing to port. Patch hooks use `runCommand`, covered by the shell row. | none |
| Node and modules | `engines` needs >=24. `oxc-parser` and the SDK have win32 packages (`runner/package-lock.json:144-57, 974-1006`). | No change. | none |

### Service, releases and sandbox

| Assumption | Examples | Windows replacement | Size |
|---|---|---|---|
| Service install | `scripts/install-runner.sh` (684 lines of launchd and systemd), `launcher/main.swift`, `launcher/build-launcher.sh` | **Per-user Scheduled Task at logon** (no admin, restart on failure, no battery stops). A Windows Service runs in Session 0 under the wrong identity. `HKCU\...\Run` is the fallback. Run a GUI-subsystem launcher so no console flashes. It must also create the log, since launchd did that. | M-L |
| Deploy branching | `scripts/deploy.mjs:72` treats everything non-Linux as macOS. Affects `plutil` (line 265), `launchctl`, restart command (line 917), `root:scripts/setup.mjs:213`. | Make it three-way. Restart through the existing `runner.restart` RPC. | M |
| Self-update | `src/update.ts` is mostly git and Node. `syncClone` and `fs.watch` are portable; `launchAgentBehind` is already gated. | Depends on `npm`, `tar`, `current` and checks. `check.sh --runner` needs Git Bash and Node. | S |
| Handover | `launch.mjs:250` IPC, drain server on its own port | Portable. Needs the stop message, and `runner.pid` must use helper creation times. | S after helper |
| Sandbox mode | `src/sandbox.ts` is **Docker, not seatbelt**. `sandbox-exec` appears only at `src/frozen-cli.ts:230`. Permission modes are per-tool SDK gates and Codex's own policy (`src/adapters/codex/adapter.ts:282-90`). | **No replacement yet.** `-v C:\x:C:\x`, uid -1 and a Windows exe can't work in a Linux container, and Docker Desktop itself needs WSL2 or Hyper-V. Reject `sandbox` config on win32 and keep the existing `CONFINED_NO_SANDBOX` refusal. Later candidates (unresearched): Windows Sandbox, AppContainer. Codex's non-full modes depend on its Windows sandbox being set up, so test that on a real machine. | S to refuse, L to replace |
| Patch trust | `src/shell-allowlist.ts:1875` refuses non-darwin and non-linux. Line 1887 walks by `/`. `src/script-trust.ts:248` reads exec bits. | Fails closed, so patches run only by hand. Port owner checks to NTFS ACLs and accept `C:\` and `.exe` forms. Needs a second-lab review. | L |
| Codex approval parsing | `src/adapters/codex/approvals.ts:111,152-56`, `src/adapters/codex/normalize.ts:868-90` | Fails safe (asks more often, shows the wrapper). Add PowerShell unwrapping once the format is confirmed. | S-M |
| Tests | 99 of 222 test files mention POSIX tools or paths. `test/deploy` uses fake launchctl. | Needs a Windows check host and platform skips. | L |

## Protocol and client paths

I read the path-related matches and sections of `docs/protocol.md`, not all of its roughly 6,000 lines.

The wire format is safe: paths are opaque strings, so no breaking change is needed. These need attention:

- **Absolute paths clients render or split:** `cwd` (line 262), worktree `path` (372), `folders` (497), `ImageRef.path` (623), `defaultCwd` (696), highlights (4081), `files.list` (1069).
- **Tilde in prebuilt display text:** `"Edited files in ~/dev/me/poddle"` (570), `privateFolders` (714).
- **Detection rules:** "absolute or `~/` paths" (4112), patch icons "from `~/`" (3170).
- **Home guessing:** lines 5041-45 match only `~`, `/Users/<name>` and `/home/<name>`. Add `C:\Users\<name>`.
- **Moves:** `src/runner/moves.ts:44` assumes `/`, and a Mac-to-Windows move needs path translation.
- **Token location:** line 11 is Mac-only.
- **Icons:** SF Symbols (line 679) are Mac-specific.
- **Slash commands:** `C:\...` can't be mistaken for one, so that's fine.

**Suggested additive field:** give runners `platform`, `pathStyle` and `homeDir`. Clients must split on both separators. The Mac app has about 30 `hasPrefix("/")`, `hasPrefix("~")` or `abbreviatingWithTilde` sites. `files.list` also needs a drive-list root.

## What blocks what

**Blocks "runs on Windows at all":**
- data and log folders, and `platformPaths`
- PATH delimiter
- Claude and Codex binary paths
- `tb.cmd`
- `windowsHide` wrapper
- Job Object kill, or `taskkill` as interim
- `npm` launch
- `getuid` crashes
- `current` link replacement
- `bash.exe` resolution and `.sh` line endings
- `tb land` lock
- graceful stop over IPC
- runner lock via process start times
- `CLAUDE_CODE_USE_POWERSHELL_TOOL=0`
- refusing Docker sandbox
- install path (Scheduled Task and launcher)

**Blocks parity:**
- full Job Object registry and orphan sweep
- peer identity and `socketOwnedBy`
- secrets guard for Windows
- patch trust on NTFS
- keep-awake, presence and battery
- stuck-job detection
- images and thumbnails (no `sips`; ImageMagick path untested)
- `deploy-backend` lock
- check-host support
- sandbox
- Codex approval unwrapping
- client path rendering

## Suggested order

1. **Foundations:** `platformPaths`, one spawn wrapper, path helpers, and `.gitattributes`. Mostly safe to land now on Mac and Linux.
2. **Decision and docs:** require Git for Windows, set the PowerShell tool off, refuse the sandbox.
3. **Helper exe:** Job Objects, process table, TCP table, launcher.
4. **Lifecycle:** swap `proc.ts` onto the helper, add the IPC stop, the runner lock and named-pipe locks.
5. **First real thread:** run from source on a Windows box with Claude, then Codex, including `read-only` policy.
6. **Install and releases:** Scheduled Task, `current` file, three-way `deploy.mjs`, self-update.
7. **Security parity:** peer identity, secrets guard, git hooks path, ACLs, with another lab trying to break each.
8. **Patch trust and tests:** port trust, stand up a Windows check host.
9. **Client and polish:** `pathStyle`, keep-awake, stuck jobs, images. Sandbox last.

Overall, a usable from-source runner looks like 2 to 3 weeks of work, and full parity about 6 to 8. Those totals are my rough estimates from the sizes above.