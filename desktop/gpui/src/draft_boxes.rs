// Drafts in a reply (rich.draft, rich.draft.copy): a ```slack, ```email or
// ```draft fence the agent wrote for Mike to send somewhere else is drawn as
// the draft it is, in a box with its label, every line where the agent put
// it, and Copy (MarkdownRenderer.draftBlock, DraftBlockCell.swift and
// CopiedBubble.swift). What a draft says, and how Slack's formatting reads, is
// threadbear_core::draft_block; this is the kit's block plugin that finds a
// closed fence, the box itself, and Copy, which puts HTML on the clipboard
// beside the plain text so a paste into Slack or Mail keeps the bold and the
// links.
//
// A block plugin rather than the reply split around the box: the kit's own
// parse finds the fence wherever it is (in a list, in a quote, as the Swift
// renderer does), streaming text reparses only its tail as it does for every
// reply, and a selection across the reply still takes the draft in, as its
// plain text. The box's own words can be selected too (`DraftText`), with
// the bar over them and ⌘C, as the Swift box's lines are the reply's text.
use crate::open_gate::{self, Origin};
use crate::rows::metrics::CODE_PAD;
use crate::style::{self, size};
use gpui_kit::base::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin};
use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::component::{h_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use threadbear_core::draft_block::{self, DraftBlock, Line};

/// The Copy button's tip (DraftBlockCell.tip), which scripts find it by.
pub(crate) const COPY_TIP: &str = "Copy the draft, with its links and formatting";

/// What the kit calls the box's nodes.
const NODE: &str = "draft-box";

/// The header's height (DraftBlockCell.height), the room under it before the
/// line across the box, and from that line to the draft's first line.
const HEADER: f32 = 22.;
const UNDER_HEADER: f32 = 6.;
const OVER_LINES: f32 = 8.;

/// How long Copy says Copied (DraftBlockCell.copiedFor), and how long the
/// bubble over it lasts, popping in, waiting and fading (CopiedBubble.shown).
const COPIED_FOR: Duration = Duration::from_millis(1500);
const BUBBLE_FOR: Duration = Duration::from_millis(1100);

/// The kit's block plugin: a closed draft fence is drawn as its box; any
/// other code block, or a draft still streaming in, stays the kit's own.
pub(crate) struct DraftBoxes {
    /// Whose thread it is, which decides where its links may open.
    pub(crate) origin: Origin,
    pub(crate) cwd: String,
    /// The row it's in, so a script can find a box's Copy.
    pub(crate) row: String,
}

#[derive(Clone)]
struct DraftRef(DraftBlock);

impl MarkdownPlugin for DraftBoxes {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        NODE
    }

    fn parse(&self, node: &markdown::mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let draft = closed_draft(node, cx.source())?;
        let source = cx.node_source(node).unwrap_or_default().to_owned();
        // A selection across the reply copies the draft's words, as the
        // Swift header copies as nothing and its lines as text.
        Some(MarkdownNode::new(NODE, DraftRef(draft.clone())).text(draft.plain_text()).markdown(source).accessibility_label(draft.label()))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        match node.data::<DraftRef>() {
            Some(DraftRef(draft)) => draft_box(draft, node.source_range().map_or(0, |r| r.start), self, window, cx),
            None => div().child(node.as_text().to_owned()).into_any_element(),
        }
    }
}

/// The draft a code block is, once its closing fence has come: one still
/// streaming in stays code, as half a draft mustn't be something to copy.
pub(crate) fn closed_draft(node: &markdown::mdast::Node, source: &str) -> Option<DraftBlock> {
    let markdown::mdast::Node::Code(code) = node else { return None };
    let draft = DraftBlock::from(code.lang.as_deref(), &code.value)?;
    let at = code.position.as_ref()?;
    draft_block::fence_closed(source, at.start.offset, at.end.offset).then_some(draft)
}

// ---------------------------------------------------------------- Copied

/// When each draft was last copied, by its words, and that click's number
/// (for its bubble's animation), so the button says Copied for a moment.
/// Read as the box draws, as DraftBlockCell's is.
static COPIED: LazyLock<Mutex<HashMap<u64, (Instant, usize)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn copied(source: &str) -> Option<(Duration, usize)> {
    let map = COPIED.lock().ok()?;
    map.get(&key(source)).map(|(at, serial)| (at.elapsed(), *serial))
}

