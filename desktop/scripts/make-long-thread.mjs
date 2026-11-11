#!/usr/bin/env node
// Makes a very long made-up thread for checking how the transcript copes:
//
//   node make-long-thread.mjs <folder> [turns=2500] [other threads=0]
//
// Other threads fill the thread list as a real one is, short ones with titles
// and previews, as the sidebar's rows cost something to draw too.
//
// It's a mock-runner fixture (app/Tools/mock-runner, --fixtures <folder>), with a
// mix of what real threads have: messages, folded work, tool calls, a reply
// with markdown, a turn footer. Two rows' worth of words per turn, more or less.
import fs from 'node:fs';
import path from 'node:path';

const dir = path.resolve(process.argv[2] ?? '');
const dir_ = dir;
const turns = Number(process.argv[3] ?? 2500);
const id = 'long_thread';
const out = path.join(dir, id);
fs.mkdirSync(out, { recursive: true });

const started = Date.parse('2026-10-20T08:00:00Z');
let seq = 0;
let clock = started;
const lines = [];
function event(body) {
  seq += 1;
  clock += 1500;
  lines.push(JSON.stringify({ seq, ts: new Date(clock).toISOString(), threadId: id, ...body }));
}

const words = 'the runner keeps each thread on disk and replays it when the app opens so nothing is lost if a build falls over halfway through'.split(' ');
const para = (n, salt) => Array.from({ length: n }, (_, i) => words[(i * 7 + salt) % words.length]).join(' ');

for (let t = 0; t < turns; t += 1) {
  const turnId = `turn_${t}`;
  event({ turnId, type: 'user.message', text: `Question ${t}: ${para(18, t)}?` });
  event({ turnId, type: 'turn.started' });
  event({ turnId, type: 'reasoning', itemId: `r${t}`, text: para(30, t + 3) });
  event({ turnId, type: 'tool.call', itemId: `c${t}`, name: 'Bash', kind: 'command', title: `grep -rn thing${t} src/`, input: { command: `grep -rn thing${t} src/` } });
  event({ turnId, type: 'tool.result', itemId: `c${t}`, ok: true, text: para(12, t), truncated: false, images: [] });
  event({
    turnId,
    type: 'assistant.text',
    itemId: `m${t}`,
    text: `**Answer ${t}.** ${para(40, t + 5)}\n\n- ${para(9, t + 1)}\n- ${para(11, t + 2)}\n\n\`\`\`ts\nconst value${t} = compute(${t});\n\`\`\`\n\n${para(25, t + 9)}`,
  });
  event({ turnId, type: 'turn.completed', outcome: 'completed', durationMs: 4200 + t, costUsd: 0.02 });
}

fs.writeFileSync(path.join(out, 'events.ndjson'), `${lines.join('\n')}\n`);
fs.writeFileSync(
  path.join(out, 'thread.json'),
  JSON.stringify({
    id,
    title: 'A very long thread',
    cwd: '/Users/mike/dev/long',
    accountId: 'claude-main',
    provider: 'claude',
    permissionMode: 'ask',
    status: 'idle',
    live: true,
    needsYou: false,
    createdAt: new Date(started).toISOString(),
    updatedAt: new Date(clock).toISOString(),
    lastActivityAt: new Date(clock).toISOString(),
    preview: 'A very long thread',
    lastSeq: seq,
    lastSeenSeq: seq,
  }, null, 2),
);
console.log(`${out}: ${turns} turns, ${seq} events`);

const others = Number(process.argv[4] ?? 0);
const topics = ['Fix the login redirect', 'Sentry: crash in the uploader', 'Plan the release notes', 'Why is the build slow?', 'Tidy the settings page', 'Draft the weekly update', 'Try the new model', 'Look at the failing test'];
for (let n = 0; n < others; n += 1) {
  const other = `other_${n}`;
  const at = clock - (n + 1) * 600_000;
  const lines = [
    { seq: 1, ts: new Date(at - 60_000).toISOString(), threadId: other, turnId: 't0', type: 'user.message', text: `${topics[n % topics.length]}, please` },
    { seq: 2, ts: new Date(at - 50_000).toISOString(), threadId: other, turnId: 't0', type: 'turn.started' },
    { seq: 3, ts: new Date(at - 10_000).toISOString(), threadId: other, turnId: 't0', type: 'assistant.text', itemId: 'm0', text: para(30, n) },
    { seq: 4, ts: new Date(at).toISOString(), threadId: other, turnId: 't0', type: 'turn.completed', outcome: 'completed', durationMs: 50_000, costUsd: 0.01 },
  ];
  const dir = path.join(dir_, other);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'events.ndjson'), `${lines.map((l) => JSON.stringify(l)).join('\n')}\n`);
  fs.writeFileSync(path.join(dir, 'thread.json'), JSON.stringify({
    id: other,
    title: `${topics[n % topics.length]} ${n + 1}`,
    cwd: `/Users/mike/dev/project-${n % 5}`,
    accountId: 'claude-main',
    provider: 'claude',
    permissionMode: 'ask',
    status: 'idle',
    live: true,
    needsYou: n % 7 === 0,
    createdAt: new Date(at - 60_000).toISOString(),
    updatedAt: new Date(at).toISOString(),
    lastActivityAt: new Date(at).toISOString(),
    preview: para(12, n),
    lastSeq: 4,
    lastSeenSeq: n % 3 === 0 ? 2 : 4,
  }, null, 2));
}
if (others) console.log(`and ${others} other threads`);
