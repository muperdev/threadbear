// What a move says about a thread's repo never carries a password or
// token (security review, 10 Nov 2026): the preview sent the thread's
// origin as `git remote get-url` gave it, through a relay command (which
// isn't scrubbed on its way, so the phone's words go as typed), and the
// runner the preview asked read `https://user:github_pat_...@github.com/...`
// whole. Now the address loses its credentials where it's read, the
// preview sends only what the target plans from, scrubbed as sync scrubs,
// and the backend strips an address an older runner sends whole.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { load } from '../helpers/convex-memory.ts';
import { FakeAccounts, makeHarness, tmpDir } from '../helpers/fakes.ts';
import { packGit, withoutCredentials } from '../../src/move-git.ts';
import { moveInfoFrom, previewInfo } from '../../src/moves.ts';
import { Scrubber } from '../../src/scrub.ts';

const TOKEN = 'github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWX';
const CASES: [string, string][] = [
  [`https://user:${TOKEN}@github.com/owner/private.git`, 'https://github.com/owner/private.git'],
  [`https://${TOKEN}@github.com/owner/private.git`, 'https://github.com/owner/private.git'],
  ['https://x-access-token:ghs_abc@github.com/o/r', 'https://github.com/o/r'],
  ['http://u:p@example.com/r.git?token=abc#frag', 'http://example.com/r.git'],
  ['ssh://git:hunter2@github.com/o/r.git', 'ssh://git@github.com/o/r.git'],
  ['ssh://git@github.com/o/r.git', 'ssh://git@github.com/o/r.git'],
  ['git@github.com:mikecann/threadbear.git', 'git@github.com:mikecann/threadbear.git'],
  ['https://github.com/mikecann/threadbear', 'https://github.com/mikecann/threadbear'],
  ['/Users/mike/origin.git', '/Users/mike/origin.git'],
];

test('an address loses anything that could be a password or token, on the runner and in the backend', () => {
  const { withoutCredentials: backend } = load('lib') as { withoutCredentials: (u: string) => string };
  for (const [url, want] of CASES) {
    assert.equal(withoutCredentials(url), want, url);
    assert.equal(backend(url), want, `backend: ${url}`);
  }
});

function repoWithOrigin(origin: string): string {
  const repo = tmpDir('tb-move-cred-');
  const g = (...a: string[]) => execFileSync('git', ['-C', repo, '-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { stdio: 'pipe' });
  g('init', '-q');
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  g('add', '.');
  g('commit', '-qm', 'a');
  g('remote', 'add', 'origin', origin);
  return repo;
}

test("a thread's description and its git work name the repo without its token", async () => {
  const repo = repoWithOrigin(`https://user:${TOKEN}@github.com/owner/private.git`);
  const s = makeHarness(tmpDir('tb-move-home-'), {}, { accounts: new FakeAccounts([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]) });
  const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: repo }).thread.id;
  const info = (await s.runner.describeForMove(id, 0))!;
  assert.equal(info.origin, 'https://github.com/owner/private.git');
  assert.ok(!JSON.stringify(info).includes(TOKEN));
  const packed = await packGit(repo, repo, path.join(tmpDir(), 'b.bundle'));
  assert.ok(packed.ok);
  assert.equal(packed.ok && packed.meta.origin, 'https://github.com/owner/private.git');
});

test('the preview sends the target only what it plans from, scrubbed, and an older target can still read it', () => {
  const scrubber = new Scrubber({ env: {}, folders: false });
  scrubber.known.add('hunter2-very-secret-value');
  const info = moveInfoFrom({
    title: 'Deploy with hunter2-very-secret-value',
    cwd: '/Users/mike/dev/app',
    home: '/Users/mike',
    provider: 'claude',
    accountId: 'claude-main',
    email: 'mike@example.com',
    model: 'opus',
    effort: 'max',
    permissionMode: 'ask',
    origin: `https://user:${TOKEN}@github.com/owner/private.git`,
    repoTop: '/Users/mike/dev/app',
    subdir: '',
    worktree: true,
    sessionId: 'sess-1',
    schedules: 3,
  })!;
  const sent = previewInfo(info, (v) => scrubber.doc(v));
  assert.deepEqual(sent, {
    title: '',
    cwd: '/Users/mike/dev/app',
    home: '/Users/mike',
    provider: 'claude',
    accountId: 'claude-main',
    permissionMode: 'ask',
    worktree: true,
    schedules: 0,
    email: 'mike@example.com',
    origin: 'https://github.com/owner/private.git',
    repoTop: '/Users/mike/dev/app',
    subdir: '',
  });
  // A target from before this reads it as before.
  assert.ok(moveInfoFrom(sent));
  // And it's scrubbed as sync scrubs (which leaves paths alone).
  const withSecret = previewInfo({ ...info, origin: 'https://example.com/hunter2-very-secret-value.git' }, (v) => scrubber.doc(v));
  assert.ok(!JSON.stringify(withSecret).includes('hunter2-very-secret-value'));
});

test("the backend strips a credential from a preview's description an older main Mac sends whole", () => {
  const { commandArgs } = load('relay') as { commandArgs: (kind: string, a: unknown) => Record<string, unknown> };
  const out = commandArgs('call', { method: 'moves.preview', params: { info: { title: 't', cwd: '/c', home: '/h', provider: 'claude', accountId: 'a', permissionMode: 'full', origin: `https://user:${TOKEN}@github.com/o/p.git` } } }) as { params: { info: { origin: string } } };
  assert.equal(out.params.info.origin, 'https://github.com/o/p.git');
  // Nor the title (made from the first message) or the session.
  const more = commandArgs('call', { method: 'moves.preview', params: { info: { title: 'my password is hunter2', sessionId: 's1', cwd: '/c', home: '/h', provider: 'claude', accountId: 'a', permissionMode: 'full' } } }) as { params: { info: Record<string, unknown> } };
  assert.equal(more.params.info.title, '');
  assert.equal(more.params.info.sessionId, undefined);
});
