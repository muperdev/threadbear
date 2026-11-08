// pools.ts: the built-in list, reading and checking runner.json's, the
// folder rule (the shared case table the app runs too), and which login a
// pool thread runs on or moves to. Pure functions; no runner.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import type { Account, Pool, UsageWindow } from '../../src/protocol.ts';
// Mike's pool folders, as his runner.json has them (poolFolders).
const DEFAULT_POOL_FOLDERS: Record<string, string[]> = { 'claude-2': ['~/dev/convex'] };
import {
  UNKNOWN_WAIT_MS,
  WAIT_SLACK_MS,
  backAt,
  blockedUntil,
  defaultPools,
  findPool,
  limitWhy,
  loginOk,
  nextLogin,
  pickInPool,
  poolForFolder,
  poolLine,
  poolProviders,
  readPools,
  roomWait,
  validatePools,
} from '../../src/pools.ts';

const NOW = Date.parse('2026-10-27T08:00:00.000Z');
const at = (mins: number) => new Date(NOW + mins * 60_000).toISOString();

function acct(id: string, opts: { provider?: 'claude' | 'codex'; note?: string; five?: number; week?: number; resets?: number; weekResets?: number; blocked?: boolean } = {}): Account {
  const a: Account = { id, provider: opts.provider ?? (id.startsWith('codex') ? 'codex' : 'claude'), title: `${id}@example.com`, configDir: `/tmp/${id}` };
  if (opts.note) a.note = opts.note;
  const windows: UsageWindow[] = [];
  if (opts.five !== undefined) windows.push({ kind: 'five_hour', title: '5-hour', remainingPercent: opts.five, ...(opts.resets !== undefined ? { resetsAt: at(opts.resets) } : {}) });
  if (opts.week !== undefined) windows.push({ kind: 'weekly', title: 'Weekly', remainingPercent: opts.week, ...(opts.weekResets !== undefined ? { resetsAt: at(opts.weekResets) } : {}) });
  if (windows.length || opts.blocked) a.usage = { windows, readAt: at(0), source: 'live', ...(opts.blocked ? { blocked: true } : {}) };
  return a;
}

const MIKE = [acct('claude-main'), acct('claude-1'), acct('claude-2', { note: 'Convex work only' }), acct('claude-3'), acct('codex-main')];

// ---------------------------------------------------------- the list

test("defaultPools: one per provider without notes, one per noted login, in Mike's order", () => {
  const pools = defaultPools(MIKE, {}, DEFAULT_POOL_FOLDERS);
  assert.deepEqual(pools, [
    { id: 'claude', title: 'Claude', accountIds: ['claude-main', 'claude-1', 'claude-3'], builtIn: true },
    { id: 'convex', title: 'Convex', accountIds: ['claude-2'], builtIn: true, folders: ['~/dev/convex'] },
    { id: 'codex', title: 'Codex', accountIds: ['codex-main'], builtIn: true },
  ]);
});

test('defaultPools: notes from the map count too; a note with no word, or a clashing one, uses the login', () => {
  const pools = defaultPools([acct('claude-main'), acct('claude-1'), acct('claude-3')], { 'claude-1': 'Tests only', 'claude-3': 'claude stuff' }, {});
  assert.deepEqual(pools.map((p) => [p.id, p.title, p.accountIds]), [
    ['claude', 'Claude', ['claude-main']],
    ['tests', 'Tests', ['claude-1']],
    ['claude-3', 'claude', ['claude-3']],
  ]);
  // No logins of a provider: no pool for it.
  assert.deepEqual(defaultPools([acct('codex-main')], {}, {}).map((p) => p.id), ['codex']);
  assert.deepEqual(defaultPools([], {}, {}), []);
});

