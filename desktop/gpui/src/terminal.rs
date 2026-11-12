// The Terminal panel (⌘J, View > Show Terminal): a real terminal in the dock,
// as VS Code and T3 Code have under their editor (TerminalPanel.swift).
// alacritty_terminal keeps the screen and runs your login shell in a pty, so
// colours, the cursor keys and full-screen programs (vim, htop, claude) work as
// in Terminal; terminal_element.rs draws it, and the rules with no window in
// them (the keys, the folder, putting a command) are threadbear_core::terminal.
//
// Each thread has a shell of its own, started in the thread's folder (its
// worktree when it has one) and kept running while you look at other threads,
// so switching back finds it as you left it. A thread on another machine gets
// a shell on this Mac in your home folder, with a line saying so, as its folder
// isn't here; with no thread open it's a shell in your home folder too.
//
// A shell only starts when you ask for one: ⌘J, clicking the empty panel, or a
// shell block's play icon. Clicking through threads with the panel open, or
// launching with it left open, starts nothing, as each login shell reads your
// whole profile. A thread's shell stops when the thread is archived or deleted
// (a dev server left running there would otherwise keep going in a worktree
// the next thread gets), and every shell stops when the app quits.
use crate::app::Threadbear;
use crate::style::{self, px};
use crate::terminal_element::TerminalElement;
use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line as GridLine, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use threadbear_core::terminal::{self as rules, KeyAction, Line, ShellBlock, Step, Target, TerminalKey};
use threadbear_dock::{DockDrop, DockEdge, DockNode, DockSplit};

// ---------------------------------------------------------------- hooking in

/// The app's one set of shells, reached from the panel, the menus, a shell
/// block's play icon and the automation.
struct TerminalsGlobal(Entity<Terminals>);
impl Global for TerminalsGlobal {}

/// Made with the app (`Threadbear::new`), before anything can ask for a shell.
pub(crate) fn install(app: &Entity<Threadbear>, cx: &mut App) {
    cx.bind_keys(own_keys());
    let terminals = cx.new(|cx| Terminals::new(app, cx));
    cx.set_global(TerminalsGlobal(terminals));
}

/// The keys that go no further than the terminal's own key handler. Tab and
/// Shift-Tab move the keyboard between controls, and ⌘C copies a text
/// selection, in the window's root (gpui-kit's own keys): in the terminal
/// they're the shell's (completion, and ^C off the Mac). And its copy, paste
/// and select-all keys off the Mac, Control-Shift C, V and A, are its own
/// too: Control-Shift-C is also Copy Transcript as Markdown there, so in the
/// terminal it copied the whole thread rather than what was selected (11
/// Nov). The app's command keys have no context, which ties with these at
/// the terminal's depth, and a tie goes to whichever was bound last, so
/// `apply_shortcuts` binds these again after binding the commands.
pub(crate) fn own_keys() -> Vec<KeyBinding> {
    let mut chords = vec!["tab", "shift-tab"];
    if cfg!(target_os = "macos") {
        // ⌘V and ⌘A too, in case shortcuts.json gives a command one of them
        // (Settings won't, but the file is anyone's to edit).
        chords.extend(["cmd-c", "cmd-v", "cmd-a"]);
    } else {
        chords.extend(["ctrl-c", "ctrl-shift-c", "ctrl-shift-v", "ctrl-shift-a"]);
    }
    chords.into_iter().map(|chord| KeyBinding::new(chord, NoAction, Some(KEY_CONTEXT))).collect()
}

pub(crate) const KEY_CONTEXT: &str = "Terminal";

fn terminals(cx: &App) -> Option<Entity<Terminals>> {
    cx.try_global::<TerminalsGlobal>().map(|g| g.0.clone())
}

/// Whether any shell has something an update's restart would lose, as
/// quitting ends every shell: a program running in it, or words at its
/// prompt (core's `holds_a_restart`). Asked once a second (app_update.rs).
pub(crate) fn holds_a_restart(cx: &App) -> bool {
    let Some(terminals) = terminals(cx) else { return false };
    terminals.read(cx).sessions.values().any(|s| {
        let s = s.read(cx);
        rules::holds_a_restart(s.running(), s.shell_alone_in_front(), &s.line)
    })
}

/// What the dock shows for the Terminal panel: the panel's own view, which
/// draws again only when it or its shell changes, not with the whole app.
pub(crate) fn panel_view(cx: &mut App) -> AnyElement {
    match terminals(cx) {
        Some(t) => AnyView::from(t).cached(StyleRefinement::default().size_full()).into_any_element(),
        None => div().into_any_element(),
    }
}

/// ⌘J (`toggleTerminal`): hides the Terminal if it's showing, else brings it
/// up under the thread (a third of its height, as VS Code opens its own) with
/// the thread's shell, started if it has none, and gives it the keyboard.
pub(crate) fn toggle(app: &mut Threadbear, window: &mut Window, cx: &mut Context<Threadbear>) {
    if app.dock.read(cx).controller.is_showing(crate::shell::TERMINAL) {
        app.dock.update(cx, |m, cx| m.close(crate::shell::TERMINAL, cx));
        return;
    }
    bring_up(app, cx);
    let target = target_of(app);
    if let Some(terminals) = terminals(cx) {
        terminals.update(cx, |t, cx| {
            t.set_target(target.clone(), cx);
            let session = t.open(&target, cx);
            t.focus(&session, window, cx);
        });
    }
}

/// Shows the panel with nothing started in it (the automation's `show`, for
/// what the panel says before a shell is asked for).
fn show_panel(app: &mut Threadbear, cx: &mut Context<Threadbear>) {
    if !app.dock.read(cx).controller.is_showing(crate::shell::TERMINAL) {
        bring_up(app, cx);
    }
}

/// `bringUpTerminal`: under the main panel with a third of its height, or
/// where it was if it's in the layout already.
fn bring_up(app: &mut Threadbear, cx: &mut Context<Threadbear>) {
    use crate::shell::{MAIN, TERMINAL};
    let layout = app.dock.read(cx).layout().clone();
    if layout.contains(TERMINAL) {
        app.dock.update(cx, |m, cx| m.show(TERMINAL, &[], DockDrop::window(DockEdge::Bottom), cx));
        return;
    }
    let Some(main) = layout.group_containing(MAIN) else {
        app.dock.update(cx, |m, cx| m.open(TERMINAL, DockDrop::window(DockEdge::Bottom), cx));
        return;
    };
    let mut layout = layout;
    layout.add(TERMINAL, DockDrop::edge(main.id, DockEdge::Bottom));
    if let Some((id, 2)) = split_around(TERMINAL, &layout.root).map(|s| (s.id.clone(), s.children.len())) {
        layout.set_fractions(&id, &[0.68, 0.32]);
    }
    app.dock.update(cx, |m, cx| m.set_layout(layout, cx));
}

/// The split a panel's group sits directly in.
fn split_around<'a>(panel: &str, node: &'a DockNode) -> Option<&'a DockSplit> {
    let DockNode::Split(split) = node else { return None };
    for child in &split.children {
        if let DockNode::Group(group) = child {
            if group.panels.iter().any(|p| p == panel) {
                return Some(split);
            }
        }
        if let Some(inner) = split_around(panel, child) {
            return Some(inner);
        }
    }
    None
}

/// The shell the open thread's Terminal is (`TerminalTarget.for`).
fn target_of(app: &Threadbear) -> Target {
    let thread = app.open_thread();
    let runner = thread.and_then(|t| t.runner_id.as_deref()).map(|id| app.runner_name_of(id));
    rules::target_for(thread, &crate::files::home_dir(), runner.as_deref(), |path| threadbear_core::open_policy::local_metadata(std::path::Path::new(path)).is_ok_and(|m| m.is_dir()))
}

