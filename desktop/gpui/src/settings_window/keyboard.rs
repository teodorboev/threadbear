// Settings › Keyboard (KeyboardSettings.swift, with AppShortcuts.swift's
// ShortcutRecorder): every command that can have a key. Click a key, press the
// new one, and it's changed; a key something else has is asked about first.
// Mike picked this from three mockups on 6 Nov, and it isn't one of Settings'
// ask-your-agent sections because starting a thread to change one key is slow.
// The keys stay in shortcuts.json, which an agent can change too, and the app
// picks up (shell.rs, `watch_shortcuts`); this writes it. The rules are
// threadbear_core::keyboard.
use super::{section, SettingsWindow};
use crate::app::Threadbear;
use crate::highlights::tipped;
use crate::push_button::PushButton;
use crate::{icons, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use threadbear_core::keyboard::{self as rules, Group, Look, Recorder};
use threadbear_core::shortcuts::{ShortcutBindings, ShortcutCommand};
use crate::tips::Tip as _;

/// The page's own state while the window is open.
#[derive(Default)]
pub(crate) struct KeyboardDraft {
    /// Which command is listening for its new key, and what came of the last press.
    pub(crate) recorder: Recorder,
    /// The search box, made the first time it's drawn.
    search: Option<Entity<InputState>>,
    /// Takes key presses before the menus do while a command is listening,
    /// so pressing ⌘E here doesn't also archive the open thread. Made at the
    /// first click on a key, and kept while the window is open.
    interceptor: Option<Subscription>,
    /// Why the last save didn't take.
    save_error: Option<String>,
    /// What's wrong with shortcuts.json, for the size and time it had then.
    file_check: Option<(u64, Option<std::time::SystemTime>, Option<String>)>,
    scroll: ScrollHandle,
}

const MACOS: bool = cfg!(target_os = "macos");

impl Threadbear {
    /// Where shortcuts.json is: the watched file, else beside the settings.
    fn shortcuts_file(&self) -> PathBuf {
        self.shortcuts_path.clone().unwrap_or_else(|| self.settings.path().parent().map(|d| d.join("shortcuts.json")).unwrap_or_else(|| "shortcuts.json".into()))
    }

    /// `AppShortcuts.change`: the keys in use, the keymap and the menus at
    /// once, and the file, holding only what's changed (none at all when
    /// nothing is). The file's watch reads it back as the same keys.
    pub(crate) fn change_shortcuts(&mut self, next: ShortcutBindings, cx: &mut Context<Self>) -> Result<(), String> {
        if next == self.shortcuts {
            return Ok(());
        }
        let saved = write_shortcuts(&self.shortcuts_file(), &next);
        self.shortcuts = next;
        self.apply_shortcuts(cx);
        self.menu_signature.clear();
        self.sync_menus(cx);
        cx.notify();
        saved
    }
}

/// shortcuts.json written whole and then put in place, so the app's watch
/// (or the Swift app's) never reads half of it.
fn write_shortcuts(path: &Path, bindings: &ShortcutBindings) -> Result<(), String> {
    let says = |e: std::io::Error| format!("Couldn't save your shortcuts: {e}");
    let Some(text) = bindings.json() else {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(says(e)),
            _ => Ok(()),
        };
    };
    // Only this user's, as everything in the data folder is (`private_file`).
    threadbear_core::private_file::write(path, format!("{text}\n").as_bytes()).map_err(says)
}

impl SettingsWindow {
    fn bindings(&self, cx: &App) -> ShortcutBindings {
        self.app.upgrade().map(|a| a.read(cx).shortcuts.clone()).unwrap_or_default()
    }

    /// Saves a change, and says so under the list if it couldn't be.
    fn save_keys(&mut self, next: ShortcutBindings, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else { return };
        self.keyboard.save_error = app.update(cx, |t, cx| t.change_shortcuts(next, cx)).err();
        cx.notify();
    }