test("readPools: runner.json's list replaces the built-in one; bad entries are skipped", () => {
  const saved = [{ id: 'mine', title: 'Mine', accountIds: ['claude-3', 'codex-main'], folders: [' ~/x ', ''] }, { id: 'broken' }, 'nope'];
  assert.deepEqual(readPools({ pools: saved }, MIKE), [{ id: 'mine', title: 'Mine', accountIds: ['claude-3', 'codex-main'], folders: ['~/x'] }]);
  // An icon is kept, trimmed; a blank or non-string one is dropped.
  const icons = [{ id: 'a', title: 'A', accountIds: ['claude-3'], icon: ' leaf ' }, { id: 'b', title: 'B', accountIds: ['claude-3'], icon: 7 }, { id: 'c', title: 'C', accountIds: ['claude-3'], icon: ' ' }];
  assert.deepEqual(readPools({ pools: icons }, MIKE).map((p) => p.icon), ['leaf', undefined, undefined]);
  // Nothing usable, or nothing saved: the built-in ones.
  assert.equal(readPools({ pools: ['nope'] }, MIKE, DEFAULT_POOL_FOLDERS)[0].builtIn, true);
  assert.equal(readPools({}, MIKE).length, 3);
  assert.equal(readPools({ accountNotes: { 'claude-1': 'Work' } }, [acct('claude-1')])[0].id, 'work');
});

test('validatePools: duplicates, blanks, empty logins and unknown logins are refused', () => {
  const ok = validatePools([{ id: 'a', title: 'A', accountIds: ['claude-1'], builtIn: true }], MIKE);
  assert.deepEqual(ok, { pools: [{ id: 'a', title: 'A', accountIds: ['claude-1'] }] }, 'builtIn is dropped');
  assert.deepEqual(validatePools([], MIKE), { pools: [] });
  const err = (raw: unknown) => ('error' in validatePools(raw, MIKE) ? (validatePools(raw, MIKE) as { error: string }).error : '');
  assert.match(err({}), /must be an array/);
  assert.match(err([{ id: 'a', title: 'A', accountIds: ['claude-1'] }, { id: 'a', title: 'B', accountIds: ['claude-3'] }]), /Two pools are called a/);
  assert.match(err([{ id: ' ', title: 'A', accountIds: ['claude-1'] }]), /blank id/);
  assert.match(err([{ id: 'a', title: '', accountIds: ['claude-1'] }]), /blank title/);
  assert.match(err([{ id: 'a', title: 'A', accountIds: [] }]), /no logins/);
  assert.match(err([{ id: 'a', title: 'A', accountIds: ['claude-9'] }]), /claude-9/);
  assert.match(err([{ id: 'a', title: 'A', accountIds: [1] }]), /needs id and title/);
});

test('findPool: by id, else id or title ignoring case', () => {
  const pools = defaultPools(MIKE, {}, {});
  assert.equal(findPool(pools, 'convex')?.id, 'convex');
  assert.equal(findPool(pools, 'Claude')?.id, 'claude');
  assert.equal(findPool(pools, 'CODEX ')?.id, 'codex');
  assert.equal(findPool(pools, 'gemini'), undefined);
});

// ---------------------------------------------------------- folders

type FolderCase = { name: string; cwd: string; expected: string | null; pools?: Pool[] };
const folderCases = JSON.parse(fs.readFileSync(path.join(import.meta.dirname, '..', 'fixtures', 'pool-folder-cases.json'), 'utf8')) as {
  home: string;
  pools: Pool[];
  cases: FolderCase[];
};

for (const c of folderCases.cases) {
  test(`poolForFolder: ${c.name}`, () => {
    assert.equal(poolForFolder(c.pools ?? folderCases.pools, c.cwd, folderCases.home)?.id ?? null, c.expected);
  });
}

// ---------------------------------------------------------- picking

const CLAUDE: Pool = { id: 'claude', title: 'Claude', accountIds: ['claude-main', 'claude-1', 'claude-3'] };
const MIXED: Pool = { id: 'all', title: 'All', accountIds: ['claude-main', 'codex-main', 'claude-1'] };

