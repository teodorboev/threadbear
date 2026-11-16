import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { imagesInEvent, localImagePath, markdownImages, PIN_TOOL, readReplyImage, readShareImage, replyImagesInEvent } from '../../src/share-images.ts';
import { tmpDir } from '../helpers/fakes.ts';

test('share images: Markdown pictures on this Mac, not on the web', () => {
  const home = '/Users/ann';
  assert.equal(localImagePath('/tmp/a.png', home), '/tmp/a.png');
  assert.equal(localImagePath('~/shots/b.JPG', home), '/Users/ann/shots/b.JPG');
  assert.equal(localImagePath('file:///tmp/c%20d.webp', home), '/tmp/c d.webp');
  assert.equal(localImagePath('https://example.com/a.png', home), undefined);
  assert.equal(localImagePath('/tmp/notes.txt', home), undefined);
  assert.equal(localImagePath('relative/a.png', home), undefined);
  assert.deepEqual(
    markdownImages('![one](/tmp/a.png) and ![two](<~/b c.gif> "title"), ![web](https://x.io/c.png), ![again](/tmp/a.png)', home),
    [{ src: '/tmp/a.png', path: '/tmp/a.png' }, { src: '~/b c.gif', path: '/Users/ann/b c.gif' }],
  );
});

test('share images: attachments, tool pictures and pictures in text', () => {
  const at = '2026-10-29T00:00:00.000Z';
  const img = { path: '/data/media/x.png', mediaType: 'image/png', width: 10, height: 20 };
  assert.deepEqual(imagesInEvent({ seq: 3, ts: at, type: 'user.message', text: 'see', images: [img] } as never), [{ seq: 3, ...img }]);
  assert.deepEqual(imagesInEvent({ seq: 5, ts: at, type: 'tool.result', itemId: 't', ok: true, text: '', truncated: false, images: [img] } as never), [{ seq: 5, ...img }]);
  assert.deepEqual(imagesInEvent({ seq: 7, ts: at, type: 'assistant.text', itemId: 'm', text: '![bear](/tmp/bear.png)' } as never, '/Users/ann'), [
    { seq: 7, path: '/tmp/bear.png', src: '/tmp/bear.png', mediaType: 'image/png' },
  ]);
  assert.deepEqual(imagesInEvent({ seq: 8, ts: at, type: 'reasoning', itemId: 'r', text: '![x](/tmp/x.png)' } as never), []);
});

test('reply images: pictures in a reply and pinned pictures, nothing else', () => {
  const home = '/Users/ann';
  const base = { threadId: 't', turnId: 'u', ts: '2026-11-01T00:00:00.000Z' };
  const ev = (seq: number, rest: object) => ({ ...base, seq, ...rest }) as never;
  const pin = (p: string) => ev(4, { type: 'tool.call', itemId: 'i', name: PIN_TOOL, kind: 'mcp', title: 'pin', input: { path: p } });
  assert.deepEqual(replyImagesInEvent(ev(1, { type: 'assistant.text', text: '![a](/w/a.png) and ![b](https://x.io/b.png)' }), '/w', home), [{ seq: 1, path: '/w/a.png', src: '/w/a.png' }]);
  assert.deepEqual(replyImagesInEvent(pin('/w/a.png'), '/w', home), [{ seq: 4, path: '/w/a.png', pinned: true }]);
  // A relative pin is from the thread's folder, and without one it's unknown.
  assert.deepEqual(replyImagesInEvent(pin('guide/a.jpg'), '/w', home), [{ seq: 4, path: '/w/guide/a.jpg', pinned: true }]);
  assert.deepEqual(replyImagesInEvent(pin('guide/a.jpg'), undefined, home), []);
  assert.deepEqual(replyImagesInEvent(pin('~/a.webp'), undefined, home), [{ seq: 4, path: '/Users/ann/a.webp', pinned: true }]);
  // A pinned doc, a link pin, Alex's attachments and a tool's screenshots stay.
  assert.deepEqual(replyImagesInEvent(pin('/w/notes.md'), '/w', home), []);
  assert.deepEqual(replyImagesInEvent(ev(5, { type: 'tool.call', itemId: 'j', name: PIN_TOOL, kind: 'mcp', title: 'pin', input: { url: 'https://x.io/a.png' } }), '/w', home), []);
  const image = { path: '/w/c.png', mediaType: 'image/png' };
  assert.deepEqual(replyImagesInEvent(ev(6, { type: 'user.message', text: 'hi', images: [image] }), '/w', home), []);
  assert.deepEqual(replyImagesInEvent(ev(7, { type: 'tool.result', itemId: 'j', ok: true, text: '', truncated: false, images: [image] }), '/w', home), []);
});

