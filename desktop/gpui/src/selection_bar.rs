// The little bar over words selected in a message (rich.selectionbar,
// rich.explain, rich.copy): Explain, which opens the pane beside the thread
// (explain_pane.rs), Quote, into the composer, and Copy, as SelectionBar.swift
// and TextKit's selection handling have them; and ⌘C on a selection, which
// puts HTML beside the words so a paste into Slack or Mail keeps its links
// (SelectionHTML, threadbear_core::selection_copy).
//
// The kit's window-wide selection says what's selected, not where, so the bar
// goes by where the press and the release were: over the selection's first
// line, starting where it starts. A press on the bar ends the kit's selection
// (a press anywhere starts a new one), so what the bar acts on is read when
// it shows, not when it's pressed.
use crate::thread::ThreadPane;
use crate::style::{self, size};
use gpui_kit::base::{SelectionFormat, TextSelection};
use gpui_kit::component::{h_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::cell::RefCell;
use threadbear_core::reducer::{Row, RowContent};
use threadbear_core::selection_copy;

/// Between the bar and the line it's over (SelectionBarWindow.gap).
const GAP: f32 = 6.;

/// A press in a message's words: where it went down, in which row.
struct Press {
    thread: String,
    row: String,
    at: Point<Pixels>,
}

/// The bar while it's up.
struct Bar {
    thread: String,
    /// The whole message the words are in, for Explain.
    message: Option<String>,
    /// What's selected, as plain words and as HTML, read when it showed.
    words: String,
    html: String,
    /// Where the selection starts and ends, as the press and release were.
    first: Point<Pixels>,
    last: Point<Pixels>,
    /// Where it was drawn, so a press on it doesn't put it away.
    drawn: Option<Bounds<Pixels>>,
    /// The selection's ends it was last placed over, when its words said.
    placed: Option<(Point<Pixels>, Point<Pixels>)>,
}

thread_local! {
    static PRESS: RefCell<Option<Press>> = const { RefCell::new(None) };
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

/// What the bar can do (SelectionBarWindow.Action).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Explain,
    Quote,
    Copy,
}

impl Action {
    pub(crate) fn from_name(name: &str) -> Option<Action> {
        match name {
            "explain" => Some(Action::Explain),
            "quote" => Some(Action::Quote),
            "copy" => Some(Action::Copy),
            _ => None,
        }
    }
}

/// The message a row is, for Explain: a reply's or Mike's words. Rows that
/// aren't a message (tools, notes, footers) get no bar.
fn message_of(row: &Row) -> Option<String> {
    match &row.content {
        RowContent::Assistant(item) => Some(item.text.clone()),
        RowContent::User(message) if !message.shows_as_note() => Some(message.agent_header().map_or_else(|| message.text.clone(), |h| h.body)),
        _ => None,
    }
}

/// A left press on a row (rows.rs's row div): the start of a selection,
/// maybe. It doesn't stop the press; the kit's selection takes it as usual.
pub(crate) fn pressed(thread: String, row: &Row) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    let (row_id, is_message) = (row.id.clone(), message_of(row).is_some());
    move |event: &MouseDownEvent, _, _| {
        PRESS.with(|p| *p.borrow_mut() = is_message.then(|| Press { thread: thread.clone(), row: row_id.clone(), at: event.position }));
    }
}

/// Watches the window's presses, releases and scrolls for the bar: a
/// release ends a selection, which shows the bar once the kit has settled
/// what's selected; a press away from the bar or a scroll puts it away.
/// Every button, wherever it is, as a release can come anywhere after a drag.
pub(crate) fn watch(pane: WeakEntity<ThreadPane>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            follow_paused_while_selecting(pane.clone(), bounds, window);
            let released = pane.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase.capture() && event.button == MouseButton::Left {
                    let (pane, release) = (released.clone(), event.position);
                    window.defer(cx, move |window, cx| selection_ended(&pane, release, window, cx));
                }
            });
            let pressed = pane.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                if phase.capture() && !on_bar(event.position) {
                    hide(&pressed, cx);
                }
            });
            let scrolled = pane.clone();
            window.on_mouse_event(move |_: &ScrollEvent, phase, _, cx| {
                if phase.capture() {
                    hide(&scrolled, cx);
                }
            });
        },
    )
    // Over the whole pane, though it draws nothing and takes no clicks: its
    // listeners hear every press and release in the window.
    .absolute()
    .inset_0()
}

