// ```mermaid and ```html blocks in a reply (rich.mermaid, rich.html,
// rich.chrome, rich.fullsize): drawn as what they show, in a box with the
// block's language, a Diagram / Source switch, Copy and full size, as the
// Swift app's RichBlockCell (RichBlocks.swift). The web runtime draws them
// (threadbear_web's rich.rs, sealed as the Swift app's are) one at a time,
// and the pictures are kept on disk in the folder the Swift app keeps its own
// in, under the same names, so a block either app drew shows at once.
//
// Until the runtime's installed a block shows its source with the question
// above it (web_runtime::prompt), and after Not now just its source. A block
// still streaming in stays the kit's code. The pure parts (which fences
// count, the ids, the pages) are threadbear_core::rich_block.
//
// A block plugin of the kit's, as the drafts are: draft_boxes.rs says why
// that, rather than the reply split around the box.
use crate::style::{self, px, size};
use crate::web_runtime::{self, Engine};
use futures::future::{select, Either};
use gpui_kit::base::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use threadbear_core::rich_block::{self as rich, png_cost, FrameBudget, PictureBudget, RichBlock, RichKind};
use threadbear_core::web_runtime::State;

/// What the kit calls the box's nodes.
const NODE: &str = "rich-box";

/// The box's measures, in points at 100% (RichBlockCell's).
const HEADER: f32 = 34.;
const PAD: f32 = 12.;
const RADIUS: f32 = 8.;
/// The tallest a picture is drawn in the reply; full size shows all of it.
const MAX_BODY: f32 = 560.;

/// A drawing given up on after this, its pages dropped (RichRenderer.timeout).
const TIMEOUT: Duration = Duration::from_secs(15);
/// How long Copy shows its tick.
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// Blocks' states kept in memory (sizes only). Their pictures are kept by
/// what they take up (`PictureBudget`).
const STATES_KEPT: usize = 2000;
/// The biggest picture file read back from the disk cache: a page's look is
/// at most 1440 by 3200 pixels, under 20 MB even as noise.
const PICTURE_FILE_MAX: u64 = 40 * 1024 * 1024;

/// The buttons' tips (RichBlockCell.Action.tip), which scripts find them by.
pub(crate) fn drawn_tip(block: &RichBlock) -> &'static str {
    match block.kind {
        RichKind::Mermaid => "Show the diagram",
        RichKind::Html => "Show the page",
    }
}
pub(crate) const SOURCE_TIP: &str = "Show the source";
pub(crate) const COPY_TIP: &str = "Copy the source";
pub(crate) const OPEN_TIP: &str = "Open it full size in its own window";

// ---------------------------------------------------------------- the plugin

/// The kit's block plugin: a closed ```mermaid or ```html fence is drawn as
/// its box; anything else, or one still streaming in, stays the kit's own.
pub(crate) struct RichBoxes;

#[derive(Clone)]
struct RichRef(RichBlock);

impl MarkdownPlugin for RichBoxes {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        NODE
    }

    fn parse(&self, node: &markdown::mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let block = closed_rich(node, cx.source())?;
        let source = cx.node_source(node).unwrap_or_default().to_owned();
        // A selection across the reply copies the block as its fenced source,
        // as the Swift box's copy text is.
        let fence = if block.source.contains("```") { "~~~~" } else { "```" };
        let fenced = format!("{fence}{}\n{}\n{fence}", block.label(), block.source);
        Some(MarkdownNode::new(NODE, RichRef(block.clone())).text(fenced).markdown(source).accessibility_label(format!("{} block", block.label())))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        match node.data::<RichRef>() {
            Some(RichRef(block)) => rich_box(block, window, cx),
            None => div().child(node.as_text().to_owned()).into_any_element(),
        }
    }
}

/// The block a code node is, once its closing fence has come: half a page
/// or diagram stays code until then (MarkdownRenderer.richBlock).
pub(crate) fn closed_rich(node: &markdown::mdast::Node, source: &str) -> Option<RichBlock> {
    let markdown::mdast::Node::Code(code) = node else { return None };
    let block = RichBlock::from(code.lang.as_deref(), &code.value)?;
    let at = code.position.as_ref()?;
    threadbear_core::draft_block::fence_closed(source, at.start.offset, at.end.offset).then_some(block)
}

