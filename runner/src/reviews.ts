// Validation and text for durable reviews of pictures, videos and pages.
// Files are copied only after every input has passed validation, so a bad
// final option leaves no card.
import fs from 'node:fs';
import path from 'node:path';
import { expandHome } from './config.ts';
import { readImageInfo, readVideoInfo } from './media.ts';
import type { DesignReview, ReviewDraft } from './protocol.ts';
import { ErrorCode, RpcError } from './protocol.ts';

// An option's pictures: `path` and then `shots`, or `shots` alone. One option
// often has several (its overview, mid-drag, a detail), which agents used to
// pass as options of their own, A2 and A3, that couldn't be picked (Mike, 4 Nov).
// kind is worked out here from the file: a picture, a video (MP4 or
// QuickTime) or an HTML page. The window plays a video and shows a page in
// place of a still, as agents had to hand Mike stills of clips that only
// made sense moving (6 Nov 2026).
export type ReviewShotKind = 'image' | 'video' | 'page';
export type ReviewShotInput = { path: string; title?: string; kind?: ReviewShotKind };
export type ReviewInput = { title: string; note?: string; recommended?: string; options: { id: string; title: string; path?: string; shots?: ReviewShotInput[]; url?: string; note?: string; reference?: boolean }[] };
// What createReview copies: every source file, first shot first.
export type ReviewOptionSource = { id: string; title: string; note?: string; reference?: boolean; url?: string; shots: ReviewShotInput[] };
export const maxReviewShots = 8;
export const maxReviewImages = 40;
const maxImageBytes = 25 * 1024 * 1024;
const maxVideoBytes = 200 * 1024 * 1024;
const maxPageBytes = 5 * 1024 * 1024;
// All of a review's videos and pages together, as each is copied in.
export const maxReviewMediaBytes = 500 * 1024 * 1024;

function bad(message: string): never {
  throw new RpcError(ErrorCode.badParams, message);
}

function text(v: unknown, name: string, max: number, required = false): string | undefined {
  if (v === undefined && !required) return undefined;
  if (typeof v !== 'string' || v.length > max || (required && !v.trim())) bad(`${name} must be ${required ? 'nonempty text' : 'text'} of at most ${max} characters.`);
  return (v as string).trim();
}

function shotFile(raw: unknown, cwd: string): { path: string; kind: ReviewShotKind; size: number } {
  const file = path.resolve(cwd, expandHome(text(raw, 'path', 8192, true)!));
  let size: number;
  try {
    const st = fs.statSync(file);
    if (!st.isFile()) bad(`Not a regular file: ${file}`);
    size = st.size;
  } catch (e) {
    if (e instanceof RpcError) throw e;
    bad(`File not found: ${file}`);
  }
  if (/\.html?$/i.test(file)) {
    if (size > maxPageBytes) bad(`An HTML page must be at most 5 MB: ${file}`);
    return { path: file, kind: 'page', size };
  }
  if (size <= maxImageBytes) {
    const info = readImageInfo(file);
    if (info?.width && info.height) return { path: file, kind: 'image', size };
  }
  if (readVideoInfo(file)) {
    if (size > maxVideoBytes) bad(`A video must be at most 200 MB: ${file}`);
    return { path: file, kind: 'video', size };
  }
  if (size > maxImageBytes) bad(`An image must be at most 25 MB: ${file}`);
  return bad(`Not a picture (PNG, JPEG, GIF, WebP), video (MP4, MOV) or HTML page: ${file}`);
}

