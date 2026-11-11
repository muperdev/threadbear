# Moving a thread to another runner

Mike, 8 Nov 2026 (thread f9077f11): "can we look into building a 'move to
runner' feature". He picked: the same thread moves (its id and place stay),
it takes its whole conversation, and its unlanded git work comes with it as
a sealed bundle, with a warning for whatever can't come. "Ideally I want it
so that its a seamless move so the work can just continue on elsewhere."

Today threads never move between runners (protocol.md, "Taking a thread
over"), and step 6's Later list has this as `move_thread` with a runner.

## What it's like when it's done

- A thread's menu (sidebar and the ... menu) has **Move to** with each other
  runner. Offline ones are greyed with "offline". Picking one moves it
  straight away, to the folder and login that machine guesses. If it can't
  go, or something won't come (a dev server that will stop, changes in a
  checkout other threads share), a small sheet shows the folder and login,
  both filled in, and says why. **Move** starts it.
- The row stays where it is and shows "Moving to cloud" with a spinner. A
  thread mid-turn shows "Moves to cloud when this turn ends". Messages sent
  from the phone meanwhile wait in its queue and go with it; in the Mac app
  the message box and any open card lock until it lands or is cancelled.
- When it lands, the row shows the new machine, the transcript has one line
  ("Moved from M5 to cloud, in ~/dev/me/threadbear-threads/slot-1") and the
  next message carries on in the same Claude session there, with the same
  branch and the same uncommitted changes in its worktree.
- If anything goes wrong before it lands, it stays where it was, carries on
  as before, and the line says why ("cloud has no copy of mikecann/foo").
- An agent can do it too: `move_thread` with `runner` ("move yourself to
  the cloud and carry on with the tests"). From the main Mac it just
  happens; from a runner key it asks Mike with a card first, as
  `create_thread` with a runner does.

## Decisions

1. **One thread, one owner at a time.** The switch is a single backend
   mutation: before it the old runner (S) owns the thread, after it the new
   one (T) does. S packs only after the thread is idle and frozen, T opens
   it only after the switch, and S deletes its copy only once T says it's
   running there. A crash anywhere leaves exactly one live copy. S keeps a
   hidden copy in `<home>/moved/<id>` for 7 days in case a move goes wrong.
2. **The main Mac's runner (O) starts every move**, because only the owner
   key may send work to other runners. S, T and O can be any of the three
   (O is often S or T). After it starts, the backend drives it: each step's
   mutation queues the next runner's command, so O needn't stay online.
3. **T says yes first.** Before anything is packed, T checks it can take the
   thread (the folder, the login, the repo) and puts its sealing key on the
   move. Only then does S pack, sealed to that key with the secret box
   (`secrets.ts`: X25519, HKDF, AES-GCM), so Convex only holds what it can't
   read. The chunks are deleted when the move ends, whichever way.
4. **What goes in the bundle**: the thread's folder (`thread.json`,
   `events.ndjson`, `blobs/`, `media/`; not `panels/`, which is rebuilt),
   the Claude session's transcript and its sub-agents folder, and the git
   bundle. gzip, sealed, cut into 9 MB chunks (a runner key's uploads are
   10 MB at most). Over 100 MB sealed it doesn't move, and says what's big
   (a runner key keeps 150 MB stored at most).
5. **The conversation carries on, not restarts.** T writes the transcript
   into its login's config folder under the new folder's name and resumes
   the same session id, as a login move does today (`copySessionTranscript`,
   which already works across logins). Codex threads get the handoff note
   for now; their rollout files can come the same way later.
6. **Git work** (the thread's own worktree only):
   - S bundles every commit on the branch that `origin` doesn't have (from
     the merge base with S's `origin/<default>`, so unpushed commits on S's
     main come too: `tb land` never pushes), plus one more commit holding
     the uncommitted and untracked changes (`.gitignore`d files stay).
   - T fetches `origin`, makes its own worktree for the thread as usual
     (same branch name, `thread/<id8>`, its own slot and setup), applies
     the bundle, checks out the branch and puts the last commit's changes
     back as uncommitted.
   - A repo with no `origin`: the whole branch goes, if it fits.
   - T has no copy of the repo: T clones it from `origin` into the same
     place S has it under `~`, if T can reach it; otherwise T says no at
     step 3 and nothing moves.
   - A thread in a checkout other threads share (no worktree of its own)
     moves to the same repo there, but its changes stay behind and the
     sheet says which files, since they aren't the thread's alone.