/// The closed rich blocks a reply holds, in order, wherever they are in it.
pub(crate) fn blocks_in(text: &str) -> Vec<RichBlock> {
    if !rich::may_hold_rich(text) {
        return Vec::new();
    }
    let Ok(tree) = markdown::to_mdast(text, &markdown::ParseOptions::gfm()) else { return Vec::new() };
    let mut found = Vec::new();
    collect(&tree, text, &mut found);
    found
}

fn collect(node: &markdown::mdast::Node, text: &str, found: &mut Vec<RichBlock>) {
    if let Some(block) = closed_rich(node, text) {
        found.push(block);
        return;
    }
    for child in node.children().into_iter().flatten() {
        collect(child, text, found);
    }
}

// ---------------------------------------------------------------- the store

/// Where a block's drawing has got to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Drawing {
    /// Waiting its turn, or for Chromium to start.
    Queued,
    Drawing,
    /// In points at the zoom it was drawn for.
    Drawn { width: f32, height: f32, clipped: bool },
    Failed(String),
}

/// Every block's state, its pictures, the queue of those still to draw, and
/// the full-size windows. One per app.
pub(crate) struct RichStore {
    states: HashMap<String, Drawing>,
    state_order: VecDeque<String>,
    /// Blocks showing their source, by id (RichBlocks.showingSource).
    showing_source: HashSet<String>,
    pictures: PictureBudget<(Arc<Image>, Arc<Image>)>,
    /// Pictures that just left `pictures`, for `release` to take out of
    /// gpui's caches.
    let_go: Vec<Arc<Image>>,
    /// The decoded pictures boxes have drawn, by image id, for `release` to
    /// drop their textures.
    textures: HashMap<u64, Arc<RenderImage>>,
    /// The pictures handed to boxes in the frame being drawn (`FrameBudget`),
    /// and whether the frame's end is waited for.
    in_frame: FrameBudget,
    frame_images: HashMap<u64, Arc<Image>>,
    frame_hooked: bool,
    queue: VecDeque<(String, RichBlock, u32)>,
    /// Asked for while the runtime couldn't draw them: asked again once it can.
    asked: HashSet<String>,
    busy: bool,
    copied: HashMap<String, Instant>,
    /// Open full-size windows, by block id (rich_window.rs).
    pub(crate) windows: HashMap<String, AnyWindowHandle>,
    /// Diagrams' full-size pages drawn this run, by block id (they reach the
    /// disk a moment later).
    window_pages: HashMap<String, String>,
    _subs: Vec<Subscription>,
}

struct Shared(Entity<RichStore>);
impl Global for Shared {}

pub(crate) fn init(cx: &mut App) {
    let store = cx.new(|cx| {
        let mut subs = Vec::new();
        // The runtime installed or started: draw what's waiting.
        if let Some(runtime) = web_runtime::runtime(cx) {
            subs.push(cx.observe(&runtime, |store: &mut RichStore, runtime, cx| {
                let (ready, failed) = {
                    let r = runtime.read(cx);
                    (r.state == State::Ready, if let Engine::Failed(why) = &r.engine { Some(why.clone()) } else { None })
                };
                if ready && !store.asked.is_empty() {
                    store.asked.clear();
                    cx.notify();
                }
                if let Some(why) = failed {
                    store.fail_queued(&why, cx);
                }
                store.pump(cx);
            }));
        }
        RichStore {
            states: HashMap::new(),
            state_order: VecDeque::new(),
            showing_source: HashSet::new(),
            pictures: PictureBudget::new(PictureBudget::<(Arc<Image>, Arc<Image>)>::BYTES),
            let_go: Vec::new(),
            textures: HashMap::new(),
            in_frame: FrameBudget::default(),
            frame_images: HashMap::new(),
            frame_hooked: false,
            queue: VecDeque::new(),
            asked: HashSet::new(),
            busy: false,
            copied: HashMap::new(),
            windows: HashMap::new(),
            window_pages: HashMap::new(),
            _subs: subs,
        }
    });
    cx.set_global(Shared(store));
}

pub(crate) fn store(cx: &App) -> Option<Entity<RichStore>> {
    cx.try_global::<Shared>().map(|s| s.0.clone())
}

