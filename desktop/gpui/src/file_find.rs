// The find bar over a text file in the pane, a port of what the Swift app's
// text view gets from AppKit (`usesFindBar`, TextFileView.swift): ⌘F puts a
// bar over the text with the words to find, ‹ and › for the match before and
// after (⇧⌘G and ⌘G, Shift-Return and Return in the field) and Done
// (Escape). The field also says how many matches there are, which AppKit's
// bar leaves out.
//
// The code editor finds and marks its own matches (gpui-kit's search, driven
// from here rather than its own panel, so the bar looks like AppKit's); the
// drawn Markdown page is searched with threadbear_core::find and its matches
// marked as range highlights on the page.
use crate::files::{FileBody, OpenFile};
use crate::highlights::{icon, palette, tipped, SidePane};
use crate::style::{self, px};
use gpui_kit::base::{RangeHighlight, RenderedText, TextViewState};
use gpui_kit::component::input::{EditorState, Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use threadbear_core::find::{self as core_find, Finder};

actions!(file_preview, [FindNext, FindPrevious]);

/// The key context of a file in the pane, which ⌘F, ⌘G and ⇧⌘G work in.
pub(crate) const CONTEXT: &str = "FilePreview";

/// The keys, bound once for every window. ⌘F is the kit's own Search action:
/// a code editor that isn't `searchable` passes it up to here.
pub(crate) fn bind_keys(cx: &mut App) {
    static BOUND: std::sync::Once = std::sync::Once::new();
    // ⌘ on the Mac, Control elsewhere, as the app's other keys are.
    let mac = cfg!(target_os = "macos");
    BOUND.call_once(|| {
        cx.bind_keys([
            KeyBinding::new(if mac { "cmd-f" } else { "ctrl-f" }, gpui_kit::component::input::Search, Some(CONTEXT)),
            KeyBinding::new(if mac { "cmd-g" } else { "ctrl-g" }, FindNext, Some(CONTEXT)),
            KeyBinding::new(if mac { "cmd-shift-g" } else { "ctrl-shift-g" }, FindPrevious, Some(CONTEXT)),
        ]);
    });
}

/// The bar, while it's up over a file.
pub(crate) struct FindBar {
    pub(crate) input: Entity<InputState>,
    /// The drawn page's matches; the code editor keeps its own.
    finder: Finder,
    /// The page's text when it was last searched: a page that changes (its
    /// parse landing, a reload) is searched again, and the highlights set
    /// here, which tell the page to draw again, aren't taken for a change.
    searched: Option<RenderedText>,
    _subs: Vec<Subscription>,
}

/// What the bar searches: the code editor (or the Markdown text), or the
/// drawn page.
#[derive(Clone)]
enum Target {
    Editor(Entity<EditorState>),
    Page(Entity<TextViewState>),
}

fn target(open: &OpenFile) -> Option<Target> {
    if let Some(editor) = open.raw_editor() {
        return Some(Target::Editor(editor));
    }
    match &open.body {
        FileBody::Text { editor: Some(editor), .. } => Some(Target::Editor(editor.clone())),
        FileBody::Markdown { doc, .. } => Some(Target::Page(doc.clone())),
        _ => None,
    }
}

/// The editor's search, as the bar shows it: the match picked and how many.
fn editor_count(editor: &Entity<EditorState>, cx: &App) -> (Option<usize>, usize) {
    let session = editor.read(cx).search_session();
    (session.matcher.current(), session.matcher.len())
}

impl SidePane {
    /// ⌘F: the bar over the file, its words picked to type over, or put in
    /// front again if it's up.
    pub(crate) fn open_find(&mut self, thread_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.files.open.get(thread_id) else { return };
        if target(open).is_none() {
            return;
        }
        if open.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx));
            let id = thread_id.to_owned();
            let sub = cx.subscribe_in(&input, window, move |this, _, ev: &InputEvent, _, cx| match ev {
                InputEvent::Change => this.find_changed(&id, cx),
                InputEvent::PressEnter { shift, .. } => this.find_step(&id, *shift, cx),
                _ => {}
            });
            let query = self.files.last_query.clone();
            input.update(cx, |s, cx| s.set_value(query, window, cx));
            if let Some(open) = self.files.open.get_mut(thread_id) {
                open.find = Some(FindBar { input, finder: Finder::default(), searched: None, _subs: vec![sub] });
            }
            self.find_changed(thread_id, cx);
        }
        if let Some(bar) = self.files.open.get(thread_id).and_then(|o| o.find.as_ref()) {
            bar.input.update(cx, |s, cx| {
                s.focus(window, cx);
                crate::web_panels::keyboard_to_app(window);
                s.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// The words changed: find them again, from the match picked.
    pub(crate) fn find_changed(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(open) = self.files.open.get_mut(thread_id) else { return };
        let Some(target) = target(open) else { return };
        let Some(bar) = open.find.as_mut() else { return };
        let query = bar.input.read(cx).value().to_string();
        self.files.last_query = query.clone();
        match target {
            Target::Editor(editor) => editor.update(cx, |s, cx| {
                if query.is_empty() {
                    s.close_search(cx);
                } else {
                    s.set_search_query(query, true, cx);
                }
            }),
            Target::Page(doc) => {
                let from = bar.finder.current_range().map_or(0, |r| r.start);
                let rendered = doc.read(cx).rendered_text();
                bar.finder.search(rendered.as_str(), &query, from);
                bar.searched = Some(rendered);
                let finder = bar.finder.clone();
                mark_page(&doc, &finder, cx);
            }
        }
        cx.notify();
    }

    /// Return, ⌘G, ›; with `back`, Shift-Return, ⇧⌘G, ‹.
    pub(crate) fn find_step(&mut self, thread_id: &str, back: bool, cx: &mut Context<Self>) {
        let Some(open) = self.files.open.get_mut(thread_id) else { return };
        let Some(target) = target(open) else { return };
        let Some(bar) = open.find.as_mut() else { return };
        match target {
            Target::Editor(editor) => editor.update(cx, |s, cx| {
                if back {
                    s.previous_search_match(cx);
                } else {
                    s.next_search_match(cx);
                }
            }),
            Target::Page(doc) => {
                if back {
                    bar.finder.previous();
                } else {
                    bar.finder.next();
                }
                let finder = bar.finder.clone();
                mark_page(&doc, &finder, cx);
            }
        }
        cx.notify();
    }

    /// ⌘G or ⇧⌘G with the bar down finds the last words again, as AppKit's
    /// do from the find pasteboard, putting the bar up to say where.
    pub(crate) fn find_again(&mut self, thread_id: &str, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let up = self.files.open.get(thread_id).is_some_and(|o| o.find.is_some());
        if !up {
            if self.files.last_query.is_empty() {
                return;
            }
            self.open_find(thread_id, window, cx);
        }
        self.find_step(thread_id, back, cx);
    }

    /// Done or Escape: the bar goes, its marks with it, and the text has the
    /// keys again.
    pub(crate) fn close_find(&mut self, thread_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.files.open.get_mut(thread_id) else { return };
        let searched = target(open);
        if open.find.take().is_none() {
            return;
        }
        match searched {
            // The keys go back to the text: the editor's own, or the file's.
            Some(Target::Editor(editor)) => editor.update(cx, |s, cx| {
                s.close_search(cx);
                s.focus(window, cx);
                crate::web_panels::keyboard_to_app(window);
            }),
            Some(Target::Page(doc)) => {
                doc.update(cx, |s, cx| s.clear_range_highlights(cx));
                if let Some(focus) = &self.files.focus {
                    window.focus(focus, cx);
                    crate::web_panels::keyboard_to_app(window);
                }
            }
            None => {}
        }
        cx.notify();
    }

    /// The drawn page changed under the bar (its parse landed, the file was
    /// read again): the same words found in what it says now.
    pub(crate) fn page_changed(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(open) = self.files.open.get_mut(thread_id) else { return };
        let Some(Target::Page(doc)) = target(open) else { return };
        let Some(bar) = open.find.as_mut() else { return };
        let rendered = doc.read(cx).rendered_text();
        if bar.searched.as_ref() == Some(&rendered) {
            return;
        }
        bar.finder.refresh(rendered.as_str());
        bar.searched = Some(rendered);
        let finder = bar.finder.clone();
        mark_page(&doc, &finder, cx);
        cx.notify();
    }

    /// The bar's words and where the picked match is among the matches, for the
    /// count in the field and the dump.
    fn find_state(&self, thread_id: &str, cx: &App) -> Option<(String, Option<usize>, usize)> {
        let open = self.files.open.get(thread_id)?;
        let bar = open.find.as_ref()?;
        let query = bar.input.read(cx).value().to_string();
        Some(match target(open)? {
            Target::Editor(editor) if !query.is_empty() => {
                let (current, total) = editor_count(&editor, cx);
                (query, current, total)
            }
            Target::Editor(_) => (query, None, 0),
            Target::Page(_) => (query, bar.finder.current, bar.finder.matches.len()),
        })
    }

    /// `dump`'s `previewFind`: "the: 2 of 8", "the: not found", while the bar is up.
    pub(crate) fn find_line(&self, thread_id: &str, cx: &App) -> Option<String> {
        self.find_state(thread_id, cx).map(|(query, current, total)| core_find::dump_line(&query, current, total))
    }

    /// The scripted `find`: the bar up with `query` typed in it.
    pub(crate) fn find_typed(&mut self, thread_id: &str, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(thread_id, window, cx);
        let Some(bar) = self.files.open.get(thread_id).and_then(|o| o.find.as_ref()) else { return };
        let input = bar.input.clone();
        // Setting the value says nothing to the subscription, as typing does.
        input.update(cx, |s, cx| s.set_value(query.to_owned(), window, cx));
        self.find_changed(thread_id, cx);
    }

    /// The bar, over the text (`NSTextFinder`'s, measured on the Swift app's
    /// pictures): a 30 point strip with a hairline under it, the field taking
    /// the room, then ‹ › and Done.
    pub(crate) fn render_find_bar(&self, thread_id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.files.open.get(thread_id)?.find.as_ref()?;
        let p = palette(cx);
        let input = bar.input.clone();
        let (query, current, total) = self.find_state(thread_id, cx).unwrap_or_default();
        let count = core_find::count_label(&query, current, total);
        let has_matches = total > 0;
        let id = thread_id.to_owned();
        let field_fill = style::primary_opacity(cx, 0.07);
        let field = h_flex()
            .flex_1()
            .min_w_0()
            .h(px(21.))
            .pl(px(7.))
            .pr(px(4.))
            .gap(px(4.))
            .rounded_full()
            .bg(field_fill)
            .child(icon("magnifyingglass", 12., p.secondary).flex_none())
            .child(div().flex_1().min_w_0().child(Input::new(&input).appearance(false).small().cleanable(true).pl(px(0.)).text_size(px(style::size::LABEL))))
            .when(!count.is_empty(), |s| s.child(div().flex_none().text_size(px(style::size::SMALL)).text_color(p.secondary).child(count)));
        let (back_id, next_id, done_id) = (id.clone(), id.clone(), id.clone());
        let step = |this_id: &'static str, symbol: &'static str, tip: &'static str, back: bool, thread: String, cx: &mut Context<Self>| {
            let hover = p.hover;
            tipped(
                div()
                    .id(this_id)
                    .w(px(16.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(has_matches, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                    .child(icon(symbol, 12., if has_matches { p.fg } else { p.faint })),
                tip,
            )
            .when(has_matches, |s| s.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.find_step(&thread, back, cx))))
        };
        let steps = h_flex()
            .flex_none()
            .h(px(21.))
            .rounded(px(6.))
            .overflow_hidden()
            .bg(field_fill)
            .child(step("find-previous", "chevron.left", "Find the match before (⇧⌘G)", true, back_id, cx))
            .child(div().w(gpui_kit::px(1.)).h(px(12.)).bg(p.hairline))
            .child(step("find-next", "chevron.right", "Find the match after (⌘G)", false, next_id, cx));
        let done = tipped(
            div()
                .id("find-done")
                .flex_none()
                .h(px(21.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .bg(field_fill)
                .cursor_pointer()
                .active(|s| s.opacity(0.7))
                .text_size(px(style::size::LABEL))
                .text_color(p.fg)
                .child("Done"),
            "Close the find bar (Escape)",
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.close_find(&done_id, window, cx)));
        let escape_id = id.clone();
        Some(
            h_flex()
                .id("find-bar")
                .flex_none()
                .w_full()
                .h(px(31.))
                .pl(px(6.))
                .pr(px(8.))
                .gap(px(7.))
                .items_center()
                .border_b_1()
                .border_color(p.hairline)
                // Escape in the field closes the bar, as AppKit's does.
                .capture_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_find(&escape_id, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(field)
                .child(steps)
                .child(done)
                .into_any_element(),
        )
    }
}

/// Every match on the page faintly, the picked one as a selection is, and
/// that one scrolled to: the code editor's look for its own matches.
fn mark_page(doc: &Entity<TextViewState>, finder: &Finder, cx: &mut App) {
    let selection = cx.theme().selection;
    let faint = Hsla { s: 0.1, ..selection };
    let current = finder.current_range();
    let marks: Vec<RangeHighlight> = finder.matches.iter().map(|m| RangeHighlight::new(m.clone(), if Some(m) == current.as_ref() { selection } else { faint })).collect();
    doc.update(cx, |s, cx| {
        if marks.is_empty() {
            s.clear_range_highlights(cx);
        } else {
            let _ = s.set_range_highlights(marks, cx);
        }
        if let Some(range) = current {
            let _ = s.reveal_range(range, cx);
        }
    });
}