/// While the button's down on a selection begun in this pane's words, the
/// transcript stops following new lines, so a working thread's words don't
/// scroll away under a slow drag; let go and it catches up, if it was
/// following when the drag began (Mike's pick, 12 Nov, "Pause while
/// selecting"). A selection left on screen after doesn't hold it.
fn follow_paused_while_selecting(pane: WeakEntity<ThreadPane>, bounds: Bounds<Pixels>, window: &mut Window) {
    let pressed = pane.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase.capture() && event.button == MouseButton::Left && bounds.contains(&event.position) && TextSelection::is_selectable_at(event.position, window, cx) {
            let _ = pressed.update(cx, |pane, cx| {
                let paused = pane.scroller.update(cx, |list, _| list.pause_for_selection());
                pane.selection_paused_follow |= paused;
            });
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase.capture() && event.button == MouseButton::Left {
            let _ = pane.update(cx, |pane, cx| {
                if std::mem::take(&mut pane.selection_paused_follow) {
                    pane.scroller.update(cx, |list, cx| list.scroll_to_end(cx));
                }
            });
        }
    });
}

/// Where the window's selection starts and ends, as its words last drew it:
/// each end's caret, at the middle of its line.
fn painted_ends(window: &Window, cx: &App) -> Option<(Point<Pixels>, Point<Pixels>)> {
    TextSelection::painted_edges(window, cx).map(|(start, end)| (point(start.left(), start.center().y), point(end.left(), end.center().y)))
}

fn on_bar(at: Point<Pixels>) -> bool {
    BAR.with(|b| b.borrow().as_ref().and_then(|b| b.drawn).is_some_and(|d| d.contains(&at)))
}

fn hide(pane: &WeakEntity<ThreadPane>, cx: &mut App) {
    if BAR.with(|b| b.borrow_mut().take()).is_some() {
        let _ = pane.update(cx, |_, cx| cx.notify());
    }
}

/// A release after a press in a message: if it left words selected, the
/// bar comes up over them.
fn selection_ended(pane: &WeakEntity<ThreadPane>, release: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let Some(press) = PRESS.with(|p| p.borrow_mut().take()) else { return };
    let Some(entity) = pane.upgrade() else { return };
    if entity.read(cx).summary.id != press.thread || !TextSelection::has_selection(window, cx) {
        return;
    }
    let message = entity.read(cx).feed.model().rows().iter().find(|r| r.id == press.row).and_then(message_of);
    if message.is_none() {
        return;
    }
    let Some((words, html)) = selection_now(&entity, window, cx) else { return };
    // The selection runs from whichever end is first in reading order.
    let same_line = (press.at.y - release.y).abs() < gpui_kit::px(style::zoomed(size::BODY_LINE) / 2.);
    let press_first = if same_line { press.at.x <= release.x } else { press.at.y < release.y };
    let (first, last) = if press_first { (press.at, release) } else { (release, press.at) };
    BAR.with(|b| *b.borrow_mut() = Some(Bar { thread: press.thread, message, words, html, first, last, drawn: None, placed: None }));
    entity.update(cx, |_, cx| cx.notify());
}

/// What's selected now, as its plain words and as HTML with its links,
/// bold and code. None when nothing is.
fn selection_now(pane: &Entity<ThreadPane>, window: &mut Window, cx: &mut App) -> Option<(String, String)> {
    let words = TextSelection::selected_text(window, cx).trim().to_owned();
    if words.is_empty() {
        return None;
    }
    // The kit gives a selection as the Markdown it came from when asked
    // (`SelectionFormat::Source`), which keeps what the plain words lose.
    // Each reply's text is asked that way for this one read, then put back.
    let documents: Vec<Entity<gpui_kit::base::TextViewState>> = pane.read(cx).rows.documents.values().cloned().collect();
    for doc in &documents {
        doc.update(cx, |d, cx| d.set_selection_format(SelectionFormat::Source, cx));
    }
    let source = TextSelection::selected_text(window, cx);
    for doc in &documents {
        doc.update(cx, |d, cx| d.set_selection_format(SelectionFormat::Plain, cx));
    }
    Some((words, selection_copy::selection_html(source.trim())))
}

