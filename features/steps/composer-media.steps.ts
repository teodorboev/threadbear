import { When, Then } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';
import { recording } from '../tools/real-screen.ts';

const sample = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../support/sample.png');

When('I open Attach images from the composer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Attach images, change the folder or schedule the message' });
  await this.app.do({ cmd: 'click', tip: 'Attach images…' });
});

Then('choosing a document cannot add a file chip', function () {
  return 'pending';
});

Then('choosing a picture can add a picture chip', function () {
  return 'pending';
});

async function drop(world: ThreadbearWorld, kind: string, picture: string, vanish = false) {
  await world.app.do({ cmd: 'picture-input', kind, path: picture, drop: true });
  await sleep(60);
  await world.app.dump();
  await world.app.do({ cmd: 'picture-drop', phase: 1 });
  await sleep(60);
  await world.app.dump();
  await world.app.do({ cmd: 'picture-drop', phase: 2, ...(vanish ? { vanish: picture } : {}) });
}

When('I drop a temporary screenshot and its source disappears', async function (this: ThreadbearWorld) {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-screenshot-'));
  const screenshot = path.join(folder, 'Screenshot.png');
  fs.copyFileSync(sample, screenshot);
  try {
    await drop(this, 'file', screenshot, true);
    for (let i = 0; i < 60 && fs.existsSync(screenshot); i++) { await this.app.dump(); await sleep(50); }
    if (fs.existsSync(screenshot)) throw new Error('The screenshot source was not removed as the drop ended');
  } finally { fs.rmSync(folder, { recursive: true, force: true }); }
});

When('I drop a browser image without a file', async function (this: ThreadbearWorld) {
  await drop(this, 'browser', sample);
});

async function pasteHeic(world: ThreadbearWorld, keepFocus = false) {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-heic-'));
  const picture = path.join(folder, 'photo.heic');
  try {
    execFileSync('/usr/bin/sips', ['-s', 'format', 'heic', sample, '--out', picture], { stdio: 'pipe' });
    await world.app.do({ cmd: 'picture-input', kind: 'heic', path: picture, keepFocus });
  } finally { fs.rmSync(folder, { recursive: true, force: true }); }
}
When('I paste a HEIC picture from the native clipboard', async function (this: ThreadbearWorld) { await pasteHeic(this); });
When('I paste a HEIC picture into the focused answer box', async function (this: ThreadbearWorld) { await pasteHeic(this, true); });

When('I paste a Photos picture beside a normal picture', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'photos', path: sample });
});

When("I drag a screenshot's floating thumbnail onto the thread", function () {
  throw new Error('Manual: take a screenshot and drag its floating thumbnail into the thread before it disappears');
});

When('I paste a picture with an unavailable file URL', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'unreadable', path: sample });
});

When('I start a long turn for the picture check', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'send', text: 'Hold the turn for picture input' });
  await this.until('the turn running', (d) => d.selection?.status === 'running');
});

Then('the picture-loading button does not stop the turn', async function (this: ThreadbearWorld) {
  const point = await this.app.do({ cmd: 'label-point', label: 'Loading picture' });
  const [, x, y] = point.trim().split(/\s+/);
  assert.ok(x && y, point);
  if (process.env.TB_MEDIA_BUSY_PROOF) await this.app.shot(process.env.TB_MEDIA_BUSY_PROOF);
  await this.app.do({ cmd: 'click-at', x: Number(x), y: Number(y) });
  const until = Date.now() + 2000;
  while (Date.now() < until) {
    assert.equal((await this.app.dump()).selection?.status, 'running', 'clicking the pending Send interrupted the turn');
    await sleep(100);
  }
});

Then('I can click Stop while the only picture is loading', async function (this: ThreadbearWorld) {
  assert.equal((await this.app.dump()).composer.images, 0);
  const point = await this.app.do({ cmd: 'label-point', label: 'Stop' });
  const [, x, y] = point.trim().split(/\s+/);
  assert.ok(x && y, point);
  if (process.env.TB_MEDIA_STOP_PROOF) await this.app.shot(process.env.TB_MEDIA_STOP_PROOF);
  await this.app.do({ cmd: 'click-at', x: Number(x), y: Number(y) });
});

When('I paste a picture whose file open is delayed', async function (this: ThreadbearWorld) {
  const start = Date.now();
  await this.app.do({ cmd: 'picture-input', kind: 'slow-file', path: sample });
  await this.app.do({ cmd: 'type', text: 'I can keep typing' });
  const dump = await this.app.dump();
  assert.ok(Date.now() - start < 5000, 'file open blocked the composer until it finished');
  assert.equal(dump.composer.images, 0, 'the delayed open finished before the responsiveness check');
  assert.match(dump.composer.text, /I can keep typing/);
});