test('reply images: a video goes to the phone, but not to a shared page', () => {
  const home = '/Users/ann';
  const base = { threadId: 't', turnId: 'u', ts: '2026-11-01T00:00:00.000Z' };
  const text = { ...base, seq: 1, type: 'assistant.text', itemId: 'm', text: '![clip](/w/a.mp4) ![old](/w/b.MOV) ![web](/w/c.webm)' } as never;
  assert.deepEqual(replyImagesInEvent(text, '/w', home), [
    { seq: 1, path: '/w/a.mp4', src: '/w/a.mp4' },
    { seq: 1, path: '/w/b.MOV', src: '/w/b.MOV' },
  ]);
  const pin = { ...base, seq: 2, type: 'tool.call', itemId: 'i', name: PIN_TOOL, kind: 'mcp', title: 'pin', input: { path: 'clip.m4v' } } as never;
  assert.deepEqual(replyImagesInEvent(pin, '/w', home), [{ seq: 2, path: '/w/clip.m4v', pinned: true }]);
  assert.equal(localImagePath('/w/a.mp4', home), undefined);
  assert.deepEqual(imagesInEvent(text, home), []);
});

// The boxes of an MP4 as far as the size: ftyp, then moov > trak > tkhd.
function mp4(brand: string, tracks: { width: number; height: number; rotated?: boolean; version?: 0 | 1 }[]): Buffer {
  const box = (type: string, ...parts: Buffer[]) => {
    const size = Buffer.alloc(4);
    size.writeUInt32BE(8 + parts.reduce((n, p) => n + p.length, 0));
    return Buffer.concat([size, Buffer.from(type, 'ascii'), ...parts]);
  };
  const trak = ({ width, height, rotated, version = 0 }: (typeof tracks)[number]) => {
    const shift = version === 1 ? 12 : 0;
    const tkhd = Buffer.alloc(84 + shift);
    tkhd[0] = version;
    tkhd.writeInt32BE(rotated ? 0 : 0x10000, 40 + shift);
    tkhd.writeUInt32BE(width * 0x10000, 76 + shift);
    tkhd.writeUInt32BE(height * 0x10000, 80 + shift);
    return box('trak', box('tkhd', tkhd));
  };
  return Buffer.concat([box('ftyp', Buffer.from(brand.padEnd(8, '\0'), 'latin1')), box('mdat', Buffer.alloc(40)), box('moov', box('mvhd', Buffer.alloc(100)), ...tracks.map(trak))]);
}

test('reply images: a video has to really be one, and says its size', () => {
  const dir = tmpDir();
  const write = (name: string, bytes: Buffer) => {
    const file = path.join(dir, name);
    fs.writeFileSync(file, bytes);
    return file;
  };
  // The first track with a picture, past an audio track of 0 x 0.
  const read = readReplyImage(write('a.mp4', mp4('isom', [{ width: 0, height: 0 }, { width: 1280, height: 720 }])));
  assert.ok(!('skip' in read));
  assert.deepEqual({ mediaType: read.mediaType, width: read.width, height: read.height }, { mediaType: 'video/mp4', width: 1280, height: 720 });
  // A clip filmed upright is stored sideways with a rotation.
  const upright = readReplyImage(write('b.mov', mp4('qt  ', [{ width: 1920, height: 1080, rotated: true, version: 1 }])));
  assert.ok(!('skip' in upright));
  assert.deepEqual({ mediaType: upright.mediaType, width: upright.width, height: upright.height }, { mediaType: 'video/quicktime', width: 1080, height: 1920 });
  // No size found still goes: the phone gets it as it loads.
  const sizeless = readReplyImage(write('c.mp4', mp4('mp42', [])));
  assert.ok(!('skip' in sizeless));
  assert.equal(sizeless.width, undefined);
  assert.deepEqual(readReplyImage(write('d.mp4', Buffer.from('not a video at all, whatever it says'))), { skip: "it isn't a picture" });
  assert.deepEqual(readReplyImage(write('e.mp4', mp4('heic', []))), { skip: "it isn't a picture" });
});

test('reply images: a GIF goes as it is, with all its frames', () => {
  const dir = tmpDir();
  const file = path.join(dir, 'a.gif');
  const bytes = Buffer.concat([Buffer.from('GIF89a'), Buffer.from([0x20, 0x00, 0x10, 0x00]), Buffer.alloc(200, 7)]);
  fs.writeFileSync(file, bytes);
  const read = readReplyImage(file);
  assert.ok(!('skip' in read));
  assert.equal(read.mediaType, 'image/gif');
  assert.deepEqual(Buffer.from(read.bytes), bytes);
});

test('reply images: a file has to really be a picture', () => {
  const dir = tmpDir();
  const png = path.join(dir, 'real.png');
  const header = Buffer.alloc(24);
  header.writeUInt32BE(0x89504e47, 0);
  header.writeUInt32BE(0x0d0a1a0a, 4);
  header.writeUInt32BE(800, 16);
  header.writeUInt32BE(600, 20);
  fs.writeFileSync(png, header);
  const read = readReplyImage(png);
  assert.ok(!('skip' in read));
  assert.deepEqual([read.mediaType, read.width, read.height], ['image/png', 800, 600]);
  const fake = path.join(dir, 'secrets.png');
  fs.writeFileSync(fake, 'API_KEY=hunter2');
  assert.deepEqual(readReplyImage(fake), { skip: "it isn't a picture" });
  assert.deepEqual(readReplyImage(path.join(dir, 'gone.png')), { skip: 'the file is gone' });
  assert.deepEqual(readReplyImage(dir + '/'), { skip: "it can't be read" });
});

