#!/usr/bin/env node
// Where the main thread's time went in a macOS `sample` profile of the GPUI
// app: the share in building elements (render), layout, prepaint, paint and
// text shaping, and the functions that spent the most themselves. For
// features/tools/render-perf.ts, which takes the profile during a scroll.
//
//   node features/tools/sample-split.mjs <sample.txt> [top=25]
import fs from 'node:fs';

const file = process.argv[2];
const top = Number(process.argv[3] ?? 25);
const lines = fs.readFileSync(file, 'utf8').split('\n');

// The call graph's main thread: from its heading to the next thread's.
// `Thread_123: main` in a Rust app's profile, the main dispatch queue in others.
let start = lines.findIndex((l) => /^\s*\d+\s+Thread_\d+:\s+main\s*$/.test(l));
if (start < 0) start = lines.findIndex((l) => /^\s*\d+\s+Thread_\d+\s+DispatchQueue_1:\s+com\.apple\.main-thread/.test(l));
if (start < 0) {
  console.log('no main thread in the profile');
  process.exit(0);
}
const nodes = [];
for (let i = start + 1; i < lines.length; i += 1) {
  const l = lines[i];
  if (!l.trim() || /^\s*\d+\s+Thread_/.test(l) || /^Total number in stack/.test(l) || /^Binary Images/.test(l)) break;
  const m = /^([\s+!:|]*)(\d+)\s+(.*)$/.exec(l);
  if (!m) continue;
  nodes.push({ depth: m[1].length, count: Number(m[2]), name: m[3].replace(/\s+\(in [^)]*\).*$/, '').replace(/\s+\+ \d+.*$/, '') });
}
const total = Number(/^\s*(\d+)/.exec(lines[start])?.[1] ?? 0);

// Rust's v0 mangled names put each part's length before it: `6Window4draw`
// is Window::draw, `6Render6render` a view's render.
const categories = [
  ['drawing a frame', /6Window4draw(?![a-z_])/],
  ['render (building elements)', /6Render6render(?![a-z_])|10RenderOnce6render(?![a-z_])/],
  ['layout (taffy)', /(?<![0-9])14request_layout(?![a-z_])|17TaffyLayoutEngine|(?<![0-9])14compute_layout/],
  ['prepaint', /(?<![0-9])8prepaint(?![a-z_])/],
  ['paint', /(?<![0-9])5paint(?![a-z_])/],
  ['text shaping', /11layout_line|10shape_line|12line_wrapper|(?<![0-9])10shape_text|CTLine|CTTypesetter|CTFont/],
  ['markdown', /markdown|pulldown/i],
  ['waiting for events', /mach_msg2_trap/],
];

// Inclusive: a node counts for a category when no node above it already did.
const inclusive = new Map(categories.map(([name]) => [name, 0]));
const self = new Map();
const stack = [];
for (let i = 0; i < nodes.length; i += 1) {
  const n = nodes[i];
  while (stack.length && stack.at(-1).depth >= n.depth) stack.pop();
  for (const [name, re] of categories) {
    if (re.test(n.name) && !stack.some((s) => re.test(s.name))) inclusive.set(name, inclusive.get(name) + n.count);
  }
  const children = [];
  for (let j = i + 1; j < nodes.length && nodes[j].depth > n.depth; j += 1) {
    if (!children.length || nodes[j].depth <= children[0]) {
      if (!children.length) children.push(nodes[j].depth);
      if (nodes[j].depth === children[0]) children.push(nodes[j].count);
    }
  }
  const own = n.count - children.slice(1).reduce((a, b) => a + b, 0);
  self.set(n.name, (self.get(n.name) ?? 0) + own);
  stack.push(n);
}

const pct = (n) => `${((100 * n) / Math.max(1, total)).toFixed(1)}%`;
console.log(`main thread: ${total} samples`);
for (const [name] of categories) console.log(`  ${name.padEnd(28)} ${pct(inclusive.get(name)).padStart(6)}`);
console.log(`\n  spent the most in themselves:`);
for (const [name, n] of [...self].sort((a, b) => b[1] - a[1]).slice(0, top)) console.log(`  ${pct(n).padStart(6)}  ${name.slice(0, 150)}`);