fn note_copied(source: &str) {
    let Ok(mut map) = COPIED.lock() else { return };
    let serial = map.values().map(|(_, s)| *s).max().unwrap_or(0) + 1;
    // Only the last few matter: Copied lasts a second and a half.
    map.retain(|_, (at, _)| at.elapsed() < COPIED_FOR * 4);
    map.insert(key(source), (Instant::now(), serial));
}

/// Puts the draft on the clipboard as plain text and as HTML, so a paste
/// into Slack or Mail keeps its links and bold (DraftBlockCell.copyDraft).
#[cfg(target_os = "macos")]
fn put_on_clipboard(draft: &DraftBlock, _cx: &mut App) {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    // SAFETY: AppKit's own constants, there for as long as the app runs.
    let (html, string) = unsafe { (NSPasteboardTypeHTML, NSPasteboardTypeString) };
    let _ = board.setString_forType(&NSString::from_str(&draft.html()), html);
    let _ = board.setString_forType(&NSString::from_str(&draft.plain_text()), string);
}

/// Elsewhere the plain text for now: GPUI's clipboard has no HTML flavour,
/// and Windows' and Linux's need code of their own (features.md, rich.draft.copy).
#[cfg(not(target_os = "macos"))]
fn put_on_clipboard(draft: &DraftBlock, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(draft.plain_text()));
}

/// The HTML on the clipboard, for scripts (`clipboard` with `"type": "html"`).
#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) fn clipboard_html() -> Option<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML};
    // SAFETY: as above.
    let html = unsafe { NSPasteboardTypeHTML };
    NSPasteboard::generalPasteboard().stringForType(html).map(|s| s.to_string())
}

#[cfg(all(feature = "automation", not(target_os = "macos")))]
pub(crate) fn clipboard_html() -> Option<String> {
    None
}

fn copy(draft: &DraftBlock, window: &mut Window, cx: &mut App) {
    put_on_clipboard(draft, cx);
    note_copied(&draft.source);
    // Copied straight away, and back to Copy once it's had its moment.
    window.refresh();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(COPIED_FOR + Duration::from_millis(50)).await;
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

// ---------------------------------------------------------------- the box

/// NSColor.systemGreen, for Copied and the bubble's tick.
fn system_green(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        rgb(0x30d158).into()
    } else {
        rgb(0x34c759).into()
    }
}

/// The box (MarkdownRenderer.draftBlock): its label and Copy, a line across
/// it, then the draft's lines, in a rounded box like a code block's.
fn draft_box(draft: &DraftBlock, at: usize, plugin: &DraftBoxes, window: &mut Window, cx: &mut App) -> AnyElement {
    let (fill, edge, secondary) = (style::code_background(cx), style::card_border(cx), style::secondary_label(cx));
    let base = format!("{}-{at}", plugin.row);
    let header = h_flex()
        .h(px(HEADER))
        .items_center()
        .justify_between()
        .child(div().text_size(px(size::DETAIL)).line_height(px(HEADER)).font_weight(FontWeight::MEDIUM).text_color(secondary).child(draft.label()))
        .child(copy_button(draft, &base, window, cx));
    let lines = draft.drawn_lines();
    // The line under the header, across the box: half a point, one pixel on
    // a Retina screen; on a 1x one, where half a pixel comes to nothing, a
    // whole one at half the strength. The lines' spacing gives its room back.
    let retina = window.scale_factor() >= 2.;
    let rule = gpui_kit::px(if retina { 0.5 } else { 1. });
    let rule_color = if retina { edge } else { edge.opacity(0.5) };
    // Its own selection, and somewhere for the keys to go once a drag starts
    // in its words, so ⌘C copies them (the thread pane's copy, selection_bar.rs).
    let selection = window
        .use_keyed_state(ElementId::Name(format!("draft-selection-{base}").into()), cx, |_, cx| {
            let handle = TextSelectionHandle::new(draft.plain_text(), cx);
            let focus = cx.focus_handle();
            let keys = focus.clone();
            handle.focus_with(
                move |window, cx| {
                    keys.focus(window, cx);
                    crate::web_panels::keyboard_to_app(window);
                },
                cx,
            );
            DraftSelection { handle, focus }
        })
        .read(cx)
        .clone();
    let body = div()
        .w_full()
        .px(px(CODE_PAD))
        .pt(px(OVER_LINES) - rule)
        .pb(px(CODE_PAD))
        .text_size(px(size::BODY))
        .line_height(px(size::BODY_LINE))
        .child(DraftText::new(format!("draft-text-{base}"), &lines, plugin, selection.handle.clone(), cx));
    div()
        .track_focus(&selection.focus)
        .w_full()
        .rounded(px(8.))
        .bg(fill)
        .border(gpui_kit::px(0.5))
        .border_color(edge)
        .child(div().px(px(CODE_PAD)).pt(px(CODE_PAD)).pb(px(UNDER_HEADER)).child(header))
        .child(div().w_full().h(rule).bg(rule_color))
        .child(body)
        .into_any_element()
}

