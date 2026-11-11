"use node";
// Sends one notification to an Android phone through Firebase Cloud Messaging
// (the HTTP v1 API), the way apns.ts does for an iPhone. Env: FCM_SERVICE_ACCOUNT,
// the contents of a Firebase service account key (the JSON file from the
// project's Settings, Service accounts, Generate new private key). Whoever
// holds that key sends for every phone that has the app built for that Firebase
// project, so it belongs to whoever publishes the app (docs/push-relay.md).
import { createSign } from 'node:crypto';
import { v } from 'convex/values';
import { internalAction } from './_generated/server';
import { fcmMessage, fcmReason } from './fcmMessage';

type Account = { client_email: string; private_key: string; project_id: string };

function account(): Account | null {
  const raw = process.env.FCM_SERVICE_ACCOUNT;
  if (!raw) return null;
  try {
    const a = JSON.parse(raw) as Partial<Account>;
    return typeof a.client_email === 'string' && typeof a.private_key === 'string' && typeof a.project_id === 'string' ? (a as Account) : null;
  } catch {
    return null;
  }
}

const b64url = (b: Buffer | string) => Buffer.from(b).toString('base64url');

let cached: { token: string; expiresAt: number; email: string } | null = null;

// Google wants a signed claim traded for an access token, good for an hour.
async function accessToken(a: Account, nowMs = Date.now()): Promise<{ token: string } | { status: number; reason: string }> {
  if (cached && cached.email === a.client_email && cached.expiresAt - nowMs > 5 * 60_000) return { token: cached.token };
  const iat = Math.floor(nowMs / 1000);
  const claims = { iss: a.client_email, scope: 'https://www.googleapis.com/auth/firebase.messaging', aud: 'https://oauth2.googleapis.com/token', iat, exp: iat + 3600 };
  const input = `${b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }))}.${b64url(JSON.stringify(claims))}`;
  let assertion: string;
  try {
    assertion = `${input}.${b64url(createSign('RSA-SHA256').update(input).sign(a.private_key))}`;
  } catch {
    return { status: 0, reason: 'send threw' };
  }
  const res = await fetch('https://oauth2.googleapis.com/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ grant_type: 'urn:ietf:params:oauth:grant-type:jwt-bearer', assertion }),
  }).catch(() => null);
  if (!res) return { status: 0, reason: 'timeout' };
  const body = (await res.json().catch(() => ({}))) as { access_token?: string; expires_in?: number };
  if (!res.ok || !body.access_token) return { status: res.status, reason: 'UNAUTHENTICATED' };
  cached = { token: body.access_token, expiresAt: nowMs + (body.expires_in ?? 3600) * 1000, email: a.client_email };
  return { token: body.access_token };
}

export const send = internalAction({
  args: { deviceToken: v.string(), threadId: v.string(), encrypted: v.optional(v.string()) },
  returns: v.object({ status: v.number(), reason: v.optional(v.string()) }),
  handler: async (_ctx, a): Promise<{ status: number; reason?: string }> => {
    const acct = account();
    if (!acct) return { status: 0, reason: 'relay not configured' };
    const auth = await accessToken(acct);
    if ('status' in auth) return auth;
    const res = await fetch(`https://fcm.googleapis.com/v1/projects/${encodeURIComponent(acct.project_id)}/messages:send`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${auth.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(fcmMessage(a)),
      signal: AbortSignal.timeout(10_000),
    }).catch((e: Error) => (e.name === 'TimeoutError' ? 'timeout' : 'send threw'));
    if (typeof res === 'string') return { status: 0, reason: res };
    if (res.ok) return { status: 200 };
    const reason = fcmReason(await res.json().catch(() => null));
    return { status: res.status, ...(reason ? { reason } : {}) };
  },
});