impl RichStore {
    fn remember(&mut self, key: String, state: Drawing, cx: &mut App) {
        self.state_order.retain(|k| *k != key);
        self.state_order.push_back(key.clone());
        self.states.insert(key, state);
        while self.state_order.len() > STATES_KEPT {
            let Some(oldest) = self.state_order.pop_front() else { break };
            if matches!(self.states.get(&oldest), Some(Drawing::Queued | Drawing::Drawing)) {
                self.state_order.push_back(oldest);
                break;
            }
            self.states.remove(&oldest);
            if let Some((dark, light)) = self.pictures.remove(&oldest) {
                self.let_go.extend([dark, light]);
            }
        }
        self.release(cx);
    }

    pub(crate) fn state(&self, block: &RichBlock, zoom: u32) -> Option<&Drawing> {
        self.states.get(&block.key(zoom))
    }

    pub(crate) fn shows_source(&self, block: &RichBlock) -> bool {
        self.showing_source.contains(&block.id)
    }

    pub(crate) fn set_shows_source(&mut self, block: &RichBlock, show: bool, cx: &mut Context<Self>) {
        let changed = if show { self.showing_source.insert(block.id.clone()) } else { self.showing_source.remove(&block.id) };
        if changed {
            cx.notify();
        }
    }

    /// A block a box has just shown: drawn already on disk, queued, or
    /// waiting for the runtime.
    fn want(&mut self, block: RichBlock, zoom: u32, cx: &mut Context<Self>) {
        let key = block.key(zoom);
        if self.states.contains_key(&key) {
            return;
        }
        if let Some(drawn) = disk::load_size(&key) {
            self.remember(key, drawn, cx);
            cx.notify();
            return;
        }
        let ready = web_runtime::runtime(cx).is_some_and(|r| r.read(cx).state == State::Ready);
        if !ready {
            self.asked.insert(key);
            return;
        }
        self.remember(key.clone(), Drawing::Queued, cx);
        self.queue.push_back((key, block, zoom));
        cx.notify();
        self.pump(cx);
    }

    fn fail_queued(&mut self, why: &str, cx: &mut Context<Self>) {
        for (key, _, _) in std::mem::take(&mut self.queue) {
            self.remember(key, Drawing::Failed(format!("the web runtime didn't start: {why}")), cx);
        }
        cx.notify();
    }

