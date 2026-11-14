// Chips in the composer's words besides a picture's, and copying them: a
// Notion card or another thread among the words is drawn as its chip
// (CardChips.swift's CardAttachment), a thread's id pasted or dropped on its
// own becomes that thread's chip, and a copy or cut of chips keeps their
// pictures, so a paste brings them back (ComposerTextView's writeSelection,
// readSelection and Clip). What's a mention, a thread's id and a clip is
// threadbear_core::inline_chips; this puts them in the box and draws them.
//
// A card's or thread's chip is an inline token whose text is the very link it
// stands for, so the draft, a save and a plain copy all read the link, as the
// Swift draft does. A picture's is the U+FFFC mark (composer.rs).
//
// The clip goes on the clipboard as the words the agent would get, with the
// draft and its pictures beside them in GPUI's own clipboard note (on the
// Mac `zed-metadata`, kept with a hash of the words, so a copy from anywhere
// else since leaves no stale note), where the Swift app has a private type.
use crate::composer::{Composer, PictureChip};
use crate::style::{self, size};
use gpui_kit::component::input::{InlineToken, InlineTokenContext, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::path::PathBuf;
use threadbear_core::chips::{Mention, MentionKind};
use threadbear_core::inline_chips::{self as inline, BoxChip, Clip, DraftPiece};
use threadbear_core::reducer::ImageRef;

/// A card or thread chip in the box: what it's called, and which.
#[derive(Clone)]
pub(crate) struct MentionChip {
    pub(crate) title: String,
    pub(crate) kind: MentionKind,
}

/// The chip's measures at the composer's 15 pt (PictureChip.Metrics): the
/// symbol in a 14-point box 3 in, 5 to its 13 pt label, 7 after it, and a
/// card's title cut short past 230 (CardChip.maxLabel).
const ICON: f32 = 14.;
const INSET: f32 = 3.;
const GAP: f32 = 5.;
const TRAILING: f32 = 7.;
const RADIUS: f32 = 6.;
const LABEL: f32 = 13.;
const LABEL_MAX: f32 = 230.;

/// A thread being dragged in the app: let go over a thread, it goes into
/// that thread's message as its chip, as pasting its id does (ThreadDrag).
/// The sidebar's rows start it (`drag.threadout`); while it's dragged it's
/// drawn as the chip it'll be.
#[derive(Clone)]
pub(crate) struct DraggedThread {
    pub(crate) id: String,
    pub(crate) title: String,
}

impl Render for DraggedThread {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.title.is_empty() { "Untitled".to_owned() } else { self.title.clone() };
        mention_chip_body(&title, MentionKind::Thread, false, cx)
    }
}

/// A chip's body: its symbol and label on the chips' fill, with a hairline
/// edge, the accent one while it's selected (CardChip.draw).
fn mention_chip_body(title: &str, kind: MentionKind, selected: bool, cx: &App) -> Div {
    let secondary = style::secondary_label(cx);
    let edge = if selected { style::accent(cx) } else { style::hairline(cx) };
    // A speech bubble for a thread, a card for a Notion card.
    let symbol = if kind == MentionKind::Thread { "bubble.left" } else { "square.text.square" };
    div()
        .flex_none()
        .h(px(size::CHIP_HEIGHT))
        // CardChip.draw strokes its half-point edge inside the chip; here
        // the edge takes room, so the padding gives it back.
        .pl(px(INSET - 0.5))
        .pr(px(TRAILING - 0.5))
        .flex()
        .items_center()
        .gap(px(GAP))
        .rounded(px(RADIUS))
        .bg(style::chip_fill(cx))
        .border(gpui_kit::px(0.5))
        .border_color(edge)
        // SF's symbols fill their box; Lucide's draw a little inside it.
        .child(div().size(px(ICON)).flex().items_center().justify_center().child(crate::icons::symbol(symbol).size(px(13.)).text_color(secondary)))
        .child(div().max_w(px(LABEL_MAX)).truncate().whitespace_nowrap().text_size(px(LABEL)).line_height(px(16.)).text_color(secondary).child(title.to_owned()))
}

impl Composer {
    /// Whether a token in the box is a card's or thread's chip.
    pub(crate) fn is_mention_token(id: &SharedString) -> bool {
        id.starts_with("mention-")
    }

    /// A mention's token, kept with its title and kind. Its text is the link
    /// itself, as it was written, so the draft holds it as it is.
    fn mention_token(&mut self, mention: &Mention, link: &str) -> InlineToken {
        self.next_picture += 1;
        let id: SharedString = format!("mention-{}", self.next_picture).into();
        self.mentions.insert(id.clone(), MentionChip { title: mention.title.clone(), kind: mention.kind });
        // A label with nothing in it isn't a token the box takes.
        let label = if mention.title.trim().is_empty() { "Link".to_owned() } else { mention.title.clone() };
        InlineToken::new(id, link.to_owned()).with_label(label)
    }

