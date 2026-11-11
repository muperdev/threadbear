import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { makePng } from '../helpers/png.ts';
import { formatThread, formatStatus, lastTurns, exportThread, lastLines, findRepoRoot, duration, restartRefusal, tbCommand, shQuote } from '../../src/tb-dev.ts';
import { Restarter, restartMethods, restartPath, resumesPath } from '../../src/restart.ts';
import { RpcServer } from '../../src/rpc.ts';
import { RunnerClient } from '../../src/client.ts';
import { readToken } from '../../src/config.ts';
import { nullLogger } from '../../src/log.ts';
import type { PersistedEvent, ThreadSummary } from '../../src/protocol.ts';
import type { RunnerStatus } from '../../src/restart.ts';
import { patiently } from '../helpers/patience.ts';

const TB = path.resolve(import.meta.dirname, '..', '..', 'bin', 'tb');
// How tb prints itself: quoted when the checkout path has a space (the
// runner's update checkout is under ~/Library/Application Support).
const TB_SH = shQuote(TB);
const MOCK = path.join(import.meta.dirname, '..', '..', '..', 'app', 'Tools', 'mock-runner');

const T: ThreadSummary = {
  id: 'abcdef12-0000-4000-8000-000000000000',
  title: 'Fix the tests',
  cwd: '/w',
  accountId: 'claude-3',
  provider: 'claude',
  permissionMode: 'full',
  status: 'idle',
  live: true,
  needsYou: false,
  createdAt: '2026-10-26T10:00:00.000Z',
  updatedAt: '2026-10-26T10:00:00.000Z',
  lastActivityAt: '2026-10-26T10:00:00.000Z',
  preview: '',
  lastSeq: 21,
  lastSeenSeq: 21,
};

function events(img = '/m/a.png'): PersistedEvent[] {
  const bodies: Record<string, unknown>[] = [
    { turnId: 't1', type: 'user.message', text: 'Fix the flaky test\nIt fails on CI', images: [{ path: img, mediaType: 'image/png', width: 10, height: 20 }] },
    { turnId: 't1', type: 'turn.started' },
    { turnId: 't1', type: 'reasoning', itemId: 'r1', text: 'Looking at the test first.\nMore' },
    { turnId: 't1', type: 'assistant.text', itemId: 'm1', text: 'I will run the tests.' },
    { turnId: 't1', type: 'tool.call', itemId: 'c1', name: 'Bash', kind: 'command', title: 'npm test', input: {} },
    { turnId: 't1', type: 'tool.result', itemId: 'c1', ok: false, text: 'FAIL a.test.ts\n1 failing', truncated: false, images: [] },
    { turnId: 't1', type: 'tool.call', itemId: 'c2', name: 'Edit', kind: 'edit', title: 'Edit src/a.ts', input: {} },
    { turnId: 't1', type: 'edit.diff', itemId: 'c2', path: '/w/src/a.ts', hunks: [{ oldStart: 1, oldLines: 2, newStart: 1, newLines: 2, lines: [' a', '-b', '+c'] }] },
    { turnId: 't1', type: 'approval.requested', approvalId: 'ap1-0000-edit', itemId: 'c2', name: 'Edit', title: 'Edit src/a.ts', input: {}, options: ['allow', 'deny'] },
    { turnId: 't1', type: 'approval.resolved', approvalId: 'ap1-0000-edit', decision: 'allow' },
    { turnId: 't1', type: 'tool.result', itemId: 'c2', ok: true, text: '', truncated: false, images: [] },
    { turnId: 't1', type: 'plan.updated', entries: [{ text: 'Run tests', status: 'completed' }, { text: 'Fix', status: 'in_progress' }, { text: 'Commit', status: 'pending' }] },
    { turnId: 't1', type: 'assistant.text', itemId: 'm2', text: 'Fixed it.\n\nAll green.' },
    { turnId: 't1', type: 'turn.completed', outcome: 'completed', durationMs: 12_345, costUsd: 0.0123 },
    { type: 'system.note', text: 'Runner restarted: a to b' },
    { turnId: 't2', type: 'user.message', text: 'Carry on', images: [], origin: 'runner' },
    { turnId: 't2', type: 'turn.started' },
    { turnId: 't2', type: 'question.requested', approvalId: 'ap2', questions: [{ question: 'Commit now?', multiSelect: false, options: [{ label: 'Yes' }, { label: 'No' }] }] },
    { turnId: 't2', type: 'approval.resolved', approvalId: 'ap2', decision: 'answered' },
    { turnId: 't2', type: 'error', message: 'Something broke' },
    { turnId: 't2', type: 'turn.completed', outcome: 'failed', durationMs: 2000, error: 'Something broke' },
  ];
  return bodies.map((b, i) => ({ seq: i + 1, ts: '2026-10-26T10:00:00.000Z', threadId: T.id, ...b }) as unknown as PersistedEvent);
}

const HEADER = ['Fix the tests', 'abcdef12-0000-4000-8000-000000000000, idle, claude-3, full access, in /w'];

const TURN_2 = ['  Note: Runner restarted: a to b', '', 'Runner: Carry on', '  Question: Commit now? (Yes / No)', '  Answer: answered', '  Error: Something broke', '  (failed, 2.0s: Something broke)'];

test('tb show: messages, one line per tool call, approvals, notes and turn footers', () => {
  assert.equal(
    formatThread(T, events()),
    [
      ...HEADER,
      '',
      'User: Fix the flaky test',
      '  It fails on CI',
      '  [image: /m/a.png, 10x20]',
      '',
      'Assistant: I will run the tests.',
      '  - npm test (failed)',
      '  - Edit src/a.ts',
      '  Approval asked (ap1-0000): Edit src/a.ts',
      '  Answer: allowed',
      '  Plan:',
      '    [x] Run tests',
      '    [>] Fix',
      '    [ ] Commit',
      '',
      'Assistant: Fixed it.',
      '',
      '  All green.',
      '  (completed, 12.3s, $0.0123)',
      ...TURN_2,
      '',
    ].join('\n'),
  );
});

