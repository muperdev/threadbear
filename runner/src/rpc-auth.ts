// The runner RPC's challenge auth (docs/protocol.md, "Auth"): neither side
// ever sends the token, each proves it has it with an HMAC over both sides'
// fresh nonces. Only node:crypto, so the .mjs tools (scripts/rpc-client.mjs,
// the mock and fake runners) can import it too.
import crypto from 'node:crypto';

export const AUTH_CONTEXT = 'threadbear-rpc-auth-v1';
export const AUTH_VERSION = 1;

export type AuthRole = 'client' | 'runner';

const THREAD_KEY_CONTEXT = 'threadbear-thread-key-v1';

// A thread's own credential (docs/protocol.md, "A thread's own key"): what its
// CLI gets in place of the runner's token, and what the runner can work out
// again from the id alone, so it stores nothing per thread and a key only ever
// names the thread it was made for.
export function threadKey(token: string, threadId: string): string {
  return crypto.createHmac('sha256', Buffer.from(token, 'utf8')).update(`${THREAD_KEY_CONTEXT}\n${threadId}`).digest('hex');
}

export function isThreadKey(x: unknown): x is string {
  return typeof x === 'string' && /^[0-9a-f]{64}$/.test(x);
}

// A thread id a client may name in auth.prove: short and on one line, as it
// goes into the HMAC and the log.
export function isThreadId(x: unknown): x is string {
  return typeof x === 'string' && x.length > 0 && x.length <= 128 && /^[\w:.-]+$/.test(x);
}

// 32 random bytes, lowercase hex.
export function newNonce(): string {
  return crypto.randomBytes(32).toString('hex');
}

export function isNonce(x: unknown): x is string {
  return typeof x === 'string' && /^[0-9a-f]{64}$/.test(x);
}

// Which server a proof is for: a runner's main server (the one on the
// runner's port, that every client talks to) or a draining runner's own,
// and the TCP port it listens on. Both proofs are bound to it, so a
// challenge relayed from a runner on another port (a draining one, say, by
// whatever took the main port while the runner was down) can't be answered
// by a client that connected here (docs/protocol.md, "Auth").
export type AuthServer = 'primary' | 'drain';
export type AuthChannel = { server: AuthServer; port: number };

// The role is in what's signed so a runner's proof can't be played back as
// a client's (or the other way round) on a connection with the same nonces.
export function authProof(token: string, role: AuthRole, runnerNonce: string, clientNonce: string, channel: AuthChannel): string {
  const text = `${AUTH_CONTEXT} ${role}\n${runnerNonce}\n${clientNonce}\n${channel.server} ${channel.port}`;
  return crypto.createHmac('sha256', Buffer.from(token, 'utf8')).update(text).digest('hex');
}

export function proofMatches(expected: string, candidate: unknown): boolean {
  if (typeof candidate !== 'string') return false;
  const a = Buffer.from(expected);
  const b = Buffer.from(candidate);
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

const isPort = (x: unknown): x is number => Number.isInteger(x) && (x as number) > 0 && (x as number) < 65536;

// The port a client binds its proof to: the one it dialed, for a loopback
// URL (what a relay on this Mac would change), else the one the runner says
// it listens on (behind `tailscale serve`, the dialed port is the proxy's).
export function loopbackPort(url: string): number | undefined {
  const u = new URL(url);
  if (!['127.0.0.1', 'localhost', '[::1]', '::1'].includes(u.hostname)) return undefined;
  return u.port ? Number(u.port) : u.protocol === 'wss:' ? 443 : 80;
}

// A client's side of it. `challenge` is the runner's first message; it
// gives the auth.prove request to send and a check for the runner's answer.
// Anything that isn't a challenge throws: a client never falls back to
// sending the token. `port` is the loopback port the client dialed (see
// loopbackPort); `server` which kind of server it meant to reach.
export function answerChallenge(token: string, challenge: unknown, opts: { id?: number | string; port?: number; server?: AuthServer; threadId?: string } = {}): {
  request: { jsonrpc: '2.0'; id: number | string; method: 'auth.prove'; params: { nonce: string; proof: string; threadId?: string } };
  runnerProved: (result: unknown) => boolean;
} {
  const msg = challenge as { method?: unknown; params?: { nonce?: unknown; port?: unknown } } | null;
  const params = msg && typeof msg === 'object' ? msg.params : undefined;
  const runnerNonce = params?.nonce;
  const port = opts.port ?? params?.port;
  if (msg?.method !== 'auth.challenge' || !isNonce(runnerNonce) || !isPort(port)) {
    throw new RunnerNotProvedError("The runner didn't send an auth challenge, so it isn't the runner or it's older than this client (it updates itself from main).");
  }
  const channel: AuthChannel = { server: opts.server ?? 'primary', port };
  const clientNonce = newNonce();
  return {
    request: { jsonrpc: '2.0', id: opts.id ?? 1, method: 'auth.prove', params: { nonce: clientNonce, proof: authProof(token, 'client', runnerNonce, clientNonce, channel), ...(opts.threadId ? { threadId: opts.threadId } : {}) } },
    runnerProved: (result) => proofMatches(authProof(token, 'runner', runnerNonce, clientNonce, channel), (result as { proof?: unknown } | null)?.proof),
  };
}

export class RunnerNotProvedError extends Error {
  readonly notProved = true;
}