test('loginOk: blocked or under 10% is not; unknown usage is', () => {
  assert.equal(loginOk(acct('a')), true);
  assert.equal(loginOk(acct('a', { five: 10, week: 50 })), true);
  assert.equal(loginOk(acct('a', { five: 9.5, week: 50 })), false);
  assert.equal(loginOk(acct('a', { five: 80, week: 5 })), false);
  assert.equal(loginOk(acct('a', { five: 80, blocked: true })), false);
});

test('pickInPool: current stays while OK; else most room; unknown after known; prefer breaks ties', () => {
  const list = [acct('claude-main', { five: 40 }), acct('claude-1', { five: 90 }), acct('claude-3', { five: 90 }), acct('codex-main', { five: 100 })];
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude', current: 'claude-main' }), 'claude-main', 'no move while it has room');
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude' }), 'claude-1', 'the earlier of a tie');
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude', prefer: 'claude-3' }), 'claude-3');
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude', exclude: ['claude-1', 'claude-3'] }), 'claude-main');
  assert.equal(pickInPool(CLAUDE, list, { provider: 'codex' }), undefined, 'codex-main is not in the pool');
  const lowCurrent = [acct('claude-main', { five: 5 }), acct('claude-1'), acct('claude-3', { five: 30 })];
  assert.equal(pickInPool(CLAUDE, lowCurrent, { provider: 'claude', current: 'claude-main' }), 'claude-3', 'known room beats unknown');
  assert.equal(pickInPool(CLAUDE, lowCurrent, { provider: 'claude', exclude: ['claude-3'] }), 'claude-1', 'then unknown');
  assert.equal(pickInPool(CLAUDE, lowCurrent, { provider: 'claude', prefer: 'claude-1' }), 'claude-1', 'a preferred login with unknown usage stays, as pickAccount');
  const dry = [acct('claude-main', { five: 5 }), acct('claude-1', { blocked: true }), acct('claude-3', { five: 2 })];
  assert.equal(pickInPool(CLAUDE, dry, { provider: 'claude' }), undefined);
});

test('pickInPool: the login whose week resets soonest goes first, while it has room', () => {
  // Mike's three logins on 5 Nov: the fullest resets in 5 days, the other
  // two today and tomorrow with a third of their week left.
  const day = 24 * 60;
  const list = [acct('claude-main', { five: 95, week: 77, weekResets: 5 * day }), acct('claude-1', { five: 90, week: 35, weekResets: 17 * 60 }), acct('claude-3', { five: 99, week: 37, weekResets: 5 * 60 })];
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude', now: NOW }), 'claude-3', 'resets first');
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude', now: NOW + 6 * 60 * 60_000 }), 'claude-1', 'once claude-3 has reset, its next reset is unknown');
  assert.equal(pickInPool(CLAUDE, list, { provider: 'claude', current: 'claude-main', now: NOW }), 'claude-main', 'a thread on a login with room stays put');
  // Down to the floor: it's left, and the next to reset is used.
  const low = [list[0], list[1], acct('claude-3', { five: 99, week: 9, weekResets: 5 * 60 })];
  assert.equal(pickInPool(CLAUDE, low, { provider: 'claude', now: NOW }), 'claude-1');
  // The tighter 5-hour window counts too.
  const busy = [list[0], list[1], acct('claude-3', { five: 4, week: 37, weekResets: 5 * 60 })];
  assert.equal(pickInPool(CLAUDE, busy, { provider: 'claude', now: NOW }), 'claude-1');
  // The same reset: most room, then prefer.
  const same = [acct('claude-main', { week: 40, weekResets: 60 }), acct('claude-1', { week: 60, weekResets: 60 }), acct('claude-3', { week: 60, weekResets: 60 })];
  assert.equal(pickInPool(CLAUDE, same, { provider: 'claude', now: NOW }), 'claude-1');
  assert.equal(pickInPool(CLAUDE, same, { provider: 'claude', prefer: 'claude-3', now: NOW }), 'claude-3');
  // A login with no weekly reset known waits behind one with.
  const mixed = [acct('claude-main', { five: 100 }), acct('claude-1', { five: 50, week: 20, weekResets: 3 * day })];
  assert.equal(pickInPool(CLAUDE, mixed, { provider: 'claude', now: NOW }), 'claude-1');
});