/// The runner said a thread changed (`thread.updated`): an archive it's agreed
/// to stops that thread's shell. Only the runner's word counts, not the app's
/// own guess at an archive it's still sending, which may yet be refused and put
/// back (`archivesInFlight`).
pub(crate) fn thread_updated(thread: &threadbear_core::inbox::ThreadSummary, cx: &mut App) {
    if thread.is_archived() && !thread.is_remote() {
        stop(&TerminalKey::Thread(thread.id.clone()), cx);
    }
}

/// A thread deleted (`forget`): its shell goes with it.
pub(crate) fn thread_deleted(id: &str, cx: &mut App) {
    stop(&TerminalKey::Thread(id.to_owned()), cx);
}

/// The runner's whole list came back (on connecting): a shell whose thread
/// isn't in it was deleted while the app wasn't listening, and one whose
/// thread is archived there goes too (`stopAll(except:)`, `stopArchived`).
pub(crate) fn list_loaded(threads: &HashMap<String, threadbear_core::inbox::ThreadSummary>, cx: &mut App) {
    let Some(terminals) = terminals(cx) else { return };
    let keys: Vec<TerminalKey> = terminals.read(cx).sessions.keys().cloned().collect();
    let keeping: HashSet<&str> = threads.keys().map(String::as_str).collect();
    let mut going = rules::gone(&keys, &keeping);
    going.extend(rules::archived(&keys, threads));
    for key in going {
        stop(&key, cx);
    }
}

/// The grid the element last drew at, for the next shell's first size.
pub(crate) fn note_size(size: GridSize, cx: &mut App) {
    if let Some(terminals) = terminals(cx) {
        if terminals.read(cx).last_size != Some(size) {
            terminals.update(cx, |t, _| t.last_size = Some(size));
        }
    }
}

fn stop(key: &TerminalKey, cx: &mut App) {
    if let Some(terminals) = terminals(cx) {
        terminals.update(cx, |t, cx| t.stop(key, cx));
    }
}

/// Whether a shell block in `block_thread`'s transcript may put its command in
/// the Terminal (`canPutInTerminal(from:)`).
pub(crate) fn can_put(app: &Threadbear, block_thread: &str) -> bool {
    rules::can_put(Some(block_thread), app.selected.as_deref(), &target_of(app))
}

/// The same for drawing a play icon, from the target the panel last worked
/// out, so a frame doesn't look for the thread's folder on disk.
pub(crate) fn shows_put(block_thread: &str, cx: &App) -> bool {
    let Some(terminals) = terminals(cx) else { return false };
    let t = terminals.read(cx);
    let Some(app) = t.app.upgrade() else { return false };
    rules::can_put(Some(block_thread), app.read(cx).selected.as_deref(), &t.target)
}

/// The app, for a shell block's icons, which are drawn where there's none to hand.
pub(crate) fn app(cx: &App) -> Option<Entity<Threadbear>> {
    terminals(cx)?.read(cx).app.upgrade()
}

/// What putting a command in a shell came to (`Terminals.PutOutcome`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PutOutcome {
    Typed,
    AlreadyThere,
    /// Copied instead: another program has the terminal, or someone's own
    /// text is on the prompt.
    Busy,
    /// Copied instead: another command put here is waiting for Return.
    Occupied,
}

/// A shell block's play icon (`putInTerminal`): the command at its thread's
/// prompt, checked again here against the thread it came from, with the panel
/// brought up for it. None when it can't go there (or one is on its way).
pub(crate) fn put_in_terminal(app: &mut Threadbear, block: ShellBlock, block_thread: &str, window: &mut Window, cx: &mut Context<Threadbear>) -> Option<Task<Option<PutOutcome>>> {
    if !can_put(app, block_thread) {
        return None;
    }
    let target = target_of(app);
    if !app.dock.read(cx).controller.is_showing(crate::shell::TERMINAL) {
        bring_up(app, cx);
    }
    let terminals = terminals(cx)?;
    Some(terminals.update(cx, |t, cx| {
        t.set_target(target.clone(), cx);
        t.put(block, target, window, cx)
    }))
}

// ---------------------------------------------------------------- the panel

/// Every shell the app has, by thread (`Terminals`), and the panel that shows
/// the open thread's.
pub(crate) struct Terminals {
    pub(crate) app: WeakEntity<Threadbear>,
    sessions: HashMap<TerminalKey, Entity<Session>>,
    /// The open thread's shell, as of the app's last change.
    pub(crate) target: Target,
    /// What the target was worked out from (the open thread, its folder, and
    /// whether it's archived or another machine's), so the folder is only
    /// looked for on disk when one of those changes, not at every event.
    seen: Option<(Option<String>, String, bool, Option<String>)>,
    /// Shells with a command on its way, so a second click while one is still
    /// starting doesn't queue another.
    putting: HashSet<TerminalKey>,
    /// The grid the last shell was drawn at, so a new one starts the size of
    /// the panel rather than 80 by 24 and redrawn.
    pub(crate) last_size: Option<GridSize>,
    _subs: Vec<Subscription>,
}

