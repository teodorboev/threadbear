import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// What the composer's dump says about the open card and its picks.
type WithCard = Dump & { decisions?: string[] | null; composer: Dump['composer'] & { card?: string[] | null } };

const cardLines = (d: Dump): string[] => (d as WithCard).composer.card ?? [];

function decisions(d: Dump): string[] {
  return (d as WithCard).decisions ?? [];
}

Then('the card in the composer says {string}', async function (this: ThreadbearWorld, heading: string) {
  await this.until(`the card saying "${heading}"`, (d) => cardLines(d)[0] === heading);
});

Then('the card in the composer asks {string}', async function (this: ThreadbearWorld, ask: string) {
  await this.until(`the card asking "${ask}"`, (d) => cardLines(d).some((l) => l.endsWith(ask)));
});

Then('the card offers the follow-up {string}', async function (this: ThreadbearWorld, followUp: string) {
  await this.until(`the card offering "${followUp}"`, (d) => cardLines(d).some((l) => l.includes(followUp)));
});

Then('the follow-up {string} is ticked', async function (this: ThreadbearWorld, followUp: string) {
  await this.until(`"${followUp}" ticked`, (d) => cardLines(d).some((l) => l.startsWith('[x] ') && l.includes(followUp)));
});

When('I pick answer {int} of ask {int} on the card', async function (this: ThreadbearWorld, choice: number, ask: number) {
  await this.app.do({ cmd: 'decide', pick: `${ask}:${choice}` });
});

When('I write {string} as my own answer to ask {int}', async function (this: ThreadbearWorld, words: string, ask: number) {
  await this.app.do({ cmd: 'decide', write: `${ask}:${words}` });
});

When('I send my picks', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'decide', send: true });
});

Then('ask {int} on the card is answered {string}', async function (this: ThreadbearWorld, ask: number, answer: string) {
  await this.until(`ask ${ask} answered "${answer}"`, (d) => decisions(d).some((l) => l.includes(`${ask}. `) && l.endsWith(`-> ${answer}`)));
});

async function openCardRow(world: ThreadbearWorld): Promise<string> {
  const d = await world.app.dump();
  const row = d.transcript.rows.find((r) => r.kind === 'asks');
  assert.ok(row, 'no card row in the transcript');
  return row.id;
}

When('I press {string} on the card', async function (this: ThreadbearWorld, action: string) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action });
});

// The link button over an ask's answers, as a click on it goes.
When('I press link {int} of ask {int} on the card', async function (this: ThreadbearWorld, link: number, ask: number) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action: `link:${ask - 1}:${link - 1}` });
});

When('I rest on link {int} of ask {int} on the card', async function (this: ThreadbearWorld, link: number, ask: number) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action: `peek:${ask - 1}:${link - 1}` });
});

// The link button's card, as the dump's card lines give it last.
Then('the card shows the peek {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the peek "${words}"`, (d) => cardLines(d).includes(`peek: ${words}`));
});

When('I tick follow-up {int} on the card', async function (this: ThreadbearWorld, number: number) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action: `next:${number - 1}` });
});

// A sample picture the runner can read, as sending.steps.ts has it.
const samplePicture = fileURLToPath(new URL('../support/sample.png', import.meta.url));

// What another lab's review left on the turn that answered it (docs/protocol.md,
// "Review by another lab"), through the mock's streamed turn.
When('the agent there finishes a turn another lab reviewed', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const review = {
    reviewer: 'Codex (codex-1)', independent: true, rounds: 1, threadId: d.selection?.id ?? '', passed: false,
    findings: [
      { id: 1, round: 1, severity: 'high', claim: 'Drops the session on refresh.', where: 'auth.ts:40', outcome: 'fixed', note: 'Kept it across the refresh' },
      { id: 2, round: 1, severity: 'medium', claim: 'Keeps the old token after sign-out.', outcome: 'left' },
    ],
  };
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text: 'Fixed the refresh, and left the sign-out one for you.', hz: 60, completed: { needs: 'nothing', review } } });
  await this.until('the reviewed turn finished', (d) => d.selection?.status === 'idle' && d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream'), 20_000);
});

// Another lab's review is a row of the transcript over the reply, its text
// the heading then "; #1 fixed: <claim>" a finding (RowDebug).
Then("the transcript's review line says {string}", async function (this: ThreadbearWorld, words: string) {
  await this.until(`a review line saying "${words}"`, (d) => d.transcript.rows.some((r) => r.kind === 'labReview' && r.text.includes(words)));
});

