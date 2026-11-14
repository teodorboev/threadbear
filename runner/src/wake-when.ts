// Wake when… (docs/protocol.md, "Hibernate in words"): Mike says in his own
// words when a hibernating thread should come back ("at the weekend", "in
// a few weeks", "next time I talk about woodworking") and the strong model
// turns that into a time, a topic to listen for, or both. A time is an
// ordinary snoozedUntil; a topic is the librarian's to notice, in the next
// thread Mike talks about it in (librarian.ts).

import { trimEndOfOrSpace } from './linear-text.ts';
import { cutSafely, safeEnd } from './safe-cut.ts';

export type WakeWhen = {
  // What Mike typed.
  said: string;
  // What to listen for, when it's a topic ("woodworking").
  topic?: string;
  // Set while the model works it out. A runner that restarts meanwhile
  // puts the thread back in the inbox, as the answer is lost.
  pending?: true;
  // Set by the thread's own agent (end_turn's snooze in words), waiting for
  // something to happen before it carries on: the topic is then what has to
  // have happened, and waking hands the agent a turn to check it really
  // has, and go back to sleep if not. Mike's own Wake when… only brings the
  // thread back for him. Kept in the runner: the apps never see it.
  // Before 13 Nov 2026 an agent's "until the GPUI app replaces the Swift
  // app" was read as a topic, woke on the first talk of the plan, and sat
  // in the inbox with nothing done.
  by?: 'agent';
};

export type WakeAnswer = { until?: string; topic?: string; why: string };

export type WakeInput = {
  said: string;
  now: number;
  timeZone: string;
  title: string;
  first?: string;
  brief?: string;
  by?: 'agent';
};

// What Mike can type, and how far ahead a time can be.
export const SAID_MAX = 300;
export const TOPIC_MAX = 80;
const LATEST_MS = 2 * 365 * 86_400_000;

export const WAKE_SYSTEM = [
  "You work out when a hibernating thread should wake up, in Threadbear, the user's app for running AI agents in threads. The user has put a thread away and said in their own words when they want it back.",
  'Answer with a time, a topic, or both:',
  '- A time when they name one, however loosely: "at the weekend" is Saturday 9 am, "tonight" 7 pm, "in a few weeks" three weeks on at 9 am, "after Christmas" 27 December 9 am. Pick a sensible hour when they give none: 9 am for a day. Never a time that has already passed.',
  '- A topic when they want it back the next time they bring something up ("next time I talk about woodworking", "when I get back to the shed"): a few words for what to listen for, as you would say it in "when the user talks about …" ("woodworking", "the shed build"). Add a time as well only when they give a limit ("or in a month at the latest").',
  "- Something you can't watch for (a reply from someone, a PR merging, a delivery): the time it's most likely to have happened by, and say so in why.",
  "When it's the thread's own agent asking, it is waiting for something to happen before it carries on (\"until the GPUI app replaces the Swift app\"). Then a topic is that thing, said as done (\"the GPUI app has replaced the Swift app\"), which another thread's messages will show once it has, not a subject to listen for; add a time only when they give a limit.",
  'Reply with JSON only, no prose and no code fence: {"until": "an ISO 8601 time with its UTC offset, or null", "topic": "a few words, or null", "why": "one short line, in plain words"}',
].join('\n\n');

export function wakePrompt(input: WakeInput): string {
  const parts = [
    `Now: ${localNow(input.now, input.timeZone)} (${input.timeZone})`,
    `The thread: "${input.title}"`,
  ];
  if (input.first) parts.push(`How it started (the user): ${cut(input.first, 800)}`);
  if (input.brief) parts.push(`Where it's up to:\n${cut(input.brief, 600)}`);
  parts.push(input.by === 'agent' ? `The thread's agent is waiting: ${input.said.trim()}` : `The user wants it back: ${input.said.trim()}`);
  return parts.join('\n\n');
}

