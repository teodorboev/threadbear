// What the push relay sends an Android phone through Firebase
// (relay/convex/fcmMessage.ts), and the sealed text the phone opens. The
// Android app opens it with Java's AES/GCM; this reads the same bytes the way
// that does, so a change to convex/pushSeal.ts seal() that the phone cannot
// read fails here, before a phone does.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { FCM_TOKEN, fcmGone, fcmMessage, fcmPick, fcmReason, fcmSettled } from '../../../relay/convex/fcmMessage.ts';
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
  // A 404 without Firebase's verdict says nothing about the phone (a wrong URL, a deleted project)
  assert.equal(fcmGone(404), false);
  assert.equal(fcmGone(404, 'NOT_FOUND'), false);
  // What Firebase really sends for a removed app: NOT_FOUND, with UNREGISTERED in the details
  assert.equal(fcmReason({ error: { code: 404, status: 'NOT_FOUND', details: [{ errorCode: 'UNREGISTERED' }] } }), 'UNREGISTERED');
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

test("Firebase's own error code wins over the status word, so another project's token is told apart", () => {
  const mismatch = { error: { code: 403, status: 'PERMISSION_DENIED', message: 'SenderId mismatch', details: [{ '@type': 'type.googleapis.com/google.firebase.fcm.v1.FcmError', errorCode: 'SENDER_ID_MISMATCH' }] } };
  assert.equal(fcmReason(mismatch), 'SENDER_ID_MISMATCH');
  // A key without the permission is the plain status word
  assert.equal(fcmReason({ error: { code: 403, status: 'PERMISSION_DENIED', details: [] } }), 'PERMISSION_DENIED');
  // A code that isn't a plain word is left out, as the status is
  assert.equal(fcmReason({ error: { status: 'UNREGISTERED', details: [{ errorCode: 'token abc was bad' }] } }), 'UNREGISTERED');
  assert.equal(fcmReason({ error: { status: 'UNREGISTERED', details: 'not a list' } }), 'UNREGISTERED');
});

test('with two keys, the answer is the one that worked, else the token\'s own project\'s', () => {
  const wrong = { status: 403, reason: 'SENDER_ID_MISMATCH' };
  // Mike's phone: Keith's key turns it down, Mike's sends
  assert.deepEqual(fcmPick([wrong, { status: 200 }]), { status: 200 });
  // Keith's phone, sent first time
  assert.deepEqual(fcmPick([{ status: 200 }]), { status: 200 });
  // A removed app on Mike's phone: his own project says UNREGISTERED, so the backend forgets it
  const gone = fcmPick([wrong, { status: 404, reason: 'UNREGISTERED' }])!;
  assert.equal(fcmGone(gone.status, gone.reason), true);
  // ...and the same the other way round
  assert.deepEqual(fcmPick([{ status: 404, reason: 'UNREGISTERED' }, wrong]), { status: 404, reason: 'UNREGISTERED' });
  // A bare 404 next to another project's no: not gone either
  const bare = fcmPick([{ status: 404, reason: 'NOT_FOUND' }, wrong])!;
  assert.equal(fcmGone(bare.status, bare.reason), false);
  // Neither project knows it: the first answer, which is not "gone"
  assert.deepEqual(fcmPick([wrong, wrong]), wrong);
  assert.equal(fcmGone(wrong.status, wrong.reason), false);
  assert.equal(fcmPick([]), undefined);
});

test("a key that fails for its own reasons doesn't hide the token's own project's answer", () => {
  const wrong = { status: 403, reason: 'SENDER_ID_MISMATCH' };
  // Keith's key broken (401), Mike's project says the token's gone: gone wins
  assert.deepEqual(fcmPick([{ status: 401, reason: 'UNAUTHENTICATED' }, { status: 404, reason: 'UNREGISTERED' }]), { status: 404, reason: 'UNREGISTERED' });
  // Keith's key timed out, Mike's sent
  assert.deepEqual(fcmPick([{ status: 0, reason: 'timeout' }, { status: 200 }]), { status: 200 });
  // Only failures of keys: the first, which isn't "gone", so nothing's dropped
  assert.deepEqual(fcmPick([{ status: 503, reason: 'UNAVAILABLE' }, wrong]), { status: 503, reason: 'UNAVAILABLE' });
  // What stops the tries: sent, or a verdict on the token from its own project
  assert.equal(fcmSettled({ status: 200 }), true);
  assert.equal(fcmSettled({ status: 404, reason: 'UNREGISTERED' }), true);
  assert.equal(fcmSettled({ status: 400, reason: 'INVALID_ARGUMENT' }), true);
  assert.equal(fcmSettled(wrong), false);
  assert.equal(fcmSettled({ status: 401, reason: 'UNAUTHENTICATED' }), false);
  assert.equal(fcmSettled({ status: 0, reason: 'timeout' }), false);
  assert.equal(fcmSettled({ status: 503 }), false);
});

test("a key's own sign-in failing never settles a send, and an unclaimed 404 doesn't drop another project's phone", () => {
  // Keith's key can't sign in (Google's token endpoint answers 400): Mike's key is still tried
  assert.equal(fcmSettled({ status: 400, reason: 'UNAUTHENTICATED' }), false);
  assert.deepEqual(fcmPick([{ status: 400, reason: 'UNAUTHENTICATED' }, { status: 200 }]), { status: 200 });
  // Keith's project hiccups, Mike's says a bare 404: the hiccup is the answer, so Keith's phone stays
  const picked = fcmPick([{ status: 503, reason: 'UNAVAILABLE' }, { status: 404, reason: 'NOT_FOUND' }])!;
  assert.deepEqual(picked, { status: 503, reason: 'UNAVAILABLE' });
  assert.equal(fcmGone(picked.status, picked.reason), false);
  // A malformed token is the same verdict from either project
  assert.equal(fcmSettled({ status: 400, reason: 'INVALID_ARGUMENT' }), true);
});