    /// A click on a command's key: it listens for the new one.
    fn listen_for(&mut self, command: ShortcutCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard.recorder.start(command);
        if self.keyboard.interceptor.is_none() {
            let me = cx.entity().downgrade();
            let settings = window.window_handle();
            self.keyboard.interceptor = Some(cx.intercept_keystrokes(move |event, window, cx| {
                // Only the Settings window's keys, and only while listening.
                if window.window_handle() != settings {
                    return;
                }
                let Some(me) = me.upgrade() else { return };
                let k = &event.keystroke;
                if me.update(cx, |this, cx| this.key_while_listening(k, cx)) {
                    cx.stop_propagation();
                }
            }));
        }
        cx.notify();
    }

    /// A key press while a command listens (`swallows`). True means it goes
    /// no further, so it doesn't also do what it does in the menus.
    fn key_while_listening(&mut self, k: &Keystroke, cx: &mut Context<Self>) -> bool {
        if self.keyboard.recorder.command.is_none() {
            return false;
        }
        // A modifier on its own, let go, is nothing to take.
        if matches!(k.key.as_str(), "shift" | "control" | "alt" | "platform" | "function") {
            return false;
        }
        let m = &k.modifiers;
        let bindings = self.bindings(cx);
        if let Some(next) = self.keyboard.recorder.key_down(&k.key, m.platform, m.alt, m.control, m.shift, &bindings, MACOS) {
            self.save_keys(next, cx);
        }
        cx.notify();
        true
    }

    /// Never mind: back to the key it had.
    pub(super) fn stop_listening(&mut self, cx: &mut Context<Self>) {
        if self.keyboard.recorder.command.is_some() {
            self.keyboard.recorder.cancel();
            cx.notify();
        }
    }

    fn use_taken_key(&mut self, cx: &mut Context<Self>) {
        let bindings = self.bindings(cx);
        if let Some(next) = self.keyboard.recorder.use_taken_key(&bindings) {
            self.save_keys(next, cx);
        }
    }

    fn reset_key(&mut self, command: ShortcutCommand, cx: &mut Context<Self>) {
        self.keyboard.recorder.cancel();
        let mut next = self.bindings(cx);
        rules::reset(&mut next, command);
        self.save_keys(next, cx);
    }

    fn reset_all_keys(&mut self, cx: &mut Context<Self>) {
        self.keyboard.recorder.cancel();
        let mut next = self.bindings(cx);
        rules::reset_all(&mut next);
        self.save_keys(next, cx);
    }

