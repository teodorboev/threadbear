// Alternate old/new release binaries, taking the shared locks for the whole run:
// node features/tools/idle-ab.mjs <old> <new> [rounds=5]
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensurePerfLock } from './perf-lock.ts';

await ensurePerfLock();

const binaries = process.argv.slice(2, 4);
if (binaries.length !== 2) throw new Error('Pass old and new release binaries.');
const digests = await Promise.all(binaries.map(async binary => {
  const hash = createHash('sha256');
  for await (const chunk of fs.createReadStream(binary)) hash.update(chunk);
  return hash.digest('hex');
}));
const rounds = Number(process.argv[4] ?? 5);
if (!Number.isInteger(rounds) || rounds < 1) throw new Error('Rounds must be a positive integer.');
const results = [];
let active;
let cancelled = false;
function cancel(code) {
  cancelled = true;
  process.exitCode = code;
  active?.kill('SIGTERM');
}
for (const [signal, code] of [['SIGINT', 130], ['SIGTERM', 143], ['SIGHUP', 129]]) process.on(signal, () => cancel(code));
process.stdout.on('error', error => { if (error.code === 'EPIPE') cancel(143); else throw error; });

function runBinary(binary, index) {
  const env = { ...process.env, TB_APP: 'gpui', TB_GPUI_BIN: path.resolve(binary) };
  if (process.env.TB_AB_REQUIRE_NEW_LOADER_PHASE === 'on') {
    delete env.TB_IDLE_REQUIRE_LOADER_PHASE;
    if (index === 1) env.TB_IDLE_REQUIRE_LOADER_PHASE = 'on';
  }
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [path.join(path.dirname(fileURLToPath(import.meta.url)), 'idle-perf.ts')], {
      env, stdio: ['ignore', 'pipe', 'pipe'],
    });
    active = child;
    let stdout = '', stderr = '';
    const timer = setTimeout(() => {
      child.kill('SIGTERM');
    }, Number(process.env.TB_AB_TIMEOUT_SECONDS ?? 300) * 1000);
    child.stdout.on('data', data => { stdout += data; });
    child.stderr.on('data', data => { stderr += data; });
    const clear = () => { clearTimeout(timer); active = undefined; };
    child.once('error', error => { clear(); reject(error); });
    child.once('close', status => { clear(); resolve({ status, stdout, stderr }); });
  });
}

try {
  for (let round = process.env.TB_AB_WARMUP === 'off' ? 0 : -1; round < rounds; round++) {
    const order = round >= 0 && round % 2 ? [1, 0] : [0, 1];
    for (const index of order) {
      const binary = binaries[index];
      const label = index ? 'new' : 'old';
      if (cancelled) throw new Error('Cancelled');
      const run = await runBinary(binary, index);
      if (cancelled) throw new Error('Cancelled');
      if (run.status !== 0) throw new Error(`${label} round ${round + 1} failed:\n${run.stdout}\n${run.stderr}`);
      for (const line of run.stdout.trim().split('\n')) {
        const entry = JSON.parse(line);
        console.log(JSON.stringify({ binary: label, binaryPath: path.resolve(binary), binarySha256: digests[index], round: round + 1, warmup: round < 0, ...entry }));
        if (entry.label && round >= 0) results.push({ binary: label, ...entry });
      }
    }
  }
  const median = xs => { xs.sort((a, b) => a - b); const m = Math.floor(xs.length / 2); return xs.length % 2 ? xs[m] : (xs[m - 1] + xs[m]) / 2; };
  for (const label of new Set(results.map(r => r.label))) {
    const summary = {};
    const metrics = { cpuPercent: r => r.cpuPercent, wakeupsPerSecond: r => r.interruptWakeupsPerSecond, commandP95Ms: r => r.commandMs.p95 };
    for (const name of ['mainCpuPercent', 'framesPerSecond', 'mainMsPerFrame']) {
      if (results.filter(r => r.label === label).every(r => typeof r[name] === 'number')) metrics[name] = r => r[name];
    }
    for (const [name, read] of Object.entries(metrics)) {
      const samples = ['old', 'new'].map(binary => results.filter(r => r.label === label && r.binary === binary).map(read));
      const [old, now] = samples.map(xs => median([...xs]));
      summary[name] = { old, new: now, ratio: old ? now / old : null, oldRange: [Math.min(...samples[0]), Math.max(...samples[0])], newRange: [Math.min(...samples[1]), Math.max(...samples[1])] };
    }
    console.log(JSON.stringify({ summary: label, ...summary }));
  }
} catch (error) {
  if (!cancelled) throw error;
}
