// Reply buttons in the transcript (docs/protocol.md, "Reply buttons"; Swift's
// MarkdownRenderer.replyButton and ReplyButtons): `[title](button:<id>)` in a
// reply is drawn as a pill in its state's colours, inside the text where the
// agent put it, and a click presses it through the runner, once. The latest
// snapshot of every button the app has seen is kept here, by id, fed from the
// open thread's `button.updated` events and `buttons.list`; the pills read it
// as they draw.
use crate::style;
use gpui_kit::base::text::{InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Mutex;
use threadbear_core::reply_buttons::{self as buttons, Look, ReplyButton};
use crate::tips::Tip as _;

#[derive(Default)]
struct Store {
    /// Every button seen, by the thread it belongs to and its id. Keyed by
    /// both, never by id alone: a pill draws and presses only a button of the
    /// transcript it's in, so `[Go](button:<id>)` copied into another
    /// thread's reply (from `read_thread`, say) reaches nothing (Daybreak
    /// 107, 12 Nov 2026), and another runner reporting the same id for one
    /// of its threads can't take this one's place.
    known: HashMap<(String, String), ReplyButton>,
    /// Presses on their way, so a second click meanwhile does nothing.
    pressing: HashSet<(String, String)>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Store::default))
}

fn key(thread: &str, id: &str) -> (String, String) {
    (thread.to_owned(), id.to_owned())
}

/// Keeps each of a thread's buttons unless what's known of it is newer. True
/// if anything changed. `thread_id` is the thread whose events or list these
/// came from: the only one they're drawn or pressed in.
pub(crate) fn receive(list: impl IntoIterator<Item = ReplyButton>, thread_id: &str) -> bool {
    with_store(|store| {
        let mut changed = false;
        for button in list {
            let k = key(thread_id, &button.id);
            if store.known.get(&k).is_some_and(|old| !button.supersedes(old)) {
                continue;
            }
            store.known.insert(k, button);
            changed = true;
        }
        changed
    })
}

/// The `button.updated` events among a page of events.
pub(crate) fn fold(events: &[threadbear_core::reducer::Event], thread_id: &str) -> bool {
    let list: Vec<ReplyButton> = events.iter().filter_map(|e| match &e.payload {
        threadbear_core::reducer::EventPayload::ButtonUpdated(v) => ReplyButton::decode(v),
        _ => None,
    }).collect();
    !list.is_empty() && receive(list, thread_id)
}

/// `thread`'s own button `id`, if this app has seen it. Another thread's
/// button with that id is None here.
pub(crate) fn known(thread: &str, id: &str) -> Option<ReplyButton> {
    with_store(|store| store.known.get(&key(thread, id)).cloned())
}

/// Marks a press as on its way (or back). False if it already was.
pub(crate) fn set_pressing(thread: &str, id: &str, on: bool) -> bool {
    with_store(|store| if on { store.pressing.insert(key(thread, id)) } else { store.pressing.remove(&key(thread, id)) })
}

/// The pills a reply's buttons show as, for the dump: "✓ Fill in the form
/// [b7k2abcd, pressed]", or "unknown <id>" (AppDump.buttons), one of another
/// thread's included.
#[cfg(feature = "automation")]
pub(crate) fn dump_labels(text: &str, thread: &str) -> Option<Vec<String>> {
    let ids = buttons::ids_in(text);
    (!ids.is_empty()).then(|| {
        ids.iter()
            .map(|id| match known(thread, id) {
                Some(button) => {
                    let look = buttons::look(&button, is_pressing(thread, id));
                    format!("{} [{id}, {}]", buttons::label(&button, look), look.name())
                }
                None => format!("unknown {id}"),
            })
            .collect()
    })
}

pub(crate) fn is_pressing(thread: &str, id: &str) -> bool {
    with_store(|store| store.pressing.contains(&key(thread, id)))
}

