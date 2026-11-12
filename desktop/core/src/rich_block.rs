// ```mermaid and ```html blocks in a reply, drawn as what they show rather
// than as code: a port of the Swift app's RichBlock.swift, whose pages were
// security-reviewed there. Which fences count, each block's id (the same as
// the Swift app's, so the two apps share the pictures in
// ~/Library/Caches/Threadbear/rich), the colours, and the pages the web
// runtime is given: an agent's html behind a Content-Security-Policy that
// allows only inline styles and data: pictures, and the app's own diagram
// page that Pele (pele.run, bundled, MIT) draws into, the diagram's source
// handed over as a string, never as markup.
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RichKind {
    Mermaid,
    Html,
}

impl RichKind {
    pub fn name(self) -> &'static str {
        match self {
            RichKind::Mermaid => "mermaid",
            RichKind::Html => "html",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RichBlock {
    pub kind: RichKind,
    pub source: String,
    /// Stable for the same kind and source, so a picture, a switch to the
    /// source and a page for the window are found again after a restart.
    pub id: String,
}

impl RichBlock {
    /// Past this a block stays plain code: Pele refuses more than 50,000
    /// characters itself, and a page that size is a file, not a reply.
    pub const MAX_SOURCE: usize = 50_000;

    pub fn new(kind: RichKind, source: impl Into<String>) -> Self {
        let source = source.into();
        let digest = Sha256::digest(format!("{}\n{source}", kind.name()).as_bytes());
        let id = digest[..10].iter().map(|b| format!("{b:02x}")).collect();
        RichBlock { kind, source, id }
    }

    /// The block for a fence's language (```mermaid, ```HTML title=x: only
    /// the first word counts), or None for anything else, an empty one, or
    /// one too long.
    pub fn from(language: Option<&str>, code: &str) -> Option<RichBlock> {
        let word = language?.split([' ', '\t', '{']).find(|w| !w.is_empty())?.to_lowercase();
        let kind = match word.as_str() {
            "mermaid" => RichKind::Mermaid,
            "html" => RichKind::Html,
            _ => return None,
        };
        let source = code.trim_end_matches('\n');
        if source.trim().is_empty() || source.chars().count() > Self::MAX_SOURCE {
            return None;
        }
        Some(RichBlock::new(kind, source))
    }

    /// What the box's header calls it.
    pub fn label(&self) -> &'static str {
        self.kind.name()
    }

    /// The left half of the switch.
    pub fn drawn_title(&self) -> &'static str {
        match self.kind {
            RichKind::Mermaid => "Diagram",
            RichKind::Html => "Preview",
        }
    }

    /// Where its pictures are kept, at the zoom they were drawn for
    /// (RichBlocks.key in the Swift app).
    pub fn key(&self, zoom: u32) -> String {
        format!("{}-{zoom}", self.id)
    }

    /// The window's title (RichRenderer.showWindow).
    pub fn window_title(&self) -> &'static str {
        match self.kind {
            RichKind::Mermaid => "Diagram",
            RichKind::Html => "Page",
        }
    }
}

/// Whether text could hold a rich block, so only those replies are looked at.
pub fn may_hold_rich(text: &str) -> bool {
    if !text.contains("```") && !text.contains("~~~") {
        return false;
    }
    let lowered = text.to_lowercase();
    lowered.contains("mermaid") || lowered.contains("html")
}

/// Pele's colours for Threadbear, matching the transcript's code box so a
/// diagram sits in it as if drawn there. `bg` is only what labels are backed
/// with; the rest of the picture is see-through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RichPalette {
    pub bg: &'static str,
    pub fg: &'static str,
    pub muted: &'static str,
    pub line: &'static str,
    pub surface: &'static str,
    pub surface_alt: &'static str,
    pub border: &'static str,
    pub accent: &'static str,
}