    /// The search box, made the first time it's drawn.
    fn search_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(search) = &self.keyboard.search {
            return search.clone();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(rules::SEARCH_PROMPT));
        let sub = cx.subscribe_in(&search, window, |_, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        });
        self._subs.push(sub);
        self.keyboard.search = Some(search.clone());
        search
    }

    fn search_text(&self, cx: &App) -> String {
        self.keyboard.search.as_ref().map(|s| s.read(cx).value().to_string()).unwrap_or_default()
    }

    /// What's wrong with shortcuts.json, read again only when the file's size
    /// or time changes, as this draws whenever the app does.
    fn file_problem(&mut self, cx: &App) -> Option<String> {
        let path = self.app.upgrade()?.read(cx).shortcuts_file();
        let meta = std::fs::metadata(&path).ok();
        let stamp = (meta.as_ref().map_or(0, |m| m.len()), meta.as_ref().and_then(|m| m.modified().ok()));
        if let Some((len, time, problem)) = &self.keyboard.file_check {
            if (*len, *time) == stamp {
                return problem.clone();
            }
        }
        let problem = rules::file_problem(std::fs::read_to_string(&path).ok().as_deref());
        self.keyboard.file_check = Some((stamp.0, stamp.1, problem.clone()));
        problem
    }

    /// The whole page (`KeyboardSettings`): the search at the top, the
    /// groups scrolling under it, and the file's line and Reset All at the
    /// foot, which stay put.
    pub(super) fn render_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search_box(window, cx);
        let words = self.search_text(cx);
        let bindings = self.bindings(cx);
        let mut sections: Vec<AnyElement> = Vec::new();
        for group in Group::ALL {
            let commands: Vec<ShortcutCommand> = group.commands().into_iter().filter(|c| rules::matches(*c, &words, &bindings)).collect();
            if commands.is_empty() {
                continue;
            }
            let rows: Vec<AnyElement> = commands.into_iter().map(|c| self.shortcut_row(c, &bindings, cx)).collect();
            let about = group.about();
            sections.push(section(group_id(group), group.title(), &about, rules::ASK, rows, cx));
        }
        let secondary = style::secondary_text(cx);
        let faint = style::faint_text(cx);
        let field = div()
            .id("settings-keyboard-search")
            .test_support()
            .aria_label(rules::SEARCH_PROMPT)
            .tip(rules::SEARCH_PROMPT)
            .h(px(29.))
            .pl(px(16.))
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(style::primary_opacity(cx, 0.035))
            .border(gpui_kit::px(0.5))
            .border_color(style::hairline(cx))
            .child(div().flex_none().w(px(16.)).flex().justify_center().child(icons::symbol("magnifyingglass").with_size(px(12.5)).text_color(faint)))
            .child(div().flex_1().min_w_0().child(Input::new(&search).appearance(false).small().cleanable(true).pl(px(0.)).text_size(px(14.))));
        let problem = self.keyboard.save_error.clone().or_else(|| self.file_problem(cx));
        let path = self.app.upgrade().map(|a| a.read(cx).shortcuts_file().display().to_string()).unwrap_or_default();
        let changed = rules::any_changed(&bindings);
        let reset_all = if changed {
            let me = cx.entity().downgrade();
            PushButton::new("settings-keys-reset-all", "Reset All", rules::RESET_ALL_TIP).render(cx, move |_, _, cx| {
                me.update(cx, |this, cx| this.reset_all_keys(cx)).ok();
            })
        } else {
            disabled_button("settings-keys-reset-all", "Reset All", rules::RESET_ALL_TIP, cx)
        };
        let orange = crate::activity_panels::system_orange(cx);
        let footer = v_flex()
            .flex_none()
            .gap(px(6.))
            .px(px(22.))
            .pt(px(8.))
            .pb(px(14.))
            .text_size(px(12.))
            .line_height(px(style::natural_line(12.)))
            .when_some(problem, |s, problem| s.child(h_flex().gap(px(6.)).items_start().text_color(orange).child(div().pt(px(1.)).child(icons::symbol("exclamationmark.triangle").with_size(px(12.)))).child(div().flex_1().child(problem))))
            .child(h_flex().items_center().child(tipped(div().id("settings-keyboard-file"), path).text_color(secondary).child(rules::FOOTER)).child(div().flex_1()).child(reset_all));
        let me = cx.entity().downgrade();
        v_flex()
            .id("settings-keyboard")
            .size_full()
            // A click anywhere else means never mind, unless a question's up,
            // whose buttons need that click. A click on a key starts again.
            .capture_any_mouse_down(move |_, _, cx| {
                me.update(cx, |this, cx| {
                    if this.keyboard.recorder.waiting() {
                        this.stop_listening(cx);
                    }
                })
                .ok();
            })
            .child(h_flex().flex_none().w_full().justify_center().child(div().w_full().max_w(px(700.)).px(px(20.)).pt(px(14.)).child(field)))
            .child(
                div().id("settings-keyboard-list").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.keyboard.scroll).child(
                    h_flex().w_full().justify_center().child(v_flex().w_full().max_w(px(700.)).px(px(20.)).pt(px(22.)).pb(px(12.)).gap(px(30.)).children(sections)),
                ),
            )
            .child(footer)
            .into_any_element()
    }

    /// `ShortcutRow`: the command, what to do while it listens, Reset when
    /// it's been changed, its key; and under it the question when the key
    /// pressed can't simply be used.
    fn shortcut_row(&self, command: ShortcutCommand, bindings: &ShortcutBindings, cx: &mut Context<Self>) -> AnyElement {
        let recorder = &self.keyboard.recorder;
        let listening = recorder.listening(command);
        let secondary = style::secondary_text(cx);
        let mut title = v_flex().flex_1().min_w_0().gap(px(1.)).child(div().child(command.title()));
        if listening && recorder.waiting() {
            title = title.child(div().text_size(px(11.)).line_height(px(style::natural_line(11.))).text_color(secondary).child(rules::HINT));
        }
        let mut line = h_flex().w_full().gap(px(10.)).items_center().child(title);
        if bindings.is_changed(command) && !listening {
            let me = cx.entity().downgrade();
            let tip = rules::reset_tip(command);
            line = line.child(
                div()
                    .id(SharedString::from(format!("settings-key-reset-{}", command.name())))
                    .test_support()
                    .aria_label(format!("Reset {}", command.title()))
                    .tip(tip.clone())
                    .flex_none()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .line_height(px(style::natural_line(11.)))
                    .text_color(secondary)
                    .on_click(move |_, _, cx| {
                        me.update(cx, |this, cx| this.reset_key(command, cx)).ok();
                    })
                    .child("Reset"),
            );
        }
        line = line.child(self.key_cap(command, bindings, cx));
        let mut row = v_flex().w_full().gap(px(8.)).child(line);
        if listening {
            if let Some(question) = rules::question(&recorder.outcome) {
                row = row.child(self.question_box(command, question, cx));
            }
        }
        // 44 from one row to the next with the line between, as the Swift
        // page's rows with a key cap are.
        h_flex().min_h(px(43.)).px(px(super::ROW_INSET)).py(px(9.)).items_center().text_size(px(13.)).line_height(px(17.)).child(row).into_any_element()
    }

    /// `KeyCap`: the key, Add with none, "Press keys…" while it listens, or
    /// in orange the key that's taken.
    fn key_cap(&self, command: ShortcutCommand, bindings: &ShortcutBindings, cx: &mut Context<Self>) -> AnyElement {
        let look = rules::look(&self.keyboard.recorder, command, bindings);
        let orange = crate::activity_panels::system_orange(cx);
        let (fg, bg, edge) = match look {
            Look::Key(_) => (style::label_text(cx), style::primary_opacity(cx, 0.09), style::primary_opacity(cx, 0.1)),
            Look::None => (style::secondary_text(cx), hsla(0., 0., 0., 0.), style::primary_opacity(cx, 0.2)),
            Look::Listening => (white(), style::control_accent(cx), hsla(0., 0., 0., 0.)),
            Look::Taken(_) => (orange, orange.opacity(0.18), orange.opacity(0.6)),
        };
        let tip = rules::cap_tip(command, bindings);
        let me = cx.entity().downgrade();
        div()
            .id(SharedString::from(format!("settings-key-{}", command.name())))
            .test_support()
            // Its name for VoiceOver and the specs, as Swift's
            // accessibilityLabel: "Archive, ⌘E".
            .aria_label(rules::cap_label(command, bindings))
            .tip(tip.clone())
            .flex_none()
            .min_w(px(78.))
            .px(px(8.))
            .py(px(4.))
            .flex()
            .justify_center()
            .rounded(px(6.))
            .bg(bg)
            .border(gpui_kit::px(0.5))
            .border_color(edge)
            .cursor_pointer()
            .text_size(px(12.))
            .line_height(px(style::natural_line(12.)))
            .whitespace_nowrap()
            .text_color(fg)
            .when(look == Look::Listening, |s| s.font_weight(FontWeight::MEDIUM))
            .on_click(move |_, window, cx| {
                me.update(cx, |this, cx| this.listen_for(command, window, cx)).ok();
            })
            .child(look.title())
            .into_any_element()
    }

    /// The line under a row when the key pressed can't simply be used, with
    /// Cancel and, for another command's key, Use It for This Instead.
    fn question_box(&self, command: ShortcutCommand, question: rules::Question, cx: &mut Context<Self>) -> AnyElement {
        let orange = crate::activity_panels::system_orange(cx);
        let me = cx.entity().downgrade();
        let cancel = PushButton::new("settings-key-question-cancel", "Cancel", "Leave the key as it was").render(cx, move |_, _, cx| {
            me.update(cx, |this, cx| this.stop_listening(cx)).ok();
        });
        let mut line = h_flex()
            .gap(px(8.))
            .items_center()
            .px(px(10.))
            .py(px(7.))
            .rounded(px(7.))
            .bg(orange.opacity(0.12))
            .text_size(px(12.))
            .line_height(px(style::natural_line(12.)))
            .child(div().flex_1().min_w_0().text_color(orange).child(question.text))
            .child(cancel);
        if question.can_take {
            let me = cx.entity().downgrade();
            let tip = format!("Give the key to {} and leave the other with none", command.title());
            line = line.child(super::devices::prominent_button("settings-key-question-take", "Use It for This Instead", tip, cx, move |_, cx| {
                me.update(cx, |this, cx| this.use_taken_key(cx)).ok();
            }));
        }
        line.into_any_element()
    }

    /// The section in the dump: a line a command as the page lists them
    /// ("Archive: ⌘K (changed)"), which is listening, the question, the
    /// file's problem, and whether Reset All can be pressed.
    #[cfg(feature = "automation")]
    pub(super) fn keyboard_dump(&self, cx: &App) -> Value {
        let bindings = self.bindings(cx);
        let words = self.search_text(cx);
        let rows: Vec<String> = Group::ALL.iter().flat_map(|g| g.commands()).filter(|c| rules::matches(*c, &words, &bindings)).map(|c| rules::line(c, &bindings)).collect();
        let recorder = &self.keyboard.recorder;
        let question = rules::question(&recorder.outcome).filter(|_| recorder.command.is_some());
        json!({
            "rows": rows,
            "listening": recorder.command.map(ShortcutCommand::title),
            "question": question.as_ref().map(|q| q.text.clone()),
            "canTake": question.is_some_and(|q| q.can_take),
            // Read afresh: the page only reads the file again when it draws.
            "problem": self.keyboard.save_error.clone().or_else(|| rules::file_problem(self.app.upgrade().and_then(|a| std::fs::read_to_string(a.read(cx).shortcuts_file()).ok()).as_deref())),
            "canResetAll": rules::any_changed(&bindings),
        })
    }
}

