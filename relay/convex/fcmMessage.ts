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

// Firebase says a token is no good with 404 NOT_FOUND whose error code is
// UNREGISTERED (the app was removed or its data cleared), which fcmReason
// reads. The backend then forgets the phone. Only that verdict counts, not
// any 404: with two keys a 404 could come from a project the token isn't
// even in, and a bare 404 (a wrong URL, a deleted project) says nothing
// about the phone.
export function fcmGone(_status: number, reason?: string): boolean {
  return reason === 'UNREGISTERED';
}

// The word Firebase puts in an error answer ({ error: { status: "UNREGISTERED" } }),
// or nothing if the answer is not that shape. Only its status word is kept: the
// message beside it can name the token. Firebase's own error code, in the
// answer's details, goes first when there is one: a token from another
// Firebase project is a 403 PERMISSION_DENIED whose code says
// SENDER_ID_MISMATCH, and only the code tells that apart from a key that
// lacks the permission.
export function fcmReason(body: unknown): string | undefined {
  const error = (body as { error?: { status?: unknown; details?: unknown } } | null)?.error;
  const word = (w: unknown) => (typeof w === 'string' && /^[A-Z_]{3,40}$/.test(w) ? w : undefined);
  const details = Array.isArray(error?.details) ? error.details : [];
  for (const d of details) {
    const code = word((d as { errorCode?: unknown } | null)?.errorCode);
    if (code) return code;
  }
  return word(error?.status);
}

// The relay can hold more than one Firebase key, one per app built against
// its own Firebase project (Keith's Android app and Mike's, since 11 Nov
// 2026). A phone's token only works with its own project, and the relay
// keeps no tokens to remember which (docs/push-relay.md), so a send tries
// each key in turn. Firebase answers another project's token with 403
// PERMISSION_DENIED whose code is SENDER_ID_MISMATCH. Only a word about
// the token itself (UNREGISTERED, INVALID_ARGUMENT) is the token's own
// project speaking: the send stops there (fcmSettled), and that answer
// wins (fcmPick). A status alone settles nothing: a key's own sign-in
// failing comes back as a 400 or 401 too, and the next key must still be
// tried.
export type FcmResult = { status: number; reason?: string };
export const wrongProject = (r: FcmResult) => r.reason === 'SENDER_ID_MISMATCH';
/** The token's own project has answered: sent, or a verdict on the token. */
export const fcmSettled = (r: FcmResult) => r.status === 200 || r.reason === 'UNREGISTERED' || r.reason === 'INVALID_ARGUMENT';
// With nothing settled, a failure of the relay's own (a timeout, a 503) is
// answered before a 404 that isn't UNREGISTERED: fcmGone takes any 404 as a
// gone phone, and a project that never claimed the token mustn't drop
// another project's live one.
export function fcmPick(results: FcmResult[]): FcmResult | undefined {
  const unsettled = results.filter((r) => !wrongProject(r));
  return results.find(fcmSettled) ?? unsettled.find((r) => r.status !== 404) ?? unsettled[0] ?? results[0];
}
