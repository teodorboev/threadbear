// A thread's search words (keywords.ts): the librarian writes them, and every
// search matches them. Mike searched "dentist" on his phone on 11 Nov and
// found nothing: the thread was "Wisdom tooth meeting location".
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { checkSummary } from '../helpers/protocol-check.ts';
import { Projects } from '../../src/projects.ts';
import { Librarian, FIRST_MESSAGE_MS, KEYWORD_BATCH, KEYWORD_SWEEP_MS, SETTLE_MS } from '../../src/librarian.ts';
import { cleanKeywords } from '../../src/keywords.ts';
import { forBackend, toThreadDoc, KEYWORDS_API_VERSION } from '../../src/sync.ts';
import { toIndexRow } from '../../src/thread-index.ts';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import type { GenerateParams, ModelResult } from '../../src/models/index.ts';

const reply = (v: unknown): ModelResult<string> => ({ ok: true, value: JSON.stringify(v), model: 'haiku' });

function setup(models: (p: GenerateParams) => ModelResult<string>, privateFolder?: (cwd: string) => boolean) {
  const root = tmpDir();
  const misc = path.join(root, 'dev/misc');
  fs.mkdirSync(misc, { recursive: true });
  const home = tmpDir();
  const projects = new Projects({ home, clock: new FakeClock(), log: nullLogger, userHome: root });
  const asked: GenerateParams[] = [];
  const h = makeHarness(home, {}, { projects, panelsRoot: root, actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 5000 });
  projects.start(h.runner);
  const librarian = new Librarian({
    host: h.runner,
    projects,
    models: {
      generate: async (p: GenerateParams) => {
        asked.push(p);
        return models(p);
      },
      isPrivateFolder: privateFolder,
    },
    clock: h.clock,
    log: nullLogger,
    home,
    userHome: root,
  });
  librarian.start();
  const create = (text: string, cwd = misc) => h.runner.create({ text, cwd, accountId: 'claude-3' }).thread;
  const finish = async (threadId: string) => {
    const s = h.adapter.sessions.filter((x) => x.opts.threadId === threadId).at(-1)!;
    s.init();
    s.result('completed');
    await tick();
  };
  const rec = (id: string) => h.runner.threadRecords().find((t) => t.id === id)!;
  return { h, root, misc, librarian, asked, create, finish, rec };
}

test('search words are lowercase, each once, none from the title, and short', () => {
  assert.equal(cleanKeywords('Dentist, dental  ORAL surgeon dentist wisdom', 'Wisdom tooth meeting'), 'dentist dental oral surgeon');
  // Single letters go; other scripts stay words.
  assert.equal(cleanKeywords('a café 東京 x'), 'café 東京');
  const long = Array.from({ length: 40 }, (_, i) => `word${i}`).join(' ');
  assert.ok(cleanKeywords(long).split(' ').length <= 20);
  assert.ok(cleanKeywords('abcdefghij '.repeat(40).replace(/ /g, (_, i) => `${i} `)).length <= 200);
});

test("a look gives the thread search words, and search finds it by them", async () => {
  let answer: unknown = { projects: [], why: 'A one-off', keywords: 'Dentist dental appointment wisdom' };
  const s = setup(() => reply(answer));
  const thread = s.create('Where have I got to go for my wisdom tooth meeting today?');
  await s.finish(thread.id);
  s.h.clock.advance(SETTLE_MS);
  await tick(6);
  // The look was asked for them.
  assert.match(s.asked[0].system ?? '', /"keywords":/);
  // "wisdom" is in the title already, so it's left out.
  assert.equal(s.h.runner.get(thread.id).keywords, 'dentist dental appointment');
  assert.deepEqual(checkSummary(s.h.runner.get(thread.id)), []);
  assert.deepEqual(s.h.runner.search('dentist').map((t) => t.id), [thread.id]);
  assert.deepEqual(s.h.runner.search('nothing like it'), []);
  // A later look that gives none keeps them.
  answer = { projects: [], why: 'Still a one-off' };
  s.librarian.run(thread.id);
  await tick(6);
  assert.equal(s.h.runner.get(thread.id).keywords, 'dentist dental appointment');
});

