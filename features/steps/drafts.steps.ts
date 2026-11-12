import assert from 'node:assert/strict';
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import { okValue, type Dump, type Row } from '../support/driver.ts';

// The replies these stream into the open thread (mock.stream with `text`):
// what an agent writes when it drafts something for Mike to send.
const replies: Record<string, string> = {
  'a Slack draft': "Here's the post for #general:\n\n```slack\n*Threadbear, 6 Nov*\n• one <https://x.dev|link>\n• two\n```\n\nPost it?",
  'an email draft': '```email\nHi Sam,\n\nSee https://x.dev/a for *this*.\n\nMike\n```\n\nSend it when you like.',
  'an unfinished Slack draft': "Here's a start:\n\n```slack\n• one\n• tw",
};

/** The streamed reply's row, once its turn has finished. */
const streamed = (d: Dump): Row | undefined => {
  const done = d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream');
  return done ? [...d.transcript.rows].reverse().find((r) => r.kind === 'assistant' && r.turn === 'turn_stream') : undefined;
};

/** The first draft the streamed reply shows, as the dump has it. */
const firstDraft = (d: Dump) => streamed(d)?.drafts?.[0];

When('a reply with {word} {word} draft streams in', async function (this: ThreadbearWorld, article: string, kind: string) {
  const text = replies[`${article} ${kind} draft`];
  assert.ok(text, `no reply called "${article} ${kind} draft"`);
  const d = await this.app.dump();
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text, hz: 60 } });
  await this.until('the streamed reply, its turn finished', streamed, 20_000);
});

When('a reply with an unfinished Slack draft streams in', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text: replies['an unfinished Slack draft'], hz: 60 } });
  await this.until('the streamed reply, its turn finished', streamed, 20_000);
});

Then('the reply shows a(n) {string} box with the lines {string}, {string} and {string}', async function (this: ThreadbearWorld, label: string, a: string, b: string, c: string) {
  await this.until(`a "${label}" box with ${[a, b, c].join(' | ')}`, (d) => {
    const draft = firstDraft(d);
    return draft?.label === label && JSON.stringify(draft.lines) === JSON.stringify([a, b, c]);
  });
});

Then('the reply shows a(n) {string} box with the lines {string}, {string}, {string}, {string} and {string}', async function (this: ThreadbearWorld, label: string, a: string, b: string, c: string, e: string, f: string) {
  await this.until(`a "${label}" box with ${[a, b, c, e, f].join(' | ')}`, (d) => {
    const draft = firstDraft(d);
    return draft?.label === label && JSON.stringify(draft.lines) === JSON.stringify([a, b, c, e, f]);
  });
});

/** Waits for the draft's formatted pieces to include each of `want`, and
 * says what they were if they never do. */
async function styledHas(world: ThreadbearWorld, want: string[]) {
  let seen: string[] = [];
  try {
    await world.until(want.join(' and '), (d) => {
      seen = firstDraft(d)?.styled ?? [];
      return want.every((w) => seen.includes(w));
    });
  } catch (error) {
    assert.fail(`${(error as Error).message.split('\n')[0]}; the draft's pieces were ${JSON.stringify(seen)}`);
  }
}

Then('in the draft {string} is bold and {string} links to {string}', async function (this: ThreadbearWorld, bold: string, words: string, url: string) {
  await styledHas(this, [`bold: ${bold}`, `link: ${words} <${url}>`]);
});

Then('in the draft {string} links to {string}', async function (this: ThreadbearWorld, words: string, url: string) {
  await styledHas(this, [`link: ${words} <${url}>`]);
});

Then('the reply shows no draft box', async function (this: ThreadbearWorld) {
  const row = await this.until('the streamed reply', streamed);
  assert.equal(row.drafts, undefined, `it drew ${JSON.stringify(row.drafts)}`);
  assert.ok(row.text.includes('• tw'), row.text);
});

When("I press the draft's Copy", async function (this: ThreadbearWorld) {
  const row = await this.until('a draft box', (d) => (firstDraft(d) ? streamed(d) : undefined));
  const out = await this.app.do({ cmd: 'draft', row: row.id, index: 0, copy: true });
  assert.equal(okValue(out), 'copied', out);
});

Then("the draft's Copy reads {string}", async function (this: ThreadbearWorld, words: string) {
  // Copied lasts a second and a half, so the first look should see it.
  await this.until(`Copy reading ${words}`, (d) => firstDraft(d)?.copy === words, 1_200);
});

/** What the clipboard holds, as the `clipboard` command gives it. */
async function clipboard(world: ThreadbearWorld, type?: 'html'): Promise<string> {
  return JSON.parse(okValue(await world.app.do({ cmd: 'clipboard', ...(type ? { type } : {}) }))) as string;
}

Then('the clipboard holds the lines {string}, {string} and {string}', async function (this: ThreadbearWorld, a: string, b: string, c: string) {
  const want = [a, b, c].join('\n');
  const deadline = Date.now() + 5000;
  let last = '';
  while (Date.now() < deadline) {
    last = await clipboard(this);
    if (last === want) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.equal(last, want);
});

Then("the clipboard's HTML has {string}", async function (this: ThreadbearWorld, html: string) {
  const deadline = Date.now() + 5000;
  let last = '';
  while (Date.now() < deadline) {
    last = await clipboard(this, 'html');
    if (last.includes(html)) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`the clipboard's HTML is ${last}`);
});

// Words in a draft box selected: both apps take `select-text` with the words.
// The GPUI app drags across them in the box, through its real hit testing
// (`draft`); the Swift app selects them wherever they are in the reply.
async function selectInDraft(world: ThreadbearWorld, words: string, action?: string): Promise<string> {
  const row = await world.until('a draft box', (d) => (firstDraft(d) ? streamed(d) : undefined));
  return world.app.do({ cmd: 'select-text', row: row.id, words, draft: true, ...(action ? { action } : {}) });
}

When('I select {string} in the draft', async function (this: ThreadbearWorld, words: string) {
  this.lastAnswer = await selectInDraft(this, words);
});

When('I press {string} on the bar over {string} in the draft', async function (this: ThreadbearWorld, action: string, words: string) {
  await selectInDraft(this, words, action);
});

// Exactly the words: the whole draft's Copy, a scenario before, has them in it.
Then('the clipboard holds just {string}', async function (this: ThreadbearWorld, words: string) {
  const deadline = Date.now() + 5000;
  let last = '';
  while (Date.now() < deadline) {
    last = await clipboard(this);
    if (last === words) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.equal(last, words);
});