    /// Draws the next block, if Chromium's up and nothing else is drawing:
    /// one at a time, as the pages are shared.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.queue.is_empty() {
            return;
        }
        let Some(runtime) = web_runtime::runtime(cx) else { return };
        if !runtime.update(cx, |r, cx| r.ensure_engine(cx)) {
            // Starting: the runtime says when it's up (the observer above).
            return;
        }
        let Some((key, block, zoom)) = self.queue.pop_front() else { return };
        self.busy = true;
        self.remember(key.clone(), Drawing::Drawing, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = draw(&block, zoom, cx).await;
            let _ = this.update(cx, |store, cx| store.finish(key, block, result, cx));
        })
        .detach();
    }

    fn finish(&mut self, key: String, block: RichBlock, result: Result<threadbear_web::Drawn, String>, cx: &mut Context<Self>) {
        self.busy = false;
        match result {
            Ok(drawn) => {
                let dark = Arc::new(Image::from_bytes(ImageFormat::Png, drawn.dark.clone()));
                let light = Arc::new(Image::from_bytes(ImageFormat::Png, drawn.light.clone()));
                self.keep_pictures(key.clone(), (dark, light), cx);
                if block.kind == RichKind::Mermaid {
                    self.window_pages.insert(block.id.clone(), drawn.window_page.clone());
                }
                self.remember(key.clone(), Drawing::Drawn { width: drawn.width, height: drawn.height, clipped: drawn.clipped }, cx);
                // Onto disk off the main thread: the next launch shows it at once.
                cx.background_executor().spawn(async move { disk::save(&key, &block.id, &drawn) }).detach();
            }
            Err(why) => self.remember(key, Drawing::Failed(why), cx),
        }
        cx.notify();
        self.pump(cx);
    }

    fn keep_pictures(&mut self, key: String, pictures: (Arc<Image>, Arc<Image>), cx: &mut App) {
        // One allocation for the same picture however many blocks drew it.
        let dark = self.shared(pictures.0);
        // A page that looks the same in both: one allocation for the two.
        let light = if pictures.1.id() == dark.id() { dark.clone() } else { self.shared(pictures.1) };
        let pictures = (dark, light);
        let in_frame = &self.in_frame;
        let parts = [(pictures.0.id(), png_cost(&pictures.0.bytes)), (pictures.1.id(), png_cost(&pictures.1.bytes))];
        for (dark, light) in self.pictures.keep_parts_with(key, pictures, &parts, |key| in_frame.pins(key)) {
            self.let_go.extend([dark, light]);
        }
        self.release(cx);
    }

    /// A drawn block's pictures, read back from disk if they've left memory.
    fn pictures(&mut self, key: &str, cx: &mut App) -> Option<(Arc<Image>, Arc<Image>)> {
        if let Some(found) = self.pictures.get(key) {
            return Some(found);
        }
        let loaded = disk::load_pictures(key)?;
        self.keep_pictures(key.to_owned(), loaded.clone(), cx);
        // The pair as kept: the same allocation as another block drawn alike.
        self.pictures.get(key).or(Some(loaded))
    }

    /// Takes the pictures that left memory out of gpui's caches at once,
    /// wherever they left from (a box needn't be on screen): their decoded
    /// pixels, which gpui keeps until an image is removed, and for one that
    /// was drawn its texture in every window, after the frame being drawn.
    fn release(&mut self, cx: &mut App) {
        let mut dropped = false;
        for picture in std::mem::take(&mut self.let_go) {
            // Still kept under another key (two blocks drawn the same):
            // still wanted.
            if self.pictures.values().any(|(dark, light)| dark.id() == picture.id() || light.id() == picture.id()) {
                continue;
            }
            // Drawn in this frame: let go once it's over (`end_frame`).
            if self.in_frame.holds(picture.id()) {
                self.let_go.push(picture);
                continue;
            }
            if let Some(texture) = self.textures.remove(&picture.id()) {
                cx.defer(move |cx| cx.drop_image(texture, None));
                dropped = true;
            }
            picture.remove_asset(cx);
        }
        // A thread drawn before (its scene kept, not built again) may still
        // show it: the panes watching the store draw again, and a box still
        // there asks for its pictures afresh.
        if dropped {
            if let Some(store) = store(cx) {
                cx.defer(move |cx| store.update(cx, |_, cx| cx.notify()));
            }
        }
    }

    /// A box's pictures for this frame, if they fit in what the frame's
    /// boxes may take up together (the first always does).
    fn hand_out(&mut self, key: &str, pictures: &(Arc<Image>, Arc<Image>)) -> bool {
        let cost = png_cost(&pictures.0.bytes).saturating_add(png_cost(&pictures.1.bytes));
        let fits = self.in_frame.hand_out(key, [pictures.0.id(), pictures.1.id()], cost, FrameBudget::BYTES);
        if fits {
            for picture in [&pictures.0, &pictures.1] {
                self.frame_images.entry(picture.id()).or_insert_with(|| picture.clone());
            }
        }
        fits
    }

    /// The picture already in memory with the same bytes, if there is one
    /// (another block that drew the same, or one drawn this frame), so a
    /// reply of many blocks drawn alike holds one copy, not one each
    /// (Codex's third look at Daybreak 92).
    fn shared(&self, picture: Arc<Image>) -> Arc<Image> {
        let id = picture.id();
        if let Some(found) = self.frame_images.get(&id) {
            return found.clone();
        }
        self.pictures.values().flat_map(|(dark, light)| [dark, light]).find(|p| p.id() == id).cloned().unwrap_or(picture)
    }

    /// The frame's drawn: what was let go during it goes now.
    fn end_frame(&mut self, cx: &mut Context<Self>) {
        self.frame_hooked = false;
        self.in_frame.end();
        self.frame_images.clear();
        // What went over the budget for the frame's pictures comes back
        // within it.
        for (dark, light) in self.pictures.trim_with(|_| false) {
            self.let_go.extend([dark, light]);
        }
        self.release(cx);
    }

    fn copy(&mut self, block: &RichBlock, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(block.source.clone()));
        self.copied.retain(|_, at| at.elapsed() < COPIED_FOR * 4);
        self.copied.insert(block.id.clone(), Instant::now());
        cx.notify();
        // Back to the copy icon once its moment's over.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR + Duration::from_millis(50)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn just_copied(&self, block: &RichBlock) -> bool {
        self.copied.get(&block.id).is_some_and(|at| at.elapsed() < COPIED_FOR)
    }

    /// The page a block's full-size window shows: the agent's html, or the
    /// diagram as it was drawn (from disk, where the drawing put it).
    pub(crate) fn window_page(&self, block: &RichBlock) -> Option<String> {
        match block.kind {
            RichKind::Html => Some(rich::html_page(&block.source)),
            RichKind::Mermaid => self.window_pages.get(&block.id).cloned().or_else(|| disk::load_window_page(&block.id)).map(|page| rich::fitted_to_window(&page)),
        }
    }
}

