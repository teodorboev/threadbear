// The changelog page (/changelog): what's new in each daily release, for people who use
// Threadbear rather than people reading its commits. Mike picked this layout on 10 Nov 2026
// ("release days") after seeing Wayne Sutton's bearfooding changelog
// (tame-weasel-262.convex.site): a thread runs down the page with a sewn button at each
// release, the release's main changes (4 to 8) get a card with a short clip or a picture, and
// the rest are plain Added / Changed / Fixed lines. Clips, not before/after sliders: Mike found
// a video shows a change better (11 Nov).
//
// Each release is a file in site/changelog/<YYYY-MM-DD>.json, written by the daily changelog
// thread (docs/changelog.md has how). Its pictures and clips aren't in the repo, so every fork
// doesn't have to download them: they're in the R2 bucket the Worker serves at /media/
// (worker.mjs), uploaded by scripts/media.mjs from site/.media/, which `npm run serve` also
// serves locally.
//
// A release file:
//   {
//     "release": "2026-11-09",
//     "highlights": [
//       { "kind": "added" | "changed" | "fixed", "title": "...", "text": "...", "credit"?: "Wayne Sutton",
//         "media"?: { "type": "clip", "src": "x.mp4", "poster": "x.webp", "width", "height", "alt" }
//                 | { "type": "image", "src": "x.webp", "width", "height", "alt" } }
//     ],
//     "added": ["...", { "text": "...", "credit": "Wayne Sutton" }], "changed": [...], "fixed": [...]
//   }
// A line is its text, or { text, credit } when it came from someone else: their PR, fork, issue
// or report (Mike, 13 Nov: "attribute any fixes or whatever to people if it came from other
// people"). Highlights take a credit the same way. Each name in a credit is linked from
// site/changelog/people.json (a name to their X account if their GitHub profile gives one, else
// their fork of Threadbear, else their GitHub profile), and the build refuses a credit that
// names nobody in it.
// Media names are relative to /media/changelog/<release>/.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const KINDS = ["added", "changed", "fixed"];
const LABELS = { added: "Added", changed: "Changed", fixed: "Fixed" };
const MEDIA_NAME = /^[a-z0-9][a-z0-9-]*\.(webp|png|jpg|mp4)$/;

/** The media address for a file of a release. */
export const mediaPath = (release, name) => `/media/changelog/${release}/${name}`;

