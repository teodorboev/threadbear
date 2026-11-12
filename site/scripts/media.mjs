#!/usr/bin/env node
// The changelog's pictures and clips: kept in site/.media/ (not committed) until they go up to
// the R2 bucket the Worker serves at /media/ (worker.mjs, wrangler.jsonc). docs/changelog.md
// has how they're made.
//
//   npm run media -- upload 2026-11-09   puts site/.media/changelog/2026-11-09/* up (all releases
//                                         without a name), skipping files R2 already has
//   npm run media -- check               every file site/changelog/*.json names answers on the
//                                         live site; exits 1 naming any that don't
//
// Upload uses wrangler with Mike's Cloudflare login, like `npm run deploy`. A file's name never
// changes once it's up, as the Worker tells browsers to keep them for good: a new picture for an
// entry gets a new name.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { SITE_URL } from "../site.mjs";
import { mediaPath, readReleases } from "./changelog.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const local = join(root, ".media", "changelog");
const BUCKET = "threadbear-site-media";
const TYPES = { webp: "image/webp", png: "image/png", jpg: "image/jpeg", mp4: "video/mp4" };

/** Every media file a release file names. */
function named() {
  return readReleases(join(root, "changelog")).flatMap((r) => r.highlights.flatMap((h) => {
    const m = h.media;
    if (!m) return [];
    const names = m.type === "clip" ? [m.src, m.poster] : [m.src];
    return names.map((n) => ({ release: r.release, name: n }));
  }));
}

/** The size of the file the live site has under that name, or undefined when it has none. */
async function liveSize(release, name) {
  const r = await fetch(SITE_URL + mediaPath(release, name), { method: "HEAD" });
  return r.ok ? Number(r.headers.get("content-length")) : undefined;
}
const isUp = async (release, name) => (await liveSize(release, name)) !== undefined;

const [command, only] = process.argv.slice(2);
if (command === "upload") {
  if (!existsSync(local)) throw new Error(`Nothing to upload: no ${local}`);
  const releases = only ? [only] : readdirSync(local).filter((d) => /^\d{4}-\d{2}-\d{2}$/.test(d));
  for (const release of releases) {
    const dir = join(local, release);
    if (!existsSync(dir)) throw new Error(`No ${dir}`);
    for (const name of readdirSync(dir).filter((f) => TYPES[f.split(".").pop()])) {
      const file = join(dir, name);
      const size = await liveSize(release, name);
      if (size !== undefined) {
        // Browsers keep a media file for good, so a name that's up can't take a different file:
        // a reshot picture needs a new name (11 Nov: reshoots under the old names were skipped
        // here and the page went on showing the old pictures).
        if (size !== statSync(file).size) throw new Error(`${release}/${name} is already up as a different file: give the new one a new name (and change the JSON)`);
        console.log(`already up: ${release}/${name}`);
        continue;
      }
      execFileSync("npx", ["-y", "wrangler@4", "r2", "object", "put", `${BUCKET}/changelog/${release}/${name}`,
        "--file", file, "--content-type", TYPES[name.split(".").pop()], "--remote"], { stdio: ["ignore", "ignore", "inherit"], cwd: root });
      console.log(`up: ${release}/${name} (${Math.round(statSync(file).size / 1024)} KB)`);
    }
  }
} else if (command === "check") {
  const missing = [];
  for (const { release, name } of named()) if (!(await isUp(release, name))) missing.push(`${release}/${name}`);
  if (missing.length) {
    console.error(`Not on ${SITE_URL}/media/changelog/:\n  ${missing.join("\n  ")}`);
    process.exit(1);
  }
  console.log(`All ${named().length} changelog media files are up.`);
} else {
  console.error("npm run media -- upload [YYYY-MM-DD] | check");
  process.exit(2);
}