test('nextLogin: same provider first, then spill in pool order, then a wait', () => {
  const thread = { accountId: 'claude-main', provider: 'claude' as const };
  let list = [acct('claude-main', { five: 0, blocked: true, resets: 120 }), acct('codex-main', { five: 80 }), acct('claude-1', { five: 50 })];
  assert.deepEqual(nextLogin(MIXED, list, thread, NOW), { accountId: 'claude-1', provider: 'claude' });
  list = [acct('claude-main', { five: 0, blocked: true, resets: 120 }), acct('codex-main', { five: 80 }), acct('claude-1', { five: 3, resets: 60 })];
  assert.deepEqual(nextLogin(MIXED, list, thread, NOW), { accountId: 'codex-main', provider: 'codex' });
  list = [acct('claude-main', { five: 0, blocked: true, resets: 120 }), acct('codex-main', { blocked: true }), acct('claude-1', { five: 3, resets: 60 })];
  assert.deepEqual(nextLogin(MIXED, list, thread, NOW), { waitUntil: new Date(NOW + 60 * 60_000 + WAIT_SLACK_MS).toISOString(), accountId: 'claude-1' });
  assert.deepEqual(poolProviders(MIXED, list), ['claude', 'codex']);
});

test('roomWait: the earliest login back plus a minute, or 30 minutes when none says', () => {
  const list = [acct('claude-main', { five: 0, blocked: true, resets: 300 }), acct('claude-1', { five: 50, week: 4, resets: 90 }), acct('claude-3', { five: 2, resets: 45 })];
  assert.deepEqual(roomWait(CLAUDE, list, NOW), { waitUntil: at(46), accountId: 'claude-3' });
  assert.deepEqual(roomWait(CLAUDE, [acct('claude-main', { blocked: true })], NOW), { waitUntil: new Date(NOW + UNKNOWN_WAIT_MS).toISOString() });
});

test('backAt and blockedUntil: when a login has room again', () => {
  assert.equal(backAt(acct('a', { five: 0, blocked: true, resets: 30 }), NOW), NOW + 30 * 60_000);
  // Blocked with no window at its limit: the tightest window's reset.
  assert.equal(backAt(acct('a', { five: 40, blocked: true, resets: 20 }), NOW), NOW + 20 * 60_000);
  // Under the minimum on a window with no reset time: unknown.
  assert.equal(backAt(acct('a', { five: 50, week: 3 }), NOW), undefined);
  assert.equal(blockedUntil(acct('a', { five: 0, resets: 30 }), NOW), NOW + 30 * 60_000);
  assert.equal(blockedUntil(acct('a'), NOW), NOW + UNKNOWN_WAIT_MS);
});

test('limitWhy and poolLine: the words in move notes and list_accounts', () => {
  const hit = acct('a', { five: 0, blocked: true, resets: 130 });
  assert.match(limitWhy(hit, NOW), /^hit its 5-hour limit, back at \S/);
  assert.equal(limitWhy(acct('a'), NOW, true), 'hit its usage limit');
  assert.match(limitWhy(acct('a', { five: 60, week: 6 }), NOW), /^is down to 6% of its weekly limit$/);
  const line = poolLine(CLAUDE, [acct('claude-main', { five: 61 }), acct('claude-1'), acct('claude-3', { five: 0, blocked: true, resets: 60 })], NOW);
  assert.match(line, /^claude-main 61%, claude-1 room unknown, claude-3 out until \S/);
  assert.match(poolLine({ id: 'x', title: 'X', accountIds: ['gone'] }, [], NOW), /gone \(not found\)/);
});
