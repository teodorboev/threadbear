// Who reviews a code change, in Mike's order (runner.json `labReviewers`,
// Settings > General > Code review). Each is a provider, with a model and effort when it
// names them; the runner picks the login (lab-review.ts, reviewLogin).
//
// Why a list: Mike's Codex week ran out on 15 Nov 2026 and every review
// quietly became a fresh Claude session, while he had Grok credits going
// unused. With a list he puts the labs in the order he wants them, and
// the next one with room takes over when one runs out.
import { EFFORT_LEVELS, PROVIDERS, type EffortLevel, type Provider } from './protocol.ts';

export type LabReviewer = { provider: Provider; model?: string; effort?: EffortLevel };

// What a runner.json without the key does, and did before the list: the
// other provider of Claude and Codex reviews.
export const DEFAULT_LAB_REVIEWERS: readonly LabReviewer[] = [{ provider: 'codex' }, { provider: 'claude' }];

// Mike may have hand-edited runner.json: an entry that isn't a known
// provider is left out rather than failing the runner's start, and a
// provider listed twice counts the first time. Undefined for no list at
// all; an empty list is kept, as it means only the thread's own provider
// reviews.
export function parseLabReviewers(v: unknown): LabReviewer[] | undefined {
  if (!Array.isArray(v)) return undefined;
  const out: LabReviewer[] = [];
  for (const x of v) {
    const r = labReviewer(x);
    if (r && !out.some((o) => o.provider === r.provider)) out.push(r);
  }
  return out;
}

// One entry, or a reason it isn't one (settings.update says which).
export function labReviewerProblem(x: unknown): string | undefined {
  if (!x || typeof x !== 'object' || Array.isArray(x)) return 'each labReviewers entry must be an object';
  const o = x as Record<string, unknown>;
  if (typeof o.provider !== 'string' || !(PROVIDERS as readonly string[]).includes(o.provider)) return `labReviewers provider must be one of ${PROVIDERS.join(', ')}`;
  if (o.model !== undefined && (typeof o.model !== 'string' || !o.model.trim() || o.model.length > 200)) return 'labReviewers model must be a model id';
  if (o.effort !== undefined && !(EFFORT_LEVELS as readonly unknown[]).includes(o.effort)) return `labReviewers effort must be one of ${EFFORT_LEVELS.join(', ')}`;
  return undefined;
}

function labReviewer(x: unknown): LabReviewer | undefined {
  if (labReviewerProblem(x)) return undefined;
  const o = x as { provider: Provider; model?: string; effort?: EffortLevel };
  return { provider: o.provider, ...(o.model ? { model: o.model.trim() } : {}), ...(o.effort ? { effort: o.effort } : {}) };
}