/// One block drawn, given up on after TIMEOUT (when the shared pages go too,
/// so a stuck one is never waited on again).
async fn draw(block: &RichBlock, zoom: u32, cx: &mut AsyncApp) -> Result<threadbear_web::Drawn, String> {
    // Two pixels a point: the box scales the picture to the column.
    let scale = 2.;
    let drawing = async move {
        match block.kind {
            RichKind::Mermaid => threadbear_web::draw_diagram(&block.source, zoom, scale).await,
            RichKind::Html => threadbear_web::draw_page(&block.source, zoom, scale).await,
        }
    };
    let timer = cx.background_executor().timer(TIMEOUT);
    match select(Box::pin(drawing), Box::pin(timer)).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => {
            threadbear_web::reset_rich();
            Err("it took too long".into())
        }
    }
}

// ---------------------------------------------------------------- on disk

/// The pictures on disk (RichDiskCache in the Swift app, the same folder and
/// names): `<key>-dark.png`, `<key>-light.png`, `<key>.size` with the size in
/// points and whether it was cut, and `page-<id>/index.html` for full size.
mod disk {
    use super::Drawing;
    use gpui_kit::{Image, ImageFormat};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// `THREADBEAR_RICH_CACHE_DIR` for the specs, else the user's Caches.
    pub(super) fn folder() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("THREADBEAR_RICH_CACHE_DIR").filter(|d| !d.is_empty()) {
            return Some(PathBuf::from(dir));
        }
        let home = PathBuf::from(std::env::var_os("HOME").filter(|h| !h.is_empty())?);
        Some(if cfg!(target_os = "macos") { home.join("Library/Caches/Threadbear/rich") } else { home.join(".cache/threadbear/rich") })
    }

    pub(super) fn load_size(key: &str) -> Option<Drawing> {
        let dir = folder()?;
        let line = std::fs::read_to_string(dir.join(format!("{key}.size"))).ok()?;
        let parts: Vec<f32> = line.split_whitespace().filter_map(|p| p.parse().ok()).collect();
        let [width, height, clipped] = parts.as_slice() else { return None };
        let both = dir.join(format!("{key}-dark.png")).is_file() && dir.join(format!("{key}-light.png")).is_file();
        both.then_some(Drawing::Drawn { width: *width, height: *height, clipped: *clipped != 0. })
    }

    pub(super) fn load_pictures(key: &str) -> Option<(Arc<Image>, Arc<Image>)> {
        let dir = folder()?;
        // Only a file no bigger than a page's look can be: the Swift app
        // shares the folder, and it's read whole.
        let read = |look: &str| {
            let file = dir.join(format!("{key}-{look}.png"));
            let meta = std::fs::metadata(&file).ok()?;
            (meta.is_file() && meta.len() <= super::PICTURE_FILE_MAX).then_some(())?;
            std::fs::read(&file).ok().map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
        };
        Some((read("dark")?, read("light")?))
    }

    pub(super) fn load_window_page(id: &str) -> Option<String> {
        std::fs::read_to_string(folder()?.join(format!("page-{id}")).join("index.html")).ok()
    }

    /// Written whole and renamed into place, so the Swift app or the next
    /// launch never reads half a picture, and only this user's: it's a
    /// picture of a reply (`private_file`).
    fn write(path: &PathBuf, bytes: &[u8]) {
        let _ = threadbear_core::private_file::write(path, bytes);
    }

    pub(super) fn save(key: &str, id: &str, drawn: &threadbear_web::Drawn) {
        let Some(dir) = folder() else { return };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        write(&dir.join(format!("{key}-dark.png")), &drawn.dark);
        write(&dir.join(format!("{key}-light.png")), &drawn.light);
        let page = dir.join(format!("page-{id}"));
        if std::fs::create_dir_all(&page).is_ok() {
            write(&page.join("index.html"), drawn.window_page.as_bytes());
        }
        // The size last: it's what says the pictures are there.
        write(&dir.join(format!("{key}.size")), format!("{} {} {}", drawn.width, drawn.height, u8::from(drawn.clipped)).as_bytes());
        prune(&dir);
    }

    /// The newest 900 files kept once there are more than 1200 (RichDiskCache.prune).
    fn prune(dir: &PathBuf) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
        if files.len() <= 1200 {
            return;
        }
        files.sort();
        let excess = files.len() - 900;
        for (_, path) in files.into_iter().take(excess) {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

// ---------------------------------------------------------------- the box

fn dark(cx: &App) -> bool {
    cx.theme().is_dark()
}

/// The switch's track and its thumb (Theme.switchTrack, switchThumb).
fn switch_track(cx: &App) -> Hsla {
    if dark(cx) {
        hsla(0., 0., 1., 0.06)
    } else {
        hsla(0., 0., 0., 0.05)
    }
}

fn switch_thumb(cx: &App) -> Hsla {
    if dark(cx) {
        hsla(0., 0., 1., 0.16)
    } else {
        hsla(0., 0., 1., 1.)
    }
}

/// codeBackground over the transcript as one solid colour, for the fade at
/// a cut-off page's foot (Theme.codeBackgroundOpaque).
fn box_fill_opaque(cx: &App) -> Hsla {
    if dark(cx) {
        rgb(0x282828).into()
    } else {
        rgb(0xf7f7f7).into()
    }
}

/// NSColor.systemOrange, for why a block couldn't be drawn.
fn orange(cx: &App) -> Hsla {
    if dark(cx) {
        rgb(0xff9f0a).into()
    } else {
        rgb(0xff9500).into()
    }
}

/// The box (RichBlockCell): a header with the language, the switch, Copy and
/// full size, and under it the picture or the source.
fn rich_box(block: &RichBlock, window: &mut Window, cx: &mut App) -> AnyElement {
    let Some(store) = store(cx) else { return div().child(block.source.clone()).into_any_element() };
    let zoom = style::zoom_percent();
    let key = block.key(zoom);
    let (state, shows_source, copied, asked) = {
        let s = store.read(cx);
        (s.state(block, zoom).cloned(), s.shows_source(block), s.just_copied(block), s.asked.contains(&key))
    };
    if state.is_none() && !asked {
        // Looked for after this frame: a box only asks as it's drawn.
        let (store, block) = (store.clone(), block.clone());
        cx.defer(move |cx| store.update(cx, |s, cx| s.want(block, zoom, cx)));
    }
    let pictures = match (&state, shows_source) {
        (Some(Drawing::Drawn { .. }), false) => store.update(cx, |s, cx| s.pictures(&key, cx)),
        _ => None,
    };
    // Only as many as the frame's budget holds (`hand_out`); the rest show
    // their source this frame.
    let pictures = pictures.filter(|pair| store.update(cx, |s, _| s.hand_out(&key, pair)));
    // The frame's end, once a frame has handed any out.
    let hook = store.update(cx, |s, _| !s.in_frame.is_empty() && !std::mem::replace(&mut s.frame_hooked, true));
    if hook {
        let store = store.clone();
        window.on_next_frame(move |_, cx| store.update(cx, |s, cx| s.end_frame(cx)));
    }
    let (fill, edge, hairline, secondary) = (style::code_background(cx), style::card_border(cx), style::hairline(cx), style::secondary_label(cx));
    let id = block.id.clone();
    let header = h_flex()
        .h(px(HEADER))
        .flex_none()
        .items_center()
        .pl(px(PAD))
        .pr(px(8.))
        .gap(px(2.))
        .child(div().flex_1().min_w_0().truncate().font_family(cx.theme().mono_font_family.clone()).text_size(px(size::DETAIL)).text_color(secondary).child(block.label()))
        .child(switch(block, shows_source, &store, cx))
        .child(div().w(px(6.)))
        .child(icon_button(format!("rich-copy-{id}"), if copied { "checkmark" } else { "doc.on.doc" }, COPY_TIP, copied, cx, {
            let (store, block) = (store.clone(), block.clone());
            move |_, cx| store.update(cx, |s, cx| s.copy(&block, cx))
        }))
        .child(icon_button(format!("rich-open-{id}"), "arrow.up.left.and.arrow.down.right", OPEN_TIP, false, cx, {
            let block = block.clone();
            move |window, cx| crate::rich_window::open(&block, window, cx)
        }));
    let body = match (&state, pictures) {
        (Some(Drawing::Drawn { width, height, clipped }), Some((dark_picture, light_picture))) => {
            let picture = if dark(cx) { dark_picture } else { light_picture };
            // Its texture, once decoded, for `release` to drop.
            if !store.read(cx).textures.contains_key(&picture.id()) {
                if let Some(texture) = picture.clone().get_render_image(window, cx) {
                    store.update(cx, |s, _| s.textures.insert(picture.id(), texture));
                }
            }
            picture_body(block, (*width, *height), *clipped, picture, cx)
        }
        _ => source_body(block, &state, cx),
    };
    v_flex()
        .id(SharedString::from(format!("rich-box-{id}")))
        .w_full()
        .my(px(4.))
        .rounded(px(RADIUS))
        .bg(fill)
        .border(gpui_kit::px(0.5))
        .border_color(edge)
        .overflow_hidden()
        // Not the start of a selection in the reply around it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(header)
        .child(div().h(gpui_kit::px(0.5)).w_full().bg(hairline))
        .child(body)
        .into_any_element()
}

/// The Diagram (or Preview) / Source switch.
fn switch(block: &RichBlock, shows_source: bool, store: &Entity<RichStore>, cx: &App) -> AnyElement {
    let (label, secondary, track, thumb) = (style::label_text(cx), style::secondary_label(cx), switch_track(cx), switch_thumb(cx));
    let segment = |title: &'static str, tip: &'static str, selected: bool, source: bool| {
        let (store, block) = (store.clone(), block.clone());
        let el = div()
            .id(SharedString::from(format!("rich-{}-{}", if source { "source" } else { "drawn" }, block.id))).role(Role::Button)
            .h(px(18.))
            .px(px(9.))
            .rounded(px(5.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(size::DETAIL))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if selected { label } else { secondary })
            .when(selected, |d| d.bg(thumb))
            .child(title)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| store.update(cx, |s, cx| s.set_shows_source(&block, source, cx)));
        crate::highlights::tipped(el, tip)
    };
    h_flex()
        .h(px(22.))
        .p(px(2.))
        .rounded(px(6.))
        .bg(track)
        .child(segment(block.drawn_title(), drawn_tip(block), !shows_source, false))
        .child(segment("Source", SOURCE_TIP, shows_source, true))
        .into_any_element()
}

