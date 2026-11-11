---
id: multiplayer
name: Shared projects
title: Shared projects: work on a project with your team, each with your own threads and agents
added: 2026-11-11
on: false
reminder: 0853ed92
---

## What it does

You and your teammates each run Threadbear, and share projects through a team that one of you hosts on their own Convex backend (Settings › Team, or `tb team create`, `tb team invite` and `tb team join`). A shared project's goals, state, notes and links stay in step on every Mac, while folders stay each person's own: the project carries its repos, and each Mac finds its own clones. The project's page shows the team: who's on it, a board of everyone's threads in the project (working, waiting on them, done) and anything waiting for you. Agents in a shared project start knowing what their teammates' threads are doing and the team's latest notes, and a note an agent adds reaches every teammate's agents. An agent can message a teammate's thread (send_message to its `team:` id on the board), which waits for the teammate to pass it on unless the project is set to deliver straight away, and can hand its thread's work to a teammate with hand_over, which they take or decline. Only what the team shares goes to the hub: never transcripts, tool output, folders or secrets.

## Lives in

`runner/src/team.ts` (the runner's side: keys, the hub client, sync, cards, messages); `convex/team.ts`, `convex/teamAccess.ts` and the `team*` tables in `convex/schema.ts` (the hub); `hand_over` and the `team:` ids in `runner/src/threadbear-mcp.ts`; `ProjectRecord.team`, `onChange` and the team brief in `runner/src/projects.ts`; `tb team` in `runner/src/cli.ts`; the Team section and Settings › Team in the Mac app; `docs/multiplayer-design.md`

## Taking it out

Delete `runner/src/team.ts`, its wiring in `main.ts` (`team`, `teamControl`, `teamMethods`), `TeamControl`, `hand_over` and the `team:` branch of send_message in `threadbear-mcp.ts`, `setTeam`, `onChange` and `useTeamBrief` in `projects.ts`, `ProjectTeam` in `protocol.ts`, `tb team` in `cli.ts`, `convex/team.ts`, `convex/teamAccess.ts` and the `team*` tables, and the app's Team views (`TeamSection.swift`, `TeamSettings.swift`, `Team.swift`).
