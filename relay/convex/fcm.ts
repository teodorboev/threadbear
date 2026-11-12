"use node";
// Sends one notification to an Android phone through Firebase Cloud Messaging
// (the HTTP v1 API), the way apns.ts does for an iPhone. Env: FCM_SERVICE_ACCOUNT,
// the contents of a Firebase service account key (the JSON file from the
// project's Settings, Service accounts, Generate new private key), and
// optionally FCM_SERVICE_ACCOUNT_2 for a second app's project. Whoever
// holds a key sends for every phone that has the app built for that Firebase
// project, so it belongs to whoever publishes the app (docs/push-relay.md).
// With two, a send tries each until one takes the token (fcmPick).
import { createSign } from 'node:crypto';
import { v } from 'convex/values';
import { internalAction } from './_generated/server';
import { fcmMessage, fcmPick, fcmReason, fcmSettled, type FcmResult } from './fcmMessage';

type Account = { client_email: string; private_key: string; project_id: string };

function account(raw: string | undefined): Account | null {
  if (!raw) return null;
  try {
    const a = JSON.parse(raw) as Partial<Account>;
    return typeof a.client_email === 'string' && typeof a.private_key === 'string' && typeof a.project_id === 'string' ? (a as Account) : null;
  } catch {
    return null;
  }
}

// Keith's first (the relay's first Android app, with the most phones), then Mike's.
function accounts(): Account[] {
  return [process.env.FCM_SERVICE_ACCOUNT, process.env.FCM_SERVICE_ACCOUNT_2].map(account).filter((a): a is Account => a !== null);
}

const b64url = (b: Buffer | string) => Buffer.from(b).toString('base64url');

// One access token per key, each good for an hour.
const cached = new Map<string, { token: string; expiresAt: number }>();

// Google wants a signed claim traded for an access token, good for an hour.
async function accessToken(a: Account, nowMs = Date.now()): Promise<{ token: string } | { status: number; reason: string }> {
  const hit = cached.get(a.client_email);
  if (hit && hit.expiresAt - nowMs > 5 * 60_000) return { token: hit.token };
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
    // Bounded like the send: one key's sign-in hanging mustn't hold up the other's.
    signal: AbortSignal.timeout(10_000),
  }).catch(() => null);
  if (!res) return { status: 0, reason: 'timeout' };
  const body = (await res.json().catch(() => ({}))) as { access_token?: string; expires_in?: number };
  if (!res.ok || !body.access_token) return { status: res.status, reason: 'UNAUTHENTICATED' };
  cached.set(a.client_email, { token: body.access_token, expiresAt: nowMs + (body.expires_in ?? 3600) * 1000 });
  return { token: body.access_token };
}

async function sendWith(acct: Account, a: { deviceToken: string; threadId: string; encrypted?: string }): Promise<FcmResult> {
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
}

export const send = internalAction({
  args: { deviceToken: v.string(), threadId: v.string(), encrypted: v.optional(v.string()) },
  returns: v.object({ status: v.number(), reason: v.optional(v.string()) }),
  handler: async (_ctx, a): Promise<{ status: number; reason?: string }> => {
    const keys = accounts();
    if (keys.length === 0) return { status: 0, reason: 'relay not configured' };
    const results: FcmResult[] = [];
    for (const acct of keys) {
      const r = await sendWith(acct, a);
      results.push(r);
      // Sent, or the token's own project said what's wrong with it: no
      // other key can do better, and Keith's phones don't wait on Mike's key.
      if (fcmSettled(r)) break;
    }
    return fcmPick(results)!;
  },
});
