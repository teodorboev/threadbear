// Mac-native document and icon proof. Run only on a check host's desktop,
// through remote.sh --desk, after building the automation app. This brings
// its own window forward and captures only that window, including AppKit.
// TB_REAL_SCREEN=1 node features/tools/pane-polish-shot.ts <output folder>
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { pane } from '../support/pane.ts';
import { sampleOffice } from '../support/office-document.ts';

if (process.platform !== 'darwin' || process.env.TB_REAL_SCREEN !== '1') throw new Error('Run on a check host desktop with TB_REAL_SCREEN=1');
const out = path.resolve(process.argv[2] ?? 'reports/pane-polish');
fs.mkdirSync(out, { recursive: true });
const fixture = path.join(out, 'fixture');
fs.mkdirSync(fixture, { recursive: true });
const document = sampleOffice(fixture);
// A small vector page and a low-resolution red / high-resolution blue icns
// catch the sizing and representation bugs as well as fallback symbols.
const icons = path.resolve('desktop/gpui/testdata/icons');
fs.copyFileSync(path.join(icons, 'Multi.icns'), path.join(fixture, 'Blue.icns'));
fs.copyFileSync(path.join(icons, 'Small.pdf'), path.join(fixture, 'Red.pdf'));
const now = new Date().toISOString();
fs.writeFileSync(path.join(fixture, 'thread.json'), JSON.stringify({
  id: 'pane-preview-example', title: 'Document previews and patch icons', cwd: fixture,
  accountId: 'claude-main', provider: 'claude', permissionMode: 'full', status: 'idle', live: false,
  needsYou: false, createdAt: now, updatedAt: now, lastActivityAt: now, preview: '',
  panels: [
    ['app', 'Calculator application icon', '/System/Applications/Calculator.app'],
    ['icns', 'Blue icns icon', path.join(fixture, 'Blue.icns')],
    ['pdf', 'Red PDF icon', path.join(fixture, 'Red.pdf')],
  ].map(([id, title, iconFile]) => ({ id: `example.${id}`, title, iconFile, icon: 'square.dashed', kind: 'list', patch: 'example' })),
}));
const events = [
  { type: 'user.message', text: 'Show the launch brief beside this conversation.', images: [] },
  { type: 'turn.started' },
  { type: 'assistant.text', itemId: 'preview-message', text: 'Read the brief in the side pane. The rail has an application icon, an icns picture and a PDF picture.' },
  { type: 'turn.completed' },
].map((event, i) => ({ seq: i + 1, ts: now, threadId: 'pane-preview-example', turnId: 'preview-turn', ...event }));
fs.writeFileSync(path.join(fixture, 'events.ndjson'), events.map((e) => JSON.stringify(e)).join('\n') + '\n');
process.env.TB_MOCK_FIXTURES = fixture;
const app = new GpuiDriver();
try {
  await app.start();
  let ready = false;
  for (let i = 0; i < 200; i++) {
    const dump = await app.dump();
    if (dump.sidebar.sections.some((s) => s.threads.some((t) => t.id === 'pane-preview-example'))) { ready = true; break; }
    await sleep(50);
  }
  if (!ready) {
    fs.writeFileSync(path.join(out, 'startup.json'), JSON.stringify(await app.dump(), null, 2));
    throw new Error('The preview fixture did not arrive');
  }
  await app.do({ cmd: 'appearance', mode: 'dark' });
  await app.do({ cmd: 'open', id: 'pane-preview-example' });
  await app.do({ cmd: 'waitFor', loaded: true });
  await app.do({ cmd: 'preview', path: document });
  let shown = false;
  for (let i = 0; i < 300; i++) {
    const preview = pane(await app.dump());
    if (preview?.previewKind === 'quickLook' && preview.previewText) { shown = true; break; }
    await sleep(100);
  }
  if (!shown) throw new Error('Quick Look did not draw the Office document');
  await app.do({ cmd: 'front' });
  await sleep(800);
  const geometry = await app.do({ cmd: 'window' });
  const number = /^ok ([0-9]+) /.exec(geometry)?.[1];
  if (!number) throw new Error(`No native window id: ${geometry}`);
  const shot = async (name: string) => {
    await sleep(500);
    execFileSync('screencapture', ['-x', '-o', `-l${number}`, path.join(out, name + '.png')]);
    fs.writeFileSync(path.join(out, name + '.json'), JSON.stringify(await app.dump(), null, 2));
  };
  await shot('office-and-icons');
  await app.do({ cmd: 'mode', mode: 'librarian' });
  const homeGeometry = await app.do({ cmd: 'window' });
  const size = / ([0-9]+)x([0-9]+)$/.exec(String(homeGeometry));
  if (!size) throw new Error(`No window size: ${homeGeometry}`);
  await app.do({ cmd: 'point', x: Number(size[1]) / 2, y: Number(size[2]) / 2, scrollY: -100 });
  await app.dump();
  await shot('librarian');
  await app.do({ cmd: 'usage', show: true });
  for (let i = 0; i < 200; i++) {
    if ((await app.dump() as unknown as { usage?: { chart?: { title?: string } } }).usage?.chart?.title) break;
    await sleep(50);
  }
  await app.do({ cmd: 'usage', scrollToSettings: true });
  await shot('usage');
  console.log(out);
} finally {
  await app.stop();
}
