// Screen readers (features/accessibility.feature): the GPUI app's `a11y`
// gives what an element labelled so tells a screen reader (AccessKit's role
// and states), from the last frame drawn.
import { When, Then } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue, sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Node = { role: string | null; selected: boolean | null; checked: boolean | null; disabled: boolean | null };

async function node(world: ThreadbearWorld, label: string, prefix = false, want: (n: Node) => boolean = () => true): Promise<Node> {
  let last: Node | undefined;
  for (let i = 0; i < 20; i += 1) {
    last = JSON.parse(okValue(await world.app.do({ cmd: 'a11y', label, prefix }))) as Node;
    if (want(last)) return last;
    await sleep(150);
  }
  return last as Node;
}

Then('{string} is a button to a screen reader', async function (this: ThreadbearWorld, label: string) {
  const n = await node(this, label, false, (x) => x.role === 'Button');
  assert.equal(n.role, 'Button', `"${label}" is ${JSON.stringify(n)}`);
});

Then('{string} is disabled to a screen reader', async function (this: ThreadbearWorld, label: string) {
  const n = await node(this, label, false, (x) => x.disabled === true);
  assert.equal(n.disabled, true, `"${label}" is ${JSON.stringify(n)}`);
});

Then('{string} is enabled to a screen reader', async function (this: ThreadbearWorld, label: string) {
  const n = await node(this, label, false, (x) => x.disabled !== true);
  assert.notEqual(n.disabled, true, `"${label}" is ${JSON.stringify(n)}`);
});

// A row is labelled by its tooltip, which starts with the thread's title.
Then('the row of {string} is a list item to a screen reader', async function (this: ThreadbearWorld, title: string) {
  const n = await node(this, title, true, (x) => x.role === 'ListItem');
  assert.equal(n.role, 'ListItem', `the row of "${title}" is ${JSON.stringify(n)}`);
});

Then('the row of {string} is selected to a screen reader', async function (this: ThreadbearWorld, title: string) {
  const n = await node(this, title, true, (x) => x.selected === true);
  assert.equal(n.selected, true, `the row of "${title}" is ${JSON.stringify(n)}`);
});

// System Settings belongs to the person using the Mac. The scripted app can
// compare palettes with THREADBEAR_INCREASE_CONTRAST without changing it.
When('I turn on Increase Contrast in macOS and return to Threadbear', () => 'pending');
When('I turn off Increase Contrast in macOS and return to Threadbear', () => 'pending');
Then("text and borders use the Mac's higher contrast colours", () => 'pending');
Then('the ordinary colours return', () => 'pending');