/// ⌘C (Control-C elsewhere) with words selected in this thread: the words,
/// and HTML beside them. False with nothing selected, so the key goes on to
/// whatever else copies.
pub(crate) fn copy(pane: &Entity<ThreadPane>, window: &mut Window, cx: &mut App) -> bool {
    if !TextSelection::has_selection(window, cx) {
        return false;
    }
    let Some((words, html)) = selection_now(pane, window, cx) else { return false };
    put_on_clipboard(&words, &html, cx);
    true
}

/// The plain words and the HTML together, as one copy (`SelectionHTML`).
#[cfg(target_os = "macos")]
fn put_on_clipboard(words: &str, html: &str, _cx: &mut App) {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    // SAFETY: AppKit's own constants, there for as long as the app runs.
    let (html_type, string_type) = unsafe { (NSPasteboardTypeHTML, NSPasteboardTypeString) };
    let _ = board.setString_forType(&NSString::from_str(html), html_type);
    let _ = board.setString_forType(&NSString::from_str(words), string_type);
}

/// Elsewhere the plain words for now: GPUI's clipboard has no HTML flavour,
/// and Windows' and Linux's need code of their own (features.md, rich.copy).
#[cfg(not(target_os = "macos"))]
fn put_on_clipboard(words: &str, _html: &str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(words.to_owned()));
}

/// A bar's button pressed, or a script's `select-text` action.
fn act(pane: &WeakEntity<ThreadPane>, action: Action, window: &mut Window, cx: &mut App) {
    let Some(bar) = BAR.with(|b| b.borrow_mut().take()) else { return };
    let Some(entity) = pane.upgrade() else { return };
    entity.update(cx, |_, cx| cx.notify());
    match action {
        Action::Copy => put_on_clipboard(&bar.words, &bar.html, cx),
        Action::Quote => quote_in_composer(&entity, &bar.words, window, cx),
        Action::Explain => {
            let thread = entity.read(cx).summary.clone();
            let Some(app) = entity.read(cx).composer.read(cx).app.upgrade() else { return };
            crate::explain_pane::explain(&app, &thread, &bar.words, bar.message, cx);
        }
    }
}

/// Quote: the words as a Markdown quote at the end of the thread's draft,
/// ready for Mike to say what about them, and the box takes the cursor
/// (`quoteInComposer`). What was in the box stays, its pictures too.
fn quote_in_composer(pane: &Entity<ThreadPane>, words: &str, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::input::{InputContent, Position};
    let composer = pane.read(cx).composer.clone();
    let input = composer.read(cx).input.clone();
    let content = input.read(cx).content();
    let draft = content.text().to_string();
    let Some((keep, add)) = selection_copy::quote_into(&draft, words) else { return };
    let text = format!("{}{add}", &draft[..keep]);
    let mut next = InputContent::new(text.clone());
    for span in content.tokens().iter().filter(|s| s.range().end <= keep) {
        if let Ok(with) = next.clone().with_token(span.range(), span.token().clone()) {
            next = with;
        }
    }
    // The cursor at the end, after the room left for his words.
    let line = text.matches('\n').count() as u32;
    let column = text.rsplit('\n').next().unwrap_or("").chars().count() as u32;
    input.update(cx, |state, cx| {
        state.set_value(next, window, cx);
        state.set_cursor_position(Position::new(line, column), window, cx);
    });
    composer.update(cx, |c, cx| c.focus(window, cx));
    crate::web_panels::keyboard_to_app(window);
}

