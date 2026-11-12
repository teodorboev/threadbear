// The web runtime (features/web-runtime.feature): the GPUI app's Chromium,
// installed on request and checked against its pinned build.
import { Then, When } from '@cucumber/cucumber';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { ThreadbearWorld } from '../support/world.ts';

/** The dump's `webRuntime` (desktop/gpui/src/web_runtime.rs). */
export type WebRuntimeInfo = { state: string; words: string; percent?: number | null; notNow: boolean; engine: string };
export const webRuntime = (d: unknown) => (d as { webRuntime?: WebRuntimeInfo | null }).webRuntime;

/** Unpacking 330 MB takes a while on a busy Tinker Desk. */
const INSTALL_SECONDS = 600;

// The step's own time, as the default 90 seconds is less than unpacking
// takes on a loaded Tinker Desk (it ran out there on 11 Nov).
When('I install the web runtime', { timeout: (INSTALL_SECONDS + 60) * 1000 }, async function (this: ThreadbearWorld) {
  // The pinned build downloaded once (support/world.ts skips this scenario
  // without one), handed over as a download would be, and checked the same.
  await this.app.do({ cmd: 'webRuntime', do: 'install', archive: process.env.TB_WEB_RUNTIME_ARCHIVE, wait: INSTALL_SECONDS }, INSTALL_SECONDS + 30);
});

When("I install the web runtime from a file that isn't the pinned build", async function (this: ThreadbearWorld) {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-not-the-build-')), 'cef_binary_minimal.tar.bz2');
  fs.writeFileSync(file, 'Not Chromium, whatever the name says.');
  await this.app.do({ cmd: 'webRuntime', do: 'install', archive: file, wait: 60 }, 90);
});

Then("the web runtime says it's installed", async function (this: ThreadbearWorld) {
  await this.until('the web runtime installed', (d) => webRuntime(d)?.state === 'ready', 30_000);
});

Then("the web runtime isn't installed", async function (this: ThreadbearWorld) {
  await this.until('the web runtime not installed', (d) => ['missing', 'failed'].includes(webRuntime(d)?.state ?? ''));
});

Then("the web runtime says the download didn't match the pinned build", async function (this: ThreadbearWorld) {
  await this.until('a refused download', (d) => webRuntime(d)?.state === 'failed' && /didn't match the pinned build/.test(webRuntime(d)?.words ?? ''));
});

Then('the web runtime draws a page with no window', async function (this: ThreadbearWorld) {
  const out = JSON.parse(await this.app.do({ cmd: 'webRuntime', do: 'smoke', wait: 90 }, 120)) as { ok: boolean; width: number; height: number };
  // 400 by 120 CSS pixels, at twice that on a Retina screen's scale.
  if (!out.ok || out.width !== 800 || out.height !== 240) throw new Error(`The page's picture was ${JSON.stringify(out)}`);
});

Then('Settings says the web runtime is {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`Settings saying the web runtime is "${words}"`, (d) => (d as { settingsWindow?: { webRuntime?: string } | null }).settingsWindow?.webRuntime === words);
});