impl Terminals {
    fn new(app: &Entity<Threadbear>, cx: &mut Context<Self>) -> Self {
        // Not read here: the app is still being made.
        let watching = cx.observe(app, |this: &mut Terminals, app, cx| {
            let a = app.read(cx);
            let thread = a.open_thread();
            let seen = (a.selected.clone(), thread.map(|t| t.cwd.clone()).unwrap_or_default(), thread.is_some_and(|t| t.is_archived()), thread.and_then(|t| t.runner_id.clone()));
            if this.seen.as_ref() != Some(&seen) {
                let target = target_of(a);
                this.seen = Some(seen);
                this.set_target(target, cx);
            }
        });
        // Every shell ends with the app, as closing Terminal's windows does.
        let quitting = cx.on_app_quit(|this: &mut Terminals, cx| {
            #[cfg(unix)]
            {
                // Those still being ended after their thread went too: the
                // thread that would finish them goes with the app.
                let mut shells: Vec<(i32, Born)> = this.sessions.values().filter_map(|s| s.read(cx).process()).collect();
                shells.extend(ENDING.lock().unwrap_or_else(|e| e.into_inner()).iter().copied());
                end_shells_now(&shells);
            }
            #[cfg(not(unix))]
            {
                let closing: Vec<_> = this.sessions.values().map(|s| s.update(cx, |s, _| { s.shut_down(); s.shared.clone() })).collect();
                // GPUI gives returned quit futures only 200ms. Like the Unix
                // branch, finish closing here before returning that future.
                let deadline = Instant::now() + Duration::from_secs(5);
                while closing.iter().any(|s| !s.reader_closed.load(Ordering::Acquire)) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            async {}
        });
        let home = crate::files::home_dir();
        Terminals { app: app.downgrade(), sessions: HashMap::new(), target: Target { key: TerminalKey::Home, cwd: home, note: None }, seen: None, putting: HashSet::new(), last_size: None, _subs: vec![watching, quitting] }
    }

    fn set_target(&mut self, target: Target, cx: &mut Context<Self>) {
        if self.target != target {
            self.target = target;
            cx.notify();
        }
    }

    /// The target's shell, starting one if it has none (`Terminals.open`).
    fn open(&mut self, target: &Target, cx: &mut Context<Self>) -> Entity<Session> {
        if let Some(session) = self.sessions.get(&target.key) {
            return session.clone();
        }
        let size = self.last_size.unwrap_or_default();
        // Its own view: what it prints redraws it, not the panel round it.
        let session = cx.new(|cx| Session::start(target.clone(), size, cx));
        self.sessions.insert(target.key.clone(), session.clone());
        cx.notify();
        session
    }

    fn focus(&self, session: &Entity<Session>, window: &mut Window, cx: &mut App) {
        let handle = session.read(cx).focus.clone();
        window.focus(&handle, cx);
        crate::web_panels::keyboard_to_app(window);
    }

    /// Ends a thread's shell (it was archived or deleted). Never the shared
    /// home shell, whatever a thread is called: its key is its own.
    fn stop(&mut self, key: &TerminalKey, cx: &mut Context<Self>) {
        if let Some(session) = self.sessions.remove(key) {
            session.update(cx, |s, _| s.shut_down());
            self.putting.remove(key);
            cx.notify();
        }
    }

    /// A new shell in place of one that has exited (New Shell).
    fn restart(&mut self, key: &TerminalKey, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.sessions.remove(key) else { return };
        let target = old.read(cx).target.clone();
        old.update(cx, |s, _| s.shut_down());
        let session = self.open(&target, cx);
        self.focus(&session, window, cx);
    }

    /// Types `block` at the target's prompt, starting a shell if it needs one,
    /// and gives it the keyboard (`Terminals.put`). Never presses Return: the
    /// command came out of a reply, so a person gets to read it first.
    fn put(&mut self, block: ShellBlock, target: Target, window: &mut Window, cx: &mut Context<Self>) -> Task<Option<PutOutcome>> {
        if !self.putting.insert(target.key.clone()) {
            return Task::ready(None);
        }
        if self.sessions.get(&target.key).is_some_and(|s| s.read(cx).exited) {
            if let Some(old) = self.sessions.remove(&target.key) {
                old.update(cx, |s, _| s.shut_down());
            }
        }
        let session = self.open(&target, cx);
        self.focus(&session, window, cx);
        let key = target.key.clone();
        cx.spawn(async move |this, cx| {
            let outcome = loop {
                // Worked out and typed in one go, so nothing comes between.
                let step = session.update(cx, |s, cx| {
                    let step = s.put_step(&block);
                    if step == Step::Type {
                        s.put(&block.command, cx);
                    }
                    step
                });
                match step {
                    Step::Wait => cx.background_executor().timer(Duration::from_millis(50)).await,
                    Step::Type => break Some(PutOutcome::Typed),
                    Step::AlreadyThere => break Some(PutOutcome::AlreadyThere),
                    Step::Occupied => break Some(PutOutcome::Occupied),
                    Step::Busy => break Some(PutOutcome::Busy),
                }
            };
            this.update(cx, |t, _| t.putting.remove(&key)).ok();
            outcome
        })
    }
}

impl Render for Terminals {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.target.clone();
        let session = self.sessions.get(&target.key).cloned();
        let folder = threadbear_core::new_thread::abbreviate_home(&target.cwd, &crate::files::home_dir());
        let mut panel = v_flex().id("terminal-panel").size_full();
        if let Some(note) = target.note.clone() {
            // Over the terminal, when its folder isn't the thread's.
            panel = panel.child(crate::highlights::tipped(
                div()
                    .id("terminal-note")
                    .flex_none()
                    .w_full()
                    .px(px(10.))
                    .py(px(5.))
                    .text_size(px(style::size::DETAIL))
                    .line_height(px(style::natural_line(style::size::DETAIL)))
                    .text_color(style::secondary_text(cx))
                    .line_clamp(2)
                    .child(note.clone()),
                note,
            ));
        }
        let body = match session {
            Some(session) => div().flex_1().min_h_0().w_full().child(session).into_any_element(),
            // No shell for this thread yet: one starts on a click here or ⌘J,
            // not just from looking.
            None => {
                let key = target.key.clone();
                // Its tip is what a spec presses it by, as in the Swift app.
                let tip = format!("Start your login shell in {folder}");
                let start = div()
                    .id("terminal-start")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let target = this.target.clone();
                        if target.key != key {
                            return;
                        }
                        let session = this.open(&target, cx);
                        this.focus(&session, window, cx);
                    }))
                    .child(
                        h_flex()
                            .gap(px(5.))
                            .text_size(px(style::size::LABEL))
                            .line_height(px(style::natural_line(style::size::LABEL)))
                            .text_color(style::secondary_text(cx))
                            .child(crate::icons::symbol("terminal").with_size(px(14.)).text_color(style::secondary_text(cx)))
                            .child("Start a shell"),
                    );
                crate::highlights::tipped(start, tip).into_any_element()
            }
        };
        panel.child(body)
    }
}

// ---------------------------------------------------------------- one shell

/// The grid a shell is drawn at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridSize {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) cell_width: f32,
    pub(crate) cell_height: f32,
}

impl Default for GridSize {
    fn default() -> Self {
        GridSize { cols: 80, rows: 24, cell_width: 8., cell_height: 17. }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

impl GridSize {
    fn window_size(self) -> WindowSize {
        WindowSize { num_lines: self.rows, num_cols: self.cols, cell_width: self.cell_width.round() as u16, cell_height: self.cell_height.round() as u16 }
    }
}

/// Where the grid was last drawn, for turning the pointer into a cell.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawnAt {
    pub(crate) origin: Point<Pixels>,
    pub(crate) cell_width: Pixels,
    pub(crate) cell_height: Pixels,
}

/// What the reader thread tells the app.
enum Note {
    Wakeup,
    Exited,
    Title(String),
    /// A program put this on the clipboard (OSC 52; reading it back is off).
    Copy(String),
    /// A program asked what colour one of its numbered colours is.
    Colour(usize, Arc<dyn Fn(alacritty_terminal::vte::ansi::Rgb) -> String + Sync + Send + 'static>),
}

/// Shared between the shell's reader thread and the app.
struct Shared {
    /// A redraw is on its way: the reader sends one wakeup per frame at most.
    dirty: AtomicBool,
    exited: AtomicBool,
    #[cfg(not(unix))]
    reader_closed: AtomicBool,
    sender: OnceLock<EventLoopSender>,
    size: Mutex<WindowSize>,
    notes: tokio::sync::mpsc::UnboundedSender<Note>,
}

/// What alacritty_terminal tells us as it reads the shell. Called on the
/// reader thread, so the app hears through a channel; replies the terminal
/// owes the program (where the cursor is, how big it is) go straight back.
#[derive(Clone)]
pub(crate) struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        let shared = &self.0;
        match event {
            TermEvent::Wakeup | TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                if !shared.dirty.swap(true, Ordering::AcqRel) {
                    let _ = shared.notes.send(Note::Wakeup);
                }
            }
            TermEvent::PtyWrite(text) => {
                if let Some(sender) = shared.sender.get() {
                    let _ = sender.send(Msg::Input(Cow::Owned(text.into_bytes())));
                }
            }
            TermEvent::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(sender) = shared.sender.get() {
                    let _ = sender.send(Msg::Input(Cow::Owned(format(size).into_bytes())));
                }
            }
            TermEvent::ChildExit(_) | TermEvent::Exit => {
                if !shared.exited.swap(true, Ordering::AcqRel) {
                    let _ = shared.notes.send(Note::Exited);
                }
            }
            TermEvent::Title(title) => {
                let _ = shared.notes.send(Note::Title(title));
            }
            TermEvent::ResetTitle => {
                let _ = shared.notes.send(Note::Title(String::new()));
            }
            TermEvent::ClipboardStore(_, text) => {
                let _ = shared.notes.send(Note::Copy(text));
            }
            TermEvent::ColorRequest(index, format) => {
                let _ = shared.notes.send(Note::Colour(index, format));
            }
            TermEvent::ClipboardLoad(..) | TermEvent::Bell => {}
        }
    }
}