/// The bar, over the selection it was made for, if it's this thread's.
pub(crate) fn overlay(pane: &ThreadPane, window: &Window, cx: &mut Context<ThreadPane>) -> Option<AnyElement> {
    let (first, last) = BAR.with(|b| b.borrow().as_ref().filter(|b| b.thread == pane.summary.id).map(|b| (b.first, b.last)))?;
    // Where the selection was drawn, when its words say (replies' and
    // messages' do): the press and release are only its ends for a plain
    // drag. A Shift-click's press is the far end, which put the bar over the
    // selection's own first line; and the words can have moved since.
    // Those are from the frame before; the bar checks them again as it's
    // drawn, after the words have said where they are now, and comes back
    // here if they've moved.
    let placed = painted_ends(window, cx);
    BAR.with(|b| {
        if let Some(bar) = b.borrow_mut().as_mut() {
            bar.placed = placed;
        }
    });
    let (first, last) = placed.unwrap_or((first, last));
    let weak = cx.weak_entity();
    let moved_pane = weak.clone();
    let line = gpui_kit::px(style::zoomed(size::BODY_LINE));
    let height = gpui_kit::px(style::zoomed(BAR_HEIGHT));
    let gap = gpui_kit::px(style::zoomed(GAP));
    // Above the selection's first line, starting where it starts; below its
    // last line when there's no room above (the top of the window).
    let above = first.y - line / 2. - gap - height;
    let at = if above >= gpui_kit::px(0.) { point(first.x, above) } else { point(first.x, last.y + line / 2. + gap) };
    let item = |id: &'static str, title: &'static str, symbol: &'static str, tip: &'static str, action: Action, cx: &mut Context<ThreadPane>| {
        let hover = style::primary_opacity(cx, 0.08);
        let weak = weak.clone();
        crate::highlights::tipped(div().id(id).flex().items_center().gap(px(4.)).px(px(8.)).py(px(4.)).rounded(gpui_kit::px(5.)).cursor_pointer().hover(move |s| s.bg(hover)), tip)
            .on_click(move |_, window, cx| act(&weak, action, window, cx))
            .child(crate::icons::symbol(symbol).size(px(size::DETAIL)).text_color(style::label_text(cx)))
            .child(div().text_size(px(size::DETAIL)).line_height(px(style::natural_line(size::DETAIL))).font_weight(FontWeight::MEDIUM).text_color(style::label_text(cx)).child(title))
    };
    let divider = |cx: &App| div().w(gpui_kit::px(0.5)).h(px(14.)).bg(style::secondary_label(cx).opacity(0.25));
    let bar = h_flex()
        .id("selection-bar")
        .occlude()
        .items_center()
        .p(px(3.))
        .rounded(px(8.))
        .bg(cx.theme().popover)
        .border(gpui_kit::px(0.5))
        .border_color(style::card_border(cx))
        .shadow_lg()
        // A press here is on the bar, not the start of another selection.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(item("selection-explain", "Explain", "sparkle.magnifyingglass", "Explain this beside the thread, without adding to it", Action::Explain, cx))
        .child(divider(cx))
        .child(item("selection-quote", "Quote", "text.quote", "Quote this in your reply", Action::Quote, cx))
        .child(divider(cx))
        .child(item("selection-copy", "Copy", "doc.on.doc", "Copy", Action::Copy, cx))
        // Where it ends up, so a press on it doesn't count as one away.
        // And whether the selection it's over is still where it was placed.
        .child(canvas(|_, _, _| {}, move |bounds, _, window, cx| {
            let now = painted_ends(window, cx);
            let moved = BAR.with(|b| {
                let mut b = b.borrow_mut();
                let Some(bar) = b.as_mut() else { return false };
                bar.drawn = Some(bounds);
                now.is_some() && now != bar.placed
            });
            if moved {
                let pane = moved_pane.clone();
                cx.defer(move |cx| {
                    let _ = pane.update(cx, |_, cx| cx.notify());
                });
            }
        }).absolute().inset_0());
    Some(deferred(anchored().position(at).snap_to_window_with_margin(px(4.)).child(bar)).with_priority(2).into_any_element())
}

/// The bar's height: its 3 of padding round a button's 4, a line of 12 pt
/// and 4.
const BAR_HEIGHT: f32 = 3. + 4. + 15. + 4. + 3.;

