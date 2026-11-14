#!/usr/bin/env node
// The GPUI app's first-run tour, end to end, against a real runner: a dev
// runner on a throwaway data folder (runner/scripts/dev-runner.sh) and a
// scripted GPUI app on it (desktop/scripts/dev-app.sh --home). The same plays
// as the Swift app's app/Tools/tour-e2e.mjs: the mock runner fakes patches,
// samples and trust, so tours that passed on it were broken on a real install
// (Roll a die waiting as "1 patch to trust" on Tinker Desk, 30 Oct). This
// plays every step as someone would, checks what each card lights up and what
// the step changed, and saves a dark shot of each.
//
//   node desktop/scripts/tour-e2e.mjs [--shots DIR] [--keep] [--no-build]
//
// It sends the tour's first thread for real (the dev runner's cheap model, in
// its temp folder), so it needs a Claude login on this Mac; a login that's out
// of room only changes what the last card says. Exits 1 on any failed check.
// Never touches the live runner or the installed app. The app's build goes to
// an updateBuildHosts Mac, as dev-app.sh's always does.
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const desktop = path.resolve(import.meta.dirname, '..');
const repo = path.resolve(desktop, '..');
const devApp = path.join(desktop, 'scripts', 'dev-app.sh');
const args = process.argv.slice(2);
const shots = args.includes('--shots') ? path.resolve(args[args.indexOf('--shots') + 1]) : fs.mkdtempSync(path.join(os.tmpdir(), 'gpui-tour-e2e-'));
const keep = args.includes('--keep');
const noBuild = args.includes('--no-build');
const id = `gpui-tour-e2e-${process.pid}`;
fs.mkdirSync(shots, { recursive: true });