/// One shell in one pty (`TerminalSession`).
pub(crate) struct Session {
    pub(crate) target: Target,
    pub(crate) term: Arc<FairMutex<Term<Listener>>>,
    shared: Arc<Shared>,
    /// The shell's process, which leads its own session, and when it
    /// started, which tells it from a later process given the same pid.
    pid: Option<i32>,
    born: Option<Born>,
    /// The shell's program, symlinks followed, which has to be what owns
    /// the terminal before anything's typed there.
    shell: Option<PathBuf>,
    /// Our own copy of the pty's controller, to ask who has the terminal.
    #[cfg(unix)]
    pty: Option<std::os::fd::OwnedFd>,
    started: Instant,
    pub(crate) exited: bool,
    /// Why it couldn't start, if it couldn't.
    pub(crate) failed: Option<String>,
    pub(crate) title: String,
    /// What's on its prompt, as far as keys and play icons say.
    pub(crate) line: Line,
    pub(crate) focus: FocusHandle,
    pub(crate) size: GridSize,
    pub(crate) drawn_at: Option<DrawnAt>,
    /// It had the keyboard when it was last drawn.
    pub(crate) focused: bool,
    /// An input method's text still being composed (a Japanese word, an
    /// accent waiting for its letter), drawn at the cursor until it's typed.
    pub(crate) marked: String,
    /// Part of a line scrolled by a trackpad that hasn't added up to a line.
    scroll_rest: f32,
    /// A drag selecting text, or one going to the program (mouse reporting).
    dragging: Option<Drag>,
    /// The cursor's blink, while it has the keyboard: shown and gone 0.7 s
    /// each, as SwiftTerm's caret fades out and back, and shown while you
    /// type (`kept` skips a beat after a key).
    pub(crate) blink_shown: bool,
    blink_kept: bool,
    _blink: Task<()>,
    _events: Task<()>,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Selecting,
    Reporting,
}

