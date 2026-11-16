// node --test web/src/picture-src.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { imageSourcePolicy, pictureSrc, withImagePolicy } from './picture-src.ts';

// Daybreak finding 133: a web address written as a picture is never an
// <img> the browser fetches by itself, public, loopback, private or
// carrying the transcript's words in its query.
test('a picture that is not the thread’s own is a link, never loaded', () => {
  const stored = new Map([['/Users/m/a.png', 'https://earnest-puffin-640.ap-southeast-2.convex.cloud/api/storage/abc']]);
  for (const src of [
    'https://attacker.example/pixel.png',
    'http://127.0.0.1:8080/admin/reboot',
    'https://192.168.1.1/x.png',
    'http://[::1]/x.png',
    'https://attacker.example/p.png?secret=the%20reply%20text',
  ]) {
    assert.deepEqual(pictureSrc(src, stored), { kind: 'link', url: src }, src);
  }
  assert.deepEqual(pictureSrc('/Users/m/a.png', stored), { kind: 'stored', url: stored.get('/Users/m/a.png') });
  assert.deepEqual(pictureSrc('/Users/m/b.png', stored), { kind: 'none' });
  assert.deepEqual(pictureSrc('javascript:alert(1)', stored), { kind: 'none' });
  assert.deepEqual(pictureSrc('https://', stored), { kind: 'none' });
});

test('the page only loads pictures from itself and its own Convex deployment', () => {
  assert.equal(
    imageSourcePolicy('https://earnest-puffin-640.ap-southeast-2.convex.cloud', undefined),
    "img-src 'self' data: blob: https://earnest-puffin-640.ap-southeast-2.convex.cloud https://earnest-puffin-640.ap-southeast-2.convex.site",
  );
  assert.equal(imageSourcePolicy('https://convex.example.com', 'https://site.example.com/'), "img-src 'self' data: blob: https://convex.example.com https://site.example.com");
  assert.equal(imageSourcePolicy(undefined, 'not a url'), "img-src 'self' data: blob:");
});

test('the built page carries the policy in its head, ahead of every picture', () => {
  const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
  const out = withImagePolicy(html, imageSourcePolicy('https://a-b-1.convex.cloud'));
  const meta = out.indexOf('<meta http-equiv="Content-Security-Policy" content="img-src \'self\' data: blob: https://a-b-1.convex.cloud https://a-b-1.convex.site" />');
  assert.ok(meta > 0 && meta < out.indexOf('</head>') && meta < out.indexOf('<link rel="icon"'));
  assert.throws(() => withImagePolicy('<html><head></head><body><meta name="referrer"></body></html>', 'img-src none'));
});
