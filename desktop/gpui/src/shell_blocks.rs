// The two icons at the top right of a ```bash block in a reply
// (ShellBlockIcons.swift): play puts the command in the thread's Terminal, at
// its prompt and never run, and copy copies it. Play only shows where the
// Terminal would be the thread's own shell, in its own folder on this Mac
// (terminal.rs `can_put`). gpui-kit draws them in the code block's corner,
// where its own block actions go.
use crate::style::{self, px};
use crate::terminal::PutOutcome;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use threadbear_core::terminal::{self as rules, ShellBlock};
use crate::tips::Tip as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Icon {
    Put,
    Copy,
}

impl Icon {
    fn symbol(self) -> &'static str {
        match self {
            Icon::Put => "play",
            Icon::Copy => "doc.on.doc",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Icon::Put => "Put in Terminal",
            Icon::Copy => "Copy",
        }
    }
}

/// How long an icon shows its tick, and the note that the Terminal was busy.
const TICK_FOR: Duration = Duration::from_millis(1500);
const NOTE_FOR: Duration = Duration::from_millis(2500);

/// When each icon of each command was last pressed, so it shows a tick for a
/// moment, and when one couldn't be typed and was copied instead. Read as
/// they're drawn; only touched on the app's own thread.
static PRESSED: Mutex<Vec<(Icon, String, Instant)>> = Mutex::new(Vec::new());
static BUSY: Mutex<Option<(String, Instant, &'static str)>> = Mutex::new(None);

fn mark(icon: Icon, command: &str) {
    let mut pressed = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
    pressed.retain(|(_, _, at)| at.elapsed() < TICK_FOR);
    pressed.push((icon, command.to_owned(), Instant::now()));
}

fn unmark(icon: Icon, command: &str) {
    PRESSED.lock().unwrap_or_else(|e| e.into_inner()).retain(|(i, c, _)| !(*i == icon && c == command));
}

fn ticked(icon: Icon, command: &str) -> bool {
    PRESSED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(i, c, at)| *i == icon && c == command && at.elapsed() < TICK_FOR)
}

/// Why `command` was just copied rather than typed, while that's shown.
fn copied_because(command: &str) -> Option<&'static str> {
    BUSY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|(c, at, _)| c == command && at.elapsed() < NOTE_FOR).map(|(_, _, why)| *why)
}