impl Session {
    fn start(target: Target, size: GridSize, cx: &mut Context<Self>) -> Self {
        let (notes, mut incoming) = tokio::sync::mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            dirty: AtomicBool::new(false), exited: AtomicBool::new(false),
            #[cfg(not(unix))]
            reader_closed: AtomicBool::new(false),
            sender: OnceLock::new(), size: Mutex::new(size.window_size()), notes
        });
        // SwiftTerm keeps 500 lines back, which loses a build's errors, so
        // this keeps alacritty's 10,000. The cursor blinks, as SwiftTerm's
        // does, unless a program asks for a steady one.
        let config = Config { default_cursor_style: CursorStyle { shape: CursorShape::Block, blinking: true }, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, Listener(shared.clone()))));
        let events = cx.spawn(async move |this, cx| {
            while let Some(note) = incoming.recv().await {
                if this.update(cx, |s, cx| s.heard(note, cx)).is_err() {
                    break;
                }
            }
        });
        // A beat every 0.7 s that only redraws while the terminal has the
        // keyboard: an idle terminal behind others costs nothing.
        let blink = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(700)).await;
            let alive = this.update(cx, |s, cx| {
                if std::mem::take(&mut s.blink_kept) || !s.focused {
                    if !s.blink_shown {
                        s.blink_shown = true;
                        cx.notify();
                    }
                    return;
                }
                s.blink_shown = !s.blink_shown;
                cx.notify();
            });
            if alive.is_err() {
                break;
            }
        });
        let mut session = Session {
            target,
            term,
            shared,
            pid: None,
            born: None,
            shell: None,
            #[cfg(unix)]
            pty: None,
            started: Instant::now(),
            exited: false,
            failed: None,
            title: String::new(),
            line: Line::Empty,
            focus: cx.focus_handle(),
            size,
            drawn_at: None,
            focused: false,
            marked: String::new(),
            scroll_rest: 0.,
            dragging: None,
            blink_shown: true,
            blink_kept: false,
            _blink: blink,
            _events: events,
        };
        if let Err(e) = session.spawn_shell() {
            session.exited = true;
            session.failed = Some(e.to_string());
            #[cfg(not(unix))]
            session.shared.reader_closed.store(true, Ordering::Release);
        }
        session
    }

    /// Starts the login shell in the target's folder, and the thread that
    /// reads it (`TerminalSession.start`).
    fn spawn_shell(&mut self) -> std::io::Result<()> {
        let shell = std::env::var("SHELL").ok();
        let lang = std::env::var("LANG").ok();
        let launch = rules::launch(rules::Os::current(), shell.as_deref(), lang.as_deref());
        let options = tty::Options {
            shell: Some(tty::Shell::new(launch.program, launch.args)),
            working_directory: Some(std::path::PathBuf::from(&self.target.cwd)),
            drain_on_exit: true,
            env: launch.env.into_iter().collect(),
            ..Default::default()
        };
        // Starts a program: the shell, as above, in a folder the thread names.
        // Nothing a link, a file or a page says (open_gate_source.rs allows it).
        let pty = tty::new(&options, self.size.window_size(), 0)?;
        self.shell = std::fs::canonicalize(&launch.shell).ok();
        #[cfg(unix)]
        {
            use std::os::fd::AsFd as _;
            let pid = pty.child().id() as i32;
            self.pid = Some(pid);
            self.born = born(pid);
            self.pty = pty.file().as_fd().try_clone_to_owned().ok();
        }
        let event_loop = EventLoop::new(self.term.clone(), Listener(self.shared.clone()), pty, true, false)?;
        let _ = self.shared.sender.set(event_loop.channel());
        // Its thread runs until the shell ends or is told to stop; nothing
        // waits for it.
        let reader = event_loop.spawn();
        #[cfg(unix)]
        drop(reader);
        #[cfg(not(unix))]
        {
            let shared = self.shared.clone();
            std::thread::spawn(move || {
                drop(reader.join()); // drops ConPTY, including its shell
                shared.reader_closed.store(true, Ordering::Release);
            });
        }
        Ok(())
    }

    fn heard(&mut self, note: Note, cx: &mut Context<Self>) {
        match note {
            Note::Wakeup => self.shared.dirty.store(false, Ordering::Release),
            Note::Exited => {
                self.exited = true;
                #[cfg(unix)]
                {
                    self.pty = None;
                }
            }
            Note::Title(title) => self.title = title,
            Note::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Note::Colour(index, format) => {
                let colours = crate::terminal_element::Colours::current(cx);
                let (r, g, b) = colours.indexed(index);
                self.write(format(alacritty_terminal::vte::ansi::Rgb { r, g, b }).into_bytes());
            }
        }
        cx.notify();
    }

    fn sender(&self) -> Option<&EventLoopSender> {
        if self.exited {
            return None;
        }
        self.shared.sender.get()
    }

    pub(crate) fn write(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(sender) = self.sender() {
            let _ = sender.send(Msg::Input(Cow::Owned(bytes)));
        }
    }

    /// The grid the element worked out for the panel's size.
    pub(crate) fn resize(&mut self, size: GridSize) {
        if self.size == size {
            return;
        }
        self.size = size;
        self.term.lock().resize(size);
        *self.shared.size.lock().unwrap_or_else(|e| e.into_inner()) = size.window_size();
        // Not once the shell has gone: the library ends the whole app if the
        // pty's size can't be set.
        if let Some(sender) = self.sender() {
            let _ = sender.send(Msg::Resize(size.window_size()));
        }
    }

    /// Ends the shell (`end_shell`), then the reader thread, which closes
    /// the pty once the shell has gone.
    fn shut_down(&mut self) {
        #[cfg(unix)]
        if let (Some(pid), Some(when), true) = (self.pid, self.born, self.running()) {
            end_shell(pid, when);
        }
        if let Some(sender) = self.shared.sender.get() {
            let _ = sender.send(Msg::Shutdown);
        }
        self.exited = true;
        #[cfg(unix)]
        {
            self.pty = None;
        }
    }

    /// The shell's process, while it's running.
    #[cfg(unix)]
    fn process(&self) -> Option<(i32, Born)> {
        Some((self.pid?, self.born?)).filter(|_| self.running())
    }

    fn running(&self) -> bool {
        !self.exited && !self.shared.exited.load(Ordering::Acquire)
    }

    /// The shell owns the terminal, rather than a program it's running.
    pub(crate) fn shell_in_front(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd as _;
            let (Some(pid), Some(pty), Some(shell)) = (self.pid, self.pty.as_ref(), self.shell.as_ref()) else { return false };
            if !self.running() {
                return false;
            }
            let group = unsafe { libc::tcgetpgrp(pty.as_raw_fd()) };
            // Its own group has the terminal, and it's still the shell:
            // `exec ssh` and `exec vim` keep both its pid and its group.
            group > 0 && group == pid && program_of(pid).is_some_and(|now| rules::is_the_shell(shell, &now))
        }
        #[cfg(not(unix))]
        false
    }

    /// The shell alone has the terminal, which is what a command is typed
    /// into: in front, and nothing else in its group. With job control off
    /// (`unsetopt monitor`) a program the shell runs stays in the shell's
    /// group, so the group having the terminal doesn't say the shell is the
    /// one reading it: `ssh -tt elsewhere` would be (Codex's review, 11 Nov).
    /// Only asked when typing, as on Linux it reads every process's group.
    fn shell_alone_in_front(&self) -> bool {
        #[cfg(unix)]
        {
            self.shell_in_front() && self.pid.is_some_and(alone_in_group)
        }
        #[cfg(not(unix))]
        false
    }

    fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// What putting `block` here should do now (`putStep`).
    fn put_step(&self, block: &ShellBlock) -> Step {
        if self.exited {
            return Step::Busy;
        }
        let starting = self.started.elapsed().as_secs_f64() < rules::STARTING_FOR_SECS;
        rules::step(block, self.shell_alone_in_front(), self.mode().contains(TermMode::BRACKETED_PASTE), starting, &self.line)
    }

    /// Pasted as the terminal's own paste is, in brackets (only ever typed
    /// when the shell has asked for them), so a newline in it runs nothing.
    fn put(&mut self, command: &str, cx: &mut Context<Self>) {
        self.scroll_to_bottom();
        self.write(rules::paste_bytes(command, true));
        self.set_line(Line::Put(command.to_owned()), cx);
    }

    fn set_line(&mut self, line: Line, cx: &mut Context<Self>) {
        if self.line != line {
            self.line = line;
            cx.notify();
        }
    }

    /// A command is waiting at the prompt for Return, so the panel says so.
    pub(crate) fn waiting_for_return(&self) -> bool {
        matches!(self.line, Line::Put(_)) && !self.exited
    }

    fn scroll_to_bottom(&mut self) {
        let mut term = self.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    }

    /// The screen's lines as text, trailing spaces gone.
    pub(crate) fn screen_lines(&self) -> Vec<String> {
        let term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        let cols = term.columns();
        (0..term.screen_lines() as i32)
            .map(|row| {
                let line = GridLine(row - offset);
                let text: String = (0..cols).map(|c| term.grid()[line][Column(c)].c).collect();
                text.trim_end().to_owned()
            })
            .collect()
    }

    /// The screen as lines of text rather than rows: a row the text wrapped
    /// from (the grid marks it) is joined to the next, and trailing spaces
    /// go. For the specs, as a command after a long prompt wraps in a narrow
    /// panel, and the rows alone can't say which of them are one line.
    #[cfg(feature = "automation")]
    pub(crate) fn screen_text_lines(&self) -> Vec<String> {
        use alacritty_terminal::term::cell::Flags;
        let term = self.term.lock();
        let grid = term.grid();
        let offset = grid.display_offset() as i32;
        let cols = term.columns();
        let mut lines = Vec::new();
        let mut line = String::new();
        for row in 0..term.screen_lines() as i32 {
            let row = &grid[GridLine(row - offset)];
            // As the screen draws them (terminal_element.rs): a wide
            // character's spacer cells aren't characters, and an accent
            // typed after its letter rides on that letter's cell.
            for c in 0..cols {
                let cell = &row[Column(c)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                line.push(cell.c);
                line.extend(cell.zerowidth().into_iter().flatten());
            }
            if cols == 0 || !row[Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
                lines.push(line.trim_end().to_owned());
                line.clear();
            }
        }
        if !line.is_empty() {
            lines.push(line.trim_end().to_owned());
        }
        lines
    }

    // ---- Keys and text ----

    pub(crate) fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = k.modifiers;
        // ⌘ on the Mac, Control and Shift elsewhere, as terminals there have it.
        let command = if cfg!(target_os = "macos") { m.platform } else { m.control && m.shift };
        if command {
            match k.key.as_str() {
                "c" => {
                    self.copy(cx);
                    cx.stop_propagation();
                }
                "v" => {
                    self.paste(cx);
                    cx.stop_propagation();
                }
                "a" => {
                    self.select_all(cx);
                    cx.stop_propagation();
                }
                // The rest are the app's (⌘J, ⌘E, ⌘N...).
                _ => {}
            }
            return;
        }
        if self.exited {
            return;
        }
        let modes = {
            let mode = self.mode();
            rules::Modes { app_cursor: mode.contains(TermMode::APP_CURSOR), alt_screen: mode.contains(TermMode::ALT_SCREEN), option_is_meta: true }
        };
        let key = rules::Key { key: &k.key, typed: k.key_char.as_deref(), shift: m.shift, control: m.control, alt: m.alt, command: m.platform };
        match rules::encode_key(&key, modes) {
            KeyAction::Send(bytes) => {
                self.note_key(&k.key, m);
                self.typed(bytes, cx);
                cx.stop_propagation();
            }
            KeyAction::ScrollPage(direction) => {
                let mut term = self.term.lock();
                term.scroll_display(if direction < 0 { Scroll::PageUp } else { Scroll::PageDown });
                drop(term);
                cx.notify();
                cx.stop_propagation();
            }
            KeyAction::Nothing => cx.stop_propagation(),
            // A plain character comes back through the input method
            // (`replace_text_in_range`), which is how accents, other
            // languages' input and dictation reach a terminal too.
            KeyAction::Pass => {}
        }
        let _ = window;
    }

    /// Keys for a running program (vim's `q`) say nothing about the prompt.
    fn note_key(&mut self, key: &str, m: Modifiers) {
        if self.shell_in_front() {
            self.line = rules::line_after_key(&self.line, key, m.control, m.alt, m.shift, m.platform);
        }
    }

    /// Bytes from the keyboard: the view goes back to the bottom, as
    /// Terminal's does when you type, and a selection goes.
    fn typed(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        {
            let mut term = self.term.lock();
            term.selection = None;
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
        }
        self.write(bytes);
        self.blink_shown = true;
        self.blink_kept = true;
        cx.notify();
    }

    /// Text from the input method, a paste or dictation: anything but Return
    /// counts as someone's own text on the prompt.
    pub(crate) fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.marked.clear();
        if text.is_empty() || self.exited {
            cx.notify();
            return;
        }
        // More than one character at once with a line break or another
        // control in it (an input method's or a text expander's snippet,
        // dictated paragraphs) is a paste, not keys: each break would be
        // Return and run the line before it (Codex's review, 11 Nov). Line
        // breaks on their own are still Return.
        if rules::is_bulk_text(text) {
            self.paste_text(text.to_owned(), cx);
            return;
        }
        if text != "\r" && text != "\n" && self.shell_in_front() {
            self.line = Line::Edited;
        }
        self.typed(text.replace('\n', "\r").into_bytes(), cx);
    }

    pub(crate) fn set_marked(&mut self, text: &str, cx: &mut Context<Self>) {
        self.marked = text.to_owned();
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self.term.lock().selection_to_string();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// ⌘V: the clipboard's text as a paste, in brackets when the program
    /// asked. When it hasn't and the text has a line break, each line would
    /// run as it arrived, so that's asked about first (`paste_runs`).
    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste_text(text, cx);
        }
    }

    /// `text` as a paste: in brackets when the program asked, or asked
    /// about first when it hasn't and a line break would run what's before
    /// it. What's asked about is what's sent.
    fn paste_text(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() || self.paste_now(&text, false, cx) {
            return;
        }
        // From a task: the question's loop can't run inside an update.
        cx.spawn(async move |this, cx| {
            let (title, detail) = rules::paste_question(&text);
            if crate::alert::confirm(&title, detail, "Paste", "Cancel") {
                let _ = this.update(cx, |s, cx| s.paste_now(&text, true, cx));
            }
        })
        .detach();
    }

    /// Pastes `text` for the mode the program's in now (read once, for both
    /// the check and the bytes), unless it would run lines and that hasn't
    /// been said yes to: false then, with nothing sent.
    fn paste_now(&mut self, text: &str, asked: bool, cx: &mut Context<Self>) -> bool {
        if self.exited {
            return true;
        }
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        if !bracketed && !asked && rules::paste_runs(text) {
            return false;
        }
        if self.shell_in_front() {
            self.line = Line::Edited;
        }
        self.typed(rules::paste_bytes(text, bracketed), cx);
        true
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        let top = term.grid().topmost_line();
        let bottom = term.grid().bottommost_line();
        let last = term.grid().last_column();
        let mut selection = Selection::new(SelectionType::Simple, GridPoint::new(top, Column(0)), Side::Left);
        selection.update(GridPoint::new(bottom, last), Side::Right);
        term.selection = Some(selection);
        drop(term);
        cx.notify();
    }

    // ---- The mouse ----

    /// The cell under `position`, and which half of it.
    fn cell_at(&self, position: Point<Pixels>, clamp: bool) -> Option<(usize, usize, Side)> {
        let at = self.drawn_at?;
        let x = f32::from(position.x - at.origin.x);
        let y = f32::from(position.y - at.origin.y);
        let (w, h) = (f32::from(at.cell_width), f32::from(at.cell_height));
        let (cols, rows) = (self.size.cols as f32, self.size.rows as f32);
        if !clamp && (x < 0. || y < 0. || x >= w * cols || y >= h * rows) {
            return None;
        }
        let col = (x / w).floor().clamp(0., cols - 1.);
        let row = (y / h).floor().clamp(0., rows - 1.);
        let side = if x - col * w < w / 2. { Side::Left } else { Side::Right };
        Some((col as usize, row as usize, side))
    }

    fn grid_point(&self, col: usize, row: usize) -> GridPoint {
        let offset = self.term.lock().grid().display_offset() as i32;
        GridPoint::new(GridLine(row as i32 - offset), Column(col))
    }

    fn reports_mouse(&self, shift: bool) -> bool {
        !shift && self.mode().intersects(TermMode::MOUSE_MODE) && !self.exited
    }

    fn report(&self, button: rules::MouseButton, pressed: bool, motion: bool, col: usize, row: usize, m: Modifiers) {
        let sgr = self.mode().contains(TermMode::SGR_MOUSE);
        if let Some(bytes) = rules::mouse_report(button, pressed, motion, col, row, m.shift, m.alt, m.control, sgr) {
            self.write(bytes);
        }
    }

    pub(crate) fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        crate::web_panels::keyboard_to_app(window);
        let Some((col, row, side)) = self.cell_at(event.position, false) else { return };
        let button = match event.button {
            MouseButton::Left => rules::MouseButton::Left,
            MouseButton::Middle => rules::MouseButton::Middle,
            MouseButton::Right => rules::MouseButton::Right,
            _ => return,
        };
        if self.reports_mouse(event.modifiers.shift) {
            self.report(button, true, false, col, row, event.modifiers);
            self.dragging = Some(Drag::Reporting);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let point = self.grid_point(col, row);
        let mut term = self.term.lock();
        match (event.click_count, event.modifiers.shift, term.selection.as_mut()) {
            // Shift-click carries the selection on to here.
            (1, true, Some(selection)) => selection.update(point, side),
            (n, _, _) => {
                let kind = match n {
                    2 => SelectionType::Semantic,
                    3.. => SelectionType::Lines,
                    _ => SelectionType::Simple,
                };
                term.selection = Some(Selection::new(kind, point, side));
            }
        }
        drop(term);
        self.dragging = Some(Drag::Selecting);
        cx.notify();
    }

    pub(crate) fn mouse_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.dragging else { return };
        if event.pressed_button.is_none() {
            self.dragging = None;
            return;
        }
        let Some((col, row, side)) = self.cell_at(event.position, true) else { return };
        match drag {
            Drag::Reporting => {
                if self.mode().intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                    self.report(rules::MouseButton::Left, true, true, col, row, event.modifiers);
                }
            }
            Drag::Selecting => {
                let point = self.grid_point(col, row);
                if let Some(selection) = self.term.lock().selection.as_mut() {
                    selection.update(point, side);
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        // Every button let go in the window comes here; only a drag that
        // started in the terminal is its business.
        let Some(drag) = self.dragging.take() else { return };
        if drag == Drag::Reporting {
            if let Some((col, row, _)) = self.cell_at(event.position, true) {
                let button = match event.button {
                    MouseButton::Middle => rules::MouseButton::Middle,
                    MouseButton::Right => rules::MouseButton::Right,
                    _ => rules::MouseButton::Left,
                };
                self.report(button, false, false, col, row, event.modifiers);
            }
        }
        // A click that selected nothing leaves no selection behind.
        let mut term = self.term.lock();
        if term.selection.as_ref().is_some_and(|s| s.is_empty()) {
            term.selection = None;
        }
        drop(term);
        cx.notify();
    }

    /// The wheel or a trackpad: back through what's scrolled off, or for a
    /// full-screen program, its own scrolling (the arrow keys, or the wheel
    /// itself when it asked for the mouse), as Terminal does.
    pub(crate) fn scrolled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let Some(at) = self.drawn_at else { return };
        let dy = f32::from(event.delta.pixel_delta(at.cell_height).y);
        self.scroll_rest += dy / f32::from(at.cell_height);
        let lines = self.scroll_rest.trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_rest -= lines as f32;
        let mode = self.mode();
        if self.reports_mouse(event.modifiers.shift) {
            let (col, row, _) = self.cell_at(event.position, true).unwrap_or((0, 0, Side::Left));
            let button = if lines > 0 { rules::MouseButton::WheelUp } else { rules::MouseButton::WheelDown };
            for _ in 0..lines.unsigned_abs().min(20) {
                self.report(button, true, false, col, row, event.modifiers);
            }
        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let arrow = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            self.write(arrow.repeat(lines.unsigned_abs().min(20) as usize).into_bytes());
        } else {
            self.term.lock().scroll_display(Scroll::Delta(lines));
            cx.notify();
        }
        cx.stop_propagation();
    }

    // ---- For the automation ----

    fn summary(&self) -> String {
        format!("{} {} in {}", self.target.key, if self.exited { "exited" } else { "running" }, self.target.cwd)
    }
}