/// A Slack quote's bar, before its words.
const QUOTE_BAR: &str = "\u{2502} ";

/// A box's own selection: its handle in the window's selection, and where
/// the keys go once a drag starts in its words.
#[derive(Clone)]
struct DraftSelection {
    handle: TextSelectionHandle,
    focus: FocusHandle,
}

/// The draft's lines as one piece of text, kept as the agent wrote them,
/// with Slack's formatting or an email's bare addresses as links: one piece,
/// so a drag across them selects them, a double click a word, and ⌘C copies
/// them, as the reply's words around the box do (Mike, 12 Nov: "I can't
/// select the text inside of here?"). A blank line keeps its height. Its
/// `code` is monospaced on its pill's colour, at the text's size, as one
/// piece of text has one size.
///
/// The kit takes a box in a reply as one thing that a selection either
/// covers or doesn't, and its window-wide selection orders what it covers
/// by when each reply's text was drawn, which a box's text drawn inside its
/// reply can't take a place in. So a drag that starts in these words keeps
/// to them, and one that starts outside takes the box whole, as before.
struct DraftText {
    id: ElementId,
    text: SharedString,
    inner: InteractiveText,
    /// The inner text's layout, shared with it, for the selection's runs.
    layout: TextLayout,
    handle: TextSelectionHandle,
}

impl DraftText {
    fn new(id: String, lines: &[Line], plugin: &DraftBoxes, handle: TextSelectionHandle, cx: &App) -> Self {
        // Opaque colours: GPUI blends a highlight's colour into the run's
        // (style::over_background).
        let (link, quoted, bar) = (style::link_text(cx), style::over_background(style::secondary_label(cx), cx), style::over_background(style::tertiary_label(cx), cx));
        let (code_bg, mono) = (style::inline_code_background(cx), cx.theme().mono_font_family.clone());
        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        let mut code: Vec<(Range<usize>, SharedString)> = Vec::new();
        let mut links: Vec<(Range<usize>, String)> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            if line.quote {
                // The bar of a Slack quote, fainter than its words.
                highlights.push((text.len()..text.len() + QUOTE_BAR.len(), HighlightStyle { color: Some(bar), ..Default::default() }));
                text.push_str(QUOTE_BAR);
            }
            for span in &line.spans {
                let range = text.len()..text.len() + span.text.len();
                text.push_str(&span.text);
                let mut look = HighlightStyle::default();
                if line.quote {
                    look.color = Some(quoted);
                }
                if span.bold {
                    look.font_weight = Some(FontWeight::SEMIBOLD);
                }
                if span.italic {
                    look.font_style = Some(FontStyle::Italic);
                }
                if span.strike {
                    look.strikethrough = Some(StrikethroughStyle { thickness: gpui_kit::px(1.), color: None });
                }
                if span.code {
                    look.background_color = Some(code_bg);
                    code.push((range.clone(), mono.clone().into()));
                }
                if let Some(url) = &span.link {
                    look.color = Some(link);
                    links.push((range.clone(), url.clone()));
                }
                if look != HighlightStyle::default() && !range.is_empty() {
                    highlights.push((range, look));
                }
            }
        }
        let styled = StyledText::new(text.clone()).with_highlights(highlights).with_font_family_overrides(code);
        let layout = styled.layout().clone();
        // Its links go where the reply's do: through the open gate, as this
        // thread's (a web page in the browser, a mail address asks first).
        let (origin, cwd) = (plugin.origin, plugin.cwd.clone());
        let ranges: Vec<Range<usize>> = links.iter().map(|(r, _)| r.clone()).collect();
        let urls: Vec<String> = links.into_iter().map(|(_, u)| u).collect();
        let id = ElementId::Name(id.into());
        let inner = InteractiveText::new(id.clone(), styled).on_click(ranges, move |ix, _, cx| {
            if let Some(url) = urls.get(ix) {
                open_gate::open_link(url, origin, &cwd, cx);
            }
        });
        DraftText { id, text: text.into(), inner, layout, handle }
    }
}

