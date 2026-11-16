// What every upload route in convex/http.ts does with its one-use ticket
// (/runner/upload, /phone/upload, /relay/upload): claim it, then read the
// body, then store it and finish against that claim.
//
// The ticket is claimed (a mutation, so only one request can) before a byte
// of the body is read or anything is stored: checking it with a query first
// and using it after storing let many requests with one ticket each read and
// store a body, all but one thrown away after, and none of it counted
// (Daybreak, 15 Nov 2026). A claimed ticket is spent whatever happens next:
// a body that's too big, breaks off, or never comes doesn't give it back.
// The body is read up to the route's cap and no further, whatever its
// Content-Length says, and a stored file the finish doesn't keep is deleted.
//
// The claim and finish mutations are passed in, so this has nothing of
// Convex's own and a test can play a route with the in-memory backend.

/** What a claim mutation answers: the claim to finish with, or why not. */
export type Claimed = { claim: string } | false | 'later';
/** What a finish mutation answers: kept, refused, or over an allowance for now. */
export type Finished = boolean | 'later';

type UploadCtx = {
  runMutation: (fn: any, args: any) => Promise<any>;
  storage: { store: (blob: Blob) => Promise<any>; delete: (id: any) => Promise<void> };
};

export type UploadRoute = {
  maxBytes: number;
  // ({ ticket }) => Claimed
  claim: unknown;
  // ({ ticket, claim, bytes, storageId? }) => Finished; no storageId: the body
  // never arrived whole, so it only closes the claim (bytes says how much came).
  finish: unknown;
  // What a 403 after storing says (a bad or used ticket, or a revoked key).
  refused: string;
};

const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
const refuse = (status: number, error: string) => json(status, { error });
const LATER = 'over the upload allowance for now: try again later';

export async function takeUpload(ctx: UploadCtx, request: Request, route: UploadRoute): Promise<Response> {
  const ticket = new URL(request.url).searchParams.get('ticket') ?? '';
  // Said up front by an honest client, so it's refused before the ticket is
  // spent. Only a plain number counts; anything else is left to the real
  // count below, which holds however the header lies.
  const said = request.headers.get('content-length')?.trim();
  if (said && /^\d+$/.test(said) && Number(said) > route.maxBytes) return refuse(413, 'too big');
  const claimed = (await ctx.runMutation(route.claim, { ticket })) as Claimed;
  if (claimed === 'later') return refuse(429, LATER);
  if (!claimed) return refuse(403, 'bad or used upload ticket');
  const { claim } = claimed;
  const body = await readCapped(request, route.maxBytes);
  if (typeof body === 'string') {
    // Spent and closed, nothing stored. Too big counts as past the cap.
    await ctx.runMutation(route.finish, { ticket, claim, bytes: body === 'tooBig' ? route.maxBytes + 1 : 0 });
    return body === 'tooBig' ? refuse(413, 'too big') : refuse(400, 'the upload broke off');
  }
  const storageId = await ctx.storage.store(body);
  let done: Finished = false;
  try {
    done = (await ctx.runMutation(route.finish, { ticket, claim, storageId, bytes: body.size })) as Finished;
  } finally {
    // Not kept (refused, or the mutation threw): nothing would ever find it.
    if (done !== true) await ctx.storage.delete(storageId);
  }
  if (done === 'later') return refuse(429, LATER);
  if (done !== true) return refuse(403, route.refused);
  return json(200, { storageId });
}

/**
 * The body, read no further than `max` bytes: "tooBig" once it goes past,
 * "broken" if it fails partway. Never holds more than `max` (plus the chunk
 * that went over) in memory.
 */
export async function readCapped(request: Request, max: number): Promise<Blob | 'tooBig' | 'broken'> {
  const type = request.headers.get('content-type') ?? '';
  if (!request.body) return new Blob([], { type });
  const reader = request.body.getReader();
  const parts: Uint8Array[] = [];
  let n = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      n += value.byteLength;
      if (n > max) {
        await reader.cancel().catch(() => undefined);
        return 'tooBig';
      }
      parts.push(value);
    }
  } catch {
    return 'broken';
  }
  return new Blob(parts as BlobPart[], { type });
}

/** A fresh claim: 16 random bytes, hex. */
export function newClaim(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

// How long after its claim a finish is still taken: longer than an HTTP
// action runs, so only a claim whose request died is left behind.
export const UPLOAD_CLAIM_MS = 15 * 60_000;