test('tb show --tools adds results, diffs and thinking; --last keeps the last turns and the note before them', () => {
  const text = formatThread(T, events(), { tools: true });
  assert.ok(
    text.includes(
      [
        '  (thinking) Looking at the test first.',
        '',
        'Assistant: I will run the tests.',
        '  - npm test (failed)',
        '      FAIL a.test.ts',
        '      1 failing',
        '  - Edit src/a.ts',
        '       a',
        '      -b',
        '      +c',
        '  Approval asked (ap1-0000): Edit src/a.ts',
        '  Answer: allowed',
        '      (no output)',
      ].join('\n'),
    ),
    text,
  );
  assert.equal(formatThread(T, events(), { last: 1 }), [...HEADER, '(1 earlier turn not shown)', ...TURN_2, ''].join('\n'));
  assert.equal(lastTurns(events(), 5).hidden, 0);
  // Long results are cut.
  const long = events();
  (long[5] as { text: string }).text = Array.from({ length: 30 }, (_, i) => `line ${i}`).join('\n');
  assert.match(formatThread(T, long, { tools: true }), /      line 19\n      \.\.\. 10 more lines\n/);
});

test('tb show --tools puts each result under its own call when calls run in parallel', () => {
  const bodies: Record<string, unknown>[] = [
    { turnId: 't1', type: 'user.message', text: 'Read both', images: [] },
    { turnId: 't1', type: 'tool.call', itemId: 'a', name: 'Read', kind: 'read', title: 'Read alpha/notes.txt', input: {} },
    { turnId: 't1', type: 'tool.call', itemId: 'b', name: 'Read', kind: 'read', title: 'Read beta/notes.txt', input: {} },
    { turnId: 't1', type: 'tool.call', itemId: 'c', name: 'Bash', kind: 'command', title: 'ls', input: { command: 'ls' } },
    { turnId: 't1', type: 'approval.requested', approvalId: 'ap-ls', itemId: 'c', name: 'Bash', title: 'ls', input: { command: 'ls' }, options: ['allow', 'deny'] },
    { turnId: 't1', type: 'tool.result', itemId: 'b', ok: true, text: 'beta says 2', truncated: false, images: [] },
    { turnId: 't1', type: 'tool.result', itemId: 'a', ok: true, text: 'alpha says 1', truncated: false, images: [] },
    { turnId: 't1', type: 'approval.resolved', approvalId: 'ap-ls', decision: 'allow' },
    { turnId: 't1', type: 'tool.result', itemId: 'c', ok: true, text: 'alpha\nbeta', truncated: false, images: [] },
    // A result whose call came before the turns shown stays where it is.
    { turnId: 't1', type: 'tool.result', itemId: 'gone', ok: true, text: 'orphan', truncated: false, images: [] },
  ];
  const evs = bodies.map((b, i) => ({ seq: i + 1, ts: '2026-10-26T10:00:00.000Z', threadId: T.id, ...b }) as unknown as PersistedEvent);
  assert.equal(
    formatThread(T, evs, { tools: true }),
    [
      ...HEADER,
      '',
      'User: Read both',
      '  - Read alpha/notes.txt',
      '      alpha says 1',
      '  - Read beta/notes.txt',
      '      beta says 2',
      '  - ls',
      '  Approval asked (ap-ls): ls',
      '  Answer: allowed',
      '      alpha',
      '      beta',
      '      orphan',
      '',
    ].join('\n'),
  );
  // Without --tools the approval still sits under its call.
  assert.equal(
    formatThread(T, evs),
    [...HEADER, '', 'User: Read both', '  - Read alpha/notes.txt', '  - Read beta/notes.txt', '  - ls', '  Approval asked (ap-ls): ls', '  Answer: allowed', ''].join('\n'),
  );
});

test('tb status text', () => {
  const now = Date.parse('2026-10-26T12:00:00.000Z');
  const text = formatStatus(
    {
      version: '0.1.0+r1',
      release: 'r1',
      pid: 42,
      startedAt: '2026-10-26T09:55:00.000Z',
      home: '/h',
      logFile: '/h/runner.log',
      branch: 'devtools',
      liveClis: 2,
      running: [T.id],
      background: [{ threadId: 'bbbbbbbb-1', tasks: ['npm run e2e', 'research subagent'] }],
      pendingRestart: { when: 'idle', release: 'r2', requestedAt: '2026-10-26T11:57:00.000Z', waitingFor: [T.id, 'bbbbbbbb-1'], reason: 'deploy', resumeThreadId: T.id },
    },
    [{ ...T, status: 'running' }, { ...T, id: 'bbbbbbbb-1', title: 'E2E run', status: 'idle' }],
    now,
    T.id,
  );
  assert.equal(
    text,
    [
      'Runner 0.1.0+r1 (branch devtools), pid 42, up 2h 5m',
      'Data dir: /h',
      'Log: /h/runner.log',
      'Live CLIs: 2',
      'Busy threads: 1',
      '  abcdef12  Fix the tests  [running]  (this thread)',
      'Background work: 1 thread',
      '  bbbbbbbb  E2E run  [idle]: npm run e2e; research subagent',
      'Restart pending: when idle onto r2, asked 3m ago, waiting for 2 threads',
      '  abcdef12  Fix the tests  [running]  (this thread)',
      '  bbbbbbbb  E2E run  [idle]',
      '  Reason: deploy',
      '  Then carries on: abcdef12  Fix the tests  [running]  (this thread)',
      '',
    ].join('\n'),
  );
  assert.equal(duration(3 * 86_400_000 + 3_600_000), '3d 1h');
});