// The model's answer, or why it can't be used.
export function readWakeAnswer(text: string, now: number): WakeAnswer | { error: string } {
  const obj = parseObject(text);
  if (!obj) return { error: "The model's answer wasn't JSON." };
  const why = typeof obj.why === 'string' ? cutSafely(obj.why.trim(), 200) : '';
  const topic = typeof obj.topic === 'string' && obj.topic.trim() ? cutSafely(trimEndOfOrSpace(obj.topic.trim(), '.'), TOPIC_MAX) : undefined;
  let until: string | undefined;
  if (typeof obj.until === 'string' && obj.until.trim()) {
    const t = Date.parse(obj.until);
    if (Number.isNaN(t)) return { error: `The model gave a time that couldn't be read: ${obj.until}` };
    if (t <= now + 60_000) return { error: why || 'That time has already passed.' };
    if (t > now + LATEST_MS) return { error: 'That is more than two years away.' };
    until = new Date(t).toISOString();
  }
  if (!until && !topic) return { error: why || "Couldn't work out when from that." };
  return { ...(until ? { until } : {}), ...(topic ? { topic } : {}), why };
}

// The note the thread gets: "Hibernating until Saturday 1 November, 9 am."
export function hibernateNote(answer: WakeAnswer, said: string, timeZone: string, by?: 'agent'): string {
  const when = answer.until ? localWhen(Date.parse(answer.until), timeZone) : undefined;
  const latest = when ? `, or ${when} at the latest` : '';
  if (by === 'agent') {
    const head = answer.topic ? `Hibernating until ${answer.topic}${latest}.` : `Hibernating until ${when}.`;
    return `${head} The agent said "${cut(said.trim(), 120)}".`;
  }
  const head = answer.topic ? `Hibernating until you next talk about ${answer.topic}${latest}.` : `Hibernating until ${when}.`;
  return `${head} You said "${cut(said.trim(), 120)}".`;
}

// How long a thread was away, for the note it wakes with: "5 minutes",
// "an hour", "3 hours", "a day", "12 days". Rounded, as Mike only wants
// a feel for it.
export function awayFor(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  if (minutes < 60) return minutes === 1 ? 'a minute' : `${minutes} minutes`;
  const hours = Math.round(ms / 3_600_000);
  if (hours < 24) return hours === 1 ? 'an hour' : `${hours} hours`;
  const days = Math.round(ms / 86_400_000);
  return days === 1 ? 'a day' : `${days} days`;
}

// "Thursday 29 October 2026, 2:05 pm"
function localNow(now: number, timeZone: string): string {
  return new Date(now).toLocaleString('en-GB', { timeZone, weekday: 'long', day: 'numeric', month: 'long', year: 'numeric', hour: 'numeric', minute: '2-digit', hour12: true });
}

// "Saturday 1 November, 9 am"; "9:30 am" when it isn't on the hour.
export function localWhen(t: number, timeZone: string): string {
  const d = new Date(t);
  const day = d.toLocaleDateString('en-GB', { timeZone, weekday: 'long', day: 'numeric', month: 'long' }).replace(',', '');
  const time = d.toLocaleTimeString('en-GB', { timeZone, hour: 'numeric', minute: '2-digit', hour12: true }).replace(':00', '').replace(/\s?([ap])\.?m\.?/i, ' $1m');
  return `${day}, ${time}`;
}

const cut = (t: string, n: number) => (t.length > n ? t.slice(0, safeEnd(t, n)) + '…' : t);

function parseObject(text: string): Record<string, unknown> | undefined {
  const t = text.trim().replace(/^```(?:json)?\s*/i, '').replace(/```\s*$/, '');
  const start = t.indexOf('{');
  const end = t.lastIndexOf('}');
  if (start < 0 || end <= start) return undefined;
  try {
    const v = JSON.parse(t.slice(start, end + 1)) as unknown;
    return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : undefined;
  } catch {
    return undefined;
  }
}