impl Focusable for Session {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Session {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colours = crate::terminal_element::Colours::current(cx);
        let focused = self.focus.is_focused(window);
        self.focused = focused;
        let me = cx.entity();
        let mut view = div()
            .id("terminal-session")
            .relative()
            .size_full()
            .bg(colours.background_hsla())
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, window, cx| {
                window.focus(&this.focus, cx);
                crate::web_panels::keyboard_to_app(window);
            }))
            // A little in from the panel's edges, on the terminal's own
            // background (TerminalHost.Container.inset).
            .child(div().size_full().px(px(10.)).py(px(6.)).child(TerminalElement::new(me, focused, self.blink_shown)));
        if self.waiting_for_return() {
            // Over whatever's in the terminal's top corner.
            view = view.child(
                div()
                    .absolute()
                    .top(px(5.))
                    .right(px(6.))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .bg(material(cx))
                    .text_size(px(style::size::DETAIL))
                    .line_height(px(style::natural_line(style::size::DETAIL)))
                    .text_color(style::secondary_text(cx))
                    .child("Return runs it"),
            );
        }
        if self.exited {
            let folder = threadbear_core::new_thread::abbreviate_home(&self.target.cwd, &crate::files::home_dir());
            let words = match &self.failed {
                Some(why) => format!("The shell couldn't start: {why}"),
                None => "The shell has exited.".to_owned(),
            };
            let key = self.target.key.clone();
            view = view.child(
                div().absolute().bottom_0().left_0().right_0().flex().justify_center().child(
                    h_flex()
                        .m(px(10.))
                        .p(px(8.))
                        .gap(px(8.))
                        .rounded(px(8.))
                        .bg(material(cx))
                        .text_size(px(style::size::LABEL))
                        .line_height(px(style::natural_line(style::size::LABEL)))
                        .child(div().text_color(style::secondary_text(cx)).child(words))
                        .child(new_shell_button(key, &folder, cx)),
                ),
            );
        }
        view
    }
}