test('tb export writes the mock runner fixture format, with images copied next to it', () => {
  const src = tmpDir();
  fs.mkdirSync(path.join(src, 'a'));
  fs.mkdirSync(path.join(src, 'b'));
  const png = makePng(4, 3);
  fs.writeFileSync(path.join(src, 'a', 'shot.png'), png);
  fs.writeFileSync(path.join(src, 'b', 'shot.png'), png);
  const evs = events(path.join(src, 'a', 'shot.png'));
  // Same name from another folder, the same file twice, and one that's gone.
  (evs[5] as { images: unknown[] }).images = [
    { path: path.join(src, 'b', 'shot.png'), mediaType: 'image/png' },
    { path: path.join(src, 'a', 'shot.png'), mediaType: 'image/png' },
    { path: path.join(src, 'gone.png'), mediaType: 'image/png' },
  ];
  const out = path.join(tmpDir(), T.id);
  // A thread exporting itself mid-turn: the mock has nothing to end the
  // turn, so it's exported idle.
  assert.deepEqual(exportThread({ ...T, status: 'running', needsYou: true }, evs, out), { events: 21, media: 2 });
  assert.deepEqual(fs.readdirSync(path.join(out, 'media')).sort(), ['2-shot.png', 'shot.png']);
  const thread = JSON.parse(fs.readFileSync(path.join(out, 'thread.json'), 'utf8'));
  assert.equal(thread.id, T.id);
  assert.equal(thread.live, false);
  assert.equal(thread.status, 'idle');
  assert.equal(thread.needsYou, false);
  const lines = fs.readFileSync(path.join(out, 'events.ndjson'), 'utf8').split('\n');
  assert.equal(lines.pop(), '', 'ends with a newline');
  const back = lines.map((l) => JSON.parse(l));
  assert.deepEqual(back.map((e) => e.seq), evs.map((e) => e.seq));
  assert.deepEqual(back[0].images[0], { path: 'media/shot.png', mediaType: 'image/png', width: 10, height: 20 });
  assert.deepEqual(back[5].images.map((i: { path: string }) => i.path), ['media/2-shot.png', 'media/shot.png', path.join(src, 'gone.png')]);
  assert.equal(back[15].origin, 'runner');
});

test('the mock runner loads an export', { skip: !fs.existsSync(path.join(MOCK, 'node_modules', 'ws')) && 'mock runner has no node_modules' }, async () => {
  const src = tmpDir();
  fs.writeFileSync(path.join(src, 'shot.png'), makePng(4, 3));
  const parent = tmpDir();
  exportThread(T, events(path.join(src, 'shot.png')), path.join(parent, T.id));
  // Something else beside it, which loading the export alone leaves out.
  exportThread({ ...T, id: 'other' }, events(path.join(src, 'shot.png')), path.join(parent, 'other'));
  const data = tmpDir();
  const ready = path.join(data, 'ready');
  const mock = spawn(process.execPath, [path.join(MOCK, 'server.mjs'), '--data-dir', data, '--fixtures', path.join(parent, T.id), '--ready-file', ready, '--quiet'], { stdio: 'ignore' });
  try {
    // Until it holds a port: an older mock wrote the file in two steps.
    const readPort = () => (fs.existsSync(ready) ? Number(fs.readFileSync(ready, 'utf8')) : 0);
    await patiently(() => readPort() > 0);
    const port = readPort();
    const c = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, fs.readFileSync(path.join(data, 'secrets', 'runner-token'), 'utf8').trim());
    const list = await c.call('threads.list', { include: ['inbox'] });
    assert.deepEqual(list.threads.map((t: ThreadSummary) => t.id), [T.id]);
    const page = await c.call('threads.events', { threadId: T.id, limit: 100 });
    assert.equal(page.events.length, 21);
    assert.equal(path.normalize(page.events[0].images[0].path), path.join(parent, T.id, 'media', 'shot.png'));
    assert.ok(fs.existsSync(page.events[0].images[0].path));
    c.close();
  } finally {
    mock.kill();
  }
});

test('tb export finds the checkout to put fixtures in', () => {
  const root = tmpDir();
  fs.mkdirSync(path.join(root, 'app', 'Tools', 'mock-runner'), { recursive: true });
  fs.writeFileSync(path.join(root, 'app', 'Tools', 'mock-runner', 'server.mjs'), '');
  fs.mkdirSync(path.join(root, 'runner', 'src'), { recursive: true });
  assert.equal(findRepoRoot([path.join(root, 'runner', 'src')]), root);
  assert.equal(findRepoRoot([tmpDir()]), undefined);
});

test('tb logs reads the last lines from the end, filtered to a thread', () => {
  const file = path.join(tmpDir(), 'runner.log');
  const lines: string[] = [];
  // Over a 64 KB block, with a multi-byte character on every line so some
  // straddle a block edge.
  for (let i = 0; i < 3000; i++) lines.push(`2026-10-26T10:00:00.000Z INFO line ${i} é ${i % 3 === 0 ? '{"threadId":"abc"}' : ''}`);
  fs.writeFileSync(file, lines.join('\n') + '\n');
  assert.deepEqual(lastLines(file, 2), lines.slice(-2));
  const all = lastLines(file, 5000);
  assert.equal(all.length, 3000);
  assert.deepEqual(all, lines);
  const mine = lastLines(file, 1000, 'abc');
  assert.equal(mine.length, 1000);
  assert.ok(mine.every((l) => l.includes('"threadId":"abc"') && l.includes('é')));
  assert.deepEqual(lastLines(file, 0), []);
});

// ------------------------------------------------------------- the CLI

// statusHome: the data dir runner.status reports (the Restarter's), so a
// test can make it look like the real runner's through tb's HOME.
async function serve(o: { statusHome?: string; release?: string; version?: string } = {}) {
  const h = makeHarness();
  const state = { restarts: 0 };
  const r = new Restarter({
    runner: h.runner,
    home: o.statusHome ?? h.home,
    clock: h.clock,
    log: nullLogger,
    version: o.version ?? '0.1.0+test',
    release: o.release,
    logFile: path.join(h.home, 'runner.log'),
    restart: () => state.restarts++,
  });
  const server = new RpcServer({ host: '127.0.0.1', port: 0, token: readToken(h.home), runner: h.runner, log: nullLogger, methods: restartMethods(r) });
  const { port } = await server.listen();
  return { h, r, state, server, url: `ws://127.0.0.1:${port}/rpc` };
}