/// A grey glyph button in the header: Copy and full size.
fn icon_button(id: String, symbol: &'static str, tip: &'static str, lit: bool, cx: &App, action: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
    let (hover, secondary, fg) = (style::primary_opacity(cx, 0.08), style::secondary_label(cx), cx.theme().foreground);
    let color = if lit { rgb(if dark(cx) { 0x30d158 } else { 0x34c759 }).into() } else { secondary };
    let button = div()
        .id(SharedString::from(id)).role(Role::Button)
        .flex_none()
        .size(px(24.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(color)
        .hover(move |s| s.bg(hover).text_color(fg))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| action(window, cx))
        .child(crate::icons::symbol(symbol).size(px(size::DETAIL)));
    crate::highlights::tipped(button, tip).into_any_element()
}

/// The picture: a diagram at its own size or smaller, centred with room
/// round it; a page filling the width, cut and faded past MAX_BODY.
fn picture_body(block: &RichBlock, natural: (f32, f32), clipped: bool, picture: Arc<Image>, cx: &App) -> AnyElement {
    let (width, height) = natural;
    let ratio = if height > 0. { width / height } else { 1. };
    // The picture fills a frame whose height follows its width: the image's
    // own size is its pixels', which would make the frame twice as tall.
    let image = img(ImageSource::Image(picture)).size_full().object_fit(ObjectFit::Fill);
    match block.kind {
        RichKind::Mermaid => {
            // No wider than it was drawn, nor taller than the box allows.
            let widest = width.min((MAX_BODY - PAD * 2.) * ratio);
            div().w_full().p(px(PAD)).flex().justify_center().child(div().w(px(widest)).max_w_full().aspect_ratio(ratio).child(image)).into_any_element()
        }
        RichKind::Html => {
            let fade = linear_gradient(180., linear_color_stop(box_fill_opaque(cx).opacity(0.), 0.), linear_color_stop(box_fill_opaque(cx), 1.));
            div()
                .relative()
                .w_full()
                .max_h(px(MAX_BODY))
                .overflow_hidden()
                .child(div().w_full().aspect_ratio(ratio).child(image))
                // Cut off: faded into the box, so it's clear there's more.
                .when(clipped || height > MAX_BODY, |d| d.child(div().absolute().left_0().right_0().bottom_0().h(px(48.)).bg(fade)))
                .into_any_element()
        }
    }
}

/// The source, under why it couldn't be drawn or the question about the
/// runtime, if there's one.
fn source_body(block: &RichBlock, state: &Option<Drawing>, cx: &App) -> AnyElement {
    let wants = match block.kind {
        RichKind::Mermaid => "this diagram",
        RichKind::Html => "this page",
    };
    let failure = match state {
        Some(Drawing::Failed(why)) => Some(why.clone()),
        _ => None,
    };
    let code = div()
        .w_full()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(size::CODE))
        .line_height(px(size::CODE_LINE))
        .text_color(style::label_text(cx))
        .whitespace_normal()
        .child(block.source.clone());
    v_flex()
        .w_full()
        .p(px(PAD))
        .gap(px(8.))
        .when_some(failure, |d, why| d.child(div().text_size(px(size::DETAIL)).line_height(px(17.)).text_color(orange(cx)).child(format!("Couldn't draw this: {why}"))))
        .when_some(web_runtime::prompt(&format!("rich-{}", block.id), wants, cx), |d, prompt| d.child(prompt))
        .child(code)
        .into_any_element()
}

