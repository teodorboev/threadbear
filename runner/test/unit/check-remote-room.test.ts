// scripts/check-remote.sh against fake hosts (helpers/check-remote-harness.ts
// says how, and why it's several files): hosts short of disk: passing them over, and what the check wants free.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { GiB, linuxDf, macDf, setup } from '../helpers/check-remote-harness.ts';

const changedSomething = (s: ReturnType<typeof setup>) => fs.writeFileSync(path.join(s.repo, 'changed.txt'), 'c\n');

test('a host low on disk is passed over for the next one, nothing copied to it', () => {
  const s = setup({ h1: { state: 'free', df: macDf(0.9 * GiB) }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /Passing over h1: it has 0\.9 GiB free, and this check wants 8 GiB\./);
  assert.match(r.stdout, /ran on h2 now/);
  assert.deepEqual(r.copies, ['h2']);
  assert.doesNotMatch(r.ssh, /^h1 .*mirror/m);
});

test('a host low on disk is passed over even if it is the only one free', () => {
  const s = setup({ h1: { state: 'free', df: macDf(GiB) }, h2: { state: 'busy' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /ran on h2 after waiting/);
  assert.deepEqual(r.copies, ['h2']);
});

test('with no host that has room it fails naming each and its free space, and not with 255', () => {
  const s = setup({ h1: { state: 'free', df: macDf(0.95 * GiB) }, h2: { state: 'busy', df: macDf(5 * GiB) } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /No check host has room for this check: h1 has 0\.9 GiB free, h2 has 5\.0 GiB free, and it wants 8 GiB\./);
  assert.deepEqual(r.copies, []);
});

test('a host that is reachable but low still fails, with another one unreachable', () => {
  const s = setup({ h1: { state: 'free', df: macDf(GiB) } });
  const r = s.run('h1,nope', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /No check host has room for this check: h1 has 1\.0 GiB free/);
});

test("df's lines from each system are read, and the limit is inclusive of exactly enough", () => {
  for (const [df, os] of [[macDf, 'Darwin'], [linuxDf, 'Linux']] as const) {
    const s = setup({ h1: { state: 'free', df: df(2 * GiB), os } });
    changedSomething(s);
    const r = s.run('h1', 3600);
    assert.equal(r.status, 0, `${os}: ${r.stdout}${r.stderr}`);
    assert.deepEqual(r.copies, ['h1'], os);
    assert.doesNotMatch(r.stderr, /Passing over/, os);
  }
  const s = setup({ h1: { state: 'free', df: macDf(8 * GiB) } });
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.deepEqual(r.copies, ['h1']);
});

test('a run that builds no Swift needs less room than one that does', () => {
  const room = macDf(3 * GiB);
  const quick = setup({ h1: { state: 'free', df: room } });
  changedSomething(quick);
  const noSwift = quick.run('h1', 3600);
  assert.equal(noSwift.status, 0, noSwift.stdout + noSwift.stderr);
  assert.deepEqual(noSwift.copies, ['h1']);

  // Touching ios/ runs ThreadbearKit's Swift tests, even with --quick.
  const ios = setup({ h1: { state: 'free', df: room } });
  fs.mkdirSync(path.join(ios.repo, 'ios'));
  fs.writeFileSync(path.join(ios.repo, 'ios', 'x.swift'), '\n');
  const swift = ios.run('h1', 3600);
  assert.equal(swift.status, 1, swift.stdout + swift.stderr);
  assert.match(swift.stderr, /h1 has 3\.0 GiB free, and it wants 8 GiB/);

  // And touching app/ or dock/ builds the Mac app in release, even with --quick.
  for (const dir of ['app', 'dock']) {
    const mac = setup({ h1: { state: 'free', df: room } });
    fs.mkdirSync(path.join(mac.repo, dir));
    fs.writeFileSync(path.join(mac.repo, dir, 'x.swift'), '\n');
    const built = mac.run('h1', 3600);
    assert.equal(built.status, 1, `${dir}: ${built.stdout}${built.stderr}`);
    assert.match(built.stderr, /h1 has 3\.0 GiB free, and it wants 8 GiB/, dir);
  }

  const low = setup({ h1: { state: 'free', df: macDf(1.5 * GiB) } });
  changedSomething(low);
  const r = low.run('h1', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 has 1\.5 GiB free, and it wants 2 GiB/);
});

test('a --quick run that touches the Mac app goes past a Linux host to a Mac', () => {
  // check.sh --quick builds the app in release when app/ or dock/ changed,
  // and swift can't build an AppKit app on Linux.
  for (const dir of ['app', 'dock']) {
    const s = setup({ h1: { state: 'free', os: 'Linux' }, h2: { state: 'free' } });
    fs.mkdirSync(path.join(s.repo, dir));
    fs.writeFileSync(path.join(s.repo, dir, 'x.swift'), '\n');
    const r = s.run('h1,h2', 3600);
    assert.equal(r.status, 0, `${dir}: ${r.stdout}${r.stderr}`);
    assert.deepEqual(r.copies, ['h2'], dir);
  }
  // Anything else still goes to the Linux host first.
  const s = setup({ h1: { state: 'free', os: 'Linux' }, h2: { state: 'free' } });
  changedSomething(s);
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.deepEqual(r.copies, ['h1']);
});

test("a --quick run that only touches what the GPUI app uses in app/ can take a Linux host", () => {
  // The Swift app is frozen, so check.sh doesn't build it for the mock
  // runner, fixtures or art (scripts/swift-freeze.sh --gpui-shared).
  const s = setup({ h1: { state: 'free', os: 'Linux' }, h2: { state: 'free' } });
  fs.mkdirSync(path.join(s.repo, 'app/Tools/mock-runner'), { recursive: true });
  fs.writeFileSync(path.join(s.repo, 'app/Tools/mock-runner/x.mjs'), '\n');
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.deepEqual(r.copies, ['h1']);
});

test('a big change to the Mac app still goes to a Mac', () => {
  // Codex's review, 12 Nov: grep -q closing the pipe early failed the grep -v
  // before it under pipefail, so thousands of changed files looked like none.
  const s = setup({ h1: { state: 'free', os: 'Linux' }, h2: { state: 'free' } });
  fs.mkdirSync(path.join(s.repo, 'app/Sources'), { recursive: true });
  for (let i = 0; i < 5000; i++) fs.writeFileSync(path.join(s.repo, `app/Sources/F${i}.swift`), '');
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.deepEqual(r.copies, ['h2']);
});

test('a Linux host that could not run it anyway is not named as short of room', () => {
  // Nothing changed, so this one needs a Mac; the Linux host's disk isn't the reason.
  const s = setup({ h1: { state: 'free', os: 'Linux', df: linuxDf(GiB) }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.doesNotMatch(r.stderr, /h1/);
  assert.deepEqual(r.copies, ['h2']);
});

// 12 Nov: Tinker Desk had 1.8 GiB free, with 63 GB of it idle GPUI targets
// (desktop/scripts/remote.sh's), and the check gave up on it.
const TRIM = path.join(import.meta.dirname, '..', '..', '..', 'desktop', 'scripts', 'trim-remote-cache.sh');
// A host whose GPUI folder (under `home`) holds `idle` idle targets and
// `busy` ones in use, with 1 GiB free and `per` more for each target gone.
function gpuiHost(s: ReturnType<typeof setup>, h: string, home: string, idle: number, busy = 0, per = 6) {
  const trim = path.join(s.repo, 'desktop', 'scripts', 'trim-remote-cache.sh');
  fs.mkdirSync(path.dirname(trim), { recursive: true });
  fs.copyFileSync(TRIM, trim);
  const gpui = path.join(home, '.cache', 'threadbear-check', 'gpui');
  const old = new Date(Date.now() - 24 * 3_600_000);
  for (let i = 0; i < idle + busy; i++) {
    const t = path.join(gpui, `target-${i}${i}${i}`);
    fs.mkdirSync(path.join(t, 'debug'), { recursive: true });
    if (i < idle) fs.utimesSync(path.join(t, 'debug'), old, old);
    fs.utimesSync(t, old, old);
  }
  const all = idle + busy;
  fs.writeFileSync(path.join(s.fake, h, 'bin', 'df'), `#!/bin/bash
[ "$*" = "-Pk ." ] || exit 9
n=$(ls -d ${JSON.stringify(gpui)}/target-* 2>/dev/null | wc -l)
echo "Filesystem 1024-blocks Used Available Capacity Mounted on"
echo "/dev/disk3s5 1 1 $(( (2 + (${all} - n) * ${per * 2}) * ${GiB / 2} )) 1% /"
`, { mode: 0o755 });
  return { gpui, targets: () => fs.readdirSync(gpui).filter((n) => n.startsWith('target-')) };
}

test('a host short only of what idle GPUI build folders hold gets them back and runs the check', () => {
  const s = setup({ h1: { state: 'free' } });
  const g = gpuiHost(s, 'h1', path.join(s.fake, 'h1', 'home'), 3);
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 was short of disk; idle GPUI build folders there were given back/);
  assert.deepEqual(r.copies, ['h1']);
  assert.equal(g.targets().length, 1, 'only until there was room');
});

test('a host still short after giving back what it can is passed over, with what it has then', () => {
  const s = setup({ h1: { state: 'free' }, h2: { state: 'free' } });
  const g = gpuiHost(s, 'h1', path.join(s.fake, 'h1', 'home'), 1, 1, 0.5);
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /Passing over h1: it has 1\.5 GiB free/);
  assert.deepEqual(r.copies, ['h2']);
  assert.equal(g.targets().length, 1, 'the one in use stays');
});

test('a host too full for the probe gets its build folders back and is probed again', () => {
  const s = setup({ h1: { state: 'free' } });
  // The fake host's ~ is this run's home, as its commands run here.
  const g = gpuiHost(s, 'h1', s.home, 3);
  // The probe fails until the folder holding this file is given back.
  fs.writeFileSync(path.join(g.gpui, 'target-000', 'full'), '');
  fs.symlinkSync(path.join(g.gpui, 'target-000', 'full'), path.join(s.fake, 'h1', 'probe-fails'));
  fs.utimesSync(path.join(g.gpui, 'target-000'), new Date(Date.now() - 48 * 3_600_000), new Date(Date.now() - 48 * 3_600_000));
  fs.utimesSync(path.join(g.gpui, 'target-000', 'full'), new Date(Date.now() - 48 * 3_600_000), new Date(Date.now() - 48 * 3_600_000));
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.deepEqual(r.copies, ['h1']);
});