Then("the card doesn't show {string}", async function (this: ThreadbearWorld, words: string) {
  const d = await this.app.dump();
  if (cardLines(d).some((l) => l.includes(words))) throw new Error(`The card shows "${words}": ${JSON.stringify(cardLines(d))}`);
});

type WithAnswers = Dump & { composer: Dump['composer'] & { answers?: string[] | null } };

Then('the box shows the picked answers {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = list.split(' | ');
  await this.until(`the picked answers ${JSON.stringify(wanted)}`, (d) => JSON.stringify((d as WithAnswers).composer.answers ?? []) === JSON.stringify(wanted));
});

Then('the box shows no picked answers', async function (this: ThreadbearWorld) {
  await this.until('no picked answers', (d) => ((d as WithAnswers).composer.answers ?? []).length === 0);
});

// The chip's ×, by its tooltip (Swift) or its name (GPUI), which are the same.
When('I take the picked answer back', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', title: 'Take this answer back', tip: 'Take this answer back' });
});

When('I put the picture in my own answer to ask {int}', async function (this: ThreadbearWorld, ask: number) {
  await this.app.do({ cmd: 'decide', picture: `${ask}:${samplePicture}` });
});

Then('ask {int} on the card is answered with {int} picture(s)', async function (this: ThreadbearWorld, ask: number, count: number) {
  const tail = `+${count} picture${count === 1 ? '' : 's'}`;
  await this.until(`ask ${ask} answered with ${tail}`, (d) => decisions(d).some((l) => l.includes(`${ask}. `) && l.endsWith(tail)));
});

Then('the transcript shows my message {string} with the chip {string}', async function (this: ThreadbearWorld, words: string, chip: string) {
  await this.until(`my message "${words}" with the chip "${chip}"`, (d) => d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(words) && (r.chips ?? []).includes(chip)));
});

// The answer's button on the card, as a click on it (both apps' `ask`).
When('I press answer {int} of ask {int} on the card', async function (this: ThreadbearWorld, choice: number, ask: number) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action: `pick:${ask - 1}:${choice - 1}` });
});

// Long enough for a paged card to move on to the next unanswered ask, which
// it does a beat after a pick (300 ms) so the tick shows first.
When('the card has had a moment to move on', async function () {
  await new Promise((done) => setTimeout(done, 900));
});

Then('the app beeps', async function (this: ThreadbearWorld) {
  for (let tries = 0; tries < 30; tries += 1) {
    const out = await this.app.do({ cmd: 'beeps' });
    if (Number(/ok (\d+)/.exec(out)?.[1] ?? 0) > 0) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail('no beep');
});

// The runner's loose-ends check on a thread's done card, as the summary
// brings it (mock.looseEnds): before the thread's opened, or while it is.
async function looseEnds(world: ThreadbearWorld, title: string, ends?: { kind: string; text: string }[], quiet = false) {
  await world.app.do({ cmd: 'mock', method: 'looseEnds', params: { title, ends, quiet } });
}

Given('the runner found {string} still open on {string}', async function (this: ThreadbearWorld, text: string, title: string) {
  await looseEnds(this, title, [{ kind: 'unpushed', text }]);
});

When('the runner finds {string} still open on {string}', async function (this: ThreadbearWorld, text: string, title: string) {
  await looseEnds(this, title, [{ kind: 'unlanded', text }]);
});

// Set on the runner with no thread.updated, as a check that finished while
// the app was away: only the list it reads on reconnecting has it.
When('the runner finds {string} still open on {string} without saying so', async function (this: ThreadbearWorld, text: string, title: string) {
  await looseEnds(this, title, [{ kind: 'uncommitted', text }], true);
});

When('the runner finds nothing left open on {string}', async function (this: ThreadbearWorld, title: string) {
  await looseEnds(this, title, []);
});

// Both apps' dump gives the done card's row as RowDebug has it: "open,
// nothing needed, loose ends: <each>; <next>", which is what the card draws
// under "Still open".
const doneRow = (d: Dump) => d.transcript.rows.find((r) => r.kind === 'asks' && r.text.includes('nothing needed'))?.text ?? '';

Then('the done card lists {string} as still open', async function (this: ThreadbearWorld, text: string) {
  await this.until(`"${text}" still open on the done card`, (d) => doneRow(d).includes(`loose ends: ${text}`));
});

Then('the done card lists nothing as still open', async function (this: ThreadbearWorld) {
  await this.until('nothing still open on the done card', (d) => doneRow(d).includes('loose ends: none'));
});