// ---------------------------------------------------------------- scripts

/// `select-text`, as the Swift app's: `{"row": id, "words": "…"}` selects a
/// message's words and shows the bar ("ok window 0"); with `"action":
/// "explain" | "quote" | "copy"`, presses that instead. The kit selects a
/// view's text whole, so `words` has to be all of the message's text as it
/// reads (a spec picks a short one).
#[cfg(feature = "automation")]
pub(crate) fn select_command(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    let (Some(prefix), Some(words)) = (cmd["row"].as_str(), cmd["words"].as_str()) else { return Err("row and words are required".into()) };
    let action = match cmd["action"].as_str() {
        None => None,
        Some(name) => Some(Action::from_name(name).ok_or_else(|| format!("unknown action {name}"))?),
    };
    let pane = app.read(cx).pane.clone().ok_or("no thread open")?;
    let (ix, row) = {
        let rows = pane.read(cx).feed.model().rows();
        let ix = rows.iter().position(|r| r.id.starts_with(prefix)).ok_or("no such row")?;
        (ix, rows[ix].clone())
    };
    let message = message_of(&row).ok_or_else(|| format!("{} isn't a message", row.id))?;
    let RowContent::Assistant(item) = &row.content else { return Err(format!("{} isn't a reply: only a reply's words can be selected from a script here", row.id)) };
    pane.update(cx, |p, cx| p.scroll_command(None, Some(&row.id), 120., cx))?;
    window.render_frame(cx);
    // Words in one of its draft boxes, dragged across as a person would
    // (draft_boxes.rs). The Swift app finds the words wherever they are.
    if cmd["draft"].as_bool() == Some(true) {
        TextSelection::clear(window, cx);
        let (first, last) = crate::draft_boxes::drag_select(&row.id, words, window, cx)?;
        let selected = TextSelection::selected_text(window, cx);
        if selected.trim() != words.trim() {
            return Err(format!("dragging across {words:?} in {}'s draft selected {:?}", row.id, selected.trim()));
        }
        let (words, html) = selection_now(&pane, window, cx).ok_or("nothing got selected")?;
        BAR.with(|b| *b.borrow_mut() = Some(Bar { thread: pane.read(cx).summary.id.clone(), message: Some(message), words, html, first, last, drawn: None, placed: None }));
        pane.update(cx, |_, cx| cx.notify());
        window.render_frame(cx);
        return match action {
            Some(action) => {
                act(&pane.downgrade(), action, window, cx);
                window.render_frame(cx);
                Ok(json!("ok"))
            }
            None => Ok(json!("ok window 0")),
        };
    }
    let doc = pane.read(cx).rows.documents.get(&item.item_id).cloned().ok_or("that reply isn't drawn")?;
    TextSelection::clear(window, cx);
    doc.update(cx, |d, cx| d.select_all(cx));
    // The reply's text takes the keys, as it does when a drag starts in it,
    // so ⌘C after this is the text's.
    let focus = doc.read(cx).focus_handle().clone();
    window.focus(&focus, cx);
    crate::web_panels::keyboard_to_app(window);
    let selected = TextSelection::selected_text(window, cx);
    if selected.trim() != words.trim() {
        return Err(format!("{words:?} isn't all of {}'s words (it reads {:?})", row.id, selected.trim()));
    }
    let (words, html) = selection_now(&pane, window, cx).ok_or("nothing got selected")?;
    // Where the bar goes: over the reply's first line, as after a drag
    // across it.
    let mine = ElementId::NamedInteger("reply-links".into(), ix as u64);
    let texts: Vec<Bounds<Pixels>> = gpui_kit::base::test_support::texts(window).into_iter().filter(|t| t.visible() && t.path().contains(&mine)).map(|t| t.bounds()).collect();
    let first = texts.first().map(|b| point(b.left(), b.top() + gpui_kit::px(style::zoomed(size::BODY_LINE) / 2.))).ok_or("that reply isn't on screen")?;
    let last = texts.last().map(|b| point(b.right(), b.bottom() - gpui_kit::px(style::zoomed(size::BODY_LINE) / 2.))).unwrap_or(first);
    BAR.with(|b| *b.borrow_mut() = Some(Bar { thread: pane.read(cx).summary.id.clone(), message: Some(message), words, html, first, last, drawn: None, placed: None }));
    pane.update(cx, |_, cx| cx.notify());
    window.render_frame(cx);
    match action {
        Some(action) => {
            act(&pane.downgrade(), action, window, cx);
            window.render_frame(cx);
            Ok(json!("ok"))
        }
        None => Ok(json!("ok window 0")),
    }
}

