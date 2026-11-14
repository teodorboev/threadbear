// The schedule editor, a port of the Swift app's ScheduleEditor and
// ScheduleEditorState: the sheet that makes or edits a schedule. The message,
// a title, when it runs (once, or a repeat preset that's only a cron string
// underneath) and where it goes. What the runner makes of the time shows
// under it as it changes (`schedules.preview`, a quarter of a second after the
// last change), so a cron typo is plain before saving.
use crate::a11y::A11yRole as _;
use crate::app::Threadbear;
use crate::{call, icons, now_ms, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::time_field::{HourCycle, TimeField, TimeFieldEvent, TimeFieldState, TimePrecision};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::json;
#[cfg(feature = "automation")]
use serde_json::Value;
use std::rc::Rc;
use std::sync::Arc;
use threadbear_client::runner::Client;
use std::time::Duration;
use threadbear_client::runner::Status;
use threadbear_core::chrono::{Local, NaiveTime, TimeZone as _, Timelike as _};
use threadbear_core::composer::effort_title;
use threadbear_core::inbox::{iso_time, parse_ms};
use threadbear_core::new_thread::abbreviate_home;
use threadbear_core::schedules::{self as rules, RepeatChoice, RepeatKind, ScheduleModels};
use crate::tips::Tip as _;

/// Monday first; cron counts Sunday as 0.
const WEEKDAYS: [(u32, &str); 7] = [(1, "Monday"), (2, "Tuesday"), (3, "Wednesday"), (4, "Thursday"), (5, "Friday"), (6, "Saturday"), (0, "Sunday")];

/// What the editor is editing (`ScheduleEditorState`): a new schedule, or one
/// that exists.
#[derive(Clone, Debug, Default)]
pub(crate) struct EditorState {
    /// The schedule being edited; None for a new one.
    pub(crate) schedule_id: Option<String>,
    pub(crate) text: String,
    pub(crate) title: String,
    pub(crate) once: bool,
    pub(crate) at_ms: i64,
    pub(crate) repeat: RepeatChoice,
    /// Sends here; None starts a new thread each time.
    pub(crate) thread_id: Option<String>,
    /// The thread it was opened from, which "This thread" means. None from
    /// the Scheduled view.
    pub(crate) from_thread_id: Option<String>,
    /// Settings for a new thread each time, as the new-thread box has them.
    pub(crate) cwd: String,
    pub(crate) account_id: Option<String>,
    /// The box's pool, which then picks each run's login rather than the one
    /// it happened to be on (`account_id` goes unsent).
    pub(crate) pool_id: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    /// The box's mode as it was: the editor has no control for it.
    pub(crate) permission_mode: String,
    /// Another runner it runs on (`runners.list`); None is this one. Only for
    /// a new thread each time, and then the folder is a path on that Mac,
    /// typed as is, and the login and model are left to it.
    pub(crate) runner_id: Option<String>,
    /// That runner's name, for an existing schedule's read-only line.
    pub(crate) runner_name: Option<String>,
    /// Pictures in the composer's draft it came from, which a schedule
    /// can't send.
    pub(crate) draft_images: usize,
    /// The folder an existing schedule had when the editor opened, as the
    /// runner has it (None: its default). A save sends a folder only when
    /// it's changed from this.
    pub(crate) opened_cwd: Option<String>,
}

impl EditorState {
    fn is_new(&self) -> bool {
        self.schedule_id.is_none()
    }
    /// Runs on another runner's Mac.
    fn is_away(&self) -> bool {
        self.runner_id.is_some()
    }

    /// The folder an update sends: none unless it changed. Every save used
    /// to send it, and the runner took a folder as a move, which approved
    /// the schedule's check again as its scripts were then, though the
    /// editor never shows the check (security review, 11 Nov 2026). The
    /// runner no longer approves on a move either; this keeps a save to
    /// what was changed.
    fn update_cwd(&self, away_folder: &str) -> Option<String> {
        let now = if self.is_away() { away_folder.trim() } else { self.cwd.trim() };
        let was = self.opened_cwd.as_deref().unwrap_or("").trim();
        (now != was).then(|| now.to_owned())
    }
}

/// What the runner makes of the time: its words and the next runs.
#[derive(Clone, Debug)]
struct Preview {
    when_text: String,
    runs: Vec<i64>,
}

/// A line of one of the editor's menus.
#[derive(Clone)]
enum Choice {
    Item { title: String, symbol: Option<&'static str>, checked: bool, pick: Rc<dyn Fn(&mut ScheduleEditor, &mut Window, &mut Context<ScheduleEditor>)> },
    Separator,
}

fn choice(title: impl Into<String>, checked: bool, pick: impl Fn(&mut ScheduleEditor, &mut Window, &mut Context<ScheduleEditor>) + 'static) -> Choice {
    Choice::Item { title: title.into(), symbol: None, checked, pick: Rc::new(pick) }
}

impl Threadbear {
    /// A new schedule from the Scheduled view (`newSchedule(from: nil)`):
    /// it starts a new thread each time, in the runner's default folder, on
    /// the new-thread box's login and model. `from` is a thread whose
    /// composer it came from, which it then sends to unless switched.
    pub(crate) fn new_schedule(&mut self, from: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let thread = from.as_ref().and_then(|id| self.threads.get(id)).map(|t| (**t).clone());
        let settings = thread.clone().unwrap_or_else(|| self.draft_summary());
        let state = EditorState {
            at_ms: rules::default_once_ms(now_ms()),
            thread_id: thread.as_ref().map(|t| t.id.clone()),
            from_thread_id: thread.as_ref().map(|t| t.id.clone()),
            cwd: if thread.is_some() { settings.cwd.clone() } else { self.logins.defaults.cwd.clone() },
            account_id: Some(settings.account_id.clone()).filter(|a| !a.is_empty()),
            pool_id: settings.pool_id.clone(),
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            permission_mode: if settings.permission_mode.is_empty() { "full".into() } else { settings.permission_mode.clone() },
            ..EditorState::default()
        };
        self.open_schedule_editor(state, window, cx);
    }

    /// Schedule… in a composer's + menu (`newSchedule(from:draftKey:)`): its
    /// words, and its thread (or for the new-thread box, a new thread each
    /// time on the box's folder, login and model). Once saved, the words
    /// leave the box.
    pub(crate) fn new_schedule_from_composer(&mut self, composer: &Entity<crate::composer::Composer>, window: &mut Window, cx: &mut Context<Self>) {
        let (text, images, thread_id) = {
            let c = composer.read(cx);
            let (text, images) = c.schedule_draft(cx);
            (text, images, c.thread_id.clone())
        };
        let thread = thread_id.as_ref().and_then(|id| self.threads.get(id)).map(|t| (**t).clone());
        let settings = thread.clone().unwrap_or_else(|| self.draft_summary());
        let state = EditorState {
            text,
            draft_images: images,
            at_ms: rules::default_once_ms(now_ms()),
            thread_id: thread.as_ref().map(|t| t.id.clone()),
            from_thread_id: thread.as_ref().map(|t| t.id.clone()),
            cwd: settings.cwd.clone(),
            account_id: Some(settings.account_id.clone()).filter(|a| !a.is_empty()),
            pool_id: settings.pool_id.clone(),
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            permission_mode: if settings.permission_mode.is_empty() { "full".into() } else { settings.permission_mode.clone() },
            ..EditorState::default()
        };
        self.open_schedule_editor_with(state, Some(composer.downgrade()), window, cx);
    }

    /// Editing a schedule from its row (`editSchedule`).
    pub(crate) fn edit_schedule(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(schedule) = self.schedule(id).cloned() else { return };
        let away = self.editor_away_name(&schedule);
        let mut state = EditorState {
            schedule_id: Some(id.to_owned()),
            text: schedule.text.clone(),
            title: schedule.title.clone(),
            once: schedule.is_one_off(),
            at_ms: schedule.at.as_deref().and_then(parse_ms).unwrap_or_else(|| rules::default_once_ms(now_ms())),
            repeat: schedule.cron.as_deref().map(RepeatChoice::from_cron).unwrap_or_default(),
            thread_id: schedule.thread_id.clone(),
            // Shown so they can be changed; only the model and effort are
            // sent back, so the login stays as it was.
            account_id: schedule.account_id.clone(),
            pool_id: schedule.pool_id.clone(),
            model: schedule.model.clone(),
            effort: schedule.effort.clone(),
            permission_mode: schedule.permission_mode.clone().unwrap_or_else(|| "full".into()),
            ..EditorState::default()
        };
        match away {
            Some(name) => {
                state.runner_id = schedule.runner_id.clone();
                state.runner_name = Some(name);
                state.cwd = schedule.cwd.clone().unwrap_or_default();
            }
            None => state.cwd = schedule.cwd.clone().unwrap_or_else(|| self.logins.defaults.cwd.clone()),
        }
        // What the editor shows for it, so a save that leaves it alone sends none.
        state.opened_cwd = Some(state.cwd.clone());
        self.open_schedule_editor(state, window, cx);
    }

    fn open_schedule_editor(&mut self, state: EditorState, window: &mut Window, cx: &mut Context<Self>) {
        self.open_schedule_editor_with(state, None, window, cx);
    }

    fn open_schedule_editor_with(&mut self, state: EditorState, draft: Option<WeakEntity<crate::composer::Composer>>, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        // Handed the client, as it can't read the app while the app is the
        // one making it.
        let client = self.client.clone();
        let view = cx.new(|cx| ScheduleEditor::new(app.clone(), client, state, draft, window, cx));
        self.schedule_editor = Some(view.clone());
        cx.notify();
        // The cursor in the message, ready to type, as the Swift sheet has it:
        // once the dialog is up, as it takes the focus as it opens.
        let message = view.read(cx).message.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let app = app.clone();
            dialog.w(px(520.)).close_button(false).child(view.clone()).footer(div()).on_close(move |_, _, cx| {
                if let Some(app) = app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.schedule_editor = None;
                        cx.notify();
                    });
                }
            })
        });
        window.defer(cx, move |window, cx| {
            message.update(cx, |m, cx| m.focus(window, cx));
            crate::web_panels::keyboard_to_app(window);
        });
    }

    /// The schedule the editor's open on, if it's open on one.
    pub(crate) fn schedule_editor_id(&self, cx: &App) -> Option<String> {
        self.schedule_editor.as_ref().and_then(|e| e.read(cx).state.schedule_id.clone())
    }
}

