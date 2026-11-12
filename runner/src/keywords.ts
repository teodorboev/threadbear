// A thread's search words (docs/protocol.md, "Search"): the librarian writes
// a few words someone might type to find a thread that its title and last
// reply don't say, like "dentist" for "Wisdom tooth meeting location", and
// every search (the Mac's sidebar, the phone, list_threads, Convex's
// phone:search) matches them as well. Mike searched "dentist" on 11 Nov and
// found nothing, as no message in that thread had the word.

// At most this many words, and bytes in all: a row's worth, not a summary.
export const KEYWORDS_MAX = 20;
export const KEYWORDS_CHARS = 200;

// Lowercase words, each once, leaving out any its title already has (search
// finds those anyway) and single letters. Letters and digits of any script,
// so "café" and "東京" stay words.
export function cleanKeywords(words: string, title = ''): string {
  const inTitle = new Set(splitWords(title));
  const out: string[] = [];
  let chars = 0;
  for (const w of splitWords(words)) {
    if (w.length < 2 || inTitle.has(w) || out.includes(w)) continue;
    if (out.length >= KEYWORDS_MAX || chars + w.length + (out.length ? 1 : 0) > KEYWORDS_CHARS) break;
    chars += w.length + (out.length ? 1 : 0);
    out.push(w);
  }
  return out.join(' ');
}

function splitWords(text: string): string[] {
  return text.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}