/// `text-lines`: where a reply's (or a message's) words are drawn, each
/// paragraph or list item the kit laid out as its own text, in window points: "ok [{text, x,
/// y, w, h}]". For a script that drags across them with the real pointer
/// (features/tools/real-select.ts), which needs to know where to press.
/// The row's place is its index in its own thread, so with two threads'
/// panels open this finds the same row of both, as `find_link` would
/// (reply_links.rs): open the one thread.
#[cfg(feature = "automation")]
pub(crate) fn text_lines_command(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    let prefix = cmd["row"].as_str().ok_or("row is required")?;
    let pane = app.read(cx).pane.clone().ok_or("no thread open")?;
    let ix = pane.read(cx).feed.model().rows().iter().position(|r| r.id.starts_with(prefix)).ok_or("no such row")?;
    window.render_frame(cx);
    // A reply's paragraphs, or a message of Mike's (rows.rs's "user-text").
    let mine = [ElementId::NamedInteger("reply-links".into(), ix as u64), ElementId::NamedInteger("user-text".into(), ix as u64)];
    // A message's words are plain selectable text, which the kit's list of
    // texts leaves out: its selectable runs, found by their words, instead.
    if let RowContent::User(message) = &pane.read(cx).feed.model().rows()[ix].content {
        let text = message.text.trim().to_owned();
        let lines: Vec<serde_json::Value> = TextSelection::participant_runs(window, cx)
            .into_iter()
            .filter(|(words, _)| words.trim() == text)
            .map(|(words, b)| json!({ "text": words, "x": f32::from(b.left()), "y": f32::from(b.top()), "w": f32::from(b.size.width), "h": f32::from(b.size.height) }))
            .collect();
        return Ok(json!(format!("ok {}", serde_json::to_string(&lines).unwrap_or_default())));
    }
    let lines: Vec<serde_json::Value> = gpui_kit::base::test_support::texts(window)
        .into_iter()
        .filter(|t| t.visible() && mine.iter().any(|m| t.path().contains(m)))
        .map(|t| {
            let b = t.bounds();
            json!({ "text": t.text(), "x": f32::from(b.left()), "y": f32::from(b.top()), "w": f32::from(b.size.width), "h": f32::from(b.size.height) })
        })
        .collect();
    Ok(json!(format!("ok {}", serde_json::to_string(&lines).unwrap_or_default())))
}

/// `selection`: what's selected in the window now, whether the bar's up (and
/// its top), and whether the dock has a panel picked up, without drawing a frame, so a
/// script can ask mid-drag: "ok {text, bar, barTop, following, panelDrag}".
#[cfg(feature = "automation")]
pub(crate) fn selection_command(app: &Entity<crate::app::Threadbear>, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let text = TextSelection::selected_text(window, cx);
    let bar = BAR.with(|b| b.borrow().as_ref().map(|b| b.words.clone()));
    // Where it was drawn, its top, in window points.
    let following = app.read(cx).pane.clone().is_some_and(|p| p.read(cx).scroller.read(cx).following());
    let bar_top = BAR.with(|b| b.borrow().as_ref().and_then(|b| b.drawn).map(|d| f32::from(d.top())));
    let model = app.read(cx).dock_view.read(cx).model.clone();
    let panel_drag = model.read(cx).controller.drag.as_ref().map(|d| d.panel.clone());
    Ok(json!(format!("ok {}", json!({ "text": text, "bar": bar, "barTop": bar_top, "following": following, "panelDrag": panel_drag }))))
}