When('I paste image bytes beside a file whose open is delayed', async function (this: ThreadbearWorld) {
  const start = Date.now();
  await this.app.do({ cmd: 'picture-input', kind: 'slow-file-bytes', path: sample });
  await this.until('the captured fallback picture', (d) => d.composer.images === 1);
  assert.ok(Date.now() - start < 5000, 'the captured bytes waited for the eight-second file open');
});

When('I paste image bytes beside a protected file containing different bytes', async function (this: ThreadbearWorld) {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-paste-alternates-'));
  const picture = path.join(folder, 'bytes.png');
  fs.copyFileSync(sample, picture);
  fs.writeFileSync(`${picture}.file.png`, 'this is a readable path, but not the pasted image');
  try { await this.app.do({ cmd: 'picture-input', kind: 'protected', path: picture }); }
  finally { fs.rmSync(folder, { recursive: true, force: true }); }
});

When('I paste an image file beside a different TIFF preview', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'file-preview', path: sample });
});

When('I paste an unavailable image file beside a TIFF icon', async function (this: ThreadbearWorld) {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-missing-picture-'));
  try { await this.app.do({ cmd: 'picture-input', kind: 'file-preview', path: path.join(folder, 'missing.png') }); }
  finally { fs.rmSync(folder, { recursive: true, force: true }); }
});

Then('the picture import reports an error', async function (this: ThreadbearWorld) {
  await this.until('an image import error', (d) => d.transcript.error?.includes("Couldn't add that picture"));
  assert.equal((await this.app.dump()).composer.images, 0, 'a file icon was imported as the picture');
});

Then(/^the imported picture contains (?:the pasted bytes|the original file bytes)$/, async function (this: ThreadbearWorld) {
  await this.until('the imported picture', (d) => d.composer.images === 1);
  const result = await this.app.do({ cmd: 'composer-peek', number: 1, press: true });
  const paths = / of (\[.*\])$/.exec(result);
  assert.ok(paths, result);
  const imported = (JSON.parse(paths[1]) as string[])[0];
  assert.ok(fs.readFileSync(imported).equals(fs.readFileSync(sample)), 'the imported bytes did not match the chosen picture');
  await this.app.do({ cmd: 'viewer', close: true });
});

When('I paste a bundle promising two pictures beside a normal picture', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'legacy', path: sample });
});
When('I paste a bundle with an unavailable picture and two good pictures beside a normal picture', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'legacy-error', path: sample });
});
When('I paste a withdrawn promise beside a normal picture', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'withdrawn', path: sample });
});
When('I paste a working picture promise beside an unavailable file URL', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'paired-file-promise', path: sample });
});
When('I paste a readable image beside a withdrawn promise', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'picture-input', kind: 'paired-withdrawn-promise', path: sample });
});

When('I paste a promised photo beside a normal picture', async function (this: ThreadbearWorld) {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-promise-'));
  const binary = path.join(folder, 'picture-source');
  const source = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../tools/picture-promise.m');
  execFileSync('/usr/bin/clang', ['-fobjc-arc', '-framework', 'AppKit', '-framework', 'ApplicationServices', source, '-o', binary], { stdio: 'pipe' });
  // Refuse to post events over someone else's recording.
  if (recording()) throw new Error('Tinker Desk is recording; refusing the real drag');
  const pid = (this.app as unknown as { pid: number }).pid;
  const child = spawn(binary, [sample, String(pid)], { stdio: ['ignore', 'pipe', 'pipe'] });
  let sourceErrors = '';
  let restore: string[] | undefined;
  child.stderr.on('data', (data) => { sourceErrors += data; });
  try {
    const coordinates = await new Promise<string>((resolve, reject) => {
      const lines = createInterface({ input: child.stdout });
      lines.once('line', (name) => { lines.close(); resolve(name); });
      child.once('exit', (code) => reject(new Error(`The file-promise source exited (${code}): ${sourceErrors}`)));
    });
    const points = coordinates.trim().split(/\s+/).map(Number);
    const { assertInFront, windowAt } = await import('../tools/real-screen.ts');
    assertInFront(child.pid!, [[points[0], points[1]]]);
    // A crash notice or another job may cover part of the destination.
    // Use another exposed point inside the thread, never dismiss their UI
    // or post a drop onto whatever happens to be over the original point.
    if (windowAt(points[2], points[3])?.pid !== pid) {
      const left = points[2] - 650, top = points[3] - 300;
      const exposed = [[500, 200], [500, 500], [650, 600], [500, 650]]
        .map(([x, y]) => [left + x, top + y] as [number, number])
        .find(([x, y]) => windowAt(x, y)?.pid === pid);
      if (exposed) { points[2] = exposed[0]; points[3] = exposed[1]; }
    }
    assertInFront(pid, [[points[2], points[3]]]);
    restore = execFileSync(binary, ['--post', ...points.map(String)], { encoding: 'utf8', stdio: 'pipe' }).trim().split(/\s+/);
    try { await this.until('the promised photo and normal picture', (d) => d.composer.images === 2); if (process.env.TB_MEDIA_PROOF) await this.app.shot(process.env.TB_MEDIA_PROOF); } catch (error) { throw new Error(`${error}\n${sourceErrors}`); }
  } finally {
    if (child.exitCode === null && child.signalCode === null) { child.kill('SIGTERM'); await new Promise<void>((resolve) => child.once('exit', () => resolve())); }
    if (restore) { await sleep(200); execFileSync(binary, ['--move', ...restore], { stdio: 'pipe' }); }
    fs.rmSync(folder, { recursive: true, force: true });
  }
});