/// Everything drawn again once a tick or a note is due to go.
fn redraw_after(wait: Duration, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(wait + Duration::from_millis(50)).await;
        let _ = cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// A reply's text with the icons on its shell blocks, when it has any. Other
/// replies are left as they are. `source` is the markdown the text was made
/// from, for telling a block still streaming in (no icons: half a command
/// mustn't be something you can press); `thread` is the transcript's.
pub(crate) fn with_icons(text: gpui_kit::base::TextView, source: &str, thread: &str) -> gpui_kit::base::TextView {
    if !ShellBlock::may_hold(source) {
        return text;
    }
    let source: Arc<str> = source.into();
    let thread = thread.to_owned();
    text.code_block_actions(move |block, _window, cx| {
        let shell = ShellBlock::from(block.lang().as_deref(), &block.code()).filter(|_| block.span.is_some_and(|span| rules::fence_closed(&source, span.start, span.end)));
        match shell {
            Some(shell) => icons(&shell.command, &thread, cx).into_any_element(),
            None => div().into_any_element(),
        }
    })
}

fn icons(command: &str, thread: &str, cx: &App) -> impl IntoElement {
    let put = crate::terminal::shows_put(thread, cx);
    let busy = copied_because(command);
    // Lined up with the block's first line, 6 in from its right edge, as the
    // Swift box draws them (ShellBlockIcons.rect). The kit's corner is 8 in
    // and fills its actions with the block's own translucent colour, which
    // over the block showed as a box: out of the flow, they leave it no size.
    div().absolute().top(px(1.75)).right(px(-2.)).child(
        h_flex()
            .gap(px(2.))
            .items_center()
            .when_some(busy, |row, why| {
                // Over the code, in the window's own colour so it reads.
                row.child(
                    div()
                        .mr(px(4.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(cx.theme().popover)
                        .text_size(px(style::size::DETAIL))
                        .line_height(px(style::natural_line(style::size::DETAIL)))
                        .text_color(style::secondary_text(cx))
                        .whitespace_nowrap()
                        .child(why),
                )
            })
            .when(put, |row| row.child(icon(Icon::Put, command, thread, cx)))
            .child(icon(Icon::Copy, command, thread, cx)),
    )
}

fn icon(which: Icon, command: &str, thread: &str, cx: &App) -> impl IntoElement {
    let ticked = ticked(which, command);
    let colour = if ticked { style::tick_green(cx) } else { style::secondary_text(cx) };
    let glyph = if ticked { crate::icons::symbol("checkmark") } else { crate::icons::symbol(which.symbol()) };
    let (command, thread) = (command.to_owned(), thread.to_owned());
    let id = SharedString::from(format!("shell-{}-{}", if which == Icon::Put { "put" } else { "copy" }, hash(&command)));
    div()
        .id(ElementId::Name(id))
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .cursor_pointer()
        .text_color(colour)
        .hover(|s| s.bg(style::primary_opacity(cx, 0.1)).text_color(style::label_text(cx)))
        .role(Role::Button)
        .aria_label(SharedString::from(which.tip()))
        .tip(which.tip())
        // Not the start of a text selection in the block under it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| press(which, &command, &thread, window, cx))
        // SF Symbols at 12 pt, as ShellBlockIcons draws them.
        .child(glyph.with_size(px(style::size::DETAIL)))
        .test_support()
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// A press: the tick shows at once. Copy copies; play puts the command in the
/// thread's Terminal (bringing the panel up), and if the shell's busy with
/// something, or someone's own words are on its prompt, copies it instead and
/// says so (TranscriptTextView.press).
fn press(which: Icon, command: &str, thread: &str, window: &mut Window, cx: &mut App) {
    if which == Icon::Copy {
        mark(which, command);
        window.refresh();
        redraw_after(TICK_FOR, cx);
        cx.write_to_clipboard(ClipboardItem::new_string(command.to_owned()));
        return;
    }
    put(command, thread, window, cx);
}

/// Play's press, and the automation's `put`: the command at the thread's
/// prompt, or copied when the shell's busy.
pub(crate) fn put(command: &str, thread: &str, window: &mut Window, cx: &mut App) {
    mark(Icon::Put, command);
    window.refresh();
    redraw_after(TICK_FOR, cx);
    let Some(app) = crate::terminal::app(cx) else { return };
    let block = ShellBlock { command: command.to_owned() };
    let Some(task) = app.update(cx, |a, cx| crate::terminal::put_in_terminal(a, block, thread, window, cx)) else { return };
    let command = command.to_owned();
    cx.spawn(async move |cx| {
        let why = match task.await {
            Some(PutOutcome::Busy) => "Terminal's in use, so it's copied",
            Some(PutOutcome::Occupied) => "Another command's waiting for Return, so it's copied",
            _ => return,
        };
        let _ = cx.update(|cx| {
            unmark(Icon::Put, &command);
            cx.write_to_clipboard(ClipboardItem::new_string(command.clone()));
            *BUSY.lock().unwrap_or_else(|e| e.into_inner()) = Some((command.clone(), Instant::now(), why));
            cx.refresh_windows();
            redraw_after(NOTE_FOR, cx);
        });
    })
    .detach();
}

/// What each closed shell block in a reply offers, for the dump: "[put, copy]
/// npm test", or "[copy] npm test" where the Terminal wouldn't be the
/// thread's own (AppDump's `shellBlocks`).
#[cfg(feature = "automation")]
pub(crate) fn dump_lines(text: &str, thread: &str, cx: &App) -> Option<Vec<String>> {
    let blocks = rules::shell_blocks(text);
    if blocks.is_empty() {
        return None;
    }
    let icons = if crate::terminal::shows_put(thread, cx) { "[put, copy]" } else { "[copy]" };
    Some(blocks.into_iter().map(|b| format!("{icons} {}", b.command)).collect())
}