/// The sheet's content (`ScheduleEditor`).
pub(crate) struct ScheduleEditor {
    app: WeakEntity<Threadbear>,
    client: Arc<Client>,
    /// The composer whose words it came from, emptied once it's saved.
    draft: Option<WeakEntity<crate::composer::Composer>>,
    pub(crate) state: EditorState,
    message: Entity<TextareaState>,
    title: Entity<InputState>,
    cron: Entity<InputState>,
    at: Entity<DatePickerState>,
    time: Entity<TimeFieldState>,
    /// A folder on another runner's Mac, typed as is.
    away_folder: Entity<InputState>,
    /// This Mac's folder, kept while another runner is picked so picking
    /// this one again puts it back.
    local_cwd: String,
    preview: Option<Result<Preview, String>>,
    /// The newest preview asked for; an older answer is dropped.
    asks: u64,
    saving: bool,
    save_error: Option<String>,
    _subs: Vec<Subscription>,
}

impl ScheduleEditor {
    fn new(app: WeakEntity<Threadbear>, client: Arc<Client>, state: EditorState, draft: Option<WeakEntity<crate::composer::Composer>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| TextareaState::new(window, cx).default_value(state.text.clone()));
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Optional").default_value(state.title.clone()));
        let cron = cx.new(|cx| InputState::new(window, cx).placeholder("0 9 * * 1-5").default_value(state.repeat.custom.clone()));
        let away_folder = cx.new(|cx| InputState::new(window, cx).placeholder("Default folder").default_value(if state.is_away() { state.cwd.clone() } else { String::new() }));
        let at_ms = state.at_ms;
        let at = cx.new(|cx| {
            let mut picker = DatePickerState::new(window, cx).time_precision(TimePrecision::Minute).hour_cycle(HourCycle::H12);
            if let Some(local) = Local.timestamp_millis_opt(at_ms).single() {
                picker.set_date_time(local.naive_local(), window, cx);
            }
            picker
        });
        let (hour, minute) = (state.repeat.hour, state.repeat.minute);
        let time = cx.new(|cx| {
            let mut field = TimeFieldState::new(window, cx).precision(TimePrecision::Minute).hour_cycle(HourCycle::H12);
            field.set_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap_or_default(), window, cx);
            field
        });
        let subs = vec![
            cx.subscribe_in(&message, window, |_, _, ev: &InputEvent, _, cx| {
                // Whether it can be saved follows what's typed.
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&title, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.save(window, cx);
                }
            }),
            cx.subscribe_in(&cron, window, |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    this.state.repeat.custom = input.read(cx).value().to_string();
                    this.when_changed(cx);
                }
                InputEvent::PressEnter { .. } => this.save(window, cx),
                _ => {}
            }),
            cx.subscribe_in(&away_folder, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.save(window, cx);
                }
            }),
            cx.subscribe_in(&at, window, |this, _, ev: &DatePickerEvent, _, cx| {
                let DatePickerEvent::Change(value) = ev;
                if let Some(ms) = value.start().and_then(|d| Local.from_local_datetime(&d).earliest()).map(|d| d.timestamp_millis()) {
                    this.state.at_ms = ms;
                    this.when_changed(cx);
                }
            }),
            cx.subscribe_in(&time, window, |this, _, ev: &TimeFieldEvent, _, cx| {
                let TimeFieldEvent::Change(t) = ev;
                this.state.repeat.hour = t.hour();
                this.state.repeat.minute = t.minute();
                this.when_changed(cx);
            }),
        ];
        let local_cwd = if state.is_away() { String::new() } else { state.cwd.clone() };
        let mut editor = ScheduleEditor { app, client, draft, state, message, title, cron, at, time, away_folder, local_cwd, preview: None, asks: 0, saving: false, save_error: None, _subs: subs };
        editor.when_changed(cx);
        editor
    }

    fn app_read<'a>(&self, cx: &'a App) -> Option<&'a Threadbear> {
        self.app.upgrade().map(|a| a.read(cx))
    }

    fn message_text(&self, cx: &App) -> String {
        self.message.read(cx).value().to_string()
    }

    /// The time changed: what the runner makes of it is asked again a moment
    /// later, so a cron being typed settles first, and a newer change wins.
    fn when_changed(&mut self, cx: &mut Context<Self>) {
        self.asks += 1;
        let ask = self.asks;
        let params = if self.state.once {
            json!({ "at": iso_time(self.state.at_ms) })
        } else {
            let cron = self.state.repeat.cron();
            if cron.is_empty() {
                self.preview = Some(Err("Type a cron, such as 0 9 * * 1-5.".into()));
                cx.notify();
                return;
            }
            json!({ "cron": cron })
        };
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            if this.update(cx, |this, _| this.asks != ask).unwrap_or(true) {
                return;
            }
            let res = call(client, "schedules.preview", params).await;
            this.update(cx, |this, cx| {
                if this.asks != ask {
                    return;
                }
                this.preview = Some(match res {
                    Ok(v) => Ok(Preview { when_text: v["whenText"].as_str().unwrap_or("").to_owned(), runs: v["runs"].as_array().into_iter().flatten().filter_map(|r| r.as_str().and_then(parse_ms)).collect() }),
                    Err(e) => Err(e.message),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_once(&mut self, once: bool, cx: &mut Context<Self>) {
        if self.state.once != once {
            self.state.once = once;
            self.when_changed(cx);
        }
    }

    fn set_kind(&mut self, kind: RepeatKind, cx: &mut Context<Self>) {
        self.state.repeat.kind = kind;
        self.when_changed(cx);
    }

    /// Which Mac runs it. Another Mac's folder is typed as a path there, so
    /// this Mac's is put aside and comes back if this Mac is picked again.
    fn set_runner(&mut self, runner: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.state.runner_id.clone();
        if was.is_none() && runner.is_some() {
            self.local_cwd = self.state.cwd.clone();
            self.state.cwd.clear();
            self.away_folder.update(cx, |f, cx| f.set_value("", window, cx));
        } else if runner.is_none() && was.is_some() {
            let fallback = self.app_read(cx).map(|a| a.logins.defaults.cwd.clone()).unwrap_or_default();
            self.state.cwd = if self.local_cwd.is_empty() { fallback } else { self.local_cwd.clone() };
        }
        self.state.runner_name = runner.as_ref().and_then(|id| self.app_read(cx).and_then(|a| a.remote.all_runners.iter().find(|r| &r.id == id).map(|r| r.name.clone())));
        self.state.runner_id = runner;
        cx.notify();
    }

    /// Picking the default row stores None, so the schedule follows the
    /// default if it changes; a level the new model hasn't got goes back to
    /// its default.
    fn set_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        self.state.model = model;
        if let Some(app) = self.app_read(cx) {
            let levels = ScheduleModels::new(self.state.account_id.as_deref(), self.state.pool_id.as_deref(), self.state.model.as_deref(), &app.logins).effort_levels;
            if self.state.effort.as_ref().is_some_and(|e| !levels.contains(e)) {
                self.state.effort = None;
            }
        }
        cx.notify();
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose".into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await {
                if let Some(path) = paths.pop() {
                    this.update(cx, |this, cx| {
                        this.state.cwd = path.to_string_lossy().into_owned();
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn can_save(&self, cx: &App) -> bool {
        if self.saving || self.message_text(cx).trim().is_empty() {
            return false;
        }
        if matches!(self.preview, Some(Err(_))) {
            return false;
        }
        self.state.once || !self.state.repeat.cron().is_empty()
    }

    /// Why the button is greyed out, when it is.
    fn save_tip(&self, cx: &App) -> &'static str {
        if self.can_save(cx) {
            "Save the schedule (Return)"
        } else if self.saving {
            "Saving…"
        } else if self.message_text(cx).trim().is_empty() {
            "Type a message first"
        } else {
            "Fix when it sends first"
        }
    }

    /// Makes or updates the schedule; the runner's reason shows here when it
    /// says no, and the sheet stays open (`saveSchedule`).
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }
        let Some(app) = self.app.upgrade() else { return };
        if !matches!(app.read(cx).status, Status::Connected { .. }) {
            self.save_error = Some("Not connected to the runner.".into());
            cx.notify();
            return;
        }
        let text = self.message_text(cx).trim().to_owned();
        let title = self.title.read(cx).value().trim().to_owned();
        let away_folder = self.away_folder.read(cx).value().trim().to_owned();
        let s = &self.state;
        let mut params = if s.once { json!({ "at": iso_time(s.at_ms) }) } else { json!({ "cron": s.repeat.cron() }) };
        params["text"] = json!(text);
        if !title.is_empty() {
            params["title"] = json!(title);
        }
        let method = if let Some(id) = &s.schedule_id {
            params["scheduleId"] = json!(id);
            if s.thread_id.is_none() {
                if let Some(cwd) = s.update_cwd(&away_folder) {
                    params["cwd"] = json!(cwd);
                }
                // This Mac's model and effort (None goes back to the default);
                // another Mac's logins aren't this one's.
                if !s.is_away() {
                    params["model"] = json!(s.model);
                    params["effort"] = json!(s.effort);
                }
            }
            "schedules.update"
        } else if s.is_away() && s.thread_id.is_none() {
            // Its folder is checked on that Mac when it runs; this Mac's
            // login, model and effort mean nothing there.
            if !away_folder.is_empty() {
                params["cwd"] = json!(away_folder);
            }
            params["permissionMode"] = json!(s.permission_mode);
            params["runnerId"] = json!(s.runner_id);
            "schedules.create"
        } else {
            match &s.thread_id {
                Some(thread) => params["threadId"] = json!(thread),
                None => {
                    params["cwd"] = json!(s.cwd);
                    // The runner takes one or the other; a pool picks the login.
                    match (&s.pool_id, &s.account_id) {
                        (Some(pool), _) => params["poolId"] = json!(pool),
                        (None, Some(account)) => params["accountId"] = json!(account),
                        (None, None) => {}
                    }
                    if let Some(model) = &s.model {
                        params["model"] = json!(model);
                    }
                    if let Some(effort) = &s.effort {
                        params["effort"] = json!(effort);
                    }
                    params["permissionMode"] = json!(s.permission_mode);
                }
            }
            "schedules.create"
        };
        self.saving = true;
        self.save_error = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = call(client, method, params).await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match res {
                    Ok(reply) => {
                        // Closed already (Cancel, Escape) while it was on its
                        // way: nothing of this editor's to close, and closing
                        // the top dialog would close something else.
                        let me = cx.entity_id();
                        let mut still_open = false;
                        if let Some(app) = this.app.upgrade() {
                            let is_new = this.state.is_new();
                            app.update(cx, |app, cx| {
                                if is_new {
                                    app.schedule_created(&reply);
                                } else {
                                    app.schedule_reply(&reply);
                                }
                                still_open = app.schedule_editor.as_ref().is_some_and(|e| e.entity_id() == me);
                                if still_open {
                                    app.schedule_editor = None;
                                }
                                cx.notify();
                            });
                        }
                        if still_open {
                            window.close_dialog(cx);
                        }
                        // The words have gone into the schedule.
                        if let Some(composer) = this.draft.as_ref().and_then(|d| d.upgrade()) {
                            composer.update(cx, |c, cx| c.clear_scheduled_draft(window, cx));
                        }
                    }
                    Err(e) => this.save_error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                app.schedule_editor = None;
                cx.notify();
            });
        }
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let (Some(id), Some(app)) = (self.state.schedule_id.clone(), self.app.upgrade()) else { return };
        // Asks over the sheet; yes closes both (scheduled.rs).
        app.update(cx, |app, cx| app.delete_schedule(id, window, cx));
    }

    // Drawing.

    /// A menu button: what's chosen, and the choices when clicked.
    /// `symbol` goes before the label, as the Swift pickers show the
    /// choice's icon (the repeat's, the Mac's).
    fn menu_button(&self, id: &'static str, label: String, symbol: Option<&'static str>, tip: &'static str, choices: Vec<Choice>, cx: &mut Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        let mut button = Button::new(id).label(label).small().outline().dropdown_caret(true).tooltip(tip);
        if let Some(symbol) = symbol {
            button = button.icon(icons::symbol(symbol));
        }
        div()
            .id(SharedString::from(format!("{id}-holder")))
            .test_support()
            .a11y_role(Role::GenericContainer).aria_label(tip.to_owned())
            .flex_none()
            .child(button.dropdown_menu(move |menu: PopupMenu, _window, _cx| {
                let mut menu = menu;
                for choice in &choices {
                    match choice.clone() {
                        Choice::Separator => menu = menu.separator(),
                        Choice::Item { title, symbol, checked, pick } => {
                            let editor = editor.clone();
                            let mut item = PopupMenuItem::new(title).checked(checked).on_click(move |_, window, cx| {
                                let pick = pick.clone();
                                editor.update(cx, |e, cx| pick(e, window, cx)).ok();
                            });
                            if let Some(symbol) = symbol.filter(|_| !checked) {
                                item = item.icon(icons::symbol(symbol));
                            }
                            menu = menu.item(item);
                        }
                    }
                }
                menu
            }))
            .into_any_element()
    }

    /// The parts of a preset that change: on which day, at what time.
    fn repeat_detail(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let secondary = style::secondary_text(cx);
        let word = |text: &'static str| div().flex_none().text_size(px(style::size::LABEL)).text_color(secondary).child(text).into_any_element();
        let time = || vec![word("at"), div().id("schedule-time").flex_none().w(px(110.)).tip("What time to send it").child(TimeField::new(&self.time).small()).into_any_element()];
        let repeat = &self.state.repeat;
        match repeat.kind {
            RepeatKind::Hourly | RepeatKind::Custom => Vec::new(),
            RepeatKind::Daily | RepeatKind::Weekdays => time(),
            RepeatKind::Weekly => {
                let current = WEEKDAYS.iter().find(|(d, _)| *d == repeat.weekday).map_or("Monday", |(_, name)| name);
                let choices = WEEKDAYS
                    .iter()
                    .map(|&(day, name)| {
                        choice(name, day == repeat.weekday, move |e, _, cx| {
                            e.state.repeat.weekday = day;
                            e.when_changed(cx);
                        })
                    })
                    .collect();
                let mut parts = vec![word("on"), self.menu_button("schedule-weekday", current.to_owned(), None, "Which day of the week", choices, cx)];
                parts.extend(time());
                parts
            }
            RepeatKind::Monthly => {
                let choices = (1..=31)
                    .map(|day| {
                        choice(day.to_string(), day == repeat.day, move |e, _, cx| {
                            e.state.repeat.day = day;
                            e.when_changed(cx);
                        })
                    })
                    .collect();
                let mut parts = vec![word("on day"), self.menu_button("schedule-day", repeat.day.to_string(), None, "Which day of the month. Months without it are skipped.", choices, cx)];
                parts.extend(time());
                parts
            }
        }
    }

    /// "Weekdays at 9:00", then the next three runs; or why the runner can't
    /// read it, in the same place.
    fn preview_text(&self, cx: &App) -> AnyElement {
        let (secondary, faint, failed) = (style::secondary_text(cx), style::faint_text(cx), style::failed(cx));
        let size = style::size::SIDEBAR_META + 1.;
        let body = match &self.preview {
            Some(Ok(preview)) => {
                // Swift's lines, not GPUI's taller default.
                let runs = if preview.runs.is_empty() {
                    if self.state.once { "That's in the past, so it runs as soon as it's saved.".to_owned() } else { "It won't run again.".to_owned() }
                } else {
                    format!("Next {}", preview.runs.iter().take(3).map(|ms| run_text(*ms)).collect::<Vec<_>>().join(" · "))
                };
                v_flex().gap(px(2.)).child(div().text_color(secondary).child(preview.when_text.clone())).child(div().text_color(faint).child(runs))
            }
            Some(Err(message)) => v_flex().child(div().text_color(failed).child(message.clone())),
            None => v_flex().child(" "),
        };
        div().id("schedule-preview").min_h(px(32.)).text_size(px(size)).line_height(px(16.)).child(body).into_any_element()
    }

    fn target_text(&self, cx: &App) -> String {
        match &self.state.thread_id {
            Some(id) => self.app_read(cx).and_then(|a| a.threads.get(id)).map(|t| t.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "This thread".into()),
            None => "A new thread each time".into(),
        }
    }
}

/// "Wed 28 Oct 9:00 am": short enough for three on one line (`runText`).
fn run_text(ms: i64) -> String {
    use threadbear_core::chrono::Datelike as _;
    let Some(d) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {} {} {}", DAYS[d.weekday().num_days_from_monday() as usize], d.day(), MONTHS[d.month0() as usize], rules::time_of_day(&d))
}

/// A row of the form: its label in the left column, right-aligned, and what
/// it sets beside it (`.formStyle(.columns)`).
fn form_row(label: &'static str, control: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .gap(px(9.))
        .child(div().w(px(56.)).flex_none().pt(px(5.)).text_right().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(cx.theme().foreground).child(label))
        .child(div().flex_1().min_w_0().child(control))
}

impl Render for ScheduleEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.upgrade() else { return div().into_any_element() };
        let secondary = style::secondary_text(cx);
        let state = self.state.clone();
        let header = if state.is_new() { "Schedule a message" } else { "Edit schedule" };

        let message_box = div()
            .id("schedule-message")
            .h(px(84.))
            .rounded(px(6.))
            .border(gpui_kit::px(0.5))
            .border_color(style::hairline(cx))
            .bg(style::primary_opacity(cx, 0.035))
            .child(Textarea::new(&self.message).h(px(84.)).appearance(false));
        // Pictures are chips in the box's text here, so the box keeps them
        // and its words both.
        let images_note = (state.draft_images > 0).then(|| {
            let what = if state.draft_images == 1 { "The picture stays in the box: a schedule sends only text." } else { "The pictures stay in the box: a schedule sends only text." };
            div().text_size(px(style::size::SIDEBAR_META)).line_height(px(14.)).text_color(secondary).child(what)
        });
        let message = v_flex().gap(px(4.)).child(message_box).children(images_note);
        let title = div().id("schedule-title").tip("Its name in the Scheduled list. Left empty, it's named after the message's first line.").child(Input::new(&self.title).small());

        let once = state.once;
        let editor = cx.entity().downgrade();
        let when = div().id("schedule-when").flex_none().tip("Send it once, or again and again").child(
            ButtonGroup::new("schedule-when-group")
                .small()
                .outline()
                .child(Button::new("schedule-once").label("Once").selected(once))
                .child(Button::new("schedule-repeat").label("Repeat").selected(!once))
                .on_click(move |picked: &Vec<usize>, _, cx| {
                    let once = picked.contains(&0);
                    editor.update(cx, |e, cx| e.set_once(once, cx)).ok();
                }),
        );

        let mut rows: Vec<AnyElement> = vec![form_row("Message", message, cx).into_any_element(), form_row("Title", title, cx).into_any_element(), form_row("When", h_flex().child(when), cx).into_any_element()];
        if once {
            let at = div().id("schedule-at").flex_none().w(px(240.)).tip("When to send it").child(DatePicker::new(&self.at).small());
            rows.push(form_row("At", h_flex().child(at), cx).into_any_element());
        } else {
            let kind = state.repeat.kind;
            let mut kinds: Vec<Choice> = Vec::new();
            for k in RepeatKind::ALL {
                if k == RepeatKind::Custom {
                    kinds.push(Choice::Separator);
                }
                kinds.push(Choice::Item { title: k.title().into(), symbol: Some(k.symbol()), checked: k == kind, pick: Rc::new(move |e, _, cx| e.set_kind(k, cx)) });
            }
            let picker = self.menu_button("schedule-kind", kind.title().to_owned(), Some(kind.symbol()), "How often to send it", kinds, cx);
            let detail = self.repeat_detail(cx);
            rows.push(form_row("Repeat", h_flex().gap(px(6.)).items_center().child(picker).children(detail), cx).into_any_element());
            if kind == RepeatKind::Custom {
                let cron = div().id("schedule-cron").tip("Minute, hour, day of the month, month and day of the week").child(Input::new(&self.cron).small().font_family(cx.theme().mono_font_family.clone()));
                rows.push(form_row("Cron", cron, cx).into_any_element());
            }
        }
        rows.push(form_row("", self.preview_text(cx), cx).into_any_element());

        // Where it goes.
        if state.is_new() && state.from_thread_id.is_some() {
            let from = state.from_thread_id.clone();
            let to_thread = state.thread_id.is_some();
            let (e1, e2) = (cx.entity().downgrade(), cx.entity().downgrade());
            let radios = v_flex()
                .id("schedule-sends-to")
                .gap(px(6.))
                .tip("Send it into this thread, or start a new thread with it each time")
                .child(Radio::new("sends-here").label("This thread").checked(to_thread).on_click(move |_, _, cx| {
                    let from = from.clone();
                    e1.update(cx, |e, cx| {
                        e.state.thread_id = from;
                        cx.notify();
                    })
                    .ok();
                }))
                .child(Radio::new("sends-new").label("A new thread each time").checked(!to_thread).on_click(move |_, _, cx| {
                    e2.update(cx, |e, cx| {
                        e.state.thread_id = None;
                        cx.notify();
                    })
                    .ok();
                }));
            rows.push(form_row("Sends to", radios, cx).into_any_element());
        } else {
            rows.push(form_row("Sends to", div().pt(px(5.)).truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child(self.target_text(cx)), cx).into_any_element());
        }
        if state.thread_id.is_none() {
            // What's needed from the app, read before the menus are built
            // (they need the context to themselves).
            let home = std::env::var("HOME").unwrap_or_default();
            let (others, self_symbol, recent, models) = {
                let t = app.read(cx);
                let others: Vec<(String, String, bool, &'static str)> = t.remote.all_runners.iter().filter(|r| !r.is_self).map(|r| (r.id.clone(), r.name.clone(), r.online, icons::runner_symbol(r.icon.as_deref()))).collect();
                let mut folders: Vec<String> = vec![state.cwd.clone(), t.logins.defaults.cwd.clone()];
                let mut threads: Vec<&threadbear_core::inbox::ThreadSummary> = t.threads.values().map(|t| &**t).filter(|t| t.runner_id.is_none()).collect();
                threads.sort_by_cached_key(|t| std::cmp::Reverse(t.activity_ms()));
                folders.extend(threads.into_iter().map(|t| t.cwd.clone()));
                let mut recent: Vec<String> = Vec::new();
                for folder in folders {
                    if !folder.is_empty() && !recent.contains(&folder) {
                        recent.push(folder);
                    }
                    if recent.len() == 8 {
                        break;
                    }
                }
                let models = ScheduleModels::new(state.account_id.as_deref(), state.pool_id.as_deref(), state.model.as_deref(), &t.logins);
                (others, t.self_runner_symbol(), recent, models)
            };
            if state.is_new() && !others.is_empty() {
                // Still a choice when offline: it runs once that Mac is back.
                let current = state.runner_id.as_ref().and_then(|id| others.iter().find(|o| &o.0 == id)).map_or_else(|| "This Mac".to_owned(), |o| o.1.clone());
                let current_symbol = state.runner_id.as_ref().and_then(|id| others.iter().find(|o| &o.0 == id)).map_or(self_symbol, |o| o.3);
                let mut choices = vec![choice("This Mac", state.runner_id.is_none(), |e, window, cx| e.set_runner(None, window, cx)), Choice::Separator];
                for (id, name, online, symbol) in &others {
                    let title = if *online { name.clone() } else { format!("{name} (offline)") };
                    let id = id.clone();
                    let checked = state.runner_id.as_deref() == Some(id.as_str());
                    choices.push(Choice::Item { title, symbol: Some(*symbol), checked, pick: Rc::new(move |e, window, cx| e.set_runner(Some(id.clone()), window, cx)) });
                }
                if let Choice::Item { symbol, .. } = &mut choices[0] {
                    *symbol = Some(self_symbol);
                }
                rows.push(form_row("Runs on", h_flex().child(self.menu_button("schedule-runner", current, Some(current_symbol), "Which Mac runs it and starts its threads", choices, cx)), cx).into_any_element());
            } else if !state.is_new() && (state.is_away() || !others.is_empty()) {
                let name = state.runner_name.clone().unwrap_or_else(|| "This Mac".into());
                let symbol = match &state.runner_id {
                    Some(id) => others.iter().find(|o| &o.0 == id).map_or("desktopcomputer", |o| o.3),
                    None => self_symbol,
                };
                let line = h_flex()
                    .id("schedule-runs-on")
                    .pt(px(4.))
                    .gap(px(5.))
                    .items_center()
                    .text_size(px(style::size::LABEL))
                    .text_color(secondary)
                    .tip("Where it runs. To run it somewhere else, make a new schedule.")
                    .child(icons::symbol(symbol).with_size(px(12.)).text_color(secondary))
                    .child(div().truncate().child(name));
                rows.push(form_row("Runs on", line, cx).into_any_element());
            }
            if state.is_away() {
                // A path on that Mac: nothing here can check or choose it.
                let away = state.runner_name.clone().unwrap_or_else(|| "another Mac".into());
                let tip = format!("A folder on {away}. Left empty, {away}'s default folder.");
                let about = format!("A path on {away}, checked there when it runs. Its threads use {away}'s default account and model.");
                let folder = h_flex().gap(px(6.)).items_center()
                    .child(div().id("schedule-away-folder").tip(tip.clone()).child(Input::new(&self.away_folder).small()))
                    .child(crate::home_page::info_tip("schedule-away-folder-about", about, cx));
                rows.push(form_row("Folder", folder, cx).into_any_element());
            } else {
                let mut choices: Vec<Choice> = recent
                    .into_iter()
                    .map(|folder| {
                        let checked = folder == state.cwd;
                        let title = abbreviate_home(&folder, &home);
                        Choice::Item {
                            title,
                            symbol: Some("folder"),
                            checked,
                            pick: Rc::new(move |e, _, cx| {
                                e.state.cwd = folder.clone();
                                cx.notify();
                            }),
                        }
                    })
                    .collect();
                choices.push(Choice::Separator);
                choices.push(Choice::Item { title: "Choose…".into(), symbol: Some("folder.badge.plus"), checked: false, pick: Rc::new(|e, window, cx| e.choose_folder(window, cx)) });
                let label = abbreviate_home(&state.cwd, &home);
                rows.push(form_row("Folder", h_flex().child(self.menu_button("schedule-folder", label, None, "The folder each new thread starts in", choices, cx)), cx).into_any_element());

                // The model and effort each new thread starts with, from the
                // list of the login it runs on. Only for this Mac: another
                // Mac's threads use its own defaults.
                let picked = models.rows.iter().find(|r| r.is_selected && !r.is_default).map(|r| r.id.clone());
                let model_label = picked.as_ref().and_then(|id| models.rows.iter().find(|r| &r.id == id)).map_or_else(|| models.default_model_title(), |r| r.title.clone());
                let mut model_choices = vec![choice(models.default_model_title(), picked.is_none(), |e, _, cx| e.set_model(None, cx)), Choice::Separator];
                let main: Vec<_> = models.rows.iter().filter(|r| !r.is_default && !r.is_older).collect();
                let older: Vec<_> = models.rows.iter().filter(|r| !r.is_default && r.is_older).collect();
                for row in main {
                    let id = row.id.clone();
                    model_choices.push(choice(row.title.clone(), picked.as_deref() == Some(row.id.as_str()), move |e, _, cx| e.set_model(Some(id.clone()), cx)));
                }
                if !older.is_empty() {
                    model_choices.push(Choice::Separator);
                    for row in older {
                        let id = row.id.clone();
                        model_choices.push(choice(row.title.clone(), picked.as_deref() == Some(row.id.as_str()), move |e, _, cx| e.set_model(Some(id.clone()), cx)));
                    }
                }
                let effort_label = state.effort.as_deref().map_or_else(|| models.default_effort_title(), effort_title);
                let mut effort_choices = vec![
                    choice(models.default_effort_title(), state.effort.is_none(), |e, _, cx| {
                        e.state.effort = None;
                        cx.notify();
                    }),
                    Choice::Separator,
                ];
                for level in &models.effort_levels {
                    let level = level.clone();
                    let checked = state.effort.as_deref() == Some(level.as_str());
                    effort_choices.push(choice(effort_title(&level), checked, move |e, _, cx| {
                        e.state.effort = Some(level.clone());
                        cx.notify();
                    }));
                }
                let model = self.menu_button("schedule-model", model_label, None, "The model each new thread starts on", model_choices, cx);
                let effort = self.menu_button("schedule-effort", effort_label, None, "How hard it thinks", effort_choices, cx);
                rows.push(form_row("Model", h_flex().gap(px(6.)).child(model).child(effort), cx).into_any_element());
            }
        }

        let can_save = self.can_save(cx);
        let save_tip = self.save_tip(cx);
        let save_label = if state.is_new() { "Schedule" } else { "Save" };
        let buttons = h_flex()
            .gap(px(8.))
            .items_center()
            // Not while a save is on its way: its answer closes the editor,
            // and would close the question over it instead.
            .when(!state.is_new(), |s| {
                s.child(Button::new("schedule-delete").label("Delete…").small().outline().disabled(self.saving).tooltip(if self.saving { "Saving…" } else { "Delete this schedule" }).on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))))
            })
            .child(div().flex_1())
            .child(Button::new("schedule-cancel").label("Cancel").small().outline().tooltip("Close without saving (Esc)").on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))))
            .child(
                div().id("schedule-save").test_support().a11y_role(Role::GenericContainer).aria_label(save_tip.to_owned()).child(
                    Button::new("schedule-save-button")
                        .label(save_label)
                        .small()
                        .primary()
                        .loading(self.saving)
                        .disabled(!can_save)
                        .tooltip(save_tip)
                        .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                ),
            );
        let failed = style::failed(cx);
        v_flex()
            .id("schedule-editor")
            .gap(px(16.))
            .child(div().text_size(px(15.)).font_semibold().child(header))
            .child(v_flex().gap(px(6.)).children(rows))
            .when_some(self.save_error.clone(), |s, error| s.child(div().text_size(px(style::size::LABEL)).text_color(failed).child(error)))
            .child(buttons)
            .into_any_element()
    }
}

