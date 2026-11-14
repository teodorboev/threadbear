// JSON text the runner sends with no half characters in it.
//
// A JS string can hold one half of an emoji's UTF-16 pair on its own (a cut
// through it with .slice, a CLI's output split mid-character), and
// JSON.stringify writes that half as an escape like "\ud83e". JS reads it back
// happily; strict parsers don't: serde_json (the GPUI app) and Swift's refuse
// the whole message. The GPUI app dropped such a reply without a word, so on
// 14 Nov a thread whose transcript page carried one never opened: each try
// waited out the 120 s timeout. Old events keep their halves on disk
// (a thread's own copy is never rewritten), so the repair is here, on the way
// out, for every client.
//
// JSON.stringify writes a whole pair as the characters themselves and only a
// lone half as an escape, always in lowercase hex, so any surrogate escape in
// its output is a lone half. It becomes U+FFFD, the replacement character.
// A backslash before it that is itself escaped ("\\ud83e" is the text
// backslash-u-d-8-3-e) is left alone: the escape counts only after an even run
// of backslashes.
const LONE_HALF = /(?<!\\)((?:\\\\)*)\\ud[89a-f][0-9a-f]{2}/g;

export function wellFormedJson(text: string): string {
  // Nearly every message has none: one scan and out.
  if (!text.includes('\\ud')) return text;
  return text.replace(LONE_HALF, '$1\\ufffd');
}