// ---------------------------------------------------------------- the dump

/// A reply's rich blocks as drawn, for the specs (`rich` on a row): its
/// language, whether the picture or the source shows, where its drawing has
/// got to, and whether it's asking about the runtime.
pub(crate) fn dump(text: &str, cx: &App) -> Option<Value> {
    let blocks = blocks_in(text);
    if blocks.is_empty() {
        return None;
    }
    let store = store(cx)?;
    let s = store.read(cx);
    let zoom = style::zoom_percent();
    let asks = web_runtime::runtime(cx).is_some_and(|r| r.read(cx).asks());
    let out: Vec<Value> = blocks
        .iter()
        .map(|block| {
            let state = s.state(block, zoom);
            let drawn = matches!(state, Some(Drawing::Drawn { .. }));
            let mut item = json!({
                "kind": block.label(),
                "shows": if drawn && !s.shows_source(block) { "drawn" } else { "source" },
                "state": match state {
                    Some(Drawing::Queued) => "queued".to_owned(),
                    Some(Drawing::Drawing) => "drawing".to_owned(),
                    Some(Drawing::Drawn { .. }) => "drawn".to_owned(),
                    Some(Drawing::Failed(why)) => format!("failed: {why}"),
                    None => "waiting".to_owned(),
                },
                "asks": asks && !drawn,
                "copy": if s.just_copied(block) { "Copied" } else { "Copy" },
            });
            if let Some(Drawing::Drawn { width, height, .. }) = state {
                item["size"] = json!(format!("{}x{}", width.round(), height.round()));
            }
            item
        })
        .collect();
    Some(json!(out))
}