impl IntoElement for DraftText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for DraftText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        self.inner.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(&mut self, id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Hitbox {
        let hitbox = self.inner.prepaint(id, inspector_id, bounds, &mut (), window, cx);
        let mine = self.handle.entity_id();
        let dragging_here = self.handle.snapshot(cx).is_some_and(|s| s.is_selecting() && s.anchor().entity_id() == Some(mine));
        let registration = if !dragging_here && crate::reply_links::left_held() {
            // A selection being made from outside the box: out of its way,
            // where nothing is, so the reply's text takes the box whole.
            let away = Bounds::new(point(bounds.left(), gpui_kit::px(1e7)), Size::default());
            TextSelectionRegistration::new(window.insert_hitbox(away, HitboxBehavior::Normal), away)
        } else {
            let own = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            if dragging_here {
                // A drag that started here stays with these words wherever
                // the pointer goes, as the reply's own text stays with it.
                window.capture_pointer(own.id);
            }
            // The window's selection picks the smallest text under the
            // pointer: as good as no size, so it's these words, never the
            // reply they're in or another under a drag that's left them.
            // Where they are is what counts for the points it hands back.
            TextSelectionRegistration::new(own, Bounds::new(bounds.origin, gpui_kit::size(gpui_kit::px(1.), gpui_kit::px(1.)))).with_text_bounds(vec![bounds])
        };
        let registration = registration.with_rendered_element(&self.handle, window, cx);
        self.handle.register(registration, window, cx);
        hitbox
    }

    fn paint(&mut self, id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), hitbox: &mut Hitbox, window: &mut Window, cx: &mut App) {
        let projection = self.handle.update_runs(&[TextSelectionRun::new(self.text.clone(), self.layout.clone(), bounds)], cx);
        let color = cx.theme().selection;
        for range in projection.ranges().iter().flatten() {
            paint_selection(&self.layout, range.clone(), color, window);
        }
        // Words to select: the I-beam, as over the reply's (its links say
        // otherwise as they paint).
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
        self.inner.paint(id, inspector_id, bounds, &mut (), hitbox, window, cx);
        #[cfg(feature = "automation")]
        PAINTED.with(|p| p.borrow_mut().insert(self.id.to_string(), (self.text.clone(), self.layout.clone())));
    }
}

#[cfg(feature = "automation")]
thread_local! {
    /// Each box's words as last painted, by its element's name, for a script
    /// to find where words are (`drag_select`). One a box, so it stays small.
    static PAINTED: std::cell::RefCell<HashMap<String, (SharedString, TextLayout)>> = std::cell::RefCell::new(HashMap::new());
}

