// The table of what each provider is called and can do (providers.ts). The
// runner's checks and the app read this rather than comparing provider names.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { discoverAccounts } from '../../src/accounts.ts';
import { readPools } from '../../src/pools.ts';
import { activeProviders, cliName, PROVIDER_INFO, providerInfo, providerName, providersForHello } from '../../src/providers.ts';
import { PROVIDERS, type Account } from '../../src/protocol.ts';
import { FakeAccounts, makeHarness, withCodex } from '../helpers/fakes.ts';

test('every provider has a row, under its own id', () => {
  for (const p of PROVIDERS) assert.equal(PROVIDER_INFO[p].id, p);
  assert.equal(Object.keys(PROVIDER_INFO).length, PROVIDERS.length);
});

test('names stay as they were before the table', () => {
  assert.equal(providerName('claude'), 'Claude');
  assert.equal(providerName('codex'), 'Codex');
  assert.equal(cliName('claude'), 'Claude Code');
  assert.equal(cliName('codex'), 'Codex');
});

test('what Claude and Codex can each do', () => {
  assert.equal(PROVIDER_INFO.claude.sandbox, true, 'only Claude runs in the sandbox');
  assert.equal(PROVIDER_INFO.codex.sandbox, false);
  assert.equal(PROVIDER_INFO.claude.autoMode, 'classifier');
  assert.equal(PROVIDER_INFO.codex.autoMode, 'asks');
  assert.equal(PROVIDER_INFO.claude.smallJobs, true);
  assert.equal(PROVIDER_INFO.codex.smallJobs, false);
  assert.equal(PROVIDER_INFO.claude.loginCommand, 'claude auth login');
  assert.equal(PROVIDER_INFO.codex.loginCommand, 'codex login');
});

test("a provider the table doesn't know can do nothing, and never reads as Claude", () => {
  // A record or another runner's hello from a build that has one more.
  const info = providerInfo('mystery');
  assert.equal(info.name, 'mystery');
  assert.equal(info.effort, 'none');
  assert.equal(info.autoMode, 'asks');
  for (const can of [info.sandbox, info.rewind, info.smallJobs, info.usageReadings]) assert.equal(can, false);
});

test("names from Object.prototype aren't providers", () => {
  // A plain lookup would hand back the function and read its `sandbox`.
  assert.equal(providerInfo('constructor').sandbox, false);
  assert.equal(providerInfo('constructor').name, 'constructor');
});

test('a thread with no provider is a Claude one, as a record from before Codex', () => {
  assert.equal(providerName(undefined), 'Claude');
  assert.equal(providerName(''), 'Claude');
});

function home(on: boolean): string {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-prov-'));
  if (on) fs.writeFileSync(path.join(d, 'runner.json'), JSON.stringify({ experiments: { acpAgents: true } }));
  return d;
}

test('the ACP agents are only offered with the experiment on', () => {
  assert.deepEqual(activeProviders(home(false)), ['claude', 'codex']);
  assert.deepEqual(activeProviders(home(true)), ['claude', 'codex', 'cursor', 'opencode']);
});

test('with the experiment off, an installed agent adds no account', () => {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-disc-'));
  const bin = path.join(d, 'oc');
  fs.writeFileSync(bin, '#!/bin/sh\n', { mode: 0o755 });
  const cfg = { claudeMainDir: path.join(d, 'none'), claudeMainJson: path.join(d, 'none.json'), tokenStatsAccountsDir: path.join(d, 'none2'), opencodePath: bin, cursorPath: path.join(d, 'no-cursor') };
  assert.deepEqual(discoverAccounts({ ...cfg, home: home(false) }), []);
  assert.deepEqual(discoverAccounts({ ...cfg, home: home(true) }).map((a) => a.id), ['opencode-main']);
});

test('an ACP model is only ever the one its account lists', () => {
  const oc: Account = { id: 'opencode-main', provider: 'opencode', title: 'OpenCode', configDir: '/x', models: [{ id: 'fake/one', model: 'fake/one', title: 'One' }, { id: 'gpt-9', model: 'gpt-9', title: 'G' }] };
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts([...withCodex(), oc]) });
  const claude = h.runner.hello().accounts.find((a) => a.provider === 'claude')!;
  assert.equal(h.runner.modelFits(oc, 'fake/one'), true);
  assert.equal(h.runner.modelFits(oc, 'claude-opus-5-5'), false, 'not by its name');
  assert.equal(h.runner.modelFits(claude, 'fake/one'), false);
  assert.equal(h.runner.modelProvider('fake/one'), 'opencode');
  assert.equal(h.runner.modelProvider('gpt-9'), 'codex', 'another provider keeps what it has always claimed');
});

test('a saved pool of only an agent this runner does not offer is gone', () => {
  const accounts: Account[] = [{ id: 'claude-main', provider: 'claude', title: 'c', configDir: '/c' }];
  const pools = [{ id: 'oc', title: 'OC', accountIds: ['opencode-main'] }, { id: 'mine', title: 'Mine', accountIds: ['claude-main', 'opencode-main'] }];
  assert.deepEqual(readPools({ pools }, accounts, {}).map((p) => p.id), ['mine']);
});

test('hello lists each provider this runner offers', () => {
  assert.deepEqual(providersForHello().map((p) => p.id), [...PROVIDERS]);
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()) });
  assert.deepEqual(h.runner.hello().providers.map((p) => p.name), ['Claude', 'Codex']);
});