test('the keyword sweep gives older threads search words, a batch a call, newest first, and asks once', async () => {
  const s = setup((p) => {
    // Every id in the batch, as the model would.
    const ids = [...(p.prompt ?? '').matchAll(/^\[([0-9a-f-]{8})\]/gm)].map((m) => m[1]);
    return reply(Object.fromEntries(ids.slice(1).map((id) => [id, `words for ${id}`])));
  });
  // Threads from before search words, which no look will see again.
  const ids: string[] = [];
  for (let i = 0; i < KEYWORD_BATCH + 2; i++) {
    const t = s.create(`Thread number ${i}`);
    ids.push(t.id);
    s.h.clock.advance(1000);
  }
  // Each started one turn, so each has messages to go on. Their looks from
  // the first message go first (a sweep waits while one is going).
  await tick();
  s.h.clock.advance(FIRST_MESSAGE_MS);
  await tick(KEYWORD_BATCH * 4);
  s.asked.length = 0;
  s.h.clock.advance(KEYWORD_SWEEP_MS);
  await tick(6);
  const sweeps = s.asked.filter((p) => /search words/.test(p.system ?? '') && !/librarian in Threadbear/.test(p.system ?? ''));
  assert.equal(sweeps.length, 1);
  // The newest KEYWORD_BATCH, and nothing from a private folder or the rest.
  const newest = [...ids].reverse().slice(0, KEYWORD_BATCH);
  for (const id of newest) assert.match(sweeps[0].prompt ?? '', new RegExp(`\\[${id.slice(0, 8)}\\]`));
  assert.doesNotMatch(sweeps[0].prompt ?? '', new RegExp(`\\[${ids[0].slice(0, 8)}\\]`));
  // The first had none from the model: "", so it isn't asked about again.
  assert.equal(s.rec(newest[0]).keywords, '');
  assert.equal(s.rec(newest[1]).keywords, `words for ${newest[1].slice(0, 8)}`);
  // The next sweep takes the rest, and after that there's nothing to ask.
  s.h.clock.advance(KEYWORD_SWEEP_MS);
  await tick(6);
  s.h.clock.advance(KEYWORD_SWEEP_MS);
  await tick(6);
  const all = s.asked.filter((p) => /search words/.test(p.system ?? '') && !/librarian in Threadbear/.test(p.system ?? ''));
  assert.equal(all.length, 2);
  for (const id of ids) assert.notEqual(s.rec(id).keywords, undefined);
});

test("the keyword sweep leaves private folders' threads out", async () => {
  const s = setup(() => reply({}), (cwd) => cwd.includes('secret'));
  const secret = path.join(s.root, 'dev/secret');
  fs.mkdirSync(secret, { recursive: true });
  const t = s.create('My private plans', secret);
  await tick();
  s.asked.length = 0;
  s.h.clock.advance(KEYWORD_SWEEP_MS);
  await tick(6);
  assert.equal(s.asked.filter((p) => /search words/.test(p.system ?? '')).length, 0);
  assert.equal(s.rec(t.id).keywords, undefined);
});

test('search words go up to a backend of API 74 or later, and not to an older one', () => {
  const s = setup(() => reply({}));
  const t = s.create('Wisdom tooth meeting location');
  s.h.runner.setKeywords(t.id, 'dentist dental');
  const summary = s.h.runner.get(t.id);
  const doc = toThreadDoc(summary, 'runner-1', Date.now());
  assert.equal(doc.keywords, 'dentist dental');
  assert.equal(forBackend(doc, KEYWORDS_API_VERSION, Date.now()).keywords, 'dentist dental');
  assert.equal(forBackend(doc, KEYWORDS_API_VERSION - 1, Date.now()).keywords, undefined);
  assert.equal(toIndexRow(summary).keywords, 'dentist dental');
  assert.equal(toIndexRow(summary, true, true, true, false).keywords, undefined);
});