/// `select-text` with `"draft": true`: selects `words` in one of the row's
/// draft boxes as a person would, pressing at the first and dragging to the
/// last, through real pointer events and hit testing. Says where the press
/// and the release were, for the bar.
#[cfg(feature = "automation")]
pub(crate) fn drag_select(row: &str, words: &str, window: &mut Window, cx: &mut App) -> Result<(Point<Pixels>, Point<Pixels>), String> {
    use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
    PAINTED.with(|p| p.borrow_mut().clear());
    window.render_frame(cx);
    let prefix = format!("draft-text-{row}-");
    let found = PAINTED.with(|p| p.borrow().iter().find(|(name, (text, _))| name.contains(&prefix) && text.contains(words)).map(|(_, found)| found.clone()));
    let (text, layout) = found.ok_or_else(|| format!("no draft box in {row} shows {words:?}"))?;
    let start = text.find(words).unwrap_or(0);
    let (Some(first), Some(last)) = (layout.position_for_index(start), layout.position_for_index(start + words.len())) else {
        return Err(format!("{words:?} isn't laid out in {row}'s draft"));
    };
    // Into the first letter and short of the end of the last, on their line.
    let half = layout.line_height() / 2.;
    let (from, to) = (point(first.x + gpui_kit::px(1.), first.y + half), point(last.x - gpui_kit::px(1.), last.y + half));
    window.pointer_move(from, None, cx);
    window.pointer_down(from, MouseButton::Left, cx);
    window.render_frame(cx);
    window.pointer_move(to, Some(MouseButton::Left), cx);
    window.render_frame(cx);
    window.pointer_up(to, MouseButton::Left, cx);
    window.render_frame(cx);
    Ok((from, to))
}

/// The selected part of the text, on its lines: the first from where it
/// starts, any between whole, the last to where it ends.
fn paint_selection(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
    let (Some(start), Some(end)) = (layout.position_for_index(range.start), layout.position_for_index(range.end)) else { return };
    let (area, line) = (layout.bounds(), layout.line_height());
    let mut quads = Vec::new();
    if start.y == end.y {
        quads.push(Bounds::from_corners(start, point(end.x, end.y + line)));
    } else {
        quads.push(Bounds::from_corners(start, point(area.right(), start.y + line)));
        if end.y > start.y + line {
            quads.push(Bounds::from_corners(point(area.left(), start.y + line), point(area.right(), end.y)));
        }
        quads.push(Bounds::from_corners(point(area.left(), end.y), point(end.x, end.y + line)));
    }
    for quad in quads {
        window.paint_quad(fill(quad, color));
    }
}

/// How wide `text` is at `font_size` in the window's font, for the button
/// that's as wide as its widest word.
fn text_width(text: &str, font_size: f32, window: &Window) -> Pixels {
    let style = window.text_style();
    let run = TextRun { len: text.len(), font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None };
    window.text_system().shape_line(text.to_owned().into(), px(font_size), &[run], None).width
}