/// The full-size windows open, for the dump's `richWindows`.
pub(crate) fn windows_dump(cx: &App) -> Value {
    let Some(store) = store(cx) else { return json!([]) };
    let ids: Vec<&String> = store.read(cx).windows.keys().collect();
    json!(ids)
}

/// A transcript's watch on the boxes' store and the runtime: it's a cached
/// view, so it only draws again when told.
pub(crate) fn watch(cx: &mut Context<crate::thread::ThreadPane>) -> Vec<Subscription> {
    let mut subs = Vec::new();
    if let Some(store) = store(cx) {
        subs.push(cx.observe(&store, |pane, _, cx| pane.rich_changed(cx)));
    }
    if let Some(runtime) = web_runtime::runtime(cx) {
        subs.push(cx.observe(&runtime, |pane, _, cx| pane.rich_changed(cx)));
    }
    subs
}

/// Every reply's boxes measured again: a drawing landing, or the switch,
/// changes how tall its row is.
impl crate::thread::ThreadPane {
    pub(crate) fn rich_changed(&mut self, cx: &mut Context<Self>) {
        let rows: Vec<usize> = self
            .feed
            .model()
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(&row.content, threadbear_core::reducer::RowContent::Assistant(item) if rich::may_hold_rich(&item.text)))
            .map(|(i, _)| i)
            .collect();
        if rows.is_empty() {
            return;
        }
        self.scroller.update(cx, |s, cx| {
            for i in rows {
                s.remeasure_items(i..i + 1, cx);
            }
        });
        cx.notify();
    }
}
