// Model Cargo's mtime freshness without running a compiler: B -> A -> unchanged
// B must rebuild B, including when A failed after replacing shared artifacts.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';

const wrapper = path.resolve(import.meta.dirname, '../../../scripts/perf-build.sh');
function setup(t: TestContext) {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-perf-build-'));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  const target = path.join(work, 'perf-target');
  const artifact = path.join(target, 'binary');
  const marker = path.join(target, '.perf-built-by');
  const fake = path.join(work, 'cargo.cjs');
  fs.writeFileSync(fake, `const fs = require('node:fs'); const [src, out] = process.argv.slice(2); if (!fs.existsSync(out) || fs.statSync(src).mtimeMs > fs.statSync(out).mtimeMs) fs.writeFileSync(out, fs.readFileSync(src)); process.exit(Number(process.env.FAKE_FAIL ?? 0));`);
  function source(name: string) {
    const root = path.join(work, name);
    fs.mkdirSync(path.join(root, 'desktop'), { recursive: true });
    fs.writeFileSync(path.join(root, 'desktop/value.rs'), name);
    fs.utimesSync(path.join(root, 'desktop/value.rs'), 1, 1);
    return root;
  }
  const a = source('A');
  const b = source('B');
  function build(root: string, extra: NodeJS.ProcessEnv = {}, prepare = false) {
    return spawnSync('/bin/bash', [wrapper, ...(prepare ? ['--prepare'] : [process.execPath, fake, path.join(root, 'desktop/value.rs'), artifact])], {
      env: { ...process.env, THREADBEAR_PERF_BUILD_KEY: path.basename(root), THREADBEAR_PERF_BUILD_SOURCE: root, CARGO_TARGET_DIR: target, ...extra }, encoding: 'utf8',
    });
  }
  return { a, b, target, artifact, marker, fake, build };
}

test('shared target ownership forces B after A even when B did not change', (t) => {
  const h = setup(t);
  for (const source of [h.b, h.a, h.b]) {
    const result = h.build(source);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(fs.readFileSync(h.artifact, 'utf8'), path.basename(source));
    assert.equal(fs.readFileSync(h.marker, 'utf8').trim(), path.basename(source));
  }
});

test('a failed foreign build invalidates the previous owner before touching artifacts', (t) => {
  const h = setup(t);
  assert.equal(h.build(h.b).status, 0);
  assert.equal(h.build(h.a, { FAKE_FAIL: '7' }).status, 7);
  assert.equal(fs.readFileSync(h.artifact, 'utf8'), 'A');
  assert.ok(!fs.existsSync(h.marker));
  assert.equal(h.build(h.b).status, 0);
  assert.equal(fs.readFileSync(h.artifact, 'utf8'), 'B');
});

test('the locked prelude also protects absolute Cargo calls bypassing PATH', (t) => {
  const h = setup(t);
  assert.equal(h.build(h.a).status, 0);
  const prepared = h.build(h.b, {}, true);
  assert.equal(prepared.status, 0, prepared.stderr);
  assert.ok(!fs.existsSync(h.marker));
  const cargo = spawnSync(process.execPath, [h.fake, path.join(h.b, 'desktop/value.rs'), h.artifact]);
  assert.equal(cargo.status, 0);
  assert.equal(fs.readFileSync(h.artifact, 'utf8'), 'B');
});

test('prepare followed by PATH Cargo preserves an unchanged owner warm build', (t) => {
  const h = setup(t);
  assert.equal(h.build(h.b).status, 0);
  const source = path.join(h.b, 'desktop/value.rs');
  const before = { source: fs.statSync(source).mtimeMs, artifact: fs.statSync(h.artifact).mtimeMs };
  assert.equal(h.build(h.b, {}, true).status, 0);
  assert.equal(h.build(h.b).status, 0);
  assert.equal(fs.statSync(source).mtimeMs, before.source, 'prelude must not force unchanged workspace inputs dirty');
  assert.equal(fs.statSync(h.artifact).mtimeMs, before.artifact, 'Cargo keeps its warm artifact');
});
