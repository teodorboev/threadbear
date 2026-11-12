// The design review window (review.feature). Both apps take the Swift app's
// `review` command, which opens the open thread's review as its card does and
// says where the window has got to; `"open": false` only reads one that's
// open. Clicks go to the review's own window by tooltip, as a person's would.
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue, sleep } from '../support/driver.ts';
import { SHOTS_REVIEW, shotsReview } from '../support/review-demo.ts';
import type { ThreadbearWorld } from '../support/world.ts';

/** The demo thread's review (app/Tools/mock-runner/fixtures/demo-design-review). */
const DEMO_REVIEW = 'review_account_picker';
/** The review each scenario opened: the demo's unless it made another. */
const opened = new WeakMap<ThreadbearWorld, string>();
const reviewId = (world: ThreadbearWorld) => opened.get(world) ?? DEMO_REVIEW;

type ReviewState = {
  id: string;
  status: string;
  mode: string;
  focused: string;
  shot: number;
  pickedIds: string[];
  none: boolean;
  reply: string;
  sendLabel: string;
  editable: boolean;
  actual: boolean;
  peeking: boolean;
  minimap: boolean;
  error: string;
  /** What the stage draws: "picture", "player", "page", "still" (a clip or a page as its still) or "all". */
  stage: string;
  /** What Open live would open, for the option on show. */
  live: string;
};

async function review(world: ThreadbearWorld, cmd: Record<string, unknown> = {}): Promise<ReviewState> {
  return JSON.parse(okValue(await world.app.do({ cmd: 'review', id: reviewId(world), ...cmd }, 30))) as ReviewState;
}

/** Waits for the open review window to pass `check`, without opening one. */
async function untilReview(world: ThreadbearWorld, what: string, check: (s: ReviewState) => boolean, ms = 15_000): Promise<ReviewState> {
  const deadline = Date.now() + ms;
  let last: ReviewState | string = 'no answer yet';
  for (;;) {
    try {
      last = await review(world, { open: false });
      if (check(last)) return last;
    } catch (e) {
      last = (e as Error).message;
    }
    if (Date.now() > deadline) throw new Error(`Never saw: ${what}\n\nThe review said: ${JSON.stringify(last)}`);
    await sleep(150);
  }
}

/** Waits until no window is open on the review. */
async function untilClosed(world: ThreadbearWorld): Promise<void> {
  const deadline = Date.now() + 15_000;
  for (;;) {
    try {
      await review(world, { open: false });
    } catch {
      return;
    }
    if (Date.now() > deadline) throw new Error('The review window stayed open');
    await sleep(150);
  }
}

/** A control in the review's window, by its tooltip. */
async function press(world: ThreadbearWorld, tip: string): Promise<void> {
  await world.app.do({ cmd: 'click', tip, review: reviewId(world) });
}

When('I open the review', async function (this: ThreadbearWorld) {
  await review(this);
});

When('I close the review', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'review', id: reviewId(this), close: true });
  await untilClosed(this);
});

When('I follow the link {string} in the agent\'s last reply', async function (this: ThreadbearWorld, url: string) {
  const row = await this.until('a reply from the agent', (d) => [...d.transcript.rows].reverse().find((r) => r.kind === 'assistant'));
  await this.app.do({ cmd: 'link', row: row.id, url });
});

Then('the review window is open on option {string}, one at a time', async function (this: ThreadbearWorld, id: string) {
  await untilReview(this, `the review on ${id}, one at a time`, (s) => s.mode === 'Single' && s.focused === id && s.status === 'pending');
});

Then('the bar over the thread says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the bar over the thread saying "${words}"`, (d) => (d.header as { pendingReviews?: string | null } | undefined)?.pendingReviews === words);
});

When('I press the bar\'s {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the bar saying "${words}"`, (d) => (d.header as { pendingReviews?: string | null } | undefined)?.pendingReviews === words);
  await this.app.do({ cmd: 'click', tip: 'Compare the images the agent made side by side, and reply' });
});

When('I press the review {string} in the bar\'s list', async function (this: ThreadbearWorld, _title: string) {
  // Its list comes from the runner, a moment after the click.
  const deadline = Date.now() + 10_000;
  for (;;) {
    try {
      await this.app.do({ cmd: 'click', tip: 'Open this review' });
      return;
    } catch (e) {
      if (Date.now() > deadline) throw e;
      await sleep(150);
    }
  }
});

When('I tick {string} in the review', async function (this: ThreadbearWorld, id: string) {
  await press(this, `Pick ${id}`);
  await untilReview(this, `${id} ticked`, (s) => s.pickedIds.includes(id));
});

When('I choose None in the review', async function (this: ThreadbearWorld) {
  await press(this, 'None of them will do');
  await untilReview(this, 'none of them chosen', (s) => s.none);
});

When('I write {string} in the review', async function (this: ThreadbearWorld, words: string) {
  await review(this, { open: false, reply: words });
});

// As Voice Type pastes: the words on the clipboard, then ⌘V to the review's
// window wherever its keys are, not into a box picked first.
When('I paste {string} in the review with ⌘V', async function (this: ThreadbearWorld, words: string) {
  await this.app.do({ cmd: 'clipboard', set: words });
  await this.app.do({ cmd: 'menu-key', key: 'v', modifiers: ['cmd'], review: reviewId(this) });
});