/** site/changelog/people.json: who's thanked, and where their name links to. */
export function readPeople(dir) {
  const people = JSON.parse(readFileSync(join(dir, "people.json"), "utf8"));
  for (const [name, url] of Object.entries(people)) {
    if (!/^https:\/\/(x\.com|github\.com)\//.test(url)) throw new Error(`site/changelog/people.json: ${name}'s link should be their X or GitHub page`);
  }
  return people;
}

/** The names of `people` that `credit` mentions, as whole words. */
const namesIn = (credit, people) =>
  Object.keys(people).filter((n) => new RegExp(`(^|[^\\p{L}])${n.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}($|[^\\p{L}])`, "u").test(credit));

/** Every release file in `dir`, checked, newest first. A bad one stops the build with its name. */
export function readReleases(dir, people = readPeople(dir)) {
  const checkCredit = (credit, at) => {
    if (credit && !namesIn(credit, people).length) throw new Error(`${at}: the credit "${credit}" names nobody in site/changelog/people.json; add them there with a link`);
  };
  const files = readdirSync(dir).filter((f) => /^\d{4}-\d{2}-\d{2}\.json$/.test(f)).sort().reverse();
  return files.map((file) => {
    const where = `site/changelog/${file}`;
    let r;
    try {
      r = JSON.parse(readFileSync(join(dir, file), "utf8"));
    } catch (e) {
      throw new Error(`${where} isn't JSON: ${e.message}`);
    }
    if (r.release !== file.slice(0, 10)) throw new Error(`${where}: "release" should be ${file.slice(0, 10)}`);
    for (const k of KINDS) {
      r[k] ??= [];
      const ok = (l) => (typeof l === "string" ? l.trim() : l && typeof l.text === "string" && l.text.trim() && typeof l.credit === "string" && l.credit.trim());
      if (!Array.isArray(r[k]) || !r[k].every(ok)) throw new Error(`${where}: "${k}" should be a list of lines, each its text or { text, credit }`);
    }
    r.highlights ??= [];
    r.highlights.forEach((h, i) => {
      const at = `${where} highlight ${i + 1}`;
      if (!KINDS.includes(h.kind)) throw new Error(`${at}: kind should be added, changed or fixed`);
      if (!h.title?.trim() || !h.text?.trim()) throw new Error(`${at}: needs a title and a text`);
      const m = h.media;
      // A card is for something you can see (Mike, 11 Nov: "More videos and screenshots please",
      // about a card that had none). If it can't be filmed, it's a plain line instead.
      if (!m) throw new Error(`${at}: a highlight needs a clip or a picture; make it a plain line if it can't have one`);
      const names = m.type === "image" ? [m.src] : m.type === "clip" ? [m.src, m.poster] : null;
      if (!names) throw new Error(`${at}: media type should be clip or image`);
      if (names.some((n) => !MEDIA_NAME.test(n ?? ""))) throw new Error(`${at}: media names are lower-case-with-dashes files (.webp, .png, .jpg or .mp4)`);
      if (!(m.width > 0 && m.height > 0)) throw new Error(`${at}: media needs its width and height`);
      if (!m.alt?.trim()) throw new Error(`${at}: media needs alt text saying what it shows`);
    });
    r.highlights.forEach((h, i) => checkCredit(h.credit, `${where} highlight ${i + 1}`));
    for (const k of KINDS) r[k].forEach((l) => checkCredit(l.credit, `${where} ${k} line "${(l.text ?? l).slice(0, 40)}"`));
    if (!r.highlights.length && !KINDS.some((k) => r[k].length)) throw new Error(`${where} has nothing in it`);
    return r;
  });
}

/**
 * Page text: HTML-escaped, and em and en dashes made a comma and a hyphen (the build refuses a
 * page with either), braces as entities so a mention of a {{placeholder}} doesn't trip the
 * build's leftover check. `code` spans in backticks become <code>.
 */
export function pageText(s) {
  return s
    .replace(/\s*—\s*/g, ", ").replace(/–/g, "-")
    .replace(/[&<>"{}]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "{": "&#123;", "}": "&#125;" })[c])
    .replace(/`([^`]+)`/g, "<code>$1</code>");
}

/** "Monday 9 November" for a YYYY-MM-DD, and the year on its own. */
function dayName(day) {
  const d = new Date(`${day}T12:00:00Z`);
  const opts = { timeZone: "UTC" };
  return {
    weekday: d.toLocaleDateString("en-AU", { ...opts, weekday: "long" }),
    date: d.toLocaleDateString("en-AU", { ...opts, day: "numeric", month: "long" }),
    year: d.getUTCFullYear(),
  };
}

function media(release, m) {
  const src = (n) => mediaPath(release, n);
  const size = `width="${m.width}" height="${m.height}"`;
  const alt = pageText(m.alt);
  if (m.type === "image") return `<img class="cl-shot" src="${src(m.src)}" alt="${alt}" ${size} loading="lazy" decoding="async">`;
  if (m.type === "clip") {
    // Muted and playsinline so phones play it in place; site.js only plays one that's on screen.
    return `<video class="cl-shot" src="${src(m.src)}" poster="${src(m.poster)}" ${size} muted loop playsinline preload="none" aria-label="${alt}"></video>`;
  }
  throw new Error(`Unknown media type ${m.type}`);
}

/** "Thanks to <name>" after a card's or a line's text, when it came from someone else. */
let people = {};
const creditTag = (who) => {
  if (!who) return "";
  let html = pageText(who);
  // Longest first, so a name inside a longer one isn't linked twice.
  for (const name of namesIn(who, people).sort((a, b) => b.length - a.length)) {
    html = html.replace(pageText(name), `<a href="${people[name]}">${pageText(name)}</a>`);
  }
  return ` <span class="cl-credit">Thanks to ${html}</span>`;
};

/** A plain line: its text, and who it came from when it says. */
const lineHtml = (l) => (typeof l === "string" ? pageText(l) : `${pageText(l.text)}${creditTag(l.credit)}`);

function highlight(release, h) {
  const credit = creditTag(h.credit);
  return `          <article class="cl-feature">
            <p class="cl-kind ${h.kind}">${LABELS[h.kind]}</p>
            <h3>${pageText(h.title)}</h3>
            <p>${pageText(h.text)}${credit}</p>
            ${h.media ? `<div class="cl-media">\n              ${media(release, h.media)}\n            </div>` : ""}
          </article>`;
}

/** The page's content, for writePage. */
export function changelogPage({ releases, githubUrl, people: who = {} }) {
  people = who;
  const sections = releases.map((r, i) => {
    const { weekday, date, year } = dayName(r.release);
    const counts = KINDS.map((k) => [k, r[k].length + r.highlights.filter((h) => h.kind === k).length]).filter(([, n]) => n);
    const tally = counts.map(([k, n]) => `${n} ${k === "added" ? "new" : k}`).join(", ");
    const previous = releases[i + 1];
    const every = previous
      ? `${githubUrl}/compare/release/${previous.release}...release/${r.release}`
      : `${githubUrl}/commits/release/${r.release}`;
    const lists = KINDS.filter((k) => r[k].length).map((k) => `            <div>
              <p class="cl-kind ${k}">${r.highlights.length ? (k === "added" ? "Also new" : `Also ${k}`) : LABELS[k]}</p>
              <ul>
${r[k].map((l) => `                <li>${lineHtml(l)}</li>`).join("\n")}
              </ul>
            </div>`);
    return `        <section class="cl-release" id="${r.release}" aria-labelledby="h-${r.release}">
          <span class="cl-button" aria-hidden="true"></span>
          <h2 id="h-${r.release}"><a href="#${r.release}">${weekday} ${date}<span class="cl-year"> ${year}</span></a></h2>
          <p class="cl-when">${tally}${tally ? " · " : ""}<a href="${every}">Every change</a></p>
${r.highlights.map((h) => highlight(r.release, h)).join("\n")}
${lists.length ? `          <div class="cl-lists">\n${lists.join("\n")}\n          </div>` : ""}
        </section>`;
  });
  return `    <article class="changelog">
      <header class="cl-head">
        <div>
          <h1>What's new</h1>
          <p class="lede">A new version of Threadbear comes out most days. The big changes are up top with a look at them, and the little ones are underneath.</p>
        </div>
        <img src="/img/bear-sit.webp" alt="" width="271" height="374">
      </header>
      <div class="cl-thread">
${sections.join("\n")}
      </div>
    </article>`;
}
