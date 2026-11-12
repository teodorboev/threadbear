// Diagrams and pages in a reply (features/rich-blocks.feature): ```mermaid
// and ```html blocks drawn as what they show, on either app.
import assert from 'node:assert/strict';
import { Given, Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import { okValue, type Dump, type Row } from '../support/driver.ts';
import { webRuntime } from './web-runtime.steps.ts';

const MERMAID = 'graph TD\n  A[Start] --> B{Is it working?}\n  B -->|Yes| C[Ship it]\n  B -->|No| D[Fix it]';
const HTML = '<div style="padding:16px;border-radius:12px;background:#6366f1;color:white"><h2 style="margin:0">Hello from a page</h2><p>Drawn with no scripts and no network.</p></div>';

// The replies these stream into the open thread (mock.stream with `text`).
const replies: Record<string, string> = {
  'a mermaid diagram': `Here's the flow:\n\n\`\`\`mermaid\n${MERMAID}\n\`\`\`\n\nThat's it.`,
  'an html page': `A card:\n\n\`\`\`html\n${HTML}\n\`\`\`\n\nThere.`,
  'an unfinished mermaid diagram': "Here's a start:\n\n```mermaid\ngraph TD\n  A --> B",
};

type RichInfo = { kind: string; shows: string; state: string; asks: boolean; copy: string; size?: string | null };

/** The streamed reply's row, once its turn has finished. */
const streamed = (d: Dump): (Row & { rich?: RichInfo[] }) | undefined => {
  const done = d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream');
  return done ? [...d.transcript.rows].reverse().find((r) => r.kind === 'assistant' && r.turn === 'turn_stream') : undefined;
};

/** The streamed reply's first box of that kind. */
const box = (d: Dump, kind: string) => streamed(d)?.rich?.find((r) => r.kind === kind);

/** Drawing a block the first time starts Chromium as well. */
const DRAWN_MS = 60_000;

// The first scenario to need it installs it, which takes longer than the
// default 90 seconds on a loaded Tinker Desk.
Given('the web runtime is installed', { timeout: 660_000 }, async function (this: ThreadbearWorld) {
  // The Swift app has WebKit, and no runtime to install.
  const d = await this.app.dump();
  if (!webRuntime(d) || webRuntime(d)?.state === 'ready') return;
  await this.app.do({ cmd: 'webRuntime', do: 'install', archive: process.env.TB_WEB_RUNTIME_ARCHIVE, wait: 600 }, 630);
  await this.until('the web runtime installed', (d) => webRuntime(d)?.state === 'ready', 30_000);
});

async function streamReply(world: ThreadbearWorld, name: string) {
  const d = await world.app.dump();
  await world.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text: replies[name], hz: 60 } });
  await world.until('the streamed reply, its turn finished', streamed, 20_000);
}

When('a reply with a mermaid diagram streams in', async function (this: ThreadbearWorld) {
  await streamReply(this, 'a mermaid diagram');
});

When('a reply with an html page streams in', async function (this: ThreadbearWorld) {
  await streamReply(this, 'an html page');
});

When('a reply with an unfinished mermaid diagram streams in', async function (this: ThreadbearWorld) {
  await streamReply(this, 'an unfinished mermaid diagram');
});

Then('the reply shows its {word} block drawn', async function (this: ThreadbearWorld, kind: string) {
  await this.until(`the ${kind} block drawn`, (d) => {
    const b = box(d, kind);
    return b?.shows === 'drawn' && b.state === 'drawn';
  }, DRAWN_MS);
});

Then("the reply shows its {word} block's source", async function (this: ThreadbearWorld, kind: string) {
  await this.until(`the ${kind} block's source`, (d) => box(d, kind)?.shows === 'source');
});

Then("the reply shows its {word} block's source, asking about the web runtime", async function (this: ThreadbearWorld, kind: string) {
  await this.until(`the ${kind} block's source and the question`, (d) => {
    const b = box(d, kind);
    return b?.shows === 'source' && b.asks;
  });
});

Then("the reply shows its {word} block's source without asking", async function (this: ThreadbearWorld, kind: string) {
  await this.until(`the ${kind} block's source, no question`, (d) => {
    const b = box(d, kind);
    return b?.shows === 'source' && !b.asks;
  });
});

Then('the reply shows no rich blocks', async function (this: ThreadbearWorld) {
  const d = await this.until('the streamed reply', streamed);
  assert.ok(!d.rich?.length, `it drew ${JSON.stringify(d.rich)}`);
});

When('I press {string} on the box', async function (this: ThreadbearWorld, tip: string) {
  await this.app.do({ cmd: 'click', tip });
});

When('I choose Not now on the box', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: "Show the source, and don't ask again until Threadbear restarts" });
});

Then("the clipboard holds the mermaid diagram's source", async function (this: ThreadbearWorld) {
  const held = JSON.parse(okValue(await this.app.do({ cmd: 'clipboard' }))) as string;
  assert.equal(held, MERMAID);
});

Then('the diagram is open in a window of its own', async function (this: ThreadbearWorld) {
  await this.until('a full-size window', (d) => ((d as unknown as { richWindows?: string[] | null }).richWindows?.length ?? 0) === 1, 20_000);
});

let probed: { height: number; reached: number; diagramReached?: number } | undefined;

When('a page that tries to run scripts and reach the network is drawn', async function (this: ThreadbearWorld) {
  probed = JSON.parse(await this.app.do({ cmd: 'webRuntime', do: 'probe', wait: 90 }, 120)) as { height: number; reached: number; diagramReached?: number };
});

Then("its scripts didn't run and nothing was reached", function () {
  assert.ok(probed, 'nothing was drawn');
  // Its scripts would have made it 3000 points tall.
  assert.ok(probed.height < 1000, `the page came out ${probed.height} points tall, so a script ran`);
  assert.equal(probed.reached, 0, `${probed.reached} connections reached the listener`);
  // Nor did a diagram trying the same from Pele's world.
  assert.equal(probed.diagramReached, 0, `${probed.diagramReached} connections reached the listener from the diagram`);
});
