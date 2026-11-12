// Every place the GPUI app asks GPUI to focus something hands AppKit's
// keyboard to GPUI's view with it (crate::web_panels::keyboard_to_app). GPUI's
// view never takes the keyboard back from a web panel's page by itself, so a
// request without it left letters going to the page, or beeping, while the
// caret sat in the box asked for (12 Nov: a picture dropped on the composer
// while a page had the keyboard). Here rather than in the crate's own tests,
// as ./check.sh --quick runs these on every landing and not those.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const src = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../desktop/gpui/src');
const request = /window\.focus\(|\.focus\(window, cx\)/;
// A drawing's fallback focus, not something asked for: said so just above it.
const leaves = "Leaves AppKit's keyboard where it is";

function rustFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? rustFiles(path.join(dir, e.name)) : e.name.endsWith('.rs') ? [path.join(dir, e.name)] : []));
}

test("every focus request in the GPUI app takes AppKit's keyboard with it", () => {
  const missing: string[] = [];
  let requests = 0;
  for (const file of rustFiles(src)) {
    const lines = fs.readFileSync(file, 'utf8').split('\n');
    lines.forEach((line, i) => {
      if (!request.test(line) || line.trim().startsWith('//')) return;
      requests += 1;
      const after = lines.slice(i, i + 3).join('\n');
      const before = lines.slice(Math.max(0, i - 4), i).join('\n');
      if (!after.includes('keyboard_to_app(window)') && !before.includes(leaves)) missing.push(`${path.relative(src, file)}:${i + 1}: ${line.trim()}`);
    });
  }
  assert.ok(requests > 20, `found only ${requests} focus requests: has the pattern stopped matching?`);
  assert.deepEqual(missing, [], 'add crate::web_panels::keyboard_to_app(window) after each of these');
});
