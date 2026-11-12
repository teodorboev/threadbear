// Where each scenario's app keeps the web runtime (THREADBEAR_WEB_RUNTIME_DIR,
// desktop/core/src/web_runtime.rs): an empty folder of its own, so no spec
// ever reads or writes the machine's, and so each starts with nothing
// installed. A scenario tagged @web-runtime gets a folder shared by them all,
// with the pinned build installed by the first that needs it (from
// TB_WEB_RUNTIME_ARCHIVE, the archive downloaded once, so a run doesn't fetch
// 135 MB per scenario), unless it's @web-runtime-fresh too.
//
// Only the Mac has a runtime so far: @web-prompt and @web-runtime scenarios
// are skipped elsewhere, and @web-runtime ones on a Mac without the archive
// (CI), rather than downloading it on every run.
//
// The web panel scenarios also get the little sites they show
// (support/web-pages.ts).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { startSites, stopSites, type Sites } from './web-pages.ts';

const made: string[] = [];
let shown: Sites | undefined;

export async function webRuntimeSetup(tags: string[]): Promise<'skipped' | undefined> {
  const mac = process.platform === 'darwin';
  // The GPUI app's pictures of diagrams and pages, kept on disk under the
  // same names as the Swift app's: a folder of its own each time, or one
  // scenario's drawing shows in the next without it being drawn.
  const pictures = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-spec-rich-'));
  made.push(pictures);
  process.env.THREADBEAR_RICH_CACHE_DIR = pictures;
  // The Swift app draws pages with WebKit: it has no runtime to need.
  const gpui = (process.env.TB_APP ?? 'gpui') === 'gpui';
  if (gpui && tags.includes('@web-prompt') && !mac) return 'skipped';
  if (tags.some((t) => t.startsWith('@panel.web') || t.startsWith('@panel.bar') || t.startsWith('@panel.convex'))) {
    shown = await startSites();
    process.env.THREADBEAR_CONVEX_EMBEDDED = shown.dashboard;
  } else {
    delete process.env.THREADBEAR_CONVEX_EMBEDDED;
  }
  if (gpui && tags.includes('@web-runtime')) {
    if (!mac || !process.env.TB_WEB_RUNTIME_ARCHIVE) return 'skipped';
    if (!tags.includes('@web-runtime-fresh')) {
      const shared = process.env.TB_WEB_RUNTIME_DIR ?? path.join(os.tmpdir(), 'tb-spec-web-runtime');
      fs.mkdirSync(shared, { recursive: true });
      process.env.THREADBEAR_WEB_RUNTIME_DIR = shared;
      return undefined;
    }
  }
  const fresh = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-spec-runtime-'));
  made.push(fresh);
  process.env.THREADBEAR_WEB_RUNTIME_DIR = fresh;
  return undefined;
}

/** The sites this scenario's panels show. */
export function sites(): Sites {
  if (!shown) throw new Error('this scenario has no sites: tag it @panel.web, @panel.bar or @panel.convex');
  return shown;
}

/** After the app's stopped: the scenario's own folder goes. */
export function webRuntimeCleanup() {
  for (const dir of made.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  stopSites();
  shown = undefined;
}