export function reviewInput(raw: unknown, cwd: string): { title: string; note?: string; recommended?: string; options: ReviewOptionSource[] } {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return bad('A review needs a title and options.');
  const p = raw as Record<string, unknown>;
  const title = text(p.title, 'title', 200, true)!;
  const note = text(p.note, 'note', 4000);
  if (!Array.isArray(p.options) || !p.options.length || p.options.length > 12) return bad('A review needs between 1 and 12 options.');
  const ids = new Set<string>();
  let images = 0;
  let mediaBytes = 0;
  const options = p.options.map((raw, i) => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return bad(`options[${i}] must be an object.`);
    const o = raw as Record<string, unknown>;
    const id = text(o.id, 'option id', 200, true)!;
    if (ids.has(id)) bad(`Duplicate option id: ${id}`);
    ids.add(id);
    const title = text(o.title, 'option title', 200, true)!;
    const note = text(o.note, 'option note', 4000);
    if (o.reference !== undefined && typeof o.reference !== 'boolean') bad('reference must be a boolean.');
    const url = text(o.url, 'url', 2000);
    if (url && !/^https?:\/\/\S+$/i.test(url)) bad(`url must be an http or https address: ${url}`);
    if (o.shots !== undefined && !Array.isArray(o.shots)) bad(`shots on ${id} must be a list.`);
    const shots: ReviewShotInput[] = [];
    const add = (raw: unknown, title?: string) => {
      const file = shotFile(raw, cwd);
      if (file.kind !== 'image') mediaBytes += file.size;
      shots.push({ path: file.path, kind: file.kind, ...(title ? { title } : {}) });
    };
    if (o.path !== undefined) add(o.path);
    for (const [j, s] of ((o.shots as unknown[] | undefined) ?? []).entries()) {
      if (!s || typeof s !== 'object' || Array.isArray(s)) bad(`shots[${j}] on ${id} must be an object with a path.`);
      const shot = s as Record<string, unknown>;
      add(shot.path, text(shot.title, 'shot title', 200));
    }
    if (!shots.length) bad(`Option ${id} needs an image, video or page: a path or shots.`);
    if (shots.length > maxReviewShots) bad(`Option ${id} has ${shots.length} images; at most ${maxReviewShots}.`);
    images += shots.length;
    return { id, title, shots, ...(note ? { note } : {}), ...(o.reference === true ? { reference: true } : {}), ...(url ? { url } : {}) };
  });
  if (images > maxReviewImages) bad(`A review can have at most ${maxReviewImages} images in all; this has ${images}.`);
  if (mediaBytes > maxReviewMediaBytes) bad(`A review's videos and pages can be at most 500 MB in all; these are ${Math.ceil(mediaBytes / 1024 / 1024)} MB.`);
  // The agent's own pick, which the window marks with a star and "Recommended" as an asks card
  // does (Mike, 7 Nov: agents wrote "I'd pick C" in the middle of the note,
  // where it was hard to find).
  const recommended = text(p.recommended, 'recommended', 200) || undefined;
  if (recommended) {
    const option = options.find(o => o.id === recommended);
    if (!option) bad(`recommended must be one of the option ids: ${recommended}`);
    if (option.reference) bad('recommended cannot be a reference option.');
  }
  return { title, ...(note ? { note } : {}), ...(recommended ? { recommended } : {}), options };
}

// A stored review cut to what reviewInput and reviewDraft take, for one this
// runner didn't make: a moved thread's reviews are the other runner's word
// (runner/moves.ts fitMovedReviews). Its words are cut rather than refused,
// so a review that fits is kept exactly. Undefined for something that isn't
// a review.
export function fitReview(raw: unknown): DesignReview | undefined {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return undefined;
  const r = { ...(raw as Record<string, any>) };
  if (typeof r.id !== 'string' || !Array.isArray(r.options)) return undefined;
  const cut = (v: unknown, max: number) => (typeof v === 'string' ? cutText(v, max) : undefined);
  const set = (o: Record<string, any>, key: string, max: number) => {
    if (o[key] === undefined) return;
    const fitted = cut(o[key], max);
    if (fitted === undefined) delete o[key];
    else o[key] = fitted;
  };
  set(r, 'title', 200);
  set(r, 'note', 4000);
  set(r, 'recommended', 200);
  r.options = r.options.slice(0, 12).flatMap((raw: unknown) => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return [];
    const o = { ...(raw as Record<string, any>) };
    set(o, 'title', 200);
    set(o, 'note', 4000);
    set(o, 'url', 2000);
    if (Array.isArray(o.shots)) o.shots = o.shots.slice(0, maxReviewShots);
    return [o];
  });
  for (const key of ['draft', 'response'] as const) {
    const d = r[key];
    if (!d || typeof d !== 'object' || Array.isArray(d)) continue;
    const draft = { ...d };
    set(draft, 'reply', 8000);
    if (draft.notes && typeof draft.notes === 'object' && !Array.isArray(draft.notes)) {
      const notes: Record<string, string> = Object.create(null);
      for (const [id, note] of Object.entries(draft.notes).slice(0, 12)) {
        const fitted = cut(note, 4000);
        if (fitted !== undefined) notes[id] = fitted;
      }
      draft.notes = { ...notes };
    }
    r[key] = draft;
  }
  return r as DesignReview;
}

// At most `max` UTF-16 units, never ending on half a surrogate pair.
function cutText(text: string, max: number): string {
  if (text.length <= max) return text;
  const end = /[\uD800-\uDBFF]/.test(text[max - 1] ?? '') ? max - 1 : max;
  return text.slice(0, end);
}

