"use node";
// How a notification is sealed for a phone, apart from push.ts (which needs
// Convex) so the runner's unit tests can check it against what the phones
// open: runner/test/unit/android-push.test.ts, ios/NotificationService and
// an Android app's PushSeal. "use node" as it needs node:crypto, so only
// push.ts (Node too) may import it.
import { createCipheriv, randomBytes } from 'node:crypto';

// AES-256-GCM with the phone's own key, as the app's notification extension
// opens it (CryptoKit's AES.GCM.SealedBox combined: nonce, ciphertext, tag).
// The Android app reads the same bytes the same way.
export function seal(pushKey: string, text: string): string {
  const nonce = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', Buffer.from(pushKey, 'base64'), nonce);
  const ct = Buffer.concat([cipher.update(text, 'utf8'), cipher.final()]);
  return Buffer.concat([nonce, ct, cipher.getAuthTag()]).toString('base64');
}