/// The editor in the dump, for the specs: what it's set to and says.
#[cfg(feature = "automation")]
impl ScheduleEditor {
    pub(crate) fn dump(&self, cx: &App) -> Value {
        json!({
            "isNew": self.state.is_new(),
            "text": self.message_text(cx),
            "once": self.state.once,
            "cron": if self.state.once { Value::Null } else { json!(self.state.repeat.cron()) },
            "preview": match &self.preview { Some(Ok(p)) => json!(p.when_text), Some(Err(e)) => json!(format!("error: {e}")), None => Value::Null },
            "canSave": self.can_save(cx),
            "error": self.save_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::EditorState;

    fn opened(cwd: &str, away: bool) -> EditorState {
        EditorState { schedule_id: Some("s1".into()), cwd: cwd.into(), opened_cwd: Some(cwd.into()), runner_id: away.then(|| "r2".into()), ..EditorState::default() }
    }

    #[test]
    fn a_save_that_keeps_the_folder_sends_none() {
        // Finding 89: the folder went with every save, and the runner took it as a move.
        assert_eq!(opened("/Users/m/dev/x", false).update_cwd(""), None);
        assert_eq!(opened("/Users/m/dev/x ", false).update_cwd(""), None);
        assert_eq!(opened("~/work", true).update_cwd(" ~/work "), None);
    }

    #[test]
    fn a_changed_folder_is_sent() {
        let mut s = opened("/Users/m/dev/x", false);
        s.cwd = "/Users/m/dev/y".into();
        assert_eq!(s.update_cwd(""), Some("/Users/m/dev/y".into()));
        assert_eq!(opened("~/work", true).update_cwd("~/other"), Some("~/other".into()));
        // Another Mac's default folder, cleared: sent as empty.
        assert_eq!(opened("~/work", true).update_cwd(""), Some(String::new()));
    }
}