    /// The box's chips by range, as the draft reads them.
    pub(crate) fn box_chips(&self, cx: &App) -> Vec<(std::ops::Range<usize>, BoxChip)> {
        self.box_chips_in(&self.input, cx)
    }

    /// The same for `target`: the box, or a Something else box.
    pub(crate) fn box_chips_in(&self, target: &Entity<TextareaState>, cx: &App) -> Vec<(std::ops::Range<usize>, BoxChip)> {
        target
            .read(cx)
            .tokens()
            .iter()
            .map(|span| {
                let id = span.token().id();
                let chip = if Self::is_mention_token(id) { BoxChip::Mention } else { BoxChip::Picture(self.pictures.get(id).and_then(|c| c.image.clone())) };
                (span.range(), chip)
            })
            .collect()
    }

    /// A card's or thread's chip among the words (CardChipCell): its symbol
    /// and title. A click puts the cursor beside it, as on a word.
    pub(crate) fn render_mention_chip(&self, ctx: &InlineTokenContext, cx: &App) -> AnyElement {
        let chip = self.mentions.get(ctx.token().id());
        let (title, kind) = chip.map_or_else(|| (ctx.token().label().to_string(), MentionKind::Card), |c| (c.title.clone(), c.kind));
        div().h_full().flex().items_center().child(mention_chip_body(&title, kind, ctx.is_selected(), cx)).into_any_element()
    }

    /// The text and tokens for a draft shown in the box (a send refused, an
    /// edit, a draft kept from before): each mark a chip for its picture,
    /// each card or thread linked in the words a chip of its own
    /// (ComposerTextView.show).
    pub(crate) fn content_for(&mut self, text: &str, images: &[ImageRef]) -> gpui_kit::component::input::InputContent {
        let mut words = String::new();
        let mut tokens: Vec<(std::ops::Range<usize>, InlineToken)> = Vec::new();
        for piece in inline::draft_pieces(text, images.len()) {
            match piece {
                DraftPiece::Words(w) => words.push_str(&w),
                DraftPiece::Picture(index) => {
                    let image = images[index].clone();
                    let id = self.fresh_picture_id();
                    let start = words.len();
                    words.push_str(threadbear_core::composer::PICTURE_MARK_STR);
                    tokens.push((start..words.len(), crate::composer::picture_token(&id)));
                    self.pictures.insert(id, PictureChip { source: PathBuf::from(&image.path), image: Some(image) });
                }
                DraftPiece::Mention(mention, link) => {
                    let token = self.mention_token(&mention, &link);
                    let start = words.len();
                    words.push_str(token.text());
                    tokens.push((start..words.len(), token));
                }
            }
        }
        let mut content = gpui_kit::component::input::InputContent::new(words);
        for (range, token) in tokens {
            content = match content.clone().with_token(range, token) {
                Ok(content) => content,
                Err(_) => content,
            };
        }
        content
    }

    /// Pieces in place of the selection, as one paste puts them
    /// (ComposerTextView.readSelection): words as words, pictures already
    /// in as their chips, cards and threads as theirs.
    pub(crate) fn insert_pieces(&mut self, pieces: Vec<DraftPiece>, images: &[ImageRef], window: &mut Window, cx: &mut Context<Self>) {
        let target = self.input.clone();
        self.insert_pieces_in(&target, pieces, images, window, cx);
    }

    pub(crate) fn insert_pieces_in(&mut self, target: &Entity<TextareaState>, pieces: Vec<DraftPiece>, images: &[ImageRef], window: &mut Window, cx: &mut Context<Self>) {
        let mut tokens: Vec<Option<InlineToken>> = Vec::new();
        for piece in &pieces {
            tokens.push(match piece {
                DraftPiece::Words(_) => None,
                DraftPiece::Picture(index) => {
                    let image = images[*index].clone();
                    let id = self.fresh_picture_id();
                    self.pictures.insert(id.clone(), PictureChip { source: PathBuf::from(&image.path), image: Some(image) });
                    Some(crate::composer::picture_token(&id))
                }
                DraftPiece::Mention(mention, link) => Some(self.mention_token(mention, link)),
            });
        }
        let selected = !target.read(cx).selected_range().is_empty();
        target.update(cx, |s, cx| {
            if selected {
                s.replace("", window, cx);
            }
            for (piece, token) in pieces.into_iter().zip(tokens) {
                match (piece, token) {
                    (DraftPiece::Words(words), _) => s.insert(words, window, cx),
                    (_, Some(token)) => {
                        let _ = s.replace_with_token(token, window, cx);
                    }
                    _ => {}
                }
            }
            s.focus(window, cx);
            crate::web_panels::keyboard_to_app(window);
        });
        cx.notify();
    }

