// What the push relay sends an Android phone through Firebase
// (relay/convex/fcmMessage.ts), and the sealed text the phone opens. The
// Android app opens it with Java's AES/GCM; this reads the same bytes the way
// that does, so a change to convex/pushSeal.ts seal() that the phone cannot
// read fails here, before a phone does.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { FCM_TOKEN, fcmGone, fcmMessage, fcmReason } from '../../../relay/convex/fcmMessage.ts';
import { seal } from '../../../convex/pushSeal.ts';

test('the message is data only, with high priority and a short life', () => {
  const m = fcmMessage({ deviceToken: 'tok:APA91bExample', threadId: 'th-1', encrypted: 'c2VhbGVk' });
  assert.deepEqual(m, { message: { token: 'tok:APA91bExample', data: { threadId: 'th-1', e: 'c2VhbGVk' }, android: { priority: 'HIGH', ttl: '3600s' } } });
  // No `notification` part: Firebase would show it as it is, and the text is sealed
  assert.equal('notification' in m.message, false);
  // Sent without sealed text (the backend had no key for the phone), only the thread
  assert.deepEqual(fcmMessage({ deviceToken: 'tok:APA91bExample', threadId: 'th-1' }).message.data, { threadId: 'th-1' });
});

test('a Firebase token is letters, digits and _ - : of a sane length', () => {
  assert.ok(FCM_TOKEN.test('fXyZ1234567890abcdef:APA91bHexample_token-with-dashes_0123456789abcdefghij'));
  for (const bad of ['', 'short', 'a'.repeat(19), 'a'.repeat(4097), 'has space in it, a long one ok', 'slash/' + 'a'.repeat(30), 'new\nline' + 'a'.repeat(30), 'tok:APA91b' + 'é'.repeat(20)]) {
    assert.equal(FCM_TOKEN.test(bad), false, JSON.stringify(bad));
  }
});

test('Firebase says a phone is gone with 404 UNREGISTERED, and only its status word is kept', () => {
  assert.equal(fcmGone(404, 'UNREGISTERED'), true);
  assert.equal(fcmGone(404), true);
  assert.equal(fcmGone(200), false);
  assert.equal(fcmGone(400, 'INVALID_ARGUMENT'), false, 'a bad payload is not a gone phone');
  assert.equal(fcmGone(503, 'UNAVAILABLE'), false);
  assert.equal(fcmReason({ error: { code: 404, status: 'UNREGISTERED', message: 'Requested entity was not found for token abc' } }), 'UNREGISTERED');
  assert.equal(fcmReason({ error: { status: 'not a status word with the token abc' } }), undefined);
  assert.equal(fcmReason({ error: {} }), undefined);
  assert.equal(fcmReason(null), undefined);
  assert.equal(fcmReason('UNREGISTERED'), undefined);
});

// Java's GCM takes the ciphertext with the tag on its end, after a 12-byte nonce
// taken off the front: this is what PushSeal.kt does.
function openLikeTheApp(sealed: string, key: Buffer): { title: string; body: string } {
  const data = Buffer.from(sealed, 'base64');
  assert.ok(data.length >= 12 + 16);
  const d = crypto.createDecipheriv('aes-256-gcm', key, data.subarray(0, 12));
  d.setAuthTag(data.subarray(data.length - 16));
  return JSON.parse(Buffer.concat([d.update(data.subarray(12, data.length - 16)), d.final()]).toString('utf8'));
}

test("what the backend seals is what the app opens: nonce, ciphertext, tag, around the title and body", () => {
  const key = crypto.randomBytes(32);
  const sealed = seal(key.toString('base64'), JSON.stringify({ title: 'Fix the dock badge', body: 'Done ✓ it clears' }));
  assert.deepEqual(openLikeTheApp(sealed, key), { title: 'Fix the dock badge', body: 'Done ✓ it clears' });
  // A different phone's key opens nothing
  assert.throws(() => openLikeTheApp(sealed, crypto.randomBytes(32)));
  // And the registration pattern in convex/phone.ts takes a key made that way
  assert.match(key.toString('base64'), /^[A-Za-z0-9+/]{43}=$/);
});