impl RichPalette {
    pub const DARK: RichPalette = RichPalette { bg: "#282828", fg: "#ececec", muted: "#9a9a9a", line: "#8e8e93", surface: "#333335", surface_alt: "#3a3a3c", border: "#5a5a5e", accent: "#0a84ff" };
    pub const LIGHT: RichPalette = RichPalette { bg: "#f7f7f7", fg: "#1d1d1f", muted: "#6e6e73", line: "#86868b", surface: "#ffffff", surface_alt: "#f0f0f2", border: "#c7c7cc", accent: "#007aff" };

    /// Pele's CSS variables (pele.run/theming), without the selector.
    fn variables(&self) -> String {
        format!(
            "--pele-bg:{};--pele-fg:{};--pele-muted:{};--pele-line:{};--pele-surface:{};--pele-surface-alt:{};--pele-border:{};--pele-accent:{};--pele-radius:6px;--pele-font:-apple-system,system-ui,sans-serif;--pele-font-mono:ui-monospace,Menlo,monospace",
            self.bg, self.fg, self.muted, self.line, self.surface, self.surface_alt, self.border, self.accent
        )
    }

    pub fn css(&self) -> String {
        format!(".pele{{{}}}", self.variables())
    }
}

/// Every sealed page gets this as its Content-Security-Policy: nothing from
/// anywhere, except inline styles and pictures written into the page itself.
/// The runtime's rules (desktop/web's sealed.rs) stop the network as well;
/// this is the page's own say on top.
pub const POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; form-action 'none'; base-uri 'none'; frame-src 'none'";

fn escape_attribute(text: &str) -> String {
    text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The agent's HTML with the policy in front of it. A `<meta>` policy only
/// counts in the head before anything it governs, so it goes first (after a
/// doctype, or the page lays out in quirks mode), ahead of the agent's own
/// head; browsers take what follows as the same document.
pub fn html_page(source: &str) -> String {
    format!("<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"{}\"><meta name=\"color-scheme\" content=\"light dark\">{source}", escape_attribute(POLICY))
}

/// The page the diagram view loads once, with both palettes; the draw script
/// renders into it. Its own policy lets no script of the page's run: Pele and
/// the draw script run apart from the page, in a world of their own.
pub fn diagram_page() -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\n<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n<style>\nhtml,body{{margin:0;padding:0;background:transparent}}\n#out{{display:inline-block;padding:2px}}\n#out.dark{{{}}}\n#out.light{{{}}}\n</style></head><body><div id=\"out\"></div></body></html>",
        escape_attribute(POLICY),
        RichPalette::DARK.variables(),
        RichPalette::LIGHT.variables()
    )
}

/// The biggest a diagram is drawn, in CSS pixels: a side, and an area.
pub const DIAGRAM_MAX_SIDE: u32 = 2400;
pub const DIAGRAM_MAX_AREA: u32 = 2400 * 1600;

/// Run in the diagram page's own world: renders `source` and gives back its
/// size, or `{error}` with Pele's message. A function body taking `source`,
/// `dark`, `fontSize`, `maxSide` and `maxArea` (the Swift app's drawScript).
pub const DRAW_SCRIPT: &str = r#"
const out = document.getElementById('out');
out.className = dark ? 'dark' : 'light';
try {
  const result = pele.render(source, { links: false, images: false, responsive: false, fontSize: fontSize,
    fontFamily: '-apple-system, system-ui, sans-serif', idPrefix: dark ? 'dk' : 'lt' });
  out.innerHTML = result.svg;
  // The box round the SVG and all it draws (a few shapes reach past its
  // viewBox), so the picture takes all of it.
  const svg = out.querySelector('svg');
  // A diagram can ask for any size (spacing in its config): drawn smaller
  // past a budget, so a hostile one can't make a picture of gigabytes.
  const w = parseFloat(svg.getAttribute('width')) || 1;
  const h = parseFloat(svg.getAttribute('height')) || 1;
  const scale = Math.min(1, maxSide / w, maxSide / h, Math.sqrt(maxArea / (w * h)));
  if (scale < 1) {
    svg.setAttribute('width', String(Math.floor(w * scale)));
    svg.setAttribute('height', String(Math.floor(h * scale)));
  }
  let right = 0, bottom = 0;
  for (const el of [out, svg, ...svg.querySelectorAll('*')]) {
    const r = el.getBoundingClientRect();
    if (r.width || r.height) { right = Math.max(right, r.right); bottom = Math.max(bottom, r.bottom); }
  }
  return { width: Math.min(Math.ceil(right) + 2, maxSide + 8), height: Math.min(Math.ceil(bottom) + 2, maxSide + 8), svg: out.innerHTML };
} catch (error) {
  out.textContent = '';
  return { error: String(error && error.message || error) };
}
"#;

