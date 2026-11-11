// The fixed proofs desktop/client/src/auth.rs checks itself against,
// worked out here with the runner's own code, so a change to the runner's
// auth fails this test until the vector (and the Rust side) follow it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { authProof } from '../../src/rpc-auth.ts';

const token = 'test-token';
const runnerNonce = '1'.repeat(64);
const clientNonce = '2'.repeat(64);
const channel = { server: 'primary' as const, port: 8797 };
const read = (name: string) => fs.readFileSync(new URL(name, import.meta.url), 'utf8').trim();

test('the Rust client signs what the runner expects', () => {
  assert.equal(authProof(token, 'client', runnerNonce, clientNonce, channel), read('../../../desktop/test/auth-vector-client.txt'));
  assert.equal(authProof(token, 'runner', runnerNonce, clientNonce, channel), read('../../../desktop/test/auth-vector-runner.txt'));
});
