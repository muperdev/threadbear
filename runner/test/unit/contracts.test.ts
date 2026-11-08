// The step 4 contract on the runner side: the new protocol types and
// settings, the adapter seam, and the checks the other tests hold events,
// summaries, accounts and CLI statuses to (docs/protocol.md).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { EFFORT_LEVELS, isEffortLevel, isProvider } from '../../src/protocol.ts';
import { DEFAULT_ACCOUNT_NOTES, loadConfig } from '../../src/config.ts';
import { SessionNotFoundError } from '../../src/adapters/types.ts';
import * as context from '../../src/adapters/context.ts';
import * as claudeNormalize from '../../src/adapters/claude/normalize.ts';
import { checkAccount, checkCliStatus, checkEvent, checkSummary } from '../helpers/protocol-check.ts';
import { tmpDir } from '../helpers/fakes.ts';

const ts = '2026-10-27T01:00:00.000Z';

test('effort levels include ultra and providers include codex', () => {
  assert.deepEqual(EFFORT_LEVELS, ['low', 'medium', 'high', 'xhigh', 'max', 'ultra']);
  assert.ok(isEffortLevel('ultra'));
  assert.ok(!isEffortLevel('extreme'));
  assert.ok(isProvider('codex') && isProvider('claude'));
  assert.ok(isProvider('grok'));
  assert.ok(!isProvider('gemini'));
});

test('config: the step 4 settings default safely', () => {
  const home = tmpDir();
  const cfg = loadConfig({ THREADBEAR_HOME: home });
  assert.equal(cfg.codexPath, undefined, 'unset means Threadbear picks the binary');
  assert.deepEqual(cfg.codexArgs, []);
  assert.ok(cfg.codexMainDir.endsWith('/.codex'));
  assert.ok(cfg.tokenStatsLimitsFile.endsWith('/Token Stats/limits.json'));
  assert.deepEqual(cfg.cliUpdates, { claude: 'auto', codex: 'auto', checkMinutes: 60, keepCodexVersions: 2 });
  // Nobody's notes or pool folders by default: Mike's are in his runner.json.
  assert.deepEqual(cfg.accountNotes, {});
  assert.deepEqual(cfg.poolFolders, {});
  assert.notEqual(cfg.accountNotes, DEFAULT_ACCOUNT_NOTES, 'a copy, so nobody edits the default');
  assert.equal(cfg.defaults.models, undefined);
  assert.equal(cfg.defaults.efforts, undefined);
});

test('config: runner.json and env set the step 4 settings', () => {
  const home = tmpDir();
  fs.writeFileSync(
    path.join(home, 'runner.json'),
    JSON.stringify({
      defaults: {
        cwd: '/tmp',
        model: 'claude-haiku-5-5',
        models: { codex: ' gpt-6-luna ', claude: '', gemini: 'x' },
        efforts: { codex: 'low', claude: 'extreme' },
      },
      codexPath: '~/bin/codex',
      codexArgs: ['-c', 'features.memories=false', 3],
      cliUpdates: { codex: 'notify', claude: 'sometimes', checkMinutes: 15, keepCodexVersions: 1 },
      accountNotes: { 'claude-3': 'Tests', 'claude-1': 4 },
    }),
  );
  const cfg = loadConfig({ THREADBEAR_HOME: home });
  assert.equal(cfg.defaults.model, 'claude-haiku-5-5');
  assert.deepEqual(cfg.defaults.models, { codex: 'gpt-6-luna' }, 'blank values and unknown providers are dropped');
  assert.deepEqual(cfg.defaults.efforts, { codex: 'low' });
  assert.ok(cfg.codexPath?.endsWith('/bin/codex') && !cfg.codexPath.startsWith('~'));
  assert.deepEqual(cfg.codexArgs, ['-c', 'features.memories=false']);
  assert.deepEqual(cfg.cliUpdates, { claude: 'auto', codex: 'notify', checkMinutes: 15, keepCodexVersions: 2 });
  assert.deepEqual(cfg.accountNotes, { 'claude-3': 'Tests' }, "runner.json's notes replace the default ones");

  const env = loadConfig({
    THREADBEAR_HOME: home,
    THREADBEAR_CODEX_PATH: '/opt/codex',
    THREADBEAR_CODEX_MAIN_DIR: '/tmp/codex-home',
    THREADBEAR_TOKEN_STATS_LIMITS: '/tmp/limits.json',
    THREADBEAR_CLI_UPDATES: 'off',
  });
  assert.equal(env.codexPath, '/opt/codex');
  assert.equal(env.codexMainDir, '/tmp/codex-home');
  assert.equal(env.tokenStatsLimitsFile, '/tmp/limits.json');
  assert.equal(env.cliUpdates.claude, 'off', 'THREADBEAR_CLI_UPDATES=off wins over runner.json');
  assert.equal(env.cliUpdates.codex, 'off');
});

test('SessionNotFoundError is an Error the runner can tell apart', () => {
  const err: Error = new SessionNotFoundError();
  assert.ok(err instanceof SessionNotFoundError);
  assert.equal(err.name, 'SessionNotFoundError');
  assert.ok(err.message);
  assert.equal(new SessionNotFoundError('thread th_1 not found').message, 'thread th_1 not found');
});