7. **The folder on T**: the worktree as above; otherwise the same repo
   found by its `origin` URL among T's folders, at the same subfolder; a
   plain folder at the same path with S's home swapped for T's. Mike can
   pick another in the sheet. None found: T says no.
8. **The login on T**: the same email if T has it, else Mike's pick, else
   T's default pool. Model and effort stay if that provider has them.
9. **What moves with it in the backend**: the switch hands over the thread's
   row, its share link, its Pocket cards and its schedules (rows with its
   `threadId`) rather than tombstoning them, as a takeover does today. Its
   mirrored messages are cleared as in a takeover and T sends the whole
   history again from its own log (a new thread to T's sync is sent whole
   already).
10. **What doesn't come, and is said**: background shells and dev servers
    (the CLI is released), Highlights that point at files outside the
    worktree (they stay, pointing at S's paths), `.gitignore`d files, a
    patch folder's trust (T asks again, as for any folder new to it), and
    the sandbox (T decides from its own settings; a move that would give
    the thread more access asks first).
11. **An experiment**, `move-to-runner`, on for Mike only.

## The steps

The states and who acts on each are in docs/protocol.md ("Moving a thread
to another runner"): `asking` (T checks) → `ready` (S waits for the turn,
then freezes) → `packing` → `unpacking` (T downloads and unpacks into
staging) → `claimed` (the switch) → `done` (S puts its copy away). Each
runner watches the moves it's in (`moves:moves`) rather than taking
commands, so a restart picks up from the state, and O needn't stay online.

Failed or cancelled before the claim: S unfreezes and carries on, T throws
its staging away. A move frozen 30 minutes ago and not claimed fails by
itself. After the claim T has the thread, so anything left is T's to finish
on its next start (the staging is kept until then).

## Protocol and code

- Runner RPC: `threads.movePreview { threadId, runnerId }` → `{ cwd?,
  accountId?, accounts, leftBehind, problems }` (a `call` to T,
  `moves.preview` on the relay's allow-list); `threads.move { threadId,
  runnerId, cwd?, accountId? }` → `{ move }`; `threads.moveCancel`;
  `ThreadSummary.moving?: { to, toName, state, note? }`.
- Backend (API 64): the `threadMoves` table, `moves:startMove` and
  `cancelMove` (owner), `moves:moves` and the step mutations (`own`), and the
  switch handing the thread's rows over in runs of their own.
- Runner: `move-pack.ts` (bundle in and out, pure), `move-git.ts` (git
  bundle out, apply in), `moves.ts` (the steps on each side, freeze,
  recovery on start), the watch in `sync.ts`, `tb move`.
- Mac app: the menu, the sheet, the row's moving state, and the row passing
  from this runner's list to the remote list without losing its place or
  selection.
- Agent tool: `move_thread` takes `runner` (and `folder` becomes optional
  with it; the old form still works), with the card on a runner key.

## Order

1. This plan, then `docs/protocol.md` ("Moving a thread to another
   runner").
2. Backend and its tests (`convex-memory`): who may start, ready, pack,
   claim; the switch keeps share, Pocket cards and schedules; S can't
   write after it, T can.
3. `move-pack.ts` and `move-git.ts` with unit tests (round trips in temp
   folders: commits, unpushed main, uncommitted and untracked files, a
   missing base, a too-big bundle).
4. `moves.ts` and the commands, with two runners on one memory backend;
   then `tb move` and an e2e between two dev runners on the local
   deployment (haiku: a code word before, asked for after).
5. The app (on the M5: this thread runs on the cloud box, where the app
   can't be built or seen). A mock of the sheet first.
6. `move_thread` with `runner`, the card, and the guide, on-your-mac,
   forking and experiment docs.

## Questions for Mike

- A thread in a shared checkout moves without its changes (decision 6).
  OK, or should it refuse?