/// A reply's text in `thread` with every `[words](button:<id>)` this app
/// can't draw as a pill left as its words alone: a button it doesn't know
/// (one the list hasn't brought yet, or another thread's, copied in) or a
/// run's, which only its card presses. The kit draws a pill as one piece that can't wrap, so these go
/// back to plain words before the text is parsed, and wrap as words do.
pub(crate) fn unknown_as_words<'a>(text: &'a str, thread: &str) -> std::borrow::Cow<'a, str> {
    if !text.contains("](button:") {
        return std::borrow::Cow::Borrowed(text);
    }
    let pillable = |id: &str| known(thread, id).is_some_and(|b| b.run.is_none());
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("](button:") {
        let (before, after) = rest.split_at(at);
        let link_end = after.find(')');
        // The link's words: back to the `[` that opens them, minding brackets inside.
        let mut depth = 0i32;
        let open = before.char_indices().rev().find_map(|(i, c)| match c {
            ']' => {
                depth += 1;
                None
            }
            '[' if depth == 0 => Some(i),
            '[' => {
                depth -= 1;
                None
            }
            _ => None,
        });
        match (open, link_end) {
            (Some(open), Some(end)) => {
                let id = after["](button:".len()..end].trim();
                if pillable(id) {
                    out.push_str(&rest[..at + end + 1]);
                } else {
                    out.push_str(&before[..open]);
                    out.push_str(&before[open + 1..]);
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..at + 1]);
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// What a pill's click calls with (the thread whose transcript it's drawn
/// in, the button's id): the app presses it (`Threadbear::press_reply_button`).
#[derive(Clone)]
pub(crate) struct PressHook(pub(crate) Rc<dyn Fn(String, String, &mut Window, &mut App)>);

impl Global for PressHook {}

/// The pill's colours, (fill, text, edge), as Theme.replyButton has them.
fn colours(look: Look, dark: bool) -> (Hsla, Hsla, Hsla) {
    let hex = |v: u32| -> Hsla { rgb(v).into() };
    let (d, l) = match look {
        Look::Start => ((0x2F3B52, 0x9CC2FF, 0x3D5070), (0xE6EFFF, 0x1F5FBF, 0xBCD3F5)),
        Look::Send => ((0x33304A, 0xC3B5FF, 0x4A4470), (0xEFEAFF, 0x5B3FC4, 0xD3C8F5)),
        Look::Pressed => ((0x24332A, 0x8FD19E, 0x35503D), (0xE5F4E8, 0x2B7A3D, 0xBFE0C6)),
        Look::Pressing => ((0x2A2A2C, 0x8E8B86, 0x3A3A3C), (0xEEEEF0, 0x8E8E93, 0xD8D8DC)),
        Look::Failed => ((0x3B2626, 0xFF9A8F, 0x5A3535), (0xFDECEB, 0xB3261E, 0xF2C4C0)),
    };
    let (fill, text, edge) = if dark { d } else { l };
    (hex(fill), hex(text), hex(edge))
}

/// The pill's height and where its words' baseline is: 13 pt medium on a
/// 16 pt line, 2.5 in from its edge, so it sits on the reply's baseline.
const PILL_HEIGHT: f32 = 21.;
const PILL_BASELINE: f32 = 15.4;

/// The markdown plugin that turns `[title](button:<id>)` into a pill, for
/// the transcript of `thread`: only that thread's own buttons are pills, and
/// a click presses as that thread, never as whichever thread made a button
/// with that id.
pub(crate) struct Pills {
    pub(crate) thread: String,
}

#[derive(Clone)]
struct PillRef {
    id: String,
    words: String,
}

impl MarkdownPlugin for Pills {
    fn name(&self) -> &str {
        "reply-button"
    }

    fn parse(&self, node: &markdown::mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown::mdast::Node::Link(link) = node else { return None };
        let id = buttons::id_of(&link.url)?.to_owned();
        let words = node.to_string();
        Some(MarkdownNode::new("reply-button", PillRef { id: id.clone(), words: words.clone() }).text(words.clone()).markdown(format!("[{words}](button:{id})")))
    }

    fn render_inline(&self, node: &MarkdownNode, _context: &InlineRenderContext, _window: &mut Window, cx: &mut App) -> Option<InlineElement> {
        let pill = node.data::<PillRef>()?.clone();
        // A button this app doesn't know, or another thread's, was left as
        // words before parsing (`unknown_as_words`); one that's gone since,
        // or text that didn't go through that, still shows only its words.
        let Some(button) = known(&self.thread, &pill.id).filter(|b| b.run.is_none()) else {
            return Some(InlineElement::new(div().child(pill.words)));
        };
        let look = buttons::look(&button, is_pressing(&self.thread, &pill.id));
        let (fill, text, edge) = colours(look, cx.theme_is_dark());
        let label = buttons::label(&button, look);
        let tip = buttons::tip(&button, look);
        let (thread, id) = (self.thread.clone(), pill.id.clone());
        let element = div()
            .id(SharedString::from(format!("reply-pill-{}", pill.id))).role(Role::Button)
            .flex_none()
            .mx(px(3.))
            .h(px(PILL_HEIGHT))
            .px(px(8. - 1.))
            .flex()
            .items_center()
            .rounded(px(PILL_HEIGHT / 2.))
            .bg(fill)
            .border_1()
            .border_color(edge)
            .text_size(px(style::size::LABEL))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(|s| s.opacity(0.85))
            .tip(tip.clone())
            // Not the start of a selection in the text around it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                if let Some(hook) = cx.try_global::<PressHook>().cloned() {
                    (hook.0)(thread.clone(), id.clone(), window, cx);
                }
            })
            .child(label);
        Some(InlineElement::new(element).with_baseline(px(PILL_BASELINE)))
    }
}

trait ThemeIsDark {
    fn theme_is_dark(&self) -> bool;
}

impl ThemeIsDark for App {
    fn theme_is_dark(&self) -> bool {
        use gpui_kit::component::ActiveTheme as _;
        self.theme().is_dark()
    }
}

#[cfg(test)]
mod tests {
    // By name: the kit's prelude has a `test` of its own.
    use super::{known, receive, unknown_as_words};
    use threadbear_core::reply_buttons::ReplyButton;