// A shared // A shared thread's pictures go to a public page, so only real pictures
// the thread was given or a tool handed back, in its media folder, go up
// (docs/protocol.md, "Sharing").
function pngHeader(w: number, h: number): Buffer {
  const b = Buffer.alloc(24);
  b.writeUInt32BE(0x89504e47, 0);
  b.writeUInt32BE(0x0d0a1a0a, 4);
  b.writeUInt32BE(w, 16);
  b.writeUInt32BE(h, 20);
  return b;
}

function write(file: string, bytes: Buffer | string): string {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, bytes);
  return file;
}

const OUTSIDE = "it isn't in the thread's media folder";
const md = (src: string) => imagesInEvent({ seq: 3, ts: '2026-11-05T10:00:00.000Z', type: 'assistant.text', itemId: 'm', text: `Look: ![chart](${src})` } as never);

// Daybreak verify-fix (final, 5 Nov), finding 10: the remediation test.
// Assistant Markdown naming a real picture anywhere in the thread's
// folder, old or just written, makes no public share image: only the
// thread's media folder (attachments, tool pictures, what the agent copies
// there on purpose) is a picture's way onto the page.
test("share images: a picture in the thread's folder stays on the Mac, new or old; one in its media folder goes", () => {
  const cwd = tmpDir();
  const media = tmpDir();
  const old = write(path.join(cwd, 'docs', 'passport-scan.png'), pngHeader(1200, 800));
  fs.utimesSync(old, new Date(Date.now() - 86_400_000), new Date(Date.now() - 86_400_000));
  const fresh = write(path.join(cwd, 'out', 'chart.png'), pngHeader(640, 480));
  for (const file of [old, fresh]) {
    const [img] = md(file);
    assert.equal(img?.path, file, 'the Markdown names it');
    assert.deepEqual(readShareImage(img!.path, media), { skip: OUTSIDE });
  }
  const shared = write(path.join(media, 'chart.jpg'), pngHeader(640, 480));
  const read = readShareImage(md(shared)[0]!.path, media);
  assert.ok(!('skip' in read), JSON.stringify(read));
  // Named .jpg but a PNG: the type comes from the bytes.
  assert.deepEqual([read.mediaType, read.width, read.height], ['image/png', 640, 480]);
  // No media folder to go by: nothing goes.
  assert.deepEqual(readShareImage(shared, undefined), { skip: OUTSIDE });
});

test("share images: in the media folder too, a symlink, a text file named .png, a folder link out and a video stay on the Mac", () => {
  const media = tmpDir();
  const elsewhere = tmpDir();
  const secret = write(path.join(elsewhere, 'id_rsa'), '-----BEGIN OPENSSH PRIVATE KEY-----');
  const link = path.join(media, 'link.png');
  fs.symlinkSync(secret, link);
  assert.deepEqual(readShareImage(link, media), { skip: "it's a link" });
  // A link to a real picture is refused too: the check is on the link.
  const realPic = write(path.join(media, 'real.png'), pngHeader(1, 1));
  const picLink = path.join(media, 'pic-link.png');
  fs.symlinkSync(realPic, picLink);
  assert.deepEqual(readShareImage(picLink, media), { skip: "it's a link" });
  assert.deepEqual(readShareImage(write(path.join(media, 'notes.png'), 'API_KEY=hunter2'), media), { skip: "it isn't a picture" });
  // A folder link inside the media folder that leads out.
  write(path.join(elsewhere, 'secret.png'), pngHeader(10, 10));
  fs.symlinkSync(elsewhere, path.join(media, 'out'));
  assert.deepEqual(readShareImage(path.join(media, 'out', 'secret.png'), media), { skip: OUTSIDE });
  // A video can't go on the page, which draws with <img>.
  const mp4 = write(path.join(media, 'clip.mp4'), Buffer.concat([Buffer.from([0, 0, 0, 16]), Buffer.from('ftypisom'), Buffer.alloc(4)]));
  assert.equal('skip' in readShareImage(mp4, media), true);
  assert.deepEqual(readShareImage(path.join(media, 'gone.png'), media), { skip: 'the file is gone' });
  assert.deepEqual(readShareImage('/Users/x/secret.png', media), { skip: OUTSIDE });
});

// Daybreak verify-fix (5 Nov): the temp folders were allowed, and they hold
// every app's screenshots, so Markdown naming one published it.
test("share images: a real picture in a temp folder that isn't the thread's stays on the Mac", () => {
  const media = tmpDir();
  for (const dir of [os.tmpdir(), '/tmp']) {
    const other = fs.mkdtempSync(path.join(dir, 'other-app-'));
    const shot = write(path.join(other, 'Screenshot.png'), pngHeader(1440, 900));
    const [img] = md(shot);
    assert.equal(img?.path, shot, 'the Markdown names it');
    assert.deepEqual(readShareImage(img!.path, media), { skip: OUTSIDE });
    fs.rmSync(other, { recursive: true });
  }
});
