// devices.list, devices.pair and devices.revoke (docs/protocol.md,
// "Devices"): the Settings window's Phone & browser tab. Each is a call to
// the backend through the mirror's client (sync.ts), so pairing a phone or
// signing a browser in needs no Convex CLI. A browser's token goes back in
// the reply, inside the sign-in link the app opens, and a phone's one-use
// pairing code inside the text its QR code draws, and nowhere
// else: not the log, not a file, and no tb command prints one, since a
// thread's transcript can command agents.
import type { ConvexSync, Device, DeviceKind } from './sync.ts';
import { DEVICE_KINDS } from './sync.ts';
import { ErrorCode, RpcError } from './protocol.ts';
import { localOnly } from './convex-admin.ts';

export const DEFAULT_DAYS = 90;
export const MAX_DAYS = 365;

export type DeviceList = { devices: Device[]; site?: string; reason?: string };
export type Pairing = { device: Device; pairingLink?: string; signInLink?: string };

// The website is served from the deployment's .convex.site host
// (docs/website.md), or a self-hosted backend's sync.site; a local or odd
// URL has no site.
export { siteOf } from './convex-admin.ts';

// What the phone apps scan (ios/ThreadbearKit Pairing.swift, the Android
// kit's Pairing.kt): `TBPAIR1_` and then, base64url without padding, the
// query `url=<deployment>&code=<one-use code>&key=<runnerId>:<public key>…`,
// a `key` for each runner whose secret-box key the phone may seal to. The
// phone takes runner keys only from here, as they come straight from this
// Mac and not through the mirror.
//
// Not a link, on purpose (Daybreak finding 127, 15 Nov 2026). It used to be
// `threadbear://pair?url=…&token=…`, and any app on the phone can claim the
// threadbear scheme, so pointing the Camera at the code could hand another
// app a 90-day token. This has no scheme for anything to claim, no dots for
// a link finder to take for a web address, and only a code that's good for
// ten minutes and once, which the phone's own scanner trades for its token
// (convex/tokens.ts, claim). `_`, letters and digits only, so the frozen
// Swift app's URL decoding of `pairingLink` still takes it as it is.
export const PAIRING_PREFIX = 'TBPAIR1_';

export function pairingText(url: string, code: string, keys: { runnerId: string; key: string }[] = []): string {
  const q = new URLSearchParams();
  q.set('url', url);
  q.set('code', code);
  for (const k of keys) q.append('key', `${k.runnerId}:${k.key}`);
  return PAIRING_PREFIX + Buffer.from(q.toString(), 'utf8').toString('base64url');
}

/** The query inside a pairing text, for tests and `tb`: undefined for anything else. */
export function readPairingText(text: string): URLSearchParams | undefined {
  if (!text.startsWith(PAIRING_PREFIX)) return undefined;
  const body = text.slice(PAIRING_PREFIX.length);
  if (!/^[A-Za-z0-9_-]+$/.test(body)) return undefined;
  return new URLSearchParams(Buffer.from(body, 'base64url').toString('utf8'));
}

// What the website takes (web/src/token.ts): the token in the #fragment,
// which a browser never sends to a server.
export function signInLink(site: string, token: string): string {
  return `${site}/#token=${token}`;
}

