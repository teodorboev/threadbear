import assert from 'node:assert/strict';
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import { sleep, type Dump, type Row } from '../support/driver.ts';

const streamed = (d: Dump): Row | undefined => d.transcript.rows.some(r => r.kind === 'footer' && r.turn === 'turn_stream')
  ? [...d.transcript.rows].reverse().find(r => r.kind === 'assistant' && r.turn === 'turn_stream') : undefined;

When('a reply links {string} to the sample file {string}', async function (this: ThreadbearWorld, words: string, relative: string) {
  const dump = await this.app.dump();
  const file = `${dump.composer.cwd}/${relative}`;
  await this.app.do({cmd:'mock',method:'stream',params:{threadId:dump.selection?.id,text:`Please check [${words}](${file}).`,hz:60}});
  await this.until('the reply with a sample file',streamed,20_000);
});

async function picture(world: ThreadbearWorld, words: string): Promise<string> {
  const row=await world.until('the reply with its file link',streamed);
  const until=Date.now()+20_000;
  let state='';
  do {
    state=await world.app.do({cmd:'peek',row:row.id,words,keep:true,picture:true});
    if(!state.includes('loading'))return state;
    await sleep(80);await world.app.dump();
  } while(Date.now()<until);
  throw Error(`The hover thumbnail did not finish: ${state}`);
}

Then('the hover on {string} shows a raster at most {int} by {int}', async function (this: ThreadbearWorld, words: string, maxWidth: number, maxHeight: number) {
  const state=await picture(this,words);
  const size=/thumbnail (\d+)x(\d+)/.exec(state);
  assert.ok(size,`The painted hover asset has no raster: ${state}`);
  assert.ok(+size[1]>0 && +size[2]>0 && +size[1]<=maxWidth && +size[2]<=maxHeight,state);
});

Then('the hover on {string} falls back to the file symbol', async function (this: ThreadbearWorld, words: string) {
  assert.match(await picture(this,words),/thumbnail failed/);
});