/// New Shell: a SwiftUI push button.
fn new_shell_button(key: TerminalKey, folder: &str, cx: &App) -> AnyElement {
    crate::push_button::PushButton::new("terminal-new-shell", "New Shell", format!("Start a new shell in {folder}")).render(cx, move |_, window, cx| {
        if let Some(terminals) = terminals(cx) {
            let key = key.clone();
            terminals.update(cx, |t, cx| t.restart(&key, window, cx));
        }
    })
}

/// SwiftUI's `.regularMaterial` over the terminal: a frosted grey, lighter
/// than the dark terminal and darker than the light one.
fn material(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        hsla(0., 0., 0.21, 0.94)
    } else {
        hsla(0., 0., 0.95, 0.94)
    }
}

// ---------------------------------------------------------------- the input method

impl EntityInputHandler for Session {
    fn text_for_range(&mut self, range: std::ops::Range<usize>, adjusted: &mut Option<std::ops::Range<usize>>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<String> {
        let utf16: Vec<u16> = self.marked.encode_utf16().collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&utf16[range]))
    }

    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _window: &mut Window, _cx: &mut Context<Self>) -> Option<UTF16Selection> {
        let end = self.marked.encode_utf16().count();
        Some(UTF16Selection { range: end..end, reversed: false })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        (!self.marked.is_empty()).then(|| 0..self.marked.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked.clear();
        cx.notify();
    }

    fn replace_text_in_range(&mut self, _range: Option<std::ops::Range<usize>>, text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.insert_text(text, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, _range: Option<std::ops::Range<usize>>, new_text: &str, _new_selected: Option<std::ops::Range<usize>>, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_marked(new_text, cx);
    }

    /// The cursor's cell, so the input method's window opens beside it.
    fn bounds_for_range(&mut self, _range: std::ops::Range<usize>, _element_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let at = self.drawn_at?;
        let term = self.term.lock();
        let cursor = term.grid().cursor.point;
        let row = cursor.line.0 + term.grid().display_offset() as i32;
        let origin = point(at.origin.x + at.cell_width * cursor.column.0 as f32, at.origin.y + at.cell_height * row.max(0) as f32);
        Some(Bounds { origin, size: size(at.cell_width, at.cell_height) })
    }

    fn character_index_for_point(&mut self, _point: Point<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        !self.exited
    }
}

// ---------------------------------------------------------------- the automation

/// The Terminal for the dump, in the Swift app's shape: whether it's showing,
/// the open thread's shell and its screen, and every shell.
#[cfg(feature = "automation")]
pub(crate) fn dump(app: &Threadbear, cx: &App) -> Value {
    let Some(terminals) = terminals(cx) else { return Value::Null };
    let t = terminals.read(cx);
    let target = target_of(app);
    let session = t.sessions.get(&target.key).map(|s| s.read(cx));
    let mut shells: Vec<String> = t.sessions.values().map(|s| s.read(cx).summary()).collect();
    shells.sort();
    json!({
        "shown": app.dock.read(cx).controller.is_showing(crate::shell::TERMINAL),
        "cwd": target.cwd,
        "note": target.note,
        "shell": session.map_or("none", |s| if s.exited { "exited" } else { "running" }),
        "waitingForReturn": session.is_some_and(|s| s.waiting_for_return()),
        "pastesMarked": session.is_some_and(|s| !s.exited && s.mode().contains(TermMode::BRACKETED_PASTE)),
        "focused": session.is_some_and(|s| s.focused),
        "cols": session.map(|s| s.size.cols),
        "rows": session.map(|s| s.size.rows),
        "screen": session.map(|s| s.screen_lines()),
        "lines": session.map(|s| s.screen_text_lines()),
        "shells": shells,
    })
}

// ---------------------------------------------------------------- the shell's process

/// When a process started, which tells it from a later one given the same
/// pid: the library reaps the shell when it ends, and its pid can then go to
/// anything else, so a signal only goes to the same pid started at the same
/// time. None once it has ended, a zombie included. Between the check and
/// the signal a pid could in theory be reaped and handed on, but both
/// systems give pids out in turn, so that needs every pid used in between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
struct Born(u64, u64);

#[cfg(target_os = "macos")]
fn born(pid: i32) -> Option<Born> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
    // 5 is SZOMB: it has ended and is waiting to be reaped.
    (read == size && info.pbi_status != 5).then_some(Born(info.pbi_start_tvsec, info.pbi_start_tvusec))
}