// `onPaired`: a phone or browser token was issued here, the moment full sync
// starts to help (convex-use.ts FullSyncOffer).
export function devicesMethods(sync: ConvexSync, onPaired?: (kind: DeviceKind) => void): Record<string, (p: Record<string, unknown>) => unknown> {
  return {
    'devices.list': async (): Promise<DeviceList> => {
      const reason = sync.offReason();
      if (reason) return { devices: [], reason };
      const devices = await sync.listTokens().catch((e) => {
        throw new RpcError(ErrorCode.other, (e as Error).message);
      });
      const site = sync.site();
      return { devices, ...(site ? { site } : {}) };
    },
    'devices.pair': async (p): Promise<Pairing> => {
      const kind = p.kind;
      if (typeof kind !== 'string' || !(DEVICE_KINDS as readonly string[]).includes(kind)) throw bad(`kind must be one of ${DEVICE_KINDS.join(', ')}`);
      const name = p.name === undefined || p.name === null ? defaultName(kind as DeviceKind) : p.name;
      if (typeof name !== 'string' || !name.trim()) throw bad('name must be a non-empty string');
      let days = DEFAULT_DAYS;
      if (p.days !== undefined && p.days !== null) {
        if (typeof p.days !== 'number' || !Number.isFinite(p.days) || p.days < 1 || p.days > MAX_DAYS) throw bad(`days must be 1 to ${MAX_DAYS}`);
        days = Math.floor(p.days);
      }
      const url = sync.url();
      if (!url) throw new RpcError(ErrorCode.other, offMessage(sync));
      // A pairing link to 127.0.0.1 would send the phone to itself.
      if (kind === 'phone' && localOnly(url)) throw new RpcError(ErrorCode.other, LOCAL_ONLY_PHONE);
      const site = sync.site();
      if (kind === 'browser' && !site) throw new RpcError(ErrorCode.other, `This runner doesn't know where the website is for ${url}. A self-hosted backend needs sync.site in runner.json (docs/self-hosting.md).`);
      if (kind === 'phone') {
        // A one-use code, never the token (pairingText says why). The
        // phone's token is made when it trades the code, so the device it
        // will be shows in devices.list from then; `device` here stands for
        // it, with the code's id and a 90-day guess at its expiry.
        let issued: { id: string; code: string; expiresAt: string };
        try {
          issued = await sync.issuePairingCode({ name: name.trim(), days });
        } catch (e) {
          throw new RpcError(ErrorCode.other, (e as Error).message);
        }
        onPaired?.('phone');
        const device: Device = { id: issued.id, kind: 'phone', name: name.trim(), createdAt: new Date().toISOString(), expiresAt: new Date(Date.now() + days * 86_400_000).toISOString(), expired: false };
        return { device, pairingLink: pairingText(url, issued.code, sync.pairingKeys()) };
      }
      let issued: { id: string; token: string; expiresAt: string };
      try {
        issued = await sync.issueToken({ kind: kind as DeviceKind, name: name.trim(), days });
      } catch (e) {
        throw new RpcError(ErrorCode.other, (e as Error).message);
      }
      onPaired?.(kind as DeviceKind);
      const device: Device = { id: issued.id, kind: kind as DeviceKind, name: name.trim(), createdAt: new Date().toISOString(), expiresAt: issued.expiresAt, expired: false };
      return { device, signInLink: signInLink(site!, issued.token) };
    },
    'devices.revoke': async (p): Promise<DeviceList> => {
      const id = p.deviceId;
      if (typeof id !== 'string' || !id) throw bad('deviceId must be a string');
      if (sync.offReason()) throw new RpcError(ErrorCode.other, offMessage(sync));
      let found: boolean;
      try {
        found = await sync.revokeToken(id);
      } catch (e) {
        throw new RpcError(ErrorCode.other, (e as Error).message);
      }
      if (!found) throw bad(`No phone or browser with id ${id}`);
      const devices = await sync.listTokens().catch(() => [] as Device[]);
      const site = sync.site();
      return { devices, ...(site ? { site } : {}) };
    },
  };
}

export const LOCAL_ONLY_PHONE = "Your backend is on this Mac only (setup --self-hosted --local), so a phone can't reach it. To use the iPhone app, run ./threadbear setup --self-hosted --tailnet (with Tailscale), or --self-hosted --url and --site-url if you reach this Mac your own way (docs/self-hosting.md).";

function defaultName(kind: DeviceKind): string {
  return kind === 'phone' ? 'iPhone' : 'Browser';
}

function offMessage(sync: ConvexSync): string {
  return `Your threads aren't mirrored to Convex (${sync.offReason() ?? 'sync is off'}), so there's nothing for a phone or browser to read. Run ./threadbear setup.`;
}

function bad(message: string): RpcError {
  return new RpcError(ErrorCode.badParams, message);
}