const failures = [];
function check(name, ok, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}${ok || !detail ? '' : `: ${detail}`}`);
  if (!ok) failures.push(name);
  return ok;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// One automation command; its result, without the command's name. An error
// comes back as the line all the same.
function run(command) {
  let out;
  try {
    out = execFileSync('bash', [devApp, 'do', '--id', id, JSON.stringify(command)], { encoding: 'utf8', timeout: 120_000 });
  } catch (error) {
    out = String(error.stdout ?? '') + String(error.stderr ?? '');
  }
  return out.trim().split('\n').pop().replace(/^\S+ /, '');
}
function dump() {
  try {
    return JSON.parse(execFileSync('bash', [devApp, 'dump', '--id', id, '--json'], { encoding: 'utf8', timeout: 60_000 }));
  } catch {
    return {};
  }
}
const tour = () => run({ cmd: 'tour' });
let shotNo = 0;
function shot(name) {
  const file = path.join(shots, `${String(++shotNo).padStart(2, '0')}-${name.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.png`);
  try {
    execFileSync('bash', [devApp, 'shot', '--id', id, file], { stdio: 'ignore', timeout: 60_000 });
  } catch {
    // A shot that fails doesn't fail the tour.
  }
}

// What `tour` says: "ok 3/10 Star or put away lit: 52,264 313x51".
function parseTour(line) {
  const m = /^ok (\d+)\/(\d+) (.+?)(?: \((done|to do)\))? lit: (nothing|(-?\d+),(-?\d+) (\d+)x(\d+))$/.exec(line);
  if (!m) return { off: line === 'ok off', raw: line };
  return { step: +m[1], count: +m[2], title: m[3], task: m[4], lit: m[5] === 'nothing' ? null : { x: +m[6], y: +m[7], w: +m[8], h: +m[9] }, raw: line };
}

async function until(what, test, seconds = 20) {
  const end = Date.now() + seconds * 1000;
  for (;;) {
    const value = test();
    if (value) return value;
    if (Date.now() > end) return check(what, false, 'timed out');
    await sleep(500);
  }
}

const sampleTitles = ['Fix the flaky login test', 'Plan the weekend', 'Draft the release notes'];
const threadsIn = (d) => (d.sidebar?.sections ?? []).flatMap((s) => s.threads ?? []);
const sampleId = (d, title) => threadsIn(d).find((t) => t.title === title)?.id;

// The dev runner, in the background, on a temp data folder.
const runnerLog = path.join(shots, 'dev-runner.out');
const runner = spawn('bash', [path.join(repo, 'runner', 'scripts', 'dev-runner.sh')], { detached: true, stdio: ['ignore', fs.openSync(runnerLog, 'w'), fs.openSync(runnerLog, 'a')] });
runner.unref();
let appStarted = false;

async function main() {
  const home = await until('dev runner started', () => /^ {2}data: (.+?)(?: \(deleted|$)/m.exec(fs.readFileSync(runnerLog, 'utf8'))?.[1], 120);
  if (!home) return;
  await until('dev runner listening', () => fs.existsSync(path.join(home, 'runner.json')) && fs.existsSync(path.join(home, 'secrets', 'runner-token')), 60);
  execFileSync('bash', [devApp, 'start', '--home', home, '--id', id, '--appearance', 'dark', ...(noBuild ? ['--no-build'] : [])], { stdio: 'inherit', timeout: 1_800_000 });
  appStarted = true;
  await until('app connected', () => dump().connection === 'connected', 30);
  check('a scripted app never starts the tour by itself', parseTour(tour()).off, tour());
  run({ cmd: 'new' });

  // The first-run tour (no Skip), as a new Mac gets it.
  let t = parseTour(run({ cmd: 'tour', do: 'start' }));
  check('starts on Welcome', t.title === 'Welcome to Threadbear', t.raw);
  check('all ten steps with the window as it starts', t.count === 10, t.raw);
  check('the first run has no Skip', dump().tour?.skippable === false);
  shot('welcome');
  const d0 = await until('samples made', () => {
    const d = dump();
    return sampleTitles.every((title) => sampleId(d, title)) && d;
  });
  if (!d0) return;

  const next = (expect, lit = true) => {
    const s = parseTour(run({ cmd: 'tour', do: 'next' }));
    check(`step ${expect}`, s.title === expect, s.raw);
    if (lit) check(`${expect} lights something up`, s.lit !== null, s.raw);
    shot(expect);
    return s;
  };

  next('Threads');
  // Shown, not pressed: the sample's row drawn hovered, its buttons out.
  t = next('Star or put away');
  check("it lights the row's star, moon and box, not the whole row", t.lit && t.lit.h < 80 && t.lit.w < 160, t.raw);
  const hovered = threadsIn(dump()).find((th) => th.hovered);
  check('that row shows its hover buttons', hovered?.title === 'Plan the weekend', JSON.stringify(hovered?.title));

  t = next('Your views');
  // The view icons only, not the whole column down to Settings.
  check('Your views lights the view icons, not the whole bar', t.lit && t.lit.h < 300, t.raw);
  t = next('Highlights');
  check('Highlights opens the sample that needs you', dump().selection?.title === 'Fix the flaky login test', JSON.stringify(dump().selection));

  // Told, not done (Mike, 2 Nov): no drag, no press.
  t = next('Move a panel');
  check('Move a panel has nothing to do', t.task === undefined, t.raw);

  next('Patches');
  // The example patch, from the runner itself: trusted, so its button shows
  // rather than "1 patch to trust".
  await until('Roll a die is a button on the sample', () => (dump().actions ?? []).find((a) => a.title === 'Roll a die'));
  t = next('Per project or tool');
  check('Per project or tool has nothing to press', t.task === undefined, t.raw);

  t = next('Your first thread');
  check('Your first thread waits for the send', t.task === 'to do', t.raw);
  check('the first message is written for them', /Make me a patch for this folder/.test(dump().composer?.text ?? ''), dump().composer?.text);
  run({ cmd: 'return' });
  t = await until('sending moves on to Finish setting up', () => {
    const s = parseTour(tour());
    return s.title === 'Finish setting up' && s;
  }, 40);
  // Its panel glides in under the threads first.
  t = (await until('Finish setting up lights its panel once it has glided in', () => {
    const s = parseTour(tour());
    return s.lit && s;
  })) || t;
  shot('finish-setting-up');
  check('Finish setting up lights its panel', t && t.lit !== null, t?.raw);
  check('and its panel is open', dump().setup?.shown === true, JSON.stringify(dump().setup));

  run({ cmd: 'tour', do: 'next' });
  check('Done ends the tour', parseTour(tour()).off);
  await sleep(1500);
  const after = dump();
  const left = threadsIn(after).filter((th) => sampleTitles.includes(th.title)).map((th) => th.title);
  check('the samples are gone from the inbox', left.length === 0, left.join(', '));
  check('no row left drawn hovered', !threadsIn(after).some((th) => th.hovered));
  check('the first thread is still there', threadsIn(after).length > 0, '');
  shot('after');

  // Pages explain themselves the first time they open, not in the tour
  // (Mike, 2 Nov). A scripted app has them off; this turns them on.
  run({ cmd: 'tour', intros: true });
  for (const [mode, title] of [['projects', 'Projects'], ['librarian', 'The librarian'], ['scheduled', 'Scheduled']]) {
    run({ cmd: 'mode', mode });
    const intro = await until(`${title} explains itself on first open`, () => {
      const s = parseTour(tour());
      return s.title === title && s;
    });
    check(`${title}'s intro is one card that points at its page`, intro && intro.count === 1 && intro.lit !== null, intro?.raw);
    shot(`intro-${mode}`);
    run({ cmd: 'tour', do: 'next' });
  }
  run({ cmd: 'usage', show: true });
  const usage = await until('Usage explains itself on first open', () => {
    const s = parseTour(tour());
    return s.title === 'Usage' && s;
  });
  check("Usage's intro points at its panel", usage && usage.lit !== null, usage?.raw);
  shot('intro-usage');
  run({ cmd: 'tour', do: 'next' });
  run({ cmd: 'usage', show: false });
  run({ cmd: 'mode', mode: 'inbox' });
  run({ cmd: 'mode', mode: 'projects' });
  await sleep(1000);
  check('an intro only shows the first time', parseTour(tour()).off, tour());
  run({ cmd: 'mode', mode: 'inbox' });

  // Again, from Help: fresh samples, and the patch still trusted.
  t = parseTour(run({ cmd: 'tour', do: 'start', skippable: true }));
  check('Help runs it again', t.title === 'Welcome to Threadbear', t.raw);
  check('the replay can be skipped', dump().tour?.skippable === true);
  await until('fresh samples the second time', () => sampleTitles.every((title) => sampleId(dump(), title)));
  run({ cmd: 'select', id: sampleId(dump(), 'Fix the flaky login test') });
  await until('Roll a die is there the second time', () => (dump().actions ?? []).find((a) => a.title === 'Roll a die'));
  // The replay has Skip as well: the buttons still fit (look at the shot).
  for (let i = 0; i < 4; i++) run({ cmd: 'tour', do: 'next' });
  shot('replay-with-skip');
  run({ cmd: 'tour', do: 'end' });
  await sleep(1500);
  check('ending it removes the samples', !threadsIn(dump()).some((th) => sampleTitles.includes(th.title)));
}

try {
  await main();
} catch (error) {
  check('the run itself', false, error.message.split('\n')[0]);
} finally {
  if (appStarted) {
    try {
      execFileSync('bash', [devApp, 'stop', '--id', id], { stdio: 'ignore', timeout: 60_000 });
    } catch {}
  }
  // The dev runner's own pid, as it printed it (never pkill: the live runner
  // runs the same file).
  const pid = /^ {2}pid: {2}(\d+)/m.exec(fs.readFileSync(runnerLog, 'utf8'))?.[1];
  if (pid && !keep) {
    try {
      process.kill(+pid);
    } catch {}
  }
  console.log(`\n${failures.length ? `${failures.length} failed` : 'all passed'}. Shots in ${shots}`);
  process.exitCode = failures.length ? 1 : 0;
}