/// The draw script called with its arguments, each written as JSON (a
/// string literal for the source, never markup), for DevTools to evaluate.
pub fn draw_call(source: &str, dark: bool, font_size: f64) -> String {
    let source = serde_json::to_string(source).unwrap_or_else(|_| "\"\"".into());
    format!("(function (source, dark, fontSize, maxSide, maxArea) {{{DRAW_SCRIPT}}})({source}, {dark}, {font_size}, {DIAGRAM_MAX_SIDE}, {DIAGRAM_MAX_AREA})")
}

/// A page for the full-size window: the diagram as it was drawn, standing
/// alone (no scripts needed to show it), centred, in both looks.
pub fn diagram_pane_page(dark_svg: &str, light_svg: &str, title: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\n<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<style>\nhtml,body{{margin:0;min-height:100%}}\nbody{{display:flex;justify-content:center;padding:24px;box-sizing:border-box;background:{}}}\n.look.dark{{display:none}}\n{}\n@media (prefers-color-scheme: dark){{\n  body{{background:{}}}\n  .look.light{{display:none}}.look.dark{{display:block}}\n}}\n.look.dark .pele{{{}}}\nsvg{{max-width:100%;height:auto}}\n</style></head><body><div class=\"look light\">{light_svg}</div><div class=\"look dark\">{dark_svg}</div></body></html>",
        escape_attribute(title),
        escape_attribute(POLICY),
        RichPalette::LIGHT.bg,
        RichPalette::LIGHT.css(),
        RichPalette::DARK.bg,
        RichPalette::DARK.variables()
    )
}

/// The window's copy of a diagram scales it to fill the window, bigger or
/// smaller (an SVG's viewBox keeps its shape), overriding Pele's max-width.
pub const FIT_TO_WINDOW: &str = "<style>html,body{height:100%}body{align-items:center}.look{width:100%;height:100%}.look svg{width:100%!important;height:100%!important;max-width:none!important;max-height:none!important}</style>";

/// The pane page fitted to its window: FIT_TO_WINDOW just before `</head>`.
pub fn fitted_to_window(page: &str) -> String {
    match page.find("</head>") {
        Some(at) => format!("{}{FIT_TO_WINDOW}{}", &page[..at], &page[at..]),
        None => page.to_owned(),
    }
}

/// How a block's picture sits in its box: a diagram at its own size or
/// smaller, centred; a page filling the width, cut past `max_body`. All in
/// points (RichBlockCell.pictureSize).
pub fn picture_size(kind: RichKind, natural: (f32, f32), room: f32, max_body: f32, pad: f32) -> (f32, f32) {
    let (width, height) = natural;
    if width <= 0. || height <= 0. {
        return (0., 0.);
    }
    match kind {
        RichKind::Html => {
            let scale = room / width;
            (room, (height * scale).round())
        }
        RichKind::Mermaid => {
            let room = room - pad * 2.;
            let scale = 1f32.min(room / width).min((max_body - pad * 2.) / height);
            ((width * scale).round(), (height * scale).round())
        }
    }
}

/// Drawn blocks' pictures kept in memory, by what they take up rather than
/// how many there are (RichBlocks.pictureBytes): past the budget the least
/// recently used go, and are read back from disk when drawn again. A count
/// let 120 tall, noisy pages hold gigabytes (the Swift app's cross-lab
/// review, 6 Nov; Daybreak 92 found the GPUI app's count, 11 Nov 2026).
pub struct PictureBudget<V> {
    budget: usize,
    used: usize,
    /// Oldest first, with the parts each one holds.
    entries: std::collections::VecDeque<(String, V, Vec<u64>)>,
    /// Each part (a picture, by its id) once, however many blocks hold it,
    /// with how many do and what it takes up: blocks drawn alike share
    /// their pictures, so they're counted once (Codex's fifth look at
    /// Daybreak 92).
    parts: std::collections::HashMap<u64, (usize, usize)>,
    /// For parts given only as a cost (`keep`): ids of their own, from the
    /// top down, apart from pictures' hashes.
    next_own: u64,
}

