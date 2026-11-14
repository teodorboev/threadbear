import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

const reply = '| Choice | Time |\n|---|---|\n| First | 2 min |\n| Second | 5 min |\n\n- First item\n- Second item\n- Third item';
type Line = {text: string; x: number; y: number; w: number; h: number};
let originalWidth: number;
async function lines(world: ThreadbearWorld): Promise<Line[]> {
  const row = [...(await world.app.dump()).transcript.rows].reverse().find(r => r.kind === 'assistant' && r.turn === 'turn_stream');
  assert.ok(row, 'the completed table reply');
  return JSON.parse(okValue(await world.app.do({cmd: 'text-lines', row: row.id}))) as Line[];
}
function tableWidth(drawn: Line[]): number {
  const cells = drawn.filter(l => ['Choice', 'Time', 'First', 'Second', '2 min', '5 min'].includes(l.text.trim()));
  assert.equal(cells.length, 6, 'all table cells are drawn');
  return Math.max(...cells.map(l => l.x + l.w)) - Math.min(...cells.map(l => l.x));
}
When('a reply with a short table and a list streams in', async function (this: ThreadbearWorld) {
  const threadId = (await this.app.dump()).selection?.id;
  await this.app.do({cmd: 'mock', method: 'stream', params: {threadId, text: reply, hz: 60}});
  await this.until('the finished table reply', d => d.transcript.rows.some(r => r.kind === 'footer' && r.turn === 'turn_stream'));
});
Then('the short table has a compact width', async function (this: ThreadbearWorld) {
  originalWidth = tableWidth(await lines(this));
  assert.ok(originalWidth > 50 && originalWidth < 320, `short table width: ${originalWidth}`);
});
When('I widen the window around the short table', async function (this: ThreadbearWorld) {
  await this.app.do({cmd: 'resize', w: 1420, h: 900});
});
Then('the short table keeps its width', async function (this: ThreadbearWorld) {
  assert.ok(Math.abs(tableWidth(await lines(this)) - originalWidth) <= 1);
});
Then('the list items have six points between their lines', async function (this: ThreadbearWorld) {
  const drawn = await lines(this);
  const a = drawn.find(l => l.text.trim() === 'First item');
  const b = drawn.find(l => l.text.trim() === 'Second item');
  assert.ok(a && b, 'both list items are drawn');
  const gap = b.y - (a.y + a.h);
  assert.ok(Math.abs(gap - 6) <= 1, `list gap: ${gap}`);
});
