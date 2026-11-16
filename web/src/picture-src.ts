// Which pictures a reply's Markdown draws (SECURITY.md, "Pictures in
// replies"): only the thread's own, which Threadbear stored in the
// backend's file storage and the page was handed by src (`pictures`).
// Any other web address is an agent's or another Mac's words, and could
// be anyone's server or one on the reader's own network: it's shown as a
// link to open, never put in an <img>, so reading the thread doesn't tell
// that server anything (Daybreak finding 133, 15 Nov 2026). The phones do
// the same (ThreadPictures.webLink), as the Mac app always has.
export type PictureSrc = { kind: 'stored'; url: string } | { kind: 'link'; url: string } | { kind: 'none' };

export function pictureSrc(src: string, pictures?: Map<string, string>): PictureSrc {
  const stored = pictures?.get(src);
  if (stored) return { kind: 'stored', url: stored };
  if (/^https?:\/\/[^/?#\s]/i.test(src)) return { kind: 'link', url: src };
  return { kind: 'none' };
}

// The page's img-src: itself, data: and blob: (icons, previews), and the
// Convex deployment whose file storage holds the stored pictures. Put in
// index.html at build (vite.config.ts), so a picture that slips past
// pictureSrc still isn't fetched from anywhere else.
export function imageSourcePolicy(convexUrl?: string, siteUrl?: string): string {
  const origins = new Set<string>();
  for (const u of [convexUrl, siteUrl, convexUrl?.replace(/\.convex\.cloud(?=$|[/:])/, '.convex.site')]) {
    if (!u) continue;
    try {
      const url = new URL(u);
      if (url.protocol === 'https:' || url.protocol === 'http:') origins.add(url.origin);
    } catch {
      // not a URL: left out, so the policy is stricter, never looser
    }
  }
  return ["img-src 'self' data: blob:", ...origins].join(' ');
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host || 'the web';
  } catch {
    return 'the web';
  }
}

// index.html with the policy in its <head>, before anything that loads a
// picture. Throws rather than ship a page without it.
export function withImagePolicy(html: string, policy: string): string {
  const at = html.indexOf('<meta name="referrer"');
  if (at < 0 || at > html.indexOf('</head>')) throw new Error('index.html has no referrer meta in its <head> to put the image policy before');
  return `${html.slice(0, at)}<meta http-equiv="Content-Security-Policy" content="${policy.replace(/"/g, '&quot;')}" />\n    ${html.slice(at)}`;
}