impl<V: Clone> PictureBudget<V> {
    /// The Swift app's 160 MiB.
    pub const BYTES: usize = 160 * 1024 * 1024;

    pub fn new(budget: usize) -> PictureBudget<V> {
        PictureBudget { budget, used: 0, entries: std::collections::VecDeque::new(), parts: std::collections::HashMap::new(), next_own: u64::MAX }
    }

    /// Keeps `value` as the newest, and gives back what had to go to fit it
    /// (the caller frees their decoded copies). The newest is always kept,
    /// even alone over the budget, as it's what's being drawn.
    pub fn keep(&mut self, key: String, value: V, cost: usize) -> Vec<V> {
        self.keep_with(key, value, cost, |_| false)
    }

    /// `keep`, never letting go of the blocks `pinned` says are being drawn,
    /// by key: it goes over the budget for them rather than take a picture
    /// off the screen (they're FrameBudget's, which holds half of it), and
    /// `trim_with` brings it back once they're not.
    pub fn keep_with(&mut self, key: String, value: V, cost: usize, pinned: impl Fn(&str) -> bool) -> Vec<V> {
        let own = self.next_own;
        self.next_own -= 1;
        self.keep_parts_with(key, value, &[(own, cost)], pinned)
    }

    /// `keep_with` for a value made of parts (pictures, by id, and what
    /// each takes up), a part counted once while any block holds it.
    pub fn keep_parts_with(&mut self, key: String, value: V, parts: &[(u64, usize)], pinned: impl Fn(&str) -> bool) -> Vec<V> {
        let mut gone = Vec::new();
        // Held before the old one under this key lets them go, so a part
        // both have isn't counted out and in again.
        for &(id, cost) in parts {
            let part = self.parts.entry(id).or_insert((0, cost));
            if part.0 == 0 {
                self.used += cost;
            }
            part.0 += 1;
        }
        if let Some(at) = self.entries.iter().position(|(k, ..)| *k == key) {
            gone.push(self.take(at));
        }
        self.entries.push_back((key, value, parts.iter().map(|&(id, _)| id).collect()));
        gone.extend(self.trim_with(pinned));
        gone
    }

    /// Entry `at` out, its parts let go of.
    fn take(&mut self, at: usize) -> V {
        let (_, value, ids) = self.entries.remove(at).expect("there");
        for id in ids {
            if let Some(part) = self.parts.get_mut(&id) {
                part.0 -= 1;
                if part.0 == 0 {
                    self.used -= part.1;
                    self.parts.remove(&id);
                }
            }
        }
        value
    }

    /// Back within the budget, oldest first, never the newest nor a pinned
    /// one; gives back what went.
    pub fn trim_with(&mut self, pinned: impl Fn(&str) -> bool) -> Vec<V> {
        let mut gone = Vec::new();
        let mut at = 0;
        while self.used > self.budget && at + 1 < self.entries.len() {
            if pinned(&self.entries[at].0) {
                at += 1;
                continue;
            }
            gone.push(self.take(at));
        }
        gone
    }

