# Shared projects (multiplayer)

Behrooz, 2026-11-11, after Cursor's "Projects are now multiplayer": a
project a whole team works on together, each person with their own
threads and agents, sharing one picture of the work. Threadbear already
has most of a single person's half of this (projects with goals, state
and notes the librarian keeps, agents starting and messaging threads, a
Notion board). What it doesn't have is a second person: each install has
its own backend with one owner key. This adds a team.

An experiment, `multiplayer` (runner/src/experiments/multiplayer.md),
off by default. Everything here is in this fork, not upstream.

## What a teammate gets

1. **A shared project.** One person marks a project shared; teammates
   who join it get its goals, state and notes, kept in step both ways.
   Folders stay each person's own (their clones are in different
   places): the project carries its repos' remotes, and each runner
   maps them to the folders on its own Mac.
2. **A team board.** Every member's threads in the project, as cards:
   who, the thread's title, what it's doing (working, needs them, done),
   its headline and branch. Built from what the threads already say
   about themselves, so nobody keeps it.
3. **Context that carries across people.** A thread in a shared
   project starts knowing who else is on it, what their threads are
   doing right now, and the team's latest decisions. A note an agent
   adds to the project (update_project's appendNote) goes into the
   team's log as well, so a decision made in one person's chat reaches
   every agent.
4. **Agents talking to each other.** An agent can send_message to a
   teammate's thread (its id on the board, `team:...`). It reaches the
   teammate's Mac labelled as coming from that person's agent. By
   default it waits for the teammate to pass it on; a project can be
   set to deliver straight away.
5. **Handovers.** hand_over gives a thread's work to a teammate: what's
   done, what's next, the repo and branch, its links. On their Mac it
   waits as a handover; Take it starts a thread in the project with
   that as its first message, and the sender's thread is told.

## Where the team lives

On one member's backend, the team's **hub**: the person who makes the
team (`tb team create`) hosts it on their own Convex deployment.
Everyone else keeps their own backend for their own threads and also
talks to the hub with a **member key**. The hub holds only what the
team shares: members, shared projects, the log, the board, messages and
handovers. Never transcripts, tool output, folders, secrets or anything
from a private folder.

Member keys are a third kind of key beside the owner key and runner keys
(docs/protocol.md, "Runner keys"), kept in their own table and checked
by their own functions (`convex/team.ts`). A member key opens nothing
in `sync.ts`, `phone.ts` or `relay.ts`, and a runner key opens nothing
in `team.ts`: being on someone's team never reaches their threads.

Keys and invite codes are made on the Mac and only their hashes go to
the hub, as with runner keys.

## Joining

- `tb team create "<name>"` (or Settings › Team) on the host's Mac. The
  host's runner makes the team on its own deployment with its owner key
  and becomes its first member.
- `tb team invite` makes an invite: a code good for one join and 3 days,
  printed as `tbteam:<hub url>#<code>`. Only the host can invite.
- `tb team join <invite>` on a teammate's Mac. Their runner makes a
  member key, joins with it, and keeps `{ url, key, memberId }` in
  `<data>/secrets/team.json`.
- `tb team members`, `tb team remove <name>` (host), `tb team leave`.

## Sharing a project

`tb team share <project>` (or Share with Team… in a project's menu)
puts a copy of it on the hub, with a shared id, and links the local
project to it (`ProjectRecord.team = { id }`). Every member sees the
team's shared projects; `tb team add <shared project>` (or Add to my
projects in the Team view) makes a linked local project, with its
folders found from the shared repos.

What syncs, both ways:

| | |
|---|---|
| title, goals, state, links | last write wins, by the hub's own clock |
| notes | last write wins for a rewrite; an appendNote is also a log entry |
| repos | each runner adds the remotes of its own folders |
| folders, icon, Notion page, thread membership | not shared |

A runner follows the hub's shared projects with a subscription and
writes a change it didn't make into its local copy (no echo back: the
row says which member wrote it). Local changes go up a couple of
seconds after they're made, so an agent's three quick edits are one
write.

## The board

Each runner sends one card per thread of its own in a shared project,
when the card would change and at most every 10 seconds per thread:

```ts
type TeamCard = {
  threadId: string     // the member's own thread id
  title: string
  status: 'working' | 'needsThem' | 'idle' | 'done'
  headline?: string    // end_turn's headline
  branch?: string      // the thread's worktree branch
  repo?: string        // its folder's origin remote
  updatedAt: number
}
```

`done` is archived. A deleted thread's card goes. Cards older than a
week that are done are dropped by the hub.

## Messages between agents

`send_message` takes a teammate's card id, `team:<member>:<thread8>`,
as `threadId`. The runner files it on the hub; the teammate's runner
follows its inbox and, for a thread of its own:

- **ask** (the default): the message shows in the project's Team
  section as waiting, with Pass it on and Dismiss, and as a note in the
  thread. Pass it on sends it into the thread as a message.
- **deliver**: it goes straight in, as `send_message` from another
  runner does today.

Either way the text the agent gets starts with who it's from:
`[From Sam's agent, thread "Review feature", via the Collabute team]`.
It's a message like any other, so the thread's own permission mode
applies; nothing a teammate sends can raise it.

The sender's tool call returns once the hub has it; whether it was
passed on comes back as a note in the sending thread.

## Handovers

`hand_over({ to, note, threadId? })`: `to` is a member's name, `note`
the brief the agent writes (what's done, what's next, what to watch).
The runner adds the thread's title, repo, branch and Highlights links,
files it on the hub, and marks the card as handed over. On the
teammate's Mac it waits in the Team section: Take it starts a thread
there in the project (in the folder that has the repo, on the branch if
it's been pushed) with the handover as its first message; Decline says
so. The sending thread gets a note either way.

## In the Mac app

- **Settings › Team**: make a team or join one, the members with a dot
  for who's been seen in the last few minutes, invite, remove, leave.
- **A project's page** gets a Team section under State when it's
  shared: who's on it, the board (a column each for working, needs
  them and done; your own threads aren't on it, as they're in the
  Threads section), and anything waiting for you (messages, handovers)
  with its buttons. Share with Team… in the project's menu.
- The project's row in the Projects view shows a count of what's waiting.

The phone and the website show none of this yet.

## Protocol

- `Project.team?: { id, members: TeamMember[], waiting: number }`.
- `team.get` → `TeamState`, `team.create`, `team.invite`, `team.join`,
  `team.leave`, `team.remove`, `team.share`, `team.add`, `team.unshare`,
  `team.board`, `team.answer` (pass on, dismiss, take, decline),
  `team.setDelivery`.
- `team.updated` notification with the `TeamState`.

## Build order

1. Hub: tables and `convex/team.ts`, tested against the in-memory backend.
2. Runner: `team.ts` (keys, the hub client, following), `tb team`, the
   project link and two-way sync.
3. The board and the brief.
4. Messages and handovers, through the tools.
5. The Mac app.
6. The guide, the protocol, the experiment's doc.