#[cfg(target_os = "linux")]
fn born(pid: i32) -> Option<Born> {
    // The 22nd field of /proc/<pid>/stat, clock ticks from boot to its
    // start, counted after the name in brackets, which can have spaces in.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut fields = stat[stat.rfind(')')? + 1..].split_whitespace();
    // The state comes first: Z or X has ended.
    if matches!(fields.next()?, "Z" | "X") {
        return None;
    }
    Some(Born(fields.nth(18)?.parse().ok()?, 0))
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn born(_pid: i32) -> Option<Born> {
    None
}

/// Whether `pid`'s group has no other process in it.
#[cfg(target_os = "macos")]
fn alone_in_group(pid: i32) -> bool {
    let mut pids = [0 as libc::pid_t; 64];
    let size = std::mem::size_of_val(&pids) as i32;
    let read = unsafe { libc::proc_listpgrppids(pid, pids.as_mut_ptr().cast(), size) };
    // Read from what's filled in rather than the count, so a group too big
    // for the list still has someone else in it.
    read >= 0 && pids.contains(&pid) && pids.iter().all(|&p| p == 0 || p == pid)
}

#[cfg(target_os = "linux")]
fn alone_in_group(pid: i32) -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    !dir.flatten().any(|entry| {
        let Some(other) = entry.file_name().to_str().and_then(|n| n.parse::<i32>().ok()).filter(|&o| o != pid) else { return false };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{other}/stat")) else { return false };
        // The group is the 5th field: the 3rd after the name in brackets.
        stat.rfind(')').and_then(|at| stat[at + 1..].split_whitespace().nth(2)).and_then(|g| g.parse::<i32>().ok()) == Some(pid)
    })
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn alone_in_group(_pid: i32) -> bool {
    false
}

/// The program a process is running now.
#[cfg(target_os = "macos")]
fn program_of(pid: i32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt as _;
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let read = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    if read <= 0 {
        return None;
    }
    path.truncate(read as usize);
    Some(PathBuf::from(std::ffi::OsString::from_vec(path)))
}

#[cfg(target_os = "linux")]
fn program_of(pid: i32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn program_of(_pid: i32) -> Option<PathBuf> {
    None
}

/// `signal` to the shell, if `pid` is still it.
#[cfg(unix)]
fn signal(pid: i32, when: Born, signal: i32) -> bool {
    born(pid) == Some(when) && unsafe { libc::kill(pid, signal) } == 0
}

/// Ends a shell as closing a Terminal window does: SIGHUP, which an
/// interactive shell passes on to its jobs. One that ignores it (`trap ''
/// HUP`) would stay for good, still in a folder the next thread may be given:
/// the library waits for it to end before closing the pty, so it never reads
/// the end of its terminal either (Codex's review, 11 Nov). So it's asked
/// again with SIGTERM, which an interactive zsh ignores too, and then made
/// to go with SIGKILL, two seconds apart, on a thread of its own.
#[cfg(unix)]
fn end_shell(pid: i32, when: Born) {
    if !signal(pid, when, libc::SIGHUP) {
        return;
    }
    ENDING.lock().unwrap_or_else(|e| e.into_inner()).push((pid, when));
    std::thread::spawn(move || {
        for next in [libc::SIGTERM, libc::SIGKILL] {
            std::thread::sleep(Duration::from_secs(2));
            if !signal(pid, when, next) {
                break;
            }
        }
        // Gone, or told to with SIGKILL, which it can't refuse.
        ENDING.lock().unwrap_or_else(|e| e.into_inner()).retain(|&shell| shell != (pid, when));
    });
}

/// Shells `end_shell` is still ending, so quitting in the meantime ends them
/// itself (Codex's review, 11 Nov: archive, then quit within four seconds).
#[cfg(unix)]
static ENDING: Mutex<Vec<(i32, Born)>> = Mutex::new(Vec::new());

/// Ends every shell as the app quits: as `end_shell`, but within about half
/// a second and over before this returns, as nothing the app starts outlives
/// it. One that ignores SIGHUP and SIGTERM and never reads its terminal
/// (`trap '' HUP TERM; while :; do :; done`) would otherwise go on after the
/// app (Codex's review, 11 Nov). A shell that does as asked is gone within
/// the first wait.
#[cfg(unix)]
fn end_shells_now(shells: &[(i32, Born)]) {
    let alive = || shells.iter().any(|&(pid, when)| born(pid) == Some(when));
    for (next, wait) in [(libc::SIGHUP, 400), (libc::SIGTERM, 200), (libc::SIGKILL, 100)] {
        for &(pid, when) in shells {
            signal(pid, when, next);
        }
        let until = Instant::now() + Duration::from_millis(wait);
        while alive() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !alive() {
            return;
        }
    }
}

/// The Swift app's `terminal` command: `{"toggle": true}` is ⌘J, `{"show":
/// true}` brings the panel up with nothing started, `{"type": "ls\r"}` types
/// into the showing thread's shell (starting one), `{"put": "ls"}` does what a
/// shell block's play icon does, `{"scroll": -5}` turns the wheel over the
/// terminal (back is up, so negative), `{"select": "all"}`, `{"copy": true}`
/// and `{"paste": "text"}` are ⌘A, ⌘C and ⌘V through its keys, `{"commit":
/// "text"}` is an input method's text all at once, and `{"wait": ms}` waits
/// after. Each answers with what the dump's `terminal` says.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    use gpui_kit::test::TestWindowExt as _;
    let terminals = terminals(cx).ok_or("no terminals")?;
    if cmd["toggle"].as_bool() == Some(true) {
        app.update(cx, |a, cx| toggle(a, window, cx));
    }
    if cmd["show"].as_bool() == Some(true) {
        app.update(cx, |a, cx| show_panel(a, cx));
    }
    let target = target_of(app.read(cx));
    terminals.update(cx, |t, cx| t.set_target(target.clone(), cx));
    if let Some(text) = cmd["type"].as_str() {
        let session = terminals.update(cx, |t, cx| t.open(&target, cx));
        let text = text.to_owned();
        session.update(cx, |s, cx| s.typed(text.into_bytes(), cx));
    }
    // `{"commit": "text"}` is an input method putting the whole text in at
    // once (a text expander's snippet, dictation), not a key at a time.
    if let Some(text) = cmd["commit"].as_str() {
        let session = terminals.update(cx, |t, cx| t.open(&target, cx));
        session.update(cx, |s, cx| s.insert_text(text, cx));
    }
    // `keys` types through the keyboard and the input method as a person
    // would, and `press` is one key ("enter", "ctrl-c", "up"), each with the
    // terminal given the keyboard first.
    if cmd["keys"].is_string() || cmd["press"].is_string() {
        let session = terminals.update(cx, |t, cx| t.open(&target, cx));
        let handle = session.read(cx).focus.clone();
        window.focus(&handle, cx);
        crate::web_panels::keyboard_to_app(window);
        window.render_frame(cx);
        if let Some(text) = cmd["keys"].as_str() {
            window.input(text, cx);
        }
        if let Some(key) = cmd["press"].as_str() {
            window.press(key, cx);
        }
    }
    // What a shell block's play icon does, the copy when the shell's busy
    // included (shell_blocks.rs).
    if let Some(command) = cmd["put"].as_str() {
        let id = app.read(cx).selected.clone().unwrap_or_default();
        crate::shell_blocks::put(command, &id, window, cx);
    }
    let session = terminals.read(cx).sessions.get(&target.key).cloned();
    if let Some(session) = &session {
        window.render_frame(cx);
        let handle = session.read(cx).focus.clone();
        if cmd["select"].as_str() == Some("all") || cmd["copy"].as_bool() == Some(true) || cmd["paste"].is_string() {
            window.focus(&handle, cx);
            crate::web_panels::keyboard_to_app(window);
            window.render_frame(cx);
        }
        // The terminal's own keys for them (`key_down`): ⌘ on the Mac, and
        // Control-Shift elsewhere, as Control-C there is the shell's
        // interrupt. `secondary-` is Control alone off the Mac, so on Linux
        // the copy went to the shell as ^A and ^C and nothing was copied.
        let key = |letter: &str| if cfg!(target_os = "macos") { format!("cmd-{letter}") } else { format!("ctrl-shift-{letter}") };
        if cmd["select"].as_str() == Some("all") {
            window.press(&key("a"), cx);
        }
        if cmd["copy"].as_bool() == Some(true) {
            window.press(&key("c"), cx);
        }
        if let Some(text) = cmd["paste"].as_str() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
            window.press(&key("v"), cx);
        }
        if let Some(lines) = cmd["scroll"].as_f64() {
            // A real wheel turn over the middle of the grid, through hit testing.
            let drawn = session.read(cx).drawn_at.ok_or("the terminal hasn't been drawn")?;
            let size = session.read(cx).size;
            let at = point(drawn.origin.x + drawn.cell_width * (size.cols as f32 / 2.), drawn.origin.y + drawn.cell_height * (size.rows as f32 / 2.));
            let event = ScrollEvent { position: at, delta: ScrollDelta::Lines(point(0., -(lines as f32))), modifiers: Modifiers::default(), touch_phase: TouchPhase::Moved, source: ScrollSource::Pointer };
            window.dispatch_event(event.to_platform_input(), cx);
        }
    }
    window.render_frame(cx);
    Ok(dump(app.read(cx), cx))
}