// Runs bin/tb against the test runner. Variables from a thread this test
// might itself run in are dropped unless the test sets them.
function tb(home: string, url: string, args: string[], env: Record<string, string> = {}) {
  const base: NodeJS.ProcessEnv = {};
  for (const [k, v] of Object.entries(process.env)) if (!/^THREADB(EAR|ARE)_/.test(k)) base[k] = v;
  let out = '';
  const p = spawn(process.execPath, [TB, ...args], { env: { ...base, THREADBEAR_HOME: home, THREADBEAR_URL: url, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  p.stdout.on('data', (d) => (out += d));
  p.stderr.on('data', (d) => (out += d));
  const exited = new Promise<number | null>((r) => p.once('exit', (code) => r(code)));
  return { out: () => out, exited, kill: () => p.kill() };
}

async function run(home: string, url: string, args: string[], env: Record<string, string> = {}) {
  const t = tb(home, url, args, env);
  const code = await t.exited;
  return { code, out: t.out() };
}

test('tb status, self, show, export and rpc against a runner', async () => {
  const { h, server, url } = await serve();
  try {
    const { thread } = h.runner.create({ text: 'Fix the tests', accountId: 'claude-3' });
    const s = h.adapter.last;
    s.init();
    s.text('m1', 'Done.');
    s.result('completed', 0.01);

    // The runner's times come from its fake clock, so "up" isn't checked.
    let r = await run(h.home, url, ['status']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /^Runner 0\.1\.0\+test, pid \d+, up [^\n]+\n/);
    assert.match(r.out, /Live CLIs: 1\nBusy threads: none\n/);
    r = await run(h.home, url, ['status', '--json']);
    assert.equal(JSON.parse(r.out).version, '0.1.0+test');

    r = await run(h.home, url, ['self'], { THREADBEAR_THREAD_ID: thread.id });
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, new RegExp(`^${thread.id}\nFix the tests\nStatus: idle, CLI live\n`));
    r = await run(h.home, url, ['self', '--json'], { THREADBEAR_THREAD_ID: thread.id });
    assert.equal(JSON.parse(r.out).id, thread.id);
    r = await run(h.home, url, ['self']);
    assert.equal(r.code, 1);
    assert.match(r.out, /THREADBEAR_THREAD_ID isn't set/);

    r = await run(h.home, url, ['show', thread.id.slice(0, 8)]);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /\nUser: Fix the tests\n\nAssistant: Done\.\n  \(completed, [\d.]+s, \$0\.0100\)\n$/);

    const out = path.join(tmpDir(), 'fixture');
    r = await run(h.home, url, ['export', thread.id, '--out', out]);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Exported 4 events and 0 images to /);
    assert.match(r.out, new RegExp(`--fixtures ${out.replaceAll('.', '\\.')}\n`), 'loads the export alone');
    // The debug app comes first, and there's no bare mock runner line to
    // run in the foreground by mistake.
    // The path is quoted when the checkout sits under a dir with a space.
    assert.match(r.out, /\n  ('[^']*|\S*)app\/Tools\/dev-app\.sh'? start --mock --fixtures /);
    assert.doesNotMatch(r.out, /server\.mjs/);
    assert.match(r.out, new RegExp(`tb'? events ${thread.id.slice(0, 8)} --from N --to M\n`));
    assert.equal(fs.readFileSync(path.join(out, 'events.ndjson'), 'utf8').trim().split('\n').length, 4);

    r = await run(h.home, url, ['rpc', 'threads.list', '{"include":["inbox"]}']);
    assert.equal(r.code, 0, r.out);
    assert.equal(JSON.parse(r.out).threads[0].id, thread.id);
    r = await run(h.home, url, ['rpc', 'threads.list', 'not json']);
    assert.equal(r.code, 1);
    assert.match(r.out, /Params aren't JSON/);
    r = await run(h.home, url, ['rpc', 'no.such.method']);
    assert.equal(r.code, 1);
    assert.match(r.out, /Unknown method/);
  } finally {
    await server.close();
  }
});

test('tb restart: waits for busy threads, resumes the calling thread, and guards --now inside a thread', async () => {
  const { h, state, server, url } = await serve();
  try {
    const { thread } = h.runner.create({ text: 'Deploy', accountId: 'claude-3' });
    h.adapter.last.init();
    const inThread = { THREADBEAR_THREAD_ID: thread.id };

    let r = await run(h.home, url, ['restart', '--now'], inThread);
    assert.equal(r.code, 1);
    assert.match(r.out, /--now stops every turn on this runner, this thread's own included/);
    assert.doesNotMatch(r.out, /--force/, 'never taught to an agent');

    // Inside a thread it carries the thread on by default, and says to end
    // the turn: waiting for the restart from inside it would never end.
    r = await run(h.home, url, ['restart', '--reason', 'new release'], inThread);
    assert.equal(r.code, 0, r.out);
    const id8 = thread.id.slice(0, 8);
    assert.equal(
      r.out,
      `The runner will restart when 1 thread finishes (a turn, or background shells and subagents):\n  ${id8}  Deploy  [running]  (this thread)\n` +
        'Then this thread carries on with: "The runner restarted (new release). Check that it worked, then carry on."\n' +
        'End your turn now; the runner restarts after it and carries this thread on.\n',
    );
    let saved = JSON.parse(fs.readFileSync(restartPath(h.home), 'utf8'));
    assert.deepEqual(saved.resumes.map((x: { threadId: string; text: string }) => [x.threadId, x.text]), [[thread.id, 'The runner restarted (new release). Check that it worked, then carry on.']]);

    r = await run(h.home, url, ['restart', '--resume', 'Check it works', '--reason', 'new release'], inThread);
    assert.equal(r.code, 0, r.out);
    saved = JSON.parse(fs.readFileSync(restartPath(h.home), 'utf8'));
    assert.deepEqual(saved.resumes.map((x: { threadId: string; text: string }) => [x.threadId, x.text]), [[thread.id, 'Check it works']]);
    assert.equal(saved.reason, 'new release');

    r = await run(h.home, url, ['status'], inThread);
    assert.match(r.out, new RegExp(`Restart pending: when idle, asked [^\\n]+ ago, waiting for 1 thread\\n  ${id8}  Deploy  \\[running\\]  \\(this thread\\)\\n  Reason: new release\\n  Then carries on: `));

    // Outside a thread, --resume needs one named.
    r = await run(h.home, url, ['restart', '--resume', 'x']);
    assert.equal(r.code, 1);
    assert.match(r.out, /--resume needs a thread/);

    // Another thread of this runner can't withdraw it: almost always it's
    // a deploy waiting for that thread's own turn to end.
    r = await run(h.home, url, ['restart', '--cancel'], { THREADBEAR_THREAD_ID: 'someone-else' });
    assert.equal(r.code, 1, r.out);
    assert.match(r.out, /^The pending restart isn't this thread's: "new release", asked [^\n]+ ago\.\n/);
    assert.ok(r.out.includes(`  It carries on: ${id8}  Deploy  [running]\n`), r.out);
    assert.match(r.out, /Withdrawing it would cancel it under whoever asked and leave that thread waiting\./);
    assert.doesNotMatch(r.out, /--force/, 'never taught to an agent');
    assert.ok(fs.existsSync(restartPath(h.home)), 'still pending');
    // The thread that asked for it can.
    r = await run(h.home, url, ['restart', '--cancel'], inThread);
    assert.equal(r.out, 'No restart is pending now.\n');
    assert.equal(fs.existsSync(restartPath(h.home)), false, 'withdrawn');

    // A thread of another runner (tb pointed at a sandbox from a real
    // thread) isn't carried on, and --now isn't refused for it.
    r = await run(h.home, url, ['restart'], { THREADBEAR_THREAD_ID: 'a-thread-of-another-runner' });
    assert.equal(r.code, 0, r.out);
    assert.doesNotMatch(r.out, /this thread/);
    assert.ok(r.out.includes(`${TB_SH} restart --cancel withdraws it`), r.out);
    assert.deepEqual(JSON.parse(fs.readFileSync(restartPath(h.home), 'utf8')).resumes, []);

    assert.equal(state.restarts, 0);
    h.adapter.last.result();
    await tick();
    assert.equal(state.restarts, 1);

    r = await run(h.home, url, ['restart', '--now', '--force', '--no-resume'], inThread);
    assert.equal(r.code, 0, r.out);
    assert.equal(r.out, 'Restarting now; this turn ends with it.\n');
  } finally {
    await server.close();
  }
});

test('tb restart and status on a runner that is older than runner.restart', async () => {
  const h = makeHarness();
  const server = new RpcServer({ host: '127.0.0.1', port: 0, token: readToken(h.home), runner: h.runner, log: nullLogger });
  const { port } = await server.listen();
  try {
    const url = `ws://127.0.0.1:${port}/rpc`;
    const { thread } = h.runner.create({ text: 'Busy', accountId: 'claude-3' });
    h.adapter.last.init();
    let r = await run(h.home, url, ['restart']);
    assert.equal(r.code, 3, r.out);
    assert.match(r.out, /This runner is too old for runner\.status and runner\.restart/);
    // status says what it can, and not to restart it.
    r = await run(h.home, url, ['status']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, new RegExp(`^Runner \\S+\\nBusy threads: 1\\n  ${thread.id.slice(0, 8)}  Busy  \\[running\\]\\n`));
    assert.match(r.out, /picks it up on its next restart\. Don't restart it yourself\./);
  } finally {
    await server.close();
  }
});

test('tb logs prints the tail, filters by thread, and follows', async () => {
  const dir = tmpDir();
  const log = path.join(dir, 'runner.log');
  fs.writeFileSync(log, ['one', 'two {"threadId":"t-1"}', 'three', 'four {"threadId":"t-1"}'].join('\n') + '\n');
  // No runner at this URL: logs still work.
  const env = { THREADBEAR_LOG: log };
  let r = await run(dir, 'ws://127.0.0.1:1/rpc', ['logs', '-n', '2'], env);
  assert.equal(r.code, 0, r.out);
  assert.equal(r.out, `==> ${log} <==\nthree\nfour {"threadId":"t-1"}\n`);
  r = await run(dir, 'ws://127.0.0.1:1/rpc', ['logs', '--thread', 't-1'], env);
  assert.equal(r.out, `==> ${log} <==\ntwo {"threadId":"t-1"}\nfour {"threadId":"t-1"}\n`);
  // With the runner down and no THREADBEAR_LOG, a dev or sandbox data dir's
  // own log, not the real runner's.
  r = await run(dir, 'ws://127.0.0.1:1/rpc', ['logs', '-n', '1']);
  assert.equal(r.out, `==> ${log} <==\nfour {"threadId":"t-1"}\n`);
  r = await run(dir, 'ws://127.0.0.1:1/rpc', ['logs'], { THREADBEAR_LOG: path.join(dir, 'none.log') });
  assert.equal(r.code, 1);
  assert.match(r.out, /No log at/);

  const f = tb(dir, 'ws://127.0.0.1:1/rpc', ['logs', '-f', '-n', '1', '--thread', 't-2'], env);
  try {
    // Following from here on: the header comes out just before the tail is
    // read. A fixed wait lost this race on a busy machine, and so did 5 s
    // for tb to start: on 29 Oct the deploy's tests ran while other threads
    // kept the Mac busy, the lines went in before tb had read the file's
    // size, and it (rightly) never printed them, which held up the runner's
    // updates. Up to 60 s now, since it's only slow when the Mac is.
    await patiently(() => f.out().includes('<=='));
    assert.match(f.out(), /<==/, 'tb logs -f started within a minute');
    await new Promise((res) => setTimeout(res, 300));
    fs.appendFileSync(log, 'five {"threadId":"t-2"}\nsix\nseven {"threadId":"t-2"}\n');
    await patiently(() => f.out().includes('seven'));
    assert.equal(f.out(), `==> ${log} <==\nfive {"threadId":"t-2"}\nseven {"threadId":"t-2"}\n`);
  } finally {
    f.kill();
  }
  assert.equal(await f.exited, 0, 'SIGTERM ends a follow cleanly');
});

test('tb restart leaves the live runner alone: refused from its own threads, and from anywhere when it runs from a checkout', () => {
  const live = { version: '0.1.0+R1', release: 'R1', home: '/live' };
  const source = { version: '0.1.0+source', home: '/live' };
  // Dev runners and sandboxes are what tb restart is for.
  for (const onIt of [true, false]) for (const force of [true, false]) assert.equal(restartRefusal(source, { onIt, real: false, force }), undefined);
  // The thread's own live runner: deploy-runner.sh's job. From a checkout,
  // not even --force.
  assert.match(restartRefusal(live, { onIt: true, real: true, force: false })!, /^This is the runner your thread runs on\. Ship runner changes with scripts\/deploy-runner\.sh/);
  assert.equal(restartRefusal(live, { onIt: true, real: true, force: true }), undefined);
  assert.match(restartRefusal(source, { onIt: true, real: true, force: true })!, /runs from a checkout \(0\.1\.0\+source\).*Runner changes wait for the user/);
  // From outside its threads (a terminal, or a T3 agent): only a runner
  // run from a checkout is refused, and --force does it.
  assert.equal(restartRefusal(live, { onIt: false, real: true, force: false }), undefined);
  assert.match(restartRefusal(source, { onIt: false, real: true, force: false })!, /^The runner at \/live runs from a checkout/);
  assert.equal(restartRefusal(source, { onIt: false, real: true, force: true }), undefined);
  for (const o of [{ onIt: true, real: true, force: false }, { onIt: false, real: true, force: false }]) {
    assert.doesNotMatch(restartRefusal(source, o) ?? '', /--force/, 'never offered to an agent');
  }
});

test('tb restart and tb rpc against the live runner from one of its threads', async () => {
  // tb takes the real data dir from HOME, so a temp HOME makes this
  // runner look like the live one without going near it.
  const fakeHome = tmpDir();
  const real = path.join(fakeHome, 'Library', 'Application Support', 'Threadbear');
  fs.mkdirSync(real, { recursive: true });
  const env = { HOME: fakeHome };
  {
    // (This test process is the runner's stand-in and tb's parent, so tb
    // sees itself as running under it.)
    const { h, server, url } = await serve({ statusHome: real, version: '0.1.0+source' });
    try {
      for (const args of [['restart'], ['restart', '--force'], ['restart', '--now', '--force']]) {
        const r = await run(h.home, url, args, env);
        assert.equal(r.code, 1, r.out);
        assert.match(r.out, /The runner your thread runs on runs from a checkout/);
      }
      assert.equal(fs.existsSync(restartPath(real)), false, 'nothing was asked for');
    } finally {
      await server.close();
    }
  }
  const { h, state, server, url } = await serve({ statusHome: real, release: 'R1', version: '0.1.0+R1' });
  try {
    let r = await run(h.home, url, ['restart'], env);
    assert.equal(r.code, 1, r.out);
    assert.match(r.out, /This is the runner your thread runs on/);
    r = await run(h.home, url, ['rpc', 'runner.restart', '{}'], env);
    assert.equal(r.code, 1, r.out);
    assert.ok(r.out.includes(`Use ${TB_SH} restart`), r.out);
    assert.equal(fs.existsSync(restartPath(real)), false);
    r = await run(h.home, url, ['restart', '--force', '--no-resume'], env);
    assert.equal(r.code, 0, r.out);
    await tick();
    assert.equal(state.restarts, 1);
  } finally {
    await server.close();
  }
});

test("tb restart --now won't cut other threads short, and names the caller's own background work", async () => {
  const { h, server, url } = await serve();
  try {
    const mine = h.runner.create({ text: 'Deploy', accountId: 'claude-3' }).thread;
    const sm = h.adapter.last;
    sm.init();
    sm.out({ kind: 'tasks', tasks: [{ id: 'b1', type: 'local_bash', kind: 'shell', description: 'scripts/dev-runner.sh' }] });
    const other = h.runner.create({ text: 'Other work', accountId: 'claude-3' }).thread;
    h.adapter.last.init();
    const inThread = { THREADBEAR_THREAD_ID: mine.id };

    // Not from a thread even with --force, and not from a terminal without it.
    for (const [args, env, whose] of [[['restart', '--now', '--force'], inThread, "1 other thread's"], [['restart', '--now'], {}, "2 threads'"]] as const) {
      const r = await run(h.home, url, [...args], env);
      assert.equal(r.code, 1, r.out);
      assert.ok(r.out.startsWith(`--now would cut short ${whose} work`), r.out);
      assert.ok(r.out.includes(`${other.id.slice(0, 8)}  Other work  [running]`), r.out);
      assert.doesNotMatch(r.out, /--force/);
    }
    assert.equal(fs.existsSync(restartPath(h.home)), false);

    // An idle restart from the thread: ending the turn isn't enough while
    // its own background shell runs, so it says so.
    const r = await run(h.home, url, ['restart', '--max-wait', '10m'], inThread);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /It also waits for this thread's background work, which ending the turn doesn't stop:\n  scripts\/dev-runner\.sh\n/);
    assert.match(r.out, /Otherwise the restart goes ahead after 10m and stops it\.\nEnd your turn now/);
    const saved = JSON.parse(fs.readFileSync(restartPath(h.home), 'utf8'));
    assert.equal(Date.parse(saved.backgroundUntil) - h.clock.now(), 10 * 60_000);
    const bad = await run(h.home, url, ['restart', '--max-wait', 'soon'], inThread);
    assert.equal(bad.code, 1);
    assert.match(bad.out, /Can't read --max-wait soon/);
  } finally {
    await server.close();
  }
});

test('tb restart --cancel prints what happened to the release, tb self names the runner it is on, and bad flags get a message', async () => {
  const { h, r: restarter, server, url } = await serve({ release: 'A', version: '0.1.0+A' });
  try {
    const dir = path.join(h.home, 'runner');
    for (const id of ['A', 'B']) {
      fs.mkdirSync(path.join(dir, 'releases', id, 'src'), { recursive: true });
      fs.writeFileSync(path.join(dir, 'releases', id, 'src', 'main.ts'), '');
    }
    fs.symlinkSync(path.join('releases', 'B'), path.join(dir, 'current'));
    fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify({ current: 'B', previous: 'A', lastGood: 'A', boots: [] }));
    h.runner.create({ text: 'Busy', accountId: 'claude-3' });
    h.adapter.last.init();
    restarter.request({ release: 'B' });
    // This test process is the runner's stand-in and tb's parent, so tb
    // counts as one of its threads: only --force (Mike's, from a terminal)
    // withdraws a restart no thread of its own asked for.
    let r = await run(h.home, url, ['restart', '--cancel']);
    assert.equal(r.code, 1, r.out);
    assert.match(r.out, /^The pending restart isn't this thread's: a restart onto B, asked [^\n]+\nWithdrawing it would undo that deploy\. Leave it/);
    r = await run(h.home, url, ['restart', '--cancel', '--force']);
    assert.equal(r.out, "No restart is pending now.\ncurrent points at A again (the release running now), so B won't start on the next restart.\n");

    r = await run(h.home, url, ['self'], { THREADBEAR_THREAD_ID: '11111111-2222', THREADBEAR_RUNNER_HOME: '/elsewhere' });
    assert.equal(r.code, 1);
    assert.equal(r.out, `This thread (11111111) runs on the runner at /elsewhere; this tb talks to the one at ${h.home}.\n`);

    r = await run(h.home, url, ['status', '--bogus']);
    assert.equal(r.code, 2);
    assert.match(r.out, /^Unknown option '--bogus'.*\nSee tb --help\.\n$/s);
    r = await run(h.home, url, ['logs', '--thread']);
    assert.equal(r.code, 2);
    assert.match(r.out, /--thread <value>' argument missing/);
    assert.doesNotMatch(r.out, /at .*node:internal/, 'no stack trace');
  } finally {
    await server.close();
  }
});

test('tb show --tools prints a Bash command whole', () => {
  const cmd = 'runner/scripts/deploy-runner.sh --allow-branch --skip-checks --resume "Say: back"';
  const evs = [
    { seq: 1, ts: T.createdAt, threadId: T.id, turnId: 't1', type: 'tool.call', itemId: 'c1', name: 'Bash', kind: 'command', title: 'runner/scripts/deploy-runner.sh --allow-branch --s…', input: { command: cmd } },
    { seq: 2, ts: T.createdAt, threadId: T.id, turnId: 't1', type: 'tool.result', itemId: 'c1', ok: true, text: 'ok', truncated: false, images: [] },
  ] as unknown as PersistedEvent[];
  assert.match(formatThread(T, evs), /\n  - runner\/scripts\/deploy-runner\.sh --allow-branch --s…\n/);
  assert.ok(formatThread(T, evs, { tools: true }).includes(`\n  - ${cmd}\n      ok\n`));
});

test("tb status says when the runner went back to this release after a crash", () => {
  const now = Date.parse('2026-10-26T12:00:00.000Z');
  const base = { version: '0.1.0+A', release: 'A', pid: 42, startedAt: '2026-10-26T11:59:00.000Z', home: '/h', liveClis: 0, running: [], background: [] };
  const text = formatStatus({ ...base, rolledBack: { from: 'B', at: '2026-10-26T11:58:30.000Z' } }, [], now);
  assert.match(text, /^Runner 0\.1\.0\+A, pid 42, up 1m\nWent back to this release 1m ago: B kept crashing at startup\.\nData dir: \/h\n/);
  assert.doesNotMatch(formatStatus(base, [], now), /Went back/);
});

test("tb prints commands that reach the runner it talked to", () => {
  const tb = TB_SH;
  // A dev runner's <home>/tb names itself.
  assert.equal(tbCommand({ THREADBEAR_TB: '/tmp/dev home/tb', THREADBEAR_HOME: '/tmp/dev home' }), "'/tmp/dev home/tb'");
  // THREADBEAR_HOME pointed away from the thread's own runner for one
  // command: a bare tb there would be the live runner.
  assert.equal(tbCommand({ THREADBEAR_HOME: '/tmp/sbx', THREADBEAR_RUNNER_HOME: '/live' }), `THREADBEAR_HOME=/tmp/sbx ${tb}`);
  assert.equal(tbCommand({ THREADBEAR_HOME: '/live', THREADBEAR_RUNNER_HOME: '/live' }), tb);
  assert.equal(tbCommand({}), tb);
});

test("tb rpc keeps tb's guards, flag errors never offer --force, and a resume the runner won't send is said", async () => {
  const { h, server, url } = await serve();
  try {
    const { thread } = h.runner.create({ text: 'Deploy', accountId: 'claude-3' });
    h.adapter.last.init();
    const inThread = { THREADBEAR_THREAD_ID: thread.id };

    // runner.restart only through tb restart, --force or not.
    let r = await run(h.home, url, ['rpc', 'runner.restart', '{"when":"now"}', '--force'], inThread);
    assert.equal(r.code, 2, r.out);
    assert.equal(r.out, "tb rpc doesn't take --force.\nSee tb rpc --help.\n");
    r = await run(h.home, url, ['rpc', 'runner.restart', '{"cancel":true}'], { THREADBEAR_TB: '/tmp/dev/tb' });
    assert.equal(r.code, 1, r.out);
    assert.match(r.out, /^Use \/tmp\/dev\/tb restart \(--cancel to withdraw one\)/);
    assert.equal(fs.existsSync(restartPath(h.home)), false);
    r = await run(h.home, url, ['restart', '--wait', '5m'], inThread);
    assert.equal(r.code, 2, r.out);
    assert.equal(r.out, "tb restart doesn't take --wait. It takes --now, --resume, --no-resume, --cancel, --reason, --thread, --max-wait.\nSee tb restart --help.\n");
    // Nor does the help that points to.
    r = await run(h.home, url, ['restart', '--help'], inThread);
    assert.match(r.out, /^  tb restart \[--now\]/);
    assert.doesNotMatch(r.out, /--force/);

    // Snoozing or archiving its own thread through rpc ends this turn, as
    // tb snooze and tb archive would.
    for (const update of [{ archived: true }, { snoozedUntil: '2030-01-01T00:00:00.000Z' }]) {
      r = await run(h.home, url, ['rpc', 'threads.update', JSON.stringify({ threadId: thread.id, ...update })], inThread);
      assert.equal(r.code, 1, r.out);
      assert.match(r.out, /Can't snooze or archive the thread this runs in/);
    }
    assert.equal(h.runner.get(thread.id).archivedAt, undefined);
    r = await run(h.home, url, ['rpc', 'threads.update', JSON.stringify({ threadId: thread.id, title: 'Renamed', snoozedUntil: null })], inThread);
    assert.equal(r.code, 0, r.out);
    assert.equal(h.runner.get(thread.id).title, 'Renamed');

    // Resumed 3 times this hour: the runner won't carry it on, so tb says
    // so instead of promising it, and nothing is used up by asking.
    const stamp = new Date(h.clock.now()).toISOString();
    fs.mkdirSync(path.dirname(resumesPath(h.home)), { recursive: true });
    fs.writeFileSync(resumesPath(h.home), JSON.stringify({ [thread.id]: [stamp, stamp, stamp] }));
    r = await run(h.home, url, ['restart'], { ...inThread, THREADBEAR_TB: '/tmp/dev/tb' });
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /\nThe runner won't carry this thread on afterwards: this thread has already been resumed 3 times in the last hour\.\n/);
    assert.match(r.out, /End your turn now, and say in your reply that the user has to message this thread once the runner is back/);
    assert.doesNotMatch(r.out, /carries this thread on/);
    assert.equal(JSON.parse(fs.readFileSync(resumesPath(h.home), 'utf8'))[thread.id].length, 3);

    // From a terminal the hint to withdraw it is the tb that was run.
    r = await run(h.home, url, ['restart', '--no-resume'], { THREADBEAR_TB: '/tmp/dev/tb' });
    assert.ok(r.out.endsWith('/tmp/dev/tb restart --cancel withdraws it.\n'), r.out);
  } finally {
    await server.close();
  }
});

test('formatStatus: the line for the runner following main', () => {
  const now = Date.parse('2026-10-27T12:00:00.000Z');
  const base = { version: '0.1.0+r1', pid: 1, startedAt: '2026-10-27T11:00:00.000Z', home: '/h', liveClis: 0, running: [] };
  const line = (update: NonNullable<RunnerStatus['update']>) => formatStatus({ ...base, update }, [], now).trim().split('\n').at(-1);
  assert.equal(line({ state: 'off', reason: 'running from a checkout, not a release' }), 'Following main: off (running from a checkout, not a release)');
  assert.equal(line({ state: 'idle', commit: 'bbabc59abcdef', checkedAt: '2026-10-27T11:58:00.000Z' }), 'Following main: up to date at bbabc59, checked 2m ago');
  assert.equal(line({ state: 'building', commit: 'bbabc59abcdef' }), 'Following main: building a release of main at bbabc59');
  assert.equal(line({ state: 'waiting', commit: 'bbabc59abcdef', release: '20261027T120000Z-bbabc59' }), 'Following main: release 20261027T120000Z-bbabc59 of main at bbabc59 is built and starts on the pending restart');
  assert.equal(
    line({ state: 'failed', commit: 'bbabc59abcdef', checkedAt: '2026-10-27T11:58:00.000Z', reason: 'Unit tests failed', logFile: '/h/runner/update.log' }),
    'Following main: the build of main at bbabc59 failed, checked 2m ago: Unit tests failed (output in /h/runner/update.log)',
  );
  assert.ok(!formatStatus(base, [], now).includes('Following main'), 'an older runner says nothing');
});

test('tb status says when the main key is still being rotated', () => {
  const now = Date.parse('2026-10-26T12:00:00.000Z');
  const text = formatStatus({ version: '1', pid: 1, startedAt: '2026-10-26T11:00:00.000Z', home: '/h', logFile: '/h/l', liveClis: 0, running: [], sync: { state: 'ok', url: 'https://x.convex.cloud', name: 'm5', ownerKeyRotating: true } } as never, [], now);
  assert.match(text, /RUNNER_KEY_NEXT/);
});

test('tb status says what a backend on this Mac only leaves out, and says nothing of it for cloud', () => {
  const now = Date.parse('2026-10-26T12:00:00.000Z');
  const status = (url: string) => formatStatus({ version: '1', pid: 1, startedAt: '2026-10-26T11:00:00.000Z', home: '/h', logFile: '/h/l', liveClis: 0, running: [], sync: { state: 'ok', url, name: 'm5', pushOff: true } } as never, [], now);
  assert.match(status('http://127.0.0.1:13210'), /On this Mac only: .*no phone or other Mac can reach it, and the website and share links only open on this Mac/);
  assert.ok(!status('https://mac.tail1.ts.net:3210').includes('On this Mac only'));
});

// Daybreak, 10 Nov (finding 76): a runner key heartbeats its own
// followProblem, and `tb status` wrote it to the terminal as it came, so ESC
// ]52 could set the owner's clipboard and cursor moves could rewrite what the
// status said. Nothing another runner wrote reaches the terminal as a control.
test('tb status: another runner\'s name and problem reach the terminal without escape sequences or extra lines', () => {
  const now = Date.parse('2026-11-10T12:00:00.000Z');
  const base = { version: '0.1.0+r1', pid: 1, startedAt: '2026-11-10T11:00:00.000Z', home: '/h', liveClis: 0, running: [] };
  const osc52 = `\u001b]52;c;${Buffer.from('THREADBEAR_OSC52').toString('base64')}\u0007`;
  const payloads = [
    osc52,
    `\u001b]52;c;${Buffer.from('st-ended').toString('base64')}\u001b\\`,
    '\u009d52;c;QUJD\u009c',
    '\u001b[2K\u001b[1A\u001b[31mred\u001b[0m',
    '\u009b2K',
    '\u001bP1$tq\u001b\\',
    'a\u0000b\u0008c\u202eevil',
  ];
  for (const p of payloads) {
    const text = formatStatus({ ...base, behindRunners: [{ name: `bru${p}ce`, problem: `git said ${p} and then\nFollowing main: up to date` }] } as never, [], now);
    assert.doesNotMatch(text, /[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u200b-\u200f\u2028-\u202e]/, JSON.stringify(p));
    assert.doesNotMatch(text, /52;c;|THREADBEAR|\[2K/, JSON.stringify(p));
    const line = text.trim().split('\n').at(-1)!;
    assert.match(line, /^bru.*ce isn't following GitHub's main properly: git said .* and then Following main: up to date$/, 'one line, not a forged one');
  }
  // Its own followProblem, a reason and thread titles likewise.
  const own = formatStatus({ ...base, update: { state: 'idle', followProblem: `no${osc52}pe` }, pendingRestart: { when: 'idle', requestedAt: '2026-11-10T11:59:00.000Z', waitingFor: ['t1'], reason: `r${osc52}` } } as never, [{ ...T, id: 't1', title: `T${osc52}`, status: 'running' }], now);
  assert.doesNotMatch(own, /\u001b|\u0007|52;c;/);
  assert.match(own, /Not following GitHub's main properly: nope/);
});