    #[test]
    fn a_button_this_app_cant_draw_is_left_as_its_words() {
        receive([ReplyButton { id: "known123".into(), title: "Go".into(), ..Default::default() }], "t1");
        let text = "Do it [Go](button:known123) or [Old one](button:gone4567), see [a [b] c](button:gone4567). [site](https://x.y)";
        assert_eq!(unknown_as_words(text, "t1"), "Do it [Go](button:known123) or Old one, see a [b] c. [site](https://x.y)");
        assert!(matches!(unknown_as_words("no buttons", "t1"), std::borrow::Cow::Borrowed(_)));
    }

    // Daybreak 107: thread A's button, its link copied into thread B's reply
    // (B's agent read it with read_thread), is only words in B: no pill,
    // so nothing to press, whether or not A is open in a panel.
    #[test]
    fn another_threads_button_is_only_words() {
        receive([ReplyButton { id: "afull001".into(), title: "Start it".into(), ..Default::default() }], "thread-a");
        assert_eq!(unknown_as_words("[Continue](button:afull001)", "thread-a"), "[Continue](button:afull001)");
        assert_eq!(unknown_as_words("[Continue](button:afull001)", "thread-b"), "Continue");
        assert!(known("thread-b", "afull001").is_none());
        assert!(known("thread-a", "afull001").is_some());
    }

    // A second thread (another runner's, say) with a button of the same id
    // neither takes the first one's place nor lends it its own.
    #[test]
    fn the_same_id_in_two_threads_stays_two_buttons() {
        receive([ReplyButton { id: "same0001".into(), title: "A's".into(), ..Default::default() }], "dup-a");
        receive([ReplyButton { id: "same0001".into(), title: "B's".into(), ..Default::default() }], "dup-b");
        assert_eq!(known("dup-a", "same0001").map(|b| b.title), Some("A's".into()));
        assert_eq!(known("dup-b", "same0001").map(|b| b.title), Some("B's".into()));
    }
}

// Daybreak 107 played in a window: thread A's start button, its link copied
// into thread B's reply. Clicked anywhere along B's line, nothing presses,
// whatever else is open (the old check let it through while A had a panel of
// its own); the same link in A's own reply presses, as A.
#[cfg(all(test, feature = "automation"))]
mod clicks {
    use super::{receive, Pills, PressHook};
    use crate::open_gate::{self, Origin};
    use gpui_kit::base::TextViewState;
    use gpui_kit::prelude::*;
    use gpui_kit::test::TestEventExt as _;
    // By name: a glob of the kit would bring its `test` over the one the
    // test macro's own expansion uses.
    use gpui_kit::{div, point, px, size, App, Bounds, Entity, MouseButton, Point, TestAppContext, Window, WindowBounds, WindowOptions};
    use std::cell::RefCell;
    use std::rc::Rc;
    use threadbear_core::reply_buttons::{ReplyButton, Start};

    struct Transcript {
        thread: String,
        reply: Entity<TextViewState>,
    }

    impl Render for Transcript {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(div().w(px(320.)).child(open_gate::text_view(&self.reply, Origin::ThisMac, "").plugin(Pills { thread: self.thread.clone() })))
        }
    }

    /// Clicks along the reply's first line in `thread`'s transcript, and
    /// what the pills asked the app to press.
    fn click_along(thread: &str, cx: &mut TestAppContext) -> Vec<(String, String)> {
        cx.update(gpui_kit::init);
        let pressed: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
        let heard = pressed.clone();
        cx.update(|cx| cx.set_global(PressHook(Rc::new(move |from: String, id: String, _: &mut Window, _: &mut App| heard.borrow_mut().push((from, id))))));
        let thread = thread.to_owned();
        let window = cx.update(|cx| {
            let bounds = Bounds { origin: Point::default(), size: size(px(400.), px(200.)) };
            let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
            gpui_kit::open_window(options, cx, move |_, cx| cx.new(|cx| Transcript { thread, reply: cx.new(|cx| TextViewState::markdown("[Continue](button:afull107)", cx)) })).expect("a test window").0
        });
        cx.update_window(window, |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            for x in (2..300).step_by(6) {
                let at = point(px(x as f32), px(10.));
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Left, cx);
                window.pointer_up(at, MouseButton::Left, cx);
            }
        })
        .expect("the test window");
        let out = pressed.borrow().clone();
        out
    }

    fn a_has_its_button() {
        let start = Start { text: "Do the privileged thing".into(), ..Default::default() };
        receive([ReplyButton { id: "afull107".into(), title: "Start it".into(), start: Some(start), ..Default::default() }], "thread-a");
    }

    #[gpui_kit::test]
    fn a_copied_button_in_another_threads_reply_presses_nothing(cx: &mut TestAppContext) {
        a_has_its_button();
        assert_eq!(click_along("thread-b", cx), Vec::<(String, String)>::new());
    }

    #[gpui_kit::test]
    fn a_button_in_its_own_threads_reply_presses_as_that_thread(cx: &mut TestAppContext) {
        a_has_its_button();
        let pressed = click_along("thread-a", cx);
        assert!(!pressed.is_empty(), "the pill never pressed");
        assert!(pressed.iter().all(|p| *p == ("thread-a".to_owned(), "afull107".to_owned())), "{pressed:?}");
    }
}