    /// The pictures kept for `key`, now the most recently used.
    pub fn get(&mut self, key: &str) -> Option<V> {
        let at = self.entries.iter().position(|(k, ..)| k == key)?;
        let entry = self.entries.remove(at).expect("there");
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    /// Takes `key`'s pictures out, giving them back to be freed.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let at = self.entries.iter().position(|(k, ..)| k == key)?;
        Some(self.take(at))
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, value, _)| value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The pictures handed to boxes in the frame being drawn. A reply's boxes
/// are all laid out at once, so one reply with more than the budget's worth
/// gets pictures in its first boxes and its source in the rest, and nothing
/// handed out is let go until the frame's over: an image element still
/// holding one would load it again, outside the budget (Codex's second look
/// at Daybreak 92).
#[derive(Default)]
pub struct FrameBudget {
    /// The blocks drawn, by key, and their pictures, by image id.
    keys: std::collections::HashSet<String>,
    ids: std::collections::HashSet<u64>,
    cost: usize,
}

impl FrameBudget {
    /// Whether pictures `ids` costing `cost` may be drawn this frame: always
    /// the first, and those already handed out.
    /// What one frame may draw: half the pictures' budget, so pictures the
    /// frame holds and a newly drawn one fit in it together.
    pub const BYTES: usize = PictureBudget::<()>::BYTES / 2;

    pub fn hand_out(&mut self, key: &str, ids: [u64; 2], cost: usize, budget: usize) -> bool {
        // The same pictures again (another block drawn alike shares them):
        // nothing more in memory.
        if !ids.iter().all(|id| self.ids.contains(id)) {
            if self.cost > 0 && self.cost.saturating_add(cost) > budget {
                return false;
            }
            self.cost = self.cost.saturating_add(cost);
            self.ids.extend(ids);
        }
        self.keys.insert(key.to_owned());
        true
    }