fn group_id(group: Group) -> &'static str {
    match group {
        Group::Threads => "settings-keys-threads",
        Group::Go => "settings-keys-go",
        Group::View => "settings-keys-view",
    }
}

/// A push button that can't be pressed now: its room and words, greyed.
fn disabled_button(id: &'static str, label: &'static str, tip: &'static str, cx: &App) -> AnyElement {
    tipped(
        div()
            .id(id)
            .flex_none()
            .h(px(24.))
            .px(px(12.5))
            .rounded(px(5.))
            .flex()
            .items_center()
            .bg(style::primary_opacity(cx, 0.04))
            .text_size(px(13.))
            .line_height(px(style::natural_line(13.)))
            .whitespace_nowrap()
            .text_color(style::tertiary_label(cx)),
        tip,
    )
    .child(label)
    .into_any_element()
}

/// The automation's `shortcuts` command, as the Swift app's (app/README.md):
/// `record` (a command's name in shortcuts.json) clicks its key, `press`
/// ("cmd+k") presses those keys in the Settings window, `post` (`keyCode`,
/// `characters`, `modifiers`) presses a key by its Mac key code (Esc is 53,
/// Delete 51), `answer` (`use` or `cancel`) presses the question's buttons,
/// and `reset` (a name, or `all`) presses Reset or Reset All, each through a
/// real click or key in the window. `search` (this app's own) types in the
/// search box. Says what's listening, what the last press came to, and
/// what's changed, as the Swift command does.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    use threadbear_core::keyboard::Outcome;
    use threadbear_core::shortcuts::ShortcutKey;
    let (handle, view) = app.read(cx).settings_window.clone().ok_or("the Settings window isn't open")?;
    let record = cmd["record"].as_str().map(|name| ShortcutCommand::from_name(name).ok_or_else(|| format!("no command {name}"))).transpose()?;
    let bindings = app.read(cx).shortcuts.clone();
    // The keys as GPUI names them, from `press` or `post`.
    let keys = match (cmd["press"].as_str(), cmd.get("post")) {
        (Some(text), _) => Some(ShortcutKey::parse(text).ok_or_else(|| format!("not a key: {text}"))?.chord(MACOS)),
        (None, Some(post)) => {
            let code = post["keyCode"].as_u64().ok_or("post needs keyCode and characters")?;
            let characters = post["characters"].as_str().ok_or("post needs keyCode and characters")?;
            let key = match code {
                53 => "escape".to_owned(),
                51 => "backspace".to_owned(),
                117 => "delete".to_owned(),
                36 | 76 => "enter".to_owned(),
                48 => "tab".to_owned(),
                49 => "space".to_owned(),
                123 => "left".to_owned(),
                124 => "right".to_owned(),
                125 => "down".to_owned(),
                126 => "up".to_owned(),
                _ => characters.to_lowercase(),
            };
            let mut chord = String::new();
            for m in post["modifiers"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                chord.push_str(match m {
                    "cmd" => "secondary-",
                    "opt" => "alt-",
                    "ctrl" => "ctrl-",
                    "shift" => "shift-",
                    _ => return Err("modifier is cmd, opt, ctrl or shift".into()),
                });
            }
            Some(chord + &key)
        }
        _ => None,
    };
    handle
        .update(cx, |_, window, cx| -> Result<(), String> {
            window.render_frame(cx);
            // A control by its name (or the start of it), through a real click.
            let click = |label: &str, prefix: bool, window: &mut Window, cx: &mut App| -> bool {
                let found = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| if prefix { l.starts_with(label) } else { l == label })).last();
                let Some(found) = found else { return false };
                let at = found.bounds().center();
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Left, cx);
                window.pointer_up(at, MouseButton::Left, cx);
                window.render_frame(cx);
                true
            };
            if let Some(words) = cmd["search"].as_str() {
                let search = view.read(cx).keyboard.search.clone().ok_or("the Keyboard page isn't showing")?;
                search.update(cx, |s, cx| s.set_value(words.to_owned(), window, cx));
                window.render_frame(cx);
            }
            if let Some(c) = record {
                // Scrolled out of sight, it's started as its click would.
                if !click(&rules::cap_label(c, &bindings), false, window, cx) {
                    view.update(cx, |this, cx| this.listen_for(c, window, cx));
                }
            }
            if let Some(keys) = &keys {
                window.press(keys, cx);
            }
            match cmd["answer"].as_str() {
                // Use It for This Instead's tip names the command, so it's
                // found by the start of it.
                Some("use") if !click("Give the key to", true, window, cx) => return Err("no question to answer".into()),
                // With no question up, it stops listening, as the Swift
                // command's `cancel` does.
                Some("cancel") if !click("Leave the key as it was", false, window, cx) => view.update(cx, |this, cx| this.stop_listening(cx)),
                Some("use" | "cancel") | None => {}
                Some(other) => return Err(format!("answer is use or cancel, not {other}")),
            }
            match cmd["reset"].as_str() {
                Some("all") if !click(rules::RESET_ALL_TIP, false, window, cx) => return Err("nothing to Reset All".into()),
                Some("all") | None => {}
                Some(name) => {
                    let c = ShortcutCommand::from_name(name).ok_or_else(|| format!("no command {name}"))?;
                    if !click(&format!("Reset {}", c.title()), false, window, cx) {
                        return Err(format!("{} has no Reset", c.title()));
                    }
                }
            }
            window.render_frame(cx);
            Ok(())
        })
        .map_err(|e| e.to_string())??;
    let recorder = view.read(cx).keyboard.recorder.clone();
    let outcome = match &recorder.outcome {
        Outcome::Waiting => "waiting".to_owned(),
        Outcome::TypesInstead => "typesInstead".to_owned(),
        Outcome::TakenByCommand(key, other) => format!("takenByCommand({}, {})", key.string(), other.name()),
        Outcome::TakenOutside(key, what) => format!("takenOutside({}, {what})", key.string()),
    };
    let now = app.read(cx).shortcuts.clone();
    let changed: Vec<String> = ShortcutCommand::ALL.iter().filter(|c| now.is_changed(**c)).map(|c| format!("{}:{}", c.name(), now.key(*c).map_or_else(|| "none".to_owned(), |k| k.string()))).collect();
    Ok(json!(format!("ok listening={} outcome={outcome} changed=[{}]", recorder.command.map_or("nothing", ShortcutCommand::name), changed.join(","))))
}