    /// What a paste of words makes chips of, before the box takes it as
    /// words: a clip of the box's own with its pictures, a known thread's id
    /// on its own, or words with a card or thread linked in them. True when
    /// it's taken.
    pub(crate) fn take_clipboard_chips(&mut self, target: &Entity<TextareaState>, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(words) = item.text() else { return false };
        // The box's own copy, its pictures beside the words.
        let note = item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::String(s) => s.metadata_json::<serde_json::Value>(),
            _ => None,
        });
        if let Some(clip) = note.and_then(|note| Clip::from_clipboard(&words, &note)) {
            let pieces = inline::draft_pieces(&clip.text, clip.images.len());
            self.insert_pieces_in(target, pieces, &clip.images, window, cx);
            return true;
        }
        // "Copy thread id" pasted back: the thread's chip, with its title.
        if let Some(reference) = self.thread_reference_for(&words, cx) {
            self.insert_pieces_in(target, inline::draft_pieces(&reference, 0), &[], window, cx);
            return true;
        }
        if inline::has_mentions(&words) {
            self.insert_pieces_in(target, inline::draft_pieces(&words, 0), &[], window, cx);
            return true;
        }
        false
    }

    /// The link a paste naming a known thread on its own goes in as.
    fn thread_reference_for(&self, pasted: &str, cx: &App) -> Option<String> {
        let app = self.app.upgrade()?;
        let threads = &app.read(cx).threads;
        let id = inline::pasted_thread_id(pasted, |id| threads.contains_key(id))?;
        Some(inline::thread_reference(&id, threads.get(&id).map_or("", |t| t.title.as_str())))
    }

    /// ⌘C or ⌘X with chips in the selection (writeSelection): the words as
    /// the agent would get them, with the pictures beside them when there
    /// are any, and a cut takes the selection out. False with no chips in
    /// it, for the box's own copy.
    pub(crate) fn copy_chips(&mut self, cut: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let selection = self.input.read(cx).selected_range();
        if selection.is_empty() {
            return false;
        }
        let chips = self.box_chips(cx);
        if !chips.iter().any(|(range, _)| range.start < selection.end && range.end > selection.start) {
            return false;
        }
        let text = self.input.read(cx).value().to_string();
        let (draft, images) = inline::draft_in(&text, &chips, selection);
        if images.is_empty() {
            // Cards alone copy as their links.
            cx.write_to_clipboard(ClipboardItem::new_string(draft));
        } else {
            let clip = Clip::new(draft, images);
            cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(clip.plain.clone(), clip.metadata()));
        }
        if cut {
            self.input.update(cx, |s, cx| s.replace("", window, cx));
            cx.notify();
        }
        true
    }

    /// A thread let go over the box, or anywhere on the thread (ThreadDrag):
    /// its chip where the cursor is, and the cursor stays here to carry on.
    pub(crate) fn take_thread_drop(&mut self, drag: &DraggedThread, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.input.clone();
        self.caret_to_drop(&target, at, cx);
        let reference = inline::thread_reference(&drag.id, &drag.title);
        self.insert_pieces(inline::draft_pieces(&reference, 0), &[], window, cx);
    }

    /// The box's chips as the automation's `composer-chips` lists them:
    /// "picture: Picture 1", "card: Fix the button", "thread: Morning brief".
    #[cfg(feature = "automation")]
    pub(crate) fn chip_descriptions(&self, cx: &App) -> Vec<String> {
        let mut number = 0;
        self.input
            .read(cx)
            .tokens()
            .iter()
            .map(|span| {
                let id = span.token().id();
                match self.mentions.get(id) {
                    Some(chip) => format!("{}: {}", if chip.kind == MentionKind::Thread { "thread" } else { "card" }, chip.title),
                    None => {
                        number += 1;
                        format!("picture: Picture {number}")
                    }
                }
            })
            .collect()
    }
}

/// The automation's `composer-clip` (both apps): the whole box copied or
/// cut, or what's on the clipboard pasted, as ⌘A then ⌘C or ⌘X, and ⌘V, do.
#[cfg(feature = "automation")]
pub(crate) fn clip_command(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use gpui_kit::test::TestWindowExt as _;
    let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
    let input = composer.read(cx).input.clone();
    let handle = input.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
    crate::web_panels::keyboard_to_app(window);
    window.render_frame(cx);
    match cmd["action"].as_str().unwrap_or("") {
        action @ ("copy" | "cut") => {
            input.update(cx, |s, cx| s.select_all(window, cx));
            window.press(if action == "cut" { "secondary-x" } else { "secondary-c" }, cx);
        }
        "paste" => window.press("secondary-v", cx),
        other => return Err(format!("composer-clip takes copy, cut or paste, not {other:?}")),
    }
    window.render_frame(cx);
    Ok(serde_json::json!("ok"))
}

/// The automation's `composer-chips`: the box's chips, in order.
#[cfg(feature = "automation")]
pub(crate) fn chips_command(app: &Entity<crate::app::Threadbear>, cx: &mut App) -> Result<serde_json::Value, String> {
    let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
    let chips = composer.read(cx).chip_descriptions(cx);
    Ok(serde_json::json!(format!("ok {}", serde_json::to_string(&chips).unwrap_or_default())))
}