// A URL served locally supplies real image bytes through the app's HTTP
// client, without depending on an outside website or signed-in browser.
import http from 'node:http';
import { After } from '@cucumber/cucumber';
const servers = new Set<http.Server>();
After(async () => {
  for (const server of servers) {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  servers.clear();
});
async function browserUrl(world: ThreadbearWorld, delay: number) {
  const server = http.createServer((_, response) => {
    const timer = setTimeout(() => { response.writeHead(200, { 'Content-Type': 'image/png' }); response.end(fs.readFileSync(sample)); }, delay);
    response.on('close', () => clearTimeout(timer));
  });
  servers.add(server);
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address() as { port: number };
  await drop(world, 'url-fixture', `http://127.0.0.1:${address.port}/photo`);
  if (delay) {
    // The outgoing draft omits unfinished pictures. Read the actual inline
    // token instead, or this test accidentally waits for the download.
    const until = Date.now() + 5000;
    while (!(await world.app.do({ cmd: 'composer-chips' })).includes('picture: Picture 1')) {
      assert.ok(Date.now() < until, 'the pending picture never made a chip');
      await world.app.dump(); await sleep(50);
    }
    assert.equal((await world.app.dump()).composer.images, 0, 'the download finished before the pending-picture check');
  }
}
When('I drop a browser picture offered only as a URL', async function (this: ThreadbearWorld) {
  await browserUrl(this, 0);
});
When('I drop a browser picture whose response is delayed', async function (this: ThreadbearWorld) {
  await browserUrl(this, 30_000);
});

async function ordinaryLink(world: ThreadbearWorld & { fetched?: () => number }, kind: string) {
  let requests = 0;
  const server = http.createServer((_, response) => { requests++; response.end('an ordinary webpage'); });
  servers.add(server);
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address() as { port: number };
  world.fetched = () => requests;
  await drop(world, kind, `http://127.0.0.1:${address.port}/one-time-link`);
}
When('I drop an ordinary web link', async function (this: ThreadbearWorld & { fetched?: () => number }) {
  await ordinaryLink(this, 'link');
});
When('I drop a linked picture whose link is a webpage', async function (this: ThreadbearWorld & { fetched?: () => number }) {
  await ordinaryLink(this, 'linked-image');
});
When('I drop a browser image URL pointing to a local service', async function (this: ThreadbearWorld & { fetched?: () => number }) {
  await ordinaryLink(this, 'url');
});
When('I drop a picture URL redirecting to a different local service', async function (this: ThreadbearWorld & { fetched?: () => number; redirectRequests?: () => number }) {
  let requests = 0;
  const target = http.createServer((_, response) => { requests++; response.end(fs.readFileSync(sample)); });
  servers.add(target);
  await new Promise<void>((resolve) => target.listen(0, '127.0.0.1', resolve));
  const port = (target.address() as { port: number }).port;
  let sourceRequests = 0;
  const source = http.createServer((_, response) => {
    sourceRequests++;
    response.writeHead(302, { Location: `http://127.0.0.1:${port}/private.png` }); response.end();
  });
  servers.add(source);
  await new Promise<void>((resolve) => source.listen(0, '127.0.0.1', resolve));
  this.fetched = () => requests;
  this.redirectRequests = () => sourceRequests;
  // Only this source's exact socket is allowed by the automation fixture.
  // A redirected connection still goes through the public-address policy.
  await drop(this, 'url-fixture', `http://127.0.0.1:${(source.address() as { port: number }).port}/photo`);
});
Then('the link has not been fetched', async function (this: ThreadbearWorld & { fetched?: () => number; redirectRequests?: () => number }) {
  if (this.redirectRequests) {
    const until = Date.now() + 5000;
    while (!this.redirectRequests() && Date.now() < until) await sleep(50);
    assert.equal(this.redirectRequests(), 1, 'the redirect source was never fetched');
  }
  await sleep(300);
  assert.equal(this.fetched?.(), 0);
  assert.equal((await this.app.dump()).composer.images, 0);
});