/// Copy at the header's right (DraftBlockCell.button): the icon and its
/// word, as wide as "Copied" so it doesn't move when it changes, reaching a
/// little into the box's padding. Green with a tick for a moment after a
/// click (Mike, 7 Nov), with the bubble over it.
fn copy_button(draft: &DraftBlock, base: &str, window: &mut Window, cx: &App) -> AnyElement {
    let shown = copied(&draft.source);
    let just = shown.is_some_and(|(since, _)| since < COPIED_FOR);
    let color = if just { system_green(cx) } else { style::secondary_label(cx) };
    let width = gpui_kit::px(style::zoomed(14. + 5. + 12.)) + text_width("Copied", style::size::DETAIL, window).ceil();
    let icon = if just { crate::icons::tall_checkmark() } else { crate::icons::symbol("doc.on.doc") }.size(px(11.)).text_color(color);
    let draft = draft.clone();
    let button = div()
        .id(ElementId::Name(format!("draft-copy-{base}").into()))
        .relative()
        .flex_none()
        .w(width)
        // Out past the header into the box's padding: the Swift button's
        // frame runs to the box's edge and 6 past it, so "Copied" ends where
        // the box does.
        .mr(px(-6. - CODE_PAD))
        .h(px(20.))
        .pl(px(6.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(div().w(px(14.)).flex().justify_center().child(icon))
        .child(div().ml(px(5.)).text_size(px(size::DETAIL)).line_height(px(20.)).text_color(color).child(if just { "Copied" } else { "Copy" }))
        // Not the start of a selection in the reply around it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| copy(&draft, window, cx))
        .children(shown.filter(|(since, _)| *since < BUBBLE_FOR).map(|(_, serial)| copied_bubble(serial, cx)));
    crate::highlights::tipped(button, COPY_TIP).into_any_element()
}

/// The little "Copied to clipboard" bubble with a green tick that pops up
/// over Copy (CopiedBubble.swift): the button changing alone was easy to
/// miss (Mike, 7 Nov). It rises a little as it appears, waits and fades.
/// Its right edge is the box's, as the Swift one is kept inside the reply.
fn copied_bubble(serial: usize, cx: &App) -> AnyElement {
    let (background, edge, label, green) = (cx.theme().background, style::card_border(cx), style::label_text(cx), system_green(cx));
    let tick = div().size(px(12.)).rounded_full().bg(green).flex().items_center().justify_center().child(crate::icons::symbol("checkmark").size(px(8.)).text_color(gpui_kit::white()));
    // As tall as its words, as the Swift bubble's stack view comes out: its
    // insets only show left and right.
    let bubble = h_flex()
        .gap(px(5.))
        .pl(px(8.))
        .pr(px(10.))
        .rounded(px(8.))
        .bg(background)
        .border(gpui_kit::px(0.5))
        .border_color(edge)
        .shadow(vec![BoxShadow::new(gpui_kit::px(0.), px(1.), hsla(0., 0., 0., 0.18)).blur_radius(px(6.))])
        .whitespace_nowrap()
        .child(tick)
        .child(div().text_size(px(size::DETAIL)).line_height(px(style::natural_line(size::DETAIL))).font_weight(FontWeight::MEDIUM).text_color(label).child("Copied to clipboard"));
    // Over the button, 4 above it (the button is 20 tall), its right edge
    // the box's, 6 in from the button's.
    let above = 20. + 4.;
    let total = BUBBLE_FOR.as_secs_f32();
    div()
        .absolute()
        .right(px(6.))
        .bottom(px(above))
        .child(bubble)
        .with_animation(("draft-copied", serial), Animation::new(BUBBLE_FOR), move |el, delta| {
            let t = delta * total;
            // A 4-point rise over 0.18 s, eased out; in over 0.12 s, out over the last 0.25 s.
            let rise = 1. - ease_out_cubic((t / 0.18).min(1.));
            let shown = (t / 0.12).min(1.) * if t > total - 0.25 { (1. - (t - (total - 0.25)) / 0.25).max(0.) } else { 1. };
            el.bottom(px(above - 4. * rise)).opacity(shown)
        })
        .into_any_element()
}

fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

// ---------------------------------------------------------------- scripts

/// What a reply's drafts show, for the dump (both apps' `drafts`): each one's
/// label, its lines as drawn, its formatted pieces, and what Copy says. Found
/// with the same parser and the same check the box is drawn by.
#[cfg(feature = "automation")]
pub(crate) fn dump(text: &str) -> Option<serde_json::Value> {
    use serde_json::json;
    if !DraftBlock::may_hold(text) {
        return None;
    }
    let source = crate::rows::markdown::prepare(text);
    let root = markdown::to_mdast(&source, &markdown::ParseOptions::gfm()).ok()?;
    let mut drafts = Vec::new();
    collect(&root, &source, &mut drafts);
    let shown: Vec<serde_json::Value> = drafts
        .iter()
        .map(|draft| {
            let lines = draft.drawn_lines();
            let copy = if copied(&draft.source).is_some_and(|(since, _)| since < COPIED_FOR) { "Copied" } else { "Copy" };
            json!({
                "label": draft.label(),
                "lines": lines.iter().map(Line::shown).collect::<Vec<_>>(),
                "styled": lines.iter().flat_map(|l| l.spans.iter().filter_map(draft_block::Span::described)).collect::<Vec<_>>(),
                "copy": copy,
            })
        })
        .collect();
    (!shown.is_empty()).then(|| json!(shown))
}

#[cfg(feature = "automation")]
fn collect(node: &markdown::mdast::Node, source: &str, out: &mut Vec<DraftBlock>) {
    if let Some(draft) = closed_draft(node, source) {
        out.push(draft);
        return;
    }
    for child in node.children().into_iter().flatten() {
        collect(child, source, out);
    }
}

/// `{"cmd":"draft","row":<id>,"index":0,"copy":true}`: presses the row's
/// `index`-th draft's Copy as a click on it would, through real pointer
/// events and hit testing; without `copy`, how many drafts the row shows.
/// With no `row`, the newest reply with a draft.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    use serde_json::json;
    use threadbear_core::reducer::RowContent;
    let prefix = cmd["row"].as_str().map(str::to_owned);
    let row = app
        .read(cx)
        .pane
        .as_ref()
        .and_then(|p| {
            let rows = p.read(cx).feed.model().rows();
            match &prefix {
                Some(prefix) => rows.iter().find(|r| r.id.starts_with(prefix)),
                None => rows.iter().rev().find(|r| matches!(&r.content, RowContent::Assistant(item) if dump(&item.text).is_some())),
            }
            .map(|r| r.id.clone())
        })
        .ok_or_else(|| prefix.map_or("no reply with a draft".to_owned(), |p| format!("no row starting {p:?}")))?;
    // On screen first, as a person would see it before pressing.
    app.update(cx, |t, cx| t.update_pane(cx, |p, cx| p.scroll_command(None, Some(&row), 40., cx)))??;
    window.render_frame(cx);
    let mine = format!("draft-copy-{row}-");
    let buttons: Vec<_> = window
        .find_all_by_label(COPY_TIP)
        .into_iter()
        .filter(|e| e.visible() && e.path().iter().any(|id| matches!(id, ElementId::Name(name) if name.starts_with(&mine))))
        .collect();
    if cmd["copy"].as_bool() != Some(true) {
        return Ok(json!(format!("ok {}", buttons.len())));
    }
    let index = cmd["index"].as_u64().unwrap_or(0) as usize;
    let Some(button) = buttons.get(index) else { return Err(format!("no draft {index} in {row}: it has {}", buttons.len())) };
    let at = button.bounds().center();
    window.pointer_move(at, None, cx);
    window.pointer_down(at, MouseButton::Left, cx);
    window.pointer_up(at, MouseButton::Left, cx);
    Ok(json!("ok copied"))
}

#[cfg(test)]
mod tests {
    // By name: the kit's prelude has a `test` of its own.
    use super::closed_draft;

    /// The drafts the kit's own parser finds in `text`, as the plugin sees
    /// its blocks (the Swift app's DraftBlockRenderTests cases).
    fn drafts(text: &str) -> Vec<String> {
        fn walk(node: &markdown::mdast::Node, text: &str, out: &mut Vec<String>) {
            if let Some(draft) = closed_draft(node, text) {
                out.push(draft.source);
                return;
            }
            for child in node.children().into_iter().flatten() {
                walk(child, text, out);
            }
        }
        let mut out = Vec::new();
        walk(&markdown::to_mdast(text, &markdown::ParseOptions::gfm()).unwrap(), text, &mut out);
        out
    }

    #[test]
    fn a_closed_draft_fence_is_a_box_wherever_it_is() {
        assert_eq!(drafts("Here's the post:\n\n```slack\n*Threadbear, 6 Nov*\n• one\n• two\n```\n\nPost it?"), ["*Threadbear, 6 Nov*\n• one\n• two"]);
        for text in [
            "> ```slack\n> hello\n> ```",
            "Here:\r\n\r\n```slack\r\nhello\r\n```\r\n",
            "- ```slack\n  hello\n  ```",
            "```slack\none\u{2028}two\n```",
            "> - ```slack\n>   héllo\n>   ```",
            "  ```slack\n  hello\n```",
            "1. ```slack\n   hello\n   ```",
            ">```slack\n>hello\n>```",
            "- parent\n  - child\n\n    ```slack\n    hello\n    ```",
        ] {
            assert_eq!(drafts(text).len(), 1, "{text:?}");
        }
    }

    #[test]
    fn one_still_streaming_in_or_not_a_draft_stays_code() {
        // Cross-lab review, 6 Nov: a closing line has to match the opening one.
        for text in ["```slack\n• one\n• tw", "```text\n• one\n```", "```slack\nhello\n~~~", "````slack\nhello\n```", "```slack\nhello\n> ```"] {
            assert!(drafts(text).is_empty(), "{text:?}");
        }
        assert_eq!(drafts("````slack\nhello\n`````").len(), 1);
    }
}
