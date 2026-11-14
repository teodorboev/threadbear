import assert from 'node:assert/strict';
import { After, Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import { sleep } from '../support/driver.ts';

const requests=new WeakMap<ThreadbearWorld,number>();
async function native(world:ThreadbearWorld,op:string,params:Record<string,unknown>={}) {
  const reply=await world.app.do({cmd:'native-text',op,...params});
  return reply==='ok'?null:JSON.parse(reply);
}
When('I select {string} for a Mac Service', async function(this:ThreadbearWorld,text:string){
  assert.equal(process.platform,'darwin');
  assert.equal(process.env.TB_REAL_SCREEN,'1','Run native AppKit focus checks only on a check host desktop');
  await this.app.do({cmd:'front'});
  await this.app.do({cmd:'type',replace:true,text});
  await this.app.do({cmd:'menu-key',key:'a',modifiers:['cmd']});
  await this.app.dump();
  const state=await native(this,'state');
  assert.equal(state.words,text);
  assert.equal(state.snapshotWords,text,'AppKit needs the automatically refreshed selection');
});
async function begin(world:ThreadbearWorld,text:string,returning:boolean){
  const selected=await native(world,'begin',{returning});
  assert.equal(selected.words,text);
  requests.set(world,selected.request);
}
Then('the native Service receives {string}',async function(this:ThreadbearWorld,text:string){await begin(this,text,true);});
Then('a send-only native Service receives {string}',async function(this:ThreadbearWorld,text:string){await begin(this,text,false);});
When('the native Service returns {string}',async function(this:ThreadbearWorld,text:string){
  const result=await native(this,'return',{request:requests.get(this),text});
  assert.equal(result.accepted,true);
  await sleep(100);
});
Then('the native Service cannot return another answer',async function(this:ThreadbearWorld){
  assert.equal((await native(this,'return',{request:requests.get(this),text:'duplicate answer'})).accepted,false);
});
When('I undo the Service edit',async function(this:ThreadbearWorld){await this.app.do({cmd:'menu-key',key:'z',modifiers:['cmd']});});
After(async function(this:ThreadbearWorld){
  const request=requests.get(this);
  if(request!==undefined){await native(this,'discard',{request}).catch(()=>{});requests.delete(this);}
});

Then('the shared text selection is {string}',async function(this:ThreadbearWorld,text:string){
  const source=JSON.parse(await this.app.do({cmd:'text-selection-source'}));
  assert.equal(source.words?.trim(),text);
});
When('I give the Terminal the keyboard',async function(this:ThreadbearWorld){
  if(process.platform==='darwin'&&process.env.TB_REAL_SCREEN==='1')await this.app.do({cmd:'front'});
  await this.app.do({cmd:'terminal',toggle:true});
  await this.app.dump();
});
Then('no reply words are offered to system text services',async function(this:ThreadbearWorld){
  const source=JSON.parse(await this.app.do({cmd:'text-selection-source'}));
  assert.equal(source.terminalFocused,true);
  assert.equal(source.words,null,'The Terminal must not send an older reply selection');
  if(process.platform==='darwin'&&process.env.TB_REAL_SCREEN==='1'){
    assert.equal((await native(this,'state')).snapshotWords,null);
    await assert.rejects(()=>native(this,'begin'),/no requestor for this selection/);
  }
});

When('I mouse-select {string} in the reply',async function(this:ThreadbearWorld,text:string){
  assert.equal(process.platform,'darwin');assert.equal(process.env.TB_REAL_SCREEN,'1');
  await this.app.do({cmd:'front'});
  const dump=await this.app.dump();
  const row=[...dump.transcript.rows].reverse().find(row=>row.kind==='assistant'&&row.turn==='turn_stream');
  assert.ok(row);
  const lines=JSON.parse((await this.app.do({cmd:'text-lines',row:row.id})).replace(/^ok /,''));
  const line=lines.find((line:{text:string})=>line.text.trim()===text);assert.ok(line);
  await this.app.do({cmd:'click-at',x:line.x+4,y:line.y+line.h/2,count:3});
  await this.app.dump();
});
Then('the automatic Services snapshot contains {string}',async function(this:ThreadbearWorld,text:string){
  const state=await native(this,'state');assert.equal(state.words?.trim(),text);assert.equal(state.snapshotWords?.trim(),text);
});
When('that reply is replaced with {string}',async function(this:ThreadbearWorld,text:string){
  const dump=await this.app.dump();
  await this.app.do({cmd:'mock',method:'replaceReply',params:{threadId:dump.selection?.id,text}});
  await this.until('the changed reply painted',dump=>dump.transcript.rows.some(row=>row.kind==='assistant'&&row.turn==='turn_stream'&&row.text===text));
  await this.app.dump();await sleep(100);
});
Then('the automatic Services snapshot has no old reply words',async function(this:ThreadbearWorld){
  const state=await native(this,'state');
  assert.equal(state.words,null,'the text highlight must already be cleared');
  assert.equal(state.snapshotWords,null,'A cleared reply must not remain in the AppKit Services requestor');
  await assert.rejects(()=>native(this,'begin'),/no requestor for this selection/);
});

const lookupRequests=new WeakMap<ThreadbearWorld,number>();
When('I use the Mac Look Up shortcut',async function(this:ThreadbearWorld){
  lookupRequests.set(this,(await native(this,'state')).lookupRequests);
  await this.app.do({cmd:'menu-key',key:'d',modifiers:['cmd','ctrl']});
  await this.app.dump();await sleep(100);
});
Then('the selected words are offered to Look Up once',async function(this:ThreadbearWorld){
  const state=await native(this,'state');
  assert.equal(state.lookupWords,'dictionary','The exact selected words must reach AppKit');
  assert.equal(state.lookupRequests,lookupRequests.get(this)!+1,
    'Control-Command-D must reach the native selection action from the focused composer');
  await native(this,'dismiss');
});

Then('the Terminal keeps the Mac Look Up shortcut',async function(this:ThreadbearWorld){
  if(process.platform!=='darwin'||process.env.TB_REAL_SCREEN!=='1')return;
  const before=await native(this,'state');
  await this.app.do({cmd:'menu-key',key:'d',modifiers:['cmd','ctrl']});
  await this.app.dump();await sleep(100);
  const after=await native(this,'state');
  assert.equal(after.lookupRequests,before.lookupRequests,'Terminal focus must not send stale reply words');
  assert.equal(after.lookupShortcuts,before.lookupShortcuts,'The wrapper must let the Terminal receive its shortcut');
});
