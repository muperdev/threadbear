// What the relay sends Firebase for an Android phone, and what it makes of the
// answer. Plain TypeScript with no Convex imports, so the runner's unit tests
// can check it (runner/test/unit/android-push.test.ts).

// A Firebase registration token, as the Android app sends it: letters, digits
// and "_", "-", ":" (about 160 characters today; Firebase promises no length).
// Keep in step with convex/phone.ts.
export const FCM_TOKEN = /^[A-Za-z0-9_:-]{20,4096}$/;

// Data only, with no `notification` part: Firebase would show that itself,
// and the app must open the sealed text first (docs/push-relay.md). High
// priority, so it comes at once with the phone asleep; and it is worth
// nothing after an hour, when the person has long since looked at the thread.
export function fcmMessage(a: { deviceToken: string; threadId: string; encrypted?: string }) {
  return {
    message: {
      token: a.deviceToken,
      data: { threadId: a.threadId, ...(a.encrypted ? { e: a.encrypted } : {}) },
      android: { priority: 'HIGH' as const, ttl: '3600s' },
    },
  };
}

// Firebase says a token is no good with 404 UNREGISTERED (the app was removed
// or its data cleared). The backend then forgets the phone.
export function fcmGone(status: number, reason?: string): boolean {
  return status === 404 || reason === 'UNREGISTERED';
}

// The word Firebase puts in an error answer ({ error: { status: "UNREGISTERED" } }),
// or nothing if the answer is not that shape. Only its status word is kept: the
// message beside it can name the token.
export function fcmReason(body: unknown): string | undefined {
  const status = (body as { error?: { status?: unknown } } | null)?.error?.status;
  return typeof status === 'string' && /^[A-Z_]{3,40}$/.test(status) ? status : undefined;
}