test('the normaliser context lives in adapters/context.ts and the Claude normaliser re-exports it', () => {
  assert.equal(claudeNormalize.fileContext, context.fileContext);
  const dir = tmpDir();
  fs.writeFileSync(path.join(dir, 'a.txt'), 'hello');
  const ctx = context.fileContext(dir, path.join(dir, 'media'), path.join(dir, 'blobs'));
  assert.equal(ctx.readFile('a.txt'), 'hello');
  assert.equal(ctx.readFile('missing.txt'), null);
  assert.ok(fs.readFileSync(ctx.saveBlob('item/1', 'big'), 'utf8') === 'big');
});

test('protocol check: Codex summaries and events', () => {
  const summary = {
    id: 'th_1', title: 'Hi', cwd: '/tmp', accountId: 'codex-main', provider: 'codex', model: 'gpt-6-luna', effort: 'ultra',
    effortInUse: 'low', permissionMode: 'full', status: 'idle', live: false, needsYou: false, createdAt: ts, updatedAt: ts,
    lastActivityAt: ts, preview: '', lastSeq: 3, lastSeenSeq: 3, contextTokens: 1200, contextWindow: 258_400,
  };
  assert.deepEqual(checkSummary(summary), []);
  assert.deepEqual(checkSummary({ ...summary, provider: 'gemini' }), ['provider']);
  assert.deepEqual(checkSummary({ ...summary, contextWindow: '258k' }), ['contextWindow']);

  const env = { seq: 1, ts, threadId: 'th_1', turnId: 't1' };
  assert.deepEqual(checkEvent({ ...env, type: 'tool.call', itemId: 'ig_1', name: 'imageGeneration', kind: 'image', title: 'A cat', input: {} }), []);
  const codexDone = {
    ...env, type: 'turn.completed', turnId: 't1', outcome: 'failed', durationMs: 900, error: 'Usage limit reached', errorKind: 'usageLimit',
    usage: { inputTokens: 10, cachedInputTokens: 4, outputTokens: 2, reasoningTokens: 1 },
  };
  assert.deepEqual(checkEvent(codexDone), []);
  assert.equal(checkEvent({ ...codexDone, errorKind: 'tooBig' }).length, 1);
  assert.equal(checkEvent({ ...codexDone, usage: { inputTokens: 'ten' } }).length, 1);
});

test('protocol check: accounts with usage, notes and Codex models', () => {
  const account = {
    id: 'codex-main', provider: 'codex', title: 'Codex · me@example.com', configDir: '/Users/me/.codex', email: 'me@example.com', plan: 'Pro Lite',
    defaultModel: 'gpt-6.1-sol', defaultEffort: 'high',
    models: [{ id: 'gpt-6-luna', model: 'gpt-6-luna', title: 'GPT-6-Luna', effortLevels: ['low', 'medium', 'high', 'ultra'], defaultEffort: 'medium' }],
    usage: {
      windows: [
        { kind: 'five_hour', title: '5-hour', remainingPercent: 89, resetsAt: ts },
        { kind: 'allowance', title: '$250 allowance', remainingPercent: 53.11612, detail: '$132.79 left' },
      ],
      readAt: ts, source: 'live', blocked: false,
    },
  };
  assert.deepEqual(checkAccount(account), []);
  assert.deepEqual(checkAccount({ id: 'claude-2', provider: 'claude', title: 'x', configDir: '/x', note: 'Convex work only' }), []);
  assert.deepEqual(checkAccount({ ...account, defaultEffort: 'extreme' }), ['defaultEffort']);
  assert.deepEqual(checkAccount({ ...account, usage: { ...account.usage, source: 'guess' } }), ['usage.source']);
  assert.deepEqual(
    checkAccount({ ...account, usage: { ...account.usage, windows: [{ kind: 'hourly', title: 'x', remainingPercent: 120 }] } }),
    ['usage.windows.kind', 'usage.windows.remainingPercent'],
  );
});

test('protocol check: CLI statuses', () => {
  const codex = {
    provider: 'codex', path: '/h/tools/codex/current', managedBy: 'threadbear', installedVersion: '0.164.0', latestVersion: '0.165.0',
    status: 'held', policy: 'auto', inUse: [{ version: '0.164.0', threadIds: ['th_1'] }], previousVersion: '0.163.0', checkedAt: ts,
    hold: { version: '0.165.0', reason: 'the app-server protocol changed', missing: ['turn/steer'] },
    update: { state: 'failed', targetVersion: '0.165.0', finishedAt: ts, message: 'held' },
  };
  assert.deepEqual(checkCliStatus(codex), []);
  assert.deepEqual(checkCliStatus({ provider: 'claude', path: '/c', managedBy: 'cli', status: 'unknown', policy: 'off', inUse: [] }), []);
  assert.deepEqual(checkCliStatus({ ...codex, status: 'stale', update: { state: 'done' } }), ['status', 'update.state']);
});
