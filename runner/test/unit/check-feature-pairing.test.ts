// check.sh --quick (what tb land runs) runs features/verify-pairing.mjs when
// a spec, its steps or features.md changed. On 13 Nov a board.feature with
// its id tag on the Feature line, not on each scenario, passed tb land and
// then failed the Desktop workflow on all three systems at that step
// (actions run 43435160875), holding up every desktop landing after it.
//
// This runs the real check.sh --quick --local in a throwaway checkout whose
// other steps are stand-ins that pass at once, so only what it picks to run
// is under test.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

function checkout(feature: string) {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-check-features-'));
  const put = (rel: string, text: string) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  const copy = (rel: string) => put(rel, fs.readFileSync(path.join(repo, rel), 'utf8'));
  copy('check.sh');
  fs.chmodSync(path.join(root, 'check.sh'), 0o755);
  copy('runner/scripts/find-node.sh');
  copy('runner/test/helpers/failed-files-reporter.mjs');
  copy('features/verify-pairing.mjs');
  // Every other step check.sh --quick runs, passing.
  put('runner/package.json', JSON.stringify({ scripts: { typecheck: 'true' } }));
  put('runner/node_modules/.keep', '');
  put('runner/test/helpers/clean-env.ts', '');
  put('runner/test/unit/ok.test.ts', "import { test } from 'node:test';\ntest('ok', () => {});\n");
  put('package.json', JSON.stringify({ scripts: { 'convex:typecheck': 'true', 'web:typecheck': 'true', 'web:test': 'true' } }));
  put('node_modules/.keep', '');
  put('app/Tools/ok.mjs', '');
  put('app/Tools/mock-runner/ok.mjs', '');
  put('docs/gpui-port/features.md', '| `highlights.board` | The board |\n');
  put('features/board.feature', feature);
  put('features/steps/board.steps.ts', '');
  return root;
}

function quick(root: string, changed: string[]) {
  const list = path.join(root, '.changed');
  fs.writeFileSync(list, changed.join('\n') + '\n');
  const env: NodeJS.ProcessEnv = { ...process.env, THREADBEAR_CHECK_CHANGED_FILE: list, THREADBEAR_NODE: process.execPath, TMPDIR: root };
  for (const k of Object.keys(env)) if (/^THREAD(BEAR|BARE)_CHECK_HOST$/.test(k)) delete env[k];
  const r = spawnSync('bash', [path.join(root, 'check.sh'), '--quick', '--local'], { env, encoding: 'utf8', timeout: 120_000 });
  return { status: r.status, out: (r.stdout ?? '') + (r.stderr ?? '') };
}

// The tag on the Feature line only, as board.feature had it.
const featureWide = '@highlights.board\nFeature: The board\n\n  Scenario: It shows\n    Given nothing\n';
const perScenario = 'Feature: The board\n\n  @highlights.board\n  Scenario: It shows\n    Given nothing\n';

test('a spec whose scenarios have no id tag fails --quick', () => {
  const root = checkout(featureWide);
  try {
    const r = quick(root, ['features/board.feature']);
    assert.equal(r.status, 1, r.out);
    assert.match(r.out, /^FAIL  feature pairing/m, r.out);
    assert.match(r.out, /board\.feature:4 has no features\.md id tag/, r.out);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('the same spec tagged per scenario passes, and so does each kind of file it reads', () => {
  const root = checkout(perScenario);
  try {
    for (const changed of ['features/board.feature', 'features/steps/board.steps.ts', 'docs/gpui-port/features.md', 'features/verify-pairing.mjs']) {
      const r = quick(root, [changed]);
      assert.equal(r.status, 0, `${changed}: ${r.out}`);
      assert.match(r.out, /^ok    feature pairing/m, `${changed}: ${r.out}`);
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('a change nowhere near the specs skips it', () => {
  const root = checkout(featureWide);
  try {
    const r = quick(root, ['convex/x.ts']);
    assert.equal(r.status, 0, r.out);
    assert.match(r.out, /^skip  feature pairing/m, r.out);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
