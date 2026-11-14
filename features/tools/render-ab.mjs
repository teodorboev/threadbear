#!/usr/bin/env node
// Builds of the GPUI app compared on features/tools/render-perf.ts, taken in
// turns (A, B, C, A, B, C...) so a check host's other work falls on each
// alike, with the medians of each and each one's as a share of A's. Run it
// on a desktop, as render-perf.ts says:
//
//   ROUNDS=3 node features/tools/render-ab.mjs <binary A> <binary B> [more...] -- [render-perf.ts args]
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensurePerfLock } from './perf-lock.ts';

// Held for every run; the runs it starts inherit it (perf-lock.ts).
await ensurePerfLock();

const here = path.dirname(fileURLToPath(import.meta.url));
const dash = process.argv.indexOf('--');
const bins = process.argv.slice(2, dash < 0 ? undefined : dash);
const rounds = process.env.ROUNDS ?? '3';
const extra = dash < 0 ? [] : process.argv.slice(dash + 1);
const names = bins.map((_, i) => String.fromCharCode(65 + i));
const runs = Object.fromEntries(names.map((n) => [n, []]));
for (let r = 0; r < Number(rounds); r += 1) {
  for (const [name, bin] of names.map((n, i) => [n, bins[i]])) {
    let out = '';
    try {
      out = execFileSync(process.execPath, [path.join(here, 'render-perf.ts'), ...extra], { env: { ...process.env, TB_GPUI_BIN: bin }, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
    } catch (e) {
      out = String(e.stdout ?? '');
    }
    process.stdout.write(`\n[${name} round ${r + 1}]\n${out.split('\n').filter((l) => !l.startsWith('RESULTS')).join('\n')}`);
    const line = out.split('\n').find((l) => l.startsWith('RESULTS '));
    if (line) runs[name].push(...JSON.parse(line.slice(8)));
  }
}
const median = (xs) => {
  const s = [...xs].sort((p, q) => p - q);
  return s.length ? (s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2) : NaN;
};
const keys = [...new Set(Object.values(runs).flat().map((r) => `${r.scenario}${r.load ? ' loaded' : ''}`))];
const fields = ['late%', 'p50', 'p90', 'p99', 'cpuPerFrame', 'cpuPerSecond'];
const pick = (rows, f) => rows.map((r) => (f === 'late%' ? (100 * r.late) / Math.max(1, r.frames) : r[f])).filter((v) => v !== undefined);
console.log(`\nMedians over ${rounds} rounds each (${names.map((n, i) => `${n}: ${bins[i]}`).join(', ')}); each one's share of A's in brackets`);
for (const k of keys) {
  const of = (name) => runs[name].filter((r) => `${r.scenario}${r.load ? ' loaded' : ''}` === k);
  console.log(`  ${k}`);
  for (const f of fields) {
    const a = median(pick(of('A'), f));
    const cells = names.map((n) => {
      const v = median(pick(of(n), f));
      return n === 'A' ? `${v.toFixed(2)}` : `${v.toFixed(2)} (${(v / a).toFixed(2)})`;
    });
    console.log(`    ${f.padEnd(12)} ${cells.join('  ')}`);
  }
}
console.log(`\nAB ${JSON.stringify(runs)}`);