    /// Whether block `key` is drawn this frame.
    pub fn pins(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    /// Whether picture `id` is drawn this frame.
    pub fn holds(&self, id: u64) -> bool {
        self.ids.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The frame's over.
    pub fn end(&mut self) {
        self.keys.clear();
        self.ids.clear();
        self.cost = 0;
    }
}

/// What a PNG takes up in memory once drawn: its bytes, which the image
/// keeps, and its pixels decoded at four bytes each, read from its header.
/// Bytes that aren't a PNG count as four times their length.
pub fn png_cost(bytes: &[u8]) -> usize {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let decoded = if bytes.len() >= 24 && bytes.starts_with(SIGNATURE) && &bytes[12..16] == b"IHDR" {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize;
        width.saturating_mul(height).saturating_mul(4)
    } else {
        bytes.len().saturating_mul(4)
    };
    bytes.len().saturating_add(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_fences_count() {
        let block = RichBlock::from(Some("mermaid"), "graph TD\n  A --> B\n\n").unwrap();
        assert_eq!(block.kind, RichKind::Mermaid);
        assert_eq!(block.source, "graph TD\n  A --> B");
        assert_eq!(RichBlock::from(Some("HTML"), "<p>hi</p>").unwrap().kind, RichKind::Html);
        assert_eq!(RichBlock::from(Some("html title=x"), "<p>hi</p>").unwrap().kind, RichKind::Html);
        assert_eq!(RichBlock::from(Some("mermaid{init}"), "graph TD").unwrap().kind, RichKind::Mermaid);
        assert!(RichBlock::from(Some("js"), "x").is_none());
        assert!(RichBlock::from(None, "x").is_none());
        assert!(RichBlock::from(Some("mermaid"), "  \n\n").is_none(), "an empty block stays code");
        assert!(RichBlock::from(Some("html"), &"x".repeat(RichBlock::MAX_SOURCE + 1)).is_none(), "too long to be a reply's");
        assert_eq!(RichBlock::from(Some("mermaid"), "graph TD").unwrap().drawn_title(), "Diagram");
        assert_eq!(RichBlock::from(Some("html"), "<b>x</b>").unwrap().drawn_title(), "Preview");
    }

    #[test]
    fn ids_are_the_swift_apps() {
        // The first ten bytes of SHA-256("mermaid\ngraph TD"), as
        // RichBlock.swift makes them, so both apps find the same pictures.
        let block = RichBlock::new(RichKind::Mermaid, "graph TD");
        let digest = Sha256::digest(b"mermaid\ngraph TD");
        assert_eq!(block.id, digest[..10].iter().map(|b| format!("{b:02x}")).collect::<String>());
        assert_eq!(block.id.len(), 20);
        assert_eq!(block.key(115), format!("{}-115", block.id));
        assert_ne!(block.id, RichBlock::new(RichKind::Html, "graph TD").id, "the kind is part of it");
    }

    #[test]
    fn only_replies_that_could_hold_one_are_looked_at() {
        assert!(may_hold_rich("see:\n```mermaid\ngraph TD\n```"));
        assert!(may_hold_rich("~~~HTML\n<p>\n~~~"));
        assert!(!may_hold_rich("plain words about mermaid"));
        assert!(!may_hold_rich("```rust\nfn main() {}\n```"));
    }

    #[test]
    fn an_agents_page_gets_the_policy_first() {
        let page = html_page("<html><head><script>alert(1)</script></head><body>hi</body></html>");
        assert!(page.starts_with("<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:;"));
        // The policy is ahead of anything the agent wrote.
        assert!(page.find("Content-Security-Policy").unwrap() < page.find("<script>").unwrap());
        assert!(POLICY.contains("default-src 'none'") && POLICY.contains("form-action 'none'") && POLICY.contains("frame-src 'none'") && POLICY.contains("base-uri 'none'"));
        assert!(!POLICY.contains("script-src"), "no script source is allowed, so default-src 'none' covers scripts");
        // Quotes in the policy can't close the attribute early.
        assert!(!escape_attribute("a\"b<c>").contains('"'));
    }

    #[test]
    fn the_source_goes_to_the_draw_script_as_a_string() {
        let hostile = "graph TD\n A[\"</script><script>alert(1)</script>\"] --> B\n`${x}`'\\";
        let call = draw_call(hostile, true, 14.);
        let literal = serde_json::to_string(hostile).unwrap();
        assert!(call.ends_with(&format!("({literal}, true, 14, 2400, 3840000)")), "{call}");
        // The diagram page itself has no script and lets none run.
        let page = diagram_page();
        assert!(!page.contains("<script"));
        assert!(page.contains("default-src 'none'"));
        assert!(page.contains("#out.dark{--pele-bg:#282828"));
    }

    #[test]
    fn the_window_page_fills_its_window() {
        let page = diagram_pane_page("<svg id=d></svg>", "<svg id=l></svg>", "Diagram");
        assert!(page.contains("<title>Diagram</title>"));
        assert!(page.contains("default-src 'none'"), "the window's page is sealed too");
        let fitted = fitted_to_window(&page);
        assert!(fitted.contains(&format!("{FIT_TO_WINDOW}</head>")));
    }

    #[test]
    fn pictures_fit_their_box() {
        // A diagram is never drawn bigger than it is, and fits the room.
        assert_eq!(picture_size(RichKind::Mermaid, (300., 200.), 700., 560., 12.), (300., 200.));
        assert_eq!(picture_size(RichKind::Mermaid, (1352., 400.), 700., 560., 12.), (676., 200.));
        // A page fills the width.
        assert_eq!(picture_size(RichKind::Html, (720., 1000.), 640., 560., 12.), (640., 889.));
    }

    fn png(width: u32, height: u32, extra: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.resize(bytes.len() + extra, 0);
        bytes
    }

    #[test]
    fn a_frame_hands_out_no_more_than_the_budget() {
        let mut frame = FrameBudget::default();
        assert!(frame.hand_out("a", [1, 2], 500, 100), "the first always");
        assert!(!frame.hand_out("b", [3, 4], 1, 100), "nothing more past it");
        assert!(frame.hand_out("c", [1, 2], 500, 100), "the same pictures again");
        // One picture shared, the other not: it costs, and isn't drawn.
        assert!(!frame.hand_out("d", [1, 9], 500, 100));
        assert!(frame.holds(1) && !frame.holds(3) && !frame.holds(9));
        assert!(frame.pins("a") && frame.pins("c") && !frame.pins("b") && !frame.pins("d"));
        frame.end();
        assert!(frame.is_empty() && !frame.pins("a"));
        assert!(frame.hand_out("b", [3, 4], 60, 100));
        assert!(frame.hand_out("e", [5, 6], 40, 100));
        assert!(!frame.hand_out("f", [7, 8], 1, 100));
    }

    #[test]
    fn a_pngs_cost_is_its_bytes_and_its_pixels() {
        let tall = png(1440, 3200, 1000);
        assert_eq!(png_cost(&tall), tall.len() + 1440 * 3200 * 4);
        assert_eq!(png_cost(b"not a png"), 9 * 5);
        assert_eq!(png_cost(&png(u32::MAX, u32::MAX, 0)), usize::MAX, "a header can't overflow it");
    }

    /// Daybreak 92: 121 distinct tall, noisy pages (two 1440 by 3200 looks
    /// of about 18 MB each) kept in memory at once, about 4 GiB by count.
    #[test]
    fn pictures_are_kept_by_what_they_take_up_not_how_many() {
        let budget = PictureBudget::<u32>::BYTES;
        let mut kept = PictureBudget::new(budget);
        let pair = 2 * (18_435_306 + 1440 * 3200 * 4);
        let mut gone = 0;
        for n in 0..121u32 {
            gone += kept.keep(format!("block-{n}"), n, pair).len();
            assert!(kept.used() <= budget, "over the budget after {n}");
        }
        assert_eq!(kept.len(), budget / pair);
        assert_eq!(gone, 121 - kept.len());
        // The newest are what's kept, and one used again stays.
        assert_eq!(kept.get("block-120"), Some(120));
        assert_eq!(kept.get("block-0"), None);
    }

    #[test]
    fn the_least_recently_used_go_first_and_the_newest_always_stays() {
        let mut kept = PictureBudget::new(100);
        assert!(kept.keep("a".into(), 'a', 40).is_empty());
        assert!(kept.keep("b".into(), 'b', 40).is_empty());
        assert_eq!(kept.get("a"), Some('a'));
        assert_eq!(kept.keep("c".into(), 'c', 40), ['b'], "b was used longest ago");
        assert_eq!(kept.keep("c".into(), 'C', 10), ['c'], "the same key again replaces it");
        assert_eq!(kept.used(), 50);
        assert_eq!(kept.keep("huge".into(), 'h', 500), ['a', 'C']);
        assert_eq!((kept.len(), kept.get("huge")), (1, Some('h')));
        assert_eq!(kept.remove("huge"), Some('h'));
        assert_eq!((kept.len(), kept.used(), kept.remove("huge")), (0, 0, None));
        // What's being drawn stays, over the budget if it must.
        assert!(kept.keep("a".into(), 'a', 60).is_empty());
        assert!(kept.keep("b".into(), 'b', 30).is_empty());
        assert_eq!(kept.keep_with("c".into(), 'c', 60, |k| k == "a"), ['b']);
        assert_eq!(kept.keep_with("d".into(), 'd', 60, |k| k == "a" || k == "c"), Vec::<char>::new());
        assert_eq!(kept.used(), 180);
        // Once they're not drawn, back within it.
        assert_eq!(kept.trim_with(|_| false), ['a', 'c'], "under the budget once a and c go");
        assert_eq!(kept.used(), 60);
    }

    /// Blocks drawn alike share their pictures, counted once: six of the
    /// same and one other fit where seven distinct ones wouldn't.
    #[test]
    fn shared_pictures_are_counted_once() {
        let mut kept = PictureBudget::new(100);
        for n in 0..6 {
            assert!(kept.keep_parts_with(format!("a{n}"), n, &[(1, 30), (2, 5)], |_| false).is_empty());
        }
        assert!(kept.keep_parts_with("b".into(), 9, &[(3, 30), (4, 5)], |_| false).is_empty());
        assert_eq!((kept.len(), kept.used()), (7, 70));
        assert!(kept.trim_with(|_| false).is_empty());
        // The part goes only with the last block holding it.
        for n in 0..5 {
            kept.remove(&format!("a{n}"));
        }
        assert_eq!(kept.used(), 70);
        kept.remove("a5");
        assert_eq!(kept.used(), 35);
        // The same key again with the same parts doesn't count them twice.
        kept.keep_parts_with("b".into(), 10, &[(3, 30), (4, 5)], |_| false);
        assert_eq!((kept.len(), kept.used()), (1, 35));
    }
}