When('I press Send in the review', async function (this: ThreadbearWorld) {
  await press(this, 'Send your picks and reply to the agent (⌘↩). Close the window to finish it later.');
});

Then('the review\'s Send says {string}', async function (this: ThreadbearWorld, words: string) {
  await untilReview(this, `Send saying "${words}"`, (s) => s.sendLabel === words);
});

Then('no option is ticked in the review', async function (this: ThreadbearWorld) {
  const s = await untilReview(this, 'no picks', (s) => s.pickedIds.length === 0);
  assert.deepEqual(s.pickedIds, []);
});

Then('{string} has no tick in the review', async function (this: ThreadbearWorld, id: string) {
  await untilReview(this, 'the review open', () => true);
  await assert.rejects(press(this, `Pick ${id}`), `${id} has a tick to press`);
  const s = await review(this, { open: false });
  assert.equal(s.pickedIds.includes(id), false);
});

Then('the review has {string} ticked and the reply {string}', async function (this: ThreadbearWorld, id: string, words: string) {
  await untilReview(this, `${id} ticked with "${words}"`, (s) => s.pickedIds.join(',') === id && s.reply === words);
});

Then('the review window has closed', async function (this: ThreadbearWorld) {
  await untilClosed(this);
});

Then('the review window shows what was sent: {string} and {string}', async function (this: ThreadbearWorld, id: string, words: string) {
  await untilReview(this, `the answered review with ${id} and "${words}"`, (s) => s.status === 'answered' && s.pickedIds.join(',') === id && s.reply === words && s.mode === 'All');
});

Then('nothing can be changed in the review', async function (this: ThreadbearWorld) {
  const s = await review(this, { open: false });
  assert.equal(s.editable, false, 'it can still be changed');
  await assert.rejects(press(this, 'Pick A'), 'a tick is still there to press');
});

// Stepping, zoom and today's version (review.nav, review.zoom, review.minimap,
// review.holdspace).

Given('the agent shows options with several shots each', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const threadId = d.selection?.id;
  assert.ok(threadId, 'no thread open');
  await this.app.do({ cmd: 'mock', method: 'review', params: { threadId, review: shotsReview() } });
  await this.until('the new review\'s card', (dump) => dump.transcript.rows.some((r) => r.kind === 'review' && r.text.includes('Picker, step by step')));
  opened.set(this, SHOTS_REVIEW);
});

When('I open that review', async function (this: ThreadbearWorld) {
  await review(this);
});

When('I press {string} in the review', async function (this: ThreadbearWorld, tip: string) {
  await press(this, tip);
});

When('I press the {string} key in the review', async function (this: ThreadbearWorld, key: string) {
  await this.app.do({ cmd: 'menu-key', key, modifiers: [], review: reviewId(this) });
});

Then('the review is on option {string}, shot {int}', async function (this: ThreadbearWorld, id: string, shot: number) {
  await untilReview(this, `${id}'s shot ${shot} on the stage`, (s) => s.mode === 'Single' && s.focused === id && s.shot === shot - 1);
});

Then('the review shows them all', async function (this: ThreadbearWorld) {
  await untilReview(this, 'all of them side by side', (s) => s.mode === 'All');
});

When('I show the shot at full size', async function (this: ThreadbearWorld) {
  await review(this, { open: false, actual: true });
});

When('I fit the shot in its space', async function (this: ThreadbearWorld) {
  await review(this, { open: false, actual: false });
});

Then('the shot is zoomed in, with the minimap showing', async function (this: ThreadbearWorld) {
  await untilReview(this, 'the shot zoomed in with its minimap', (s) => s.actual && s.minimap);
});

Then('the shot fits, with no minimap', async function (this: ThreadbearWorld) {
  await untilReview(this, 'the shot fitting, with no minimap', (s) => !s.actual && !s.minimap);
});

Then('the review shows today\'s version over option {string}', async function (this: ThreadbearWorld, id: string) {
  await untilReview(this, `today's version over ${id}`, (s) => s.peeking && s.focused === id && s.mode === 'Single');
});

Then('the review shows option {string} again', async function (this: ThreadbearWorld, id: string) {
  await untilReview(this, `${id} again`, (s) => !s.peeking && s.focused === id && s.mode === 'Single');
});

// Media and Open live (review.media, review.openlive). The state says what
// the stage draws ("picture", "player", "page", or "still" for a clip or a
// page shown as its still) and what Open live would open, so nothing gets
// opened on the machine running the specs.

Then('the caption offers nothing to try live', async function (this: ThreadbearWorld) {
  await untilReview(this, 'no Open live', (s) => s.mode === 'Single' && s.live === '');
});

Then('the caption offers {string} to try live', async function (this: ThreadbearWorld, url: string) {
  await untilReview(this, `Open live for ${url}`, (s) => s.live === url);
});

Then('the stage plays the clip', async function (this: ThreadbearWorld) {
  await untilReview(this, 'the clip playing on the stage', (s) => s.stage === 'player');
});

Then('the stage shows the page', async function (this: ThreadbearWorld) {
  await untilReview(this, 'the page on the stage', (s) => s.stage === 'page');
});
