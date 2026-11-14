// Each scenario plays one thread and reads it three ways (helpers/routes.ts):
// on this runner, on another runner's thread from the Mac through Convex, and
// on the iPhone and the website. What a person sees (the card, its notes and
// pick, the follow-ups, the messages and their pictures) has to be the same
// on all three, or the difference has to be in KNOWN below with why. A
// decision card's notes (8 Nov) went missing on the way to another Mac with
// every test passing, as nothing compared the routes.
//
// A new feature that changes what a thread shows gets a scenario here. The
// agent building it writes it; it runs in the quick check in a second or two.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { threeWays, type View } from '../helpers/routes.ts';
import { SCENARIOS } from '../helpers/scenario-list.ts';

type Route = 'relay' | 'phone';
type Field = 'needs' | 'asks' | 'next' | 'looseEnds' | 'review' | 'plans' | 'messages' | 'notes' | 'pictures' | 'failed' | 'origins';

// Differences that are meant, each with why. A test below fails once one
// of these stops being a difference, so the list can't outlive its reason.
const KNOWN: { route: Route; field: Field; why: string }[] = [
  { route: 'relay', field: 'pictures', why: "Pictures in messages stay on that runner; the Mac shows each one's chip in the words." },
  { route: 'phone', field: 'pictures', why: "phone:thread carries words only; the phone gets a message's pictures from phone:images." },
  { route: 'phone', field: 'plans', why: "phone:thread carries words only; the phone gets a message's plan flag from phone:messages (API 72) and draws its own plan card." },
];


function pick(v: View, field: Field): unknown {
  switch (field) {
    case 'messages':
      return v.messages.map(({ role, text }) => ({ role, text }));
    case 'pictures':
      return v.messages.map((m) => m.pictures);
    case 'origins':
      return v.messages.map((m) => m.origin ?? null);
    default:
      return field === 'needs' || field === 'review' ? (v[field] ?? null) : v[field];
  }
}
const FIELDS: Field[] = ['needs', 'asks', 'next', 'looseEnds', 'review', 'plans', 'messages', 'notes', 'pictures', 'failed', 'origins'];
const known = (route: Route, field: Field) => KNOWN.find((k) => k.route === route && k.field === field);

for (const s of SCENARIOS) {
  test(`the same on every route: ${s.name}`, async () => {
    const { views, refused } = await threeWays(s.play);
    assert.deepEqual(refused, [], 'the backend took everything the runner sent');
    for (const route of ['relay', 'phone'] as const) {
      for (const field of FIELDS) {
        if (known(route, field)) continue;
        assert.deepEqual(pick(views[route], field), pick(views.here, field), `${field} differs on ${route === 'relay' ? 'another Mac (through Convex)' : 'the phone and website'}`);
      }
    }
  });
}

test('each known difference is still a difference, and says why', async () => {
  const all = await Promise.all(SCENARIOS.map((s) => threeWays(s.play)));
  for (const k of KNOWN) {
    assert.ok(k.why.trim().endsWith('.'), 'says why, as a sentence');
    const differs = all.some(({ views }) => JSON.stringify(pick(views[k.route], k.field)) !== JSON.stringify(pick(views.here, k.field)));
    assert.ok(differs, `${k.field} on ${k.route} is the same as here now: take it out of KNOWN`);
  }
});

// The same real backend as the route scenarios: a second pane already has
// every row except an older reply. Reopening it must restore that reply
// without replaying all the synthetic markers it already showed.
test('another Mac reopens a thread with only the reply its pane missed', async () => {
  const { relay, relayEvents, threadId, refused } = await threeWays((p) => {
    p.say('The earlier reply.');
    p.endTurn({ needs: 'nothing' });
    p.send('Carry on.');
    p.say('The latest reply.');
    p.endTurn({ needs: 'nothing' });
  });
  assert.deepEqual(refused, []);
  const missing = relayEvents.find((e) => e.type === 'assistant.text' && e.text === 'The earlier reply.');
  assert.ok(missing);
  const seen: [number, number][] = [];
  for (const seq of relayEvents.map((e) => Number(e.seq)).filter((seq) => seq !== missing.seq).sort((a, b) => a - b)) {
    const last = seen.at(-1);
    if (last && seq <= last[1] + 1) last[1] = Math.max(seq, last[1]);
    else seen.push([seq, seq]);
  }
  const reply = await relay.call(threadId, 'threads.subscribe', { afterSeq: Math.max(...relayEvents.map((e) => Number(e.seq))), relaySeen: seen }) as { events: Record<string, unknown>[] };
  assert.deepEqual(reply.events.map((e) => e.seq), [missing.seq]);
  assert.equal(reply.events[0]?.text, 'The earlier reply.');
});