export function reviewDraft(review: DesignReview, raw: unknown, answer = false): ReviewDraft {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return bad('draft must contain notes and optional picks.');
  const d = raw as Record<string, unknown>;
  if (!d.notes || typeof d.notes !== 'object' || Array.isArray(d.notes)) return bad('notes must be an object keyed by option ID.');
  if (d.pickedId !== undefined && typeof d.pickedId !== 'string') bad('pickedId must be an option ID.');
  if (d.pickedIds !== undefined && (!Array.isArray(d.pickedIds) || d.pickedIds.some(id => typeof id !== 'string'))) bad('pickedIds must be a list of option IDs.');
  if (d.none !== undefined && typeof d.none !== 'boolean') bad('none must be a boolean.');
  // pickedIds wins; a client from before it sends pickedId alone.
  const wanted = new Set((d.pickedIds as string[] | undefined) ?? (d.pickedId !== undefined ? [d.pickedId as string] : []));
  for (const id of wanted) {
    const option = review.options.find(o => o.id === id);
    if (!option) bad(`Unknown option: ${id}`);
    if (option.reference) bad('A reference image cannot be picked.');
  }
  // In the review's order, so the same picks always read the same.
  const picks = review.options.filter(o => wanted.has(o.id)).map(o => o.id);
  const none = d.none === true;
  if (none && picks.length) bad('Pick options or none of them, not both.');
  // Avoid prototype setters for option IDs such as __proto__.
  const notes: Record<string, string> = Object.create(null);
  for (const [id, value] of Object.entries(d.notes)) {
    if (!review.options.some(o => o.id === id)) bad(`Unknown option: ${id}`);
    if (typeof value !== 'string') bad(`Notes on ${id} must be text.`);
    const note = text(value, `Notes on ${id}`, 4000)!;
    if (note) notes[id] = note;
  }
  const reply = text(d.reply, 'reply', 8000);
  if (answer && !picks.length && !none && !reply && !Object.keys(notes).length) bad('Pick an option or write a reply.');
  return {
    // pickedIds only for more than one, so a single pick saves as it always did.
    ...(picks.length ? { pickedId: picks[0] } : {}),
    ...(picks.length > 1 ? { pickedIds: picks } : {}),
    notes: { ...notes },
    ...(reply ? { reply } : {}),
    ...(none ? { none: true as const } : {}),
  };
}

export function sameDraft(a: ReviewDraft | undefined, b: ReviewDraft): boolean {
  if (!a || a.pickedId !== b.pickedId || (a.pickedIds ?? []).join('\n') !== (b.pickedIds ?? []).join('\n')) return false;
  if ((a.reply ?? '') !== (b.reply ?? '') || !!a.none !== !!b.none) return false;
  const ids = Object.keys(a.notes);
  return ids.length === Object.keys(b.notes).length && ids.every(id => a.notes[id] === b.notes[id]);
}

// The end of the "Passed over:" line, which the Mac, phone and GPUI bubbles
// look for to leave the line out (ReviewAnswer), so change them with it.
export const PASSED_OVER_TAIL = "Don't offer one of these later (a next step, a follow-up) as if it were new: say it's one they passed over.";

// What the agent reads. "Picked A." with one, "Picked A + D." with more, as
// the window's Send button says. The Mac and the phone read it back
// (ReviewAnswer) for Mike's bubble, so change them with it.
//
// With a pick, the last line names the options he passed over. On 10 Nov an
// agent's done card offered the option he'd turned down, in neutral words,
// as a follow-up; he ticked it without recognising it and got the big card
// he hadn't wanted. The agent writing follow-ups needs this in front of it.
export function reviewReply(review: DesignReview, draft: ReviewDraft): string {
  const lines = [`Review: ${review.title} (${review.id})`];
  const picks = draft.pickedIds ?? (draft.pickedId ? [draft.pickedId] : []);
  if (picks.length) lines.push(`Picked ${picks.join(' + ')}. ${picks.map(id => review.options.find(o => o.id === id)!.title).join(' + ')}.`);
  if (draft.none) lines.push('Picked none of them.');
  if (draft.reply) lines.push(draft.reply);
  for (const o of review.options) if (Object.hasOwn(draft.notes, o.id) && draft.notes[o.id]) lines.push(`Notes on ${o.id} (${o.title}): ${draft.notes[o.id]}`);
  const passed = picks.length ? review.options.filter(o => !o.reference && !picks.includes(o.id)) : [];
  if (passed.length) lines.push(`Passed over: ${passed.map(o => `${o.id}. ${o.title}`).join('; ')}. ${PASSED_OVER_TAIL}`);
  return lines.join('\n\n');
}
