// The Scheduled view, a port of the Swift app's ScheduledList, ScheduledHome,
// HomePage and the row actions in Schedules.swift: every schedule in the
// sidebar under Active, Paused and Done headings that fold like the inbox's,
// soonest first in each, and the page beside it (the bear winding its alarm
// clock, what schedules are, what's next and the numbers). Clicking a row
// opens the editor (schedule_editor.rs); its target opens the thread.
//
// Every click shows at once: Pause and Delete change the list before the
// runner hears and go back if it says no, and Run now, which has nothing to
// guess, spins in the row and takes no more clicks until it lands. Another
// Mac's Run now waits on a line under the row until that Mac has run it.
use crate::app::Threadbear;
use crate::home_page::{home_status_line, home_title, mode_header, Art, Pokes};
use crate::sidebar::{icon, icon_button, palette, tipped, MenuAction, MenuRow, Palette, CORNER, LEADING, OUTER_INSET, TRAILING};
use crate::{call, icons, now_ms, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::HashSet;
use threadbear_core::chrono::Local;
use threadbear_core::highlights::Schedule;
use threadbear_core::schedules::{self as rules, ScheduleSection};
use crate::tips::Tip as _;


/// The Scheduled view's own state; the app holds one (`Threadbear::scheduled`).
pub(crate) struct ScheduledState {
    /// The runner's list (`schedules.list`, then `schedules.updated`), in its
    /// order: soonest first.
    pub(crate) list: Vec<Schedule>,
    /// The headings folded away, by `ScheduleSection`'s key, saved as
    /// `collapsedScheduleSections`.
    pub(crate) folded: HashSet<&'static str>,
    /// The row under the pointer. A script's hover is the sidebar's
    /// `hover_override`, which wins.
    pub(crate) hovered: Option<String>,
    /// Work on the way with nothing to show till it lands: "run:<id>", and
    /// "pause:<id>" so a second click can't overlap the first's rollback.
    pub(crate) busy: HashSet<String>,
    scroll: ScrollHandle,
    /// What the page's picture says when it's poked (home_page.rs).
    pub(crate) pokes: Pokes,
}

impl ScheduledState {
    pub(crate) fn new(saved_folded: Option<&[String]>) -> Self {
        let folded = match saved_folded {
            Some(saved) => ScheduleSection::ALL.into_iter().map(ScheduleSection::key).filter(|k| saved.iter().any(|s| s == k)).collect(),
            // Done starts folded (`storedCollapsedScheduleSections`).
            None => HashSet::from(["done"]),
        };
        ScheduledState { list: Vec::new(), folded, hovered: None, busy: HashSet::new(), scroll: ScrollHandle::new(), pokes: Pokes::new(Art::Scheduled) }
    }

    /// `activeScheduleCount`: the activity bar's badge and the view's count.
    pub(crate) fn active_count(&self) -> usize {
        self.list.iter().filter(|s| s.is_active()).count()
    }
}

impl Threadbear {
    pub(crate) fn schedule(&self, id: &str) -> Option<&Schedule> {
        self.scheduled.list.iter().find(|s| s.id == id)
    }

    /// The whole list, from `schedules.list` or `schedules.updated`.
    pub(crate) fn set_schedules(&mut self, value: &Value) {
        self.scheduled.list = Schedule::list(value);
        let live: HashSet<String> = self.scheduled.list.iter().map(|s| s.id.clone()).collect();
        if self.scheduled.hovered.as_ref().is_some_and(|id| !live.contains(id)) {
            self.scheduled.hovered = None;
        }
    }

    /// A reply's schedule goes in straight away; `schedules.updated` brings
    /// the whole list (in order) a moment later anyway. Only a new schedule
    /// is added: a Run now or Pause answered after the schedule was deleted
    /// would otherwise bring it back as a row with nothing behind it.
    fn replace_schedule(&mut self, schedule: Schedule, insert: bool) {
        match self.scheduled.list.iter_mut().find(|s| s.id == schedule.id) {
            Some(old) => *old = schedule,
            None if insert => self.scheduled.list.insert(0, schedule),
            None => {}
        }
    }

    /// The schedule in a reply about one already listed.
    pub(crate) fn schedule_reply(&mut self, reply: &Value) {
        if let Ok(schedule) = serde_json::from_value::<Schedule>(reply["schedule"].clone()) {
            self.replace_schedule(schedule, false);
        }
    }

    /// The schedule `schedules.create` made.
    pub(crate) fn schedule_created(&mut self, reply: &Value) {
        if let Ok(schedule) = serde_json::from_value::<Schedule>(reply["schedule"].clone()) {
            self.replace_schedule(schedule, true);
        }
    }

    fn self_runner_id(&self) -> Option<&str> {
        self.remote.all_runners.iter().find(|r| r.is_self).map(|r| r.id.as_str())
    }

    /// The other runner a schedule runs on, by name; None when it's this one
    /// (`awayRunnerName`).
    pub(crate) fn away_runner_name(&self, schedule: &Schedule) -> Option<String> {
        if !schedule.runs_elsewhere(self.self_runner_id()) {
            return None;
        }
        let id = schedule.runner_id.as_deref()?;
        Some(schedule.runner_name.clone().or_else(|| self.remote.all_runners.iter().find(|r| r.id == id).map(|r| r.name.clone())).unwrap_or_else(|| "another Mac".into()))
    }

    /// The Mac the editor treats a schedule as running on, when it isn't
    /// this one. Until this Mac's own runner is known (`runners.list` at
    /// connect), a schedule that names a runner might be another Mac's, so
    /// it's taken to be: saving it then sends its own folder back as it was,
    /// rather than this Mac's default folder and models.
    pub(crate) fn editor_away_name(&self, schedule: &Schedule) -> Option<String> {
        self.away_runner_name(schedule).or_else(|| match (schedule.runner_id.as_ref(), self.self_runner_id()) {
            (Some(_), None) => Some(schedule.runner_name.clone().unwrap_or_else(|| "another Mac".into())),
            _ => None,
        })
    }

    /// The icon of the runner a schedule runs on (`runnerIcon`).
    pub(crate) fn schedule_runner_symbol(&self, schedule: &Schedule) -> &'static str {
        let icon = schedule.runner_icon.as_deref().or_else(|| self.remote.all_runners.iter().find(|r| Some(r.id.as_str()) == schedule.runner_id.as_deref()).and_then(|r| r.icon.as_deref()));
        icons::runner_symbol(icon)
    }

    /// Where it sends: its thread's title, or "New thread in misc" (`scheduleTarget`).
    pub(crate) fn schedule_target(&self, schedule: &Schedule) -> String {
        let title = schedule.thread_id.as_deref().and_then(|id| self.threads.get(id)).map(|t| t.title.as_str());
        rules::target_text(schedule, title, self.away_runner_name(schedule).is_some(), &self.logins.defaults.cwd)
    }

    /// The thread its target opens: its own, or the last one a schedule that
    /// starts a new thread each time started, once there is one.
    fn schedule_opens(&self, schedule: &Schedule) -> Option<String> {
        schedule.thread_id.clone().or_else(|| schedule.last_run.as_ref().and_then(|r| r.thread_id.clone())).filter(|id| self.threads.contains_key(id))
    }

    /// Folding a heading is remembered, as the inbox's are.
    fn schedule_settings_changed(&mut self, cx: &mut Context<Self>) {
        let mut folded: Vec<String> = self.scheduled.folded.iter().map(|k| k.to_string()).collect();
        folded.sort();
        self.settings.saved.collapsed_schedule_sections = Some(folded);
        self.save_settings_soon(cx);
    }

    // The row's actions.

    /// Run now. Another runner's comes back queued, with `runRequestedAt`
    /// set, which its row shows until that runner has run it. The row spins
    /// until the reply (`runSchedule`).
    pub(crate) fn run_schedule(&mut self, id: String, cx: &mut Context<Self>) {
        let key = format!("run:{id}");
        if !self.scheduled.busy.insert(key.clone()) {
            return;
        }
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "schedules.run", json!({ "scheduleId": id })).await;
            this.update(cx, |this, cx| {
                this.scheduled.busy.remove(&key);
                match res {
                    Ok(reply) => this.schedule_reply(&reply),
                    Err(e) => this.show_error(e.message, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pause or Resume: shows at once, and goes back if the runner refuses.
    /// The control waits for one to land before taking another, so two can't
    /// overlap: if both failed, the second's rollback would leave the first's
    /// guess showing (`setSchedulePaused`).
    pub(crate) fn set_schedule_paused(&mut self, id: String, paused: bool, cx: &mut Context<Self>) {
        let key = format!("pause:{id}");
        if self.scheduled.busy.contains(&key) {
            return;
        }
        let Some(before) = self.schedule(&id).cloned() else { return };
        let mut guess = before.clone();
        guess.paused = paused;
        self.replace_schedule(guess.clone(), false);
        self.scheduled.busy.insert(key.clone());
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "schedules.update", json!({ "scheduleId": id, "paused": paused })).await;
            this.update(cx, |this, cx| {
                this.scheduled.busy.remove(&key);
                match res {
                    Ok(reply) => this.schedule_reply(&reply),
                    Err(e) => {
                        if this.schedule(&id) == Some(&guess) {
                            this.replace_schedule(before, false);
                        }
                        this.show_error(e.message, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks first: a deleted schedule can't be got back (`deleteSchedule`).
    pub(crate) fn delete_schedule(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(schedule) = self.schedule(&id) else { return };
        let title = format!("Delete \u{201C}{}\u{201D}?", rules::title(schedule));
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (entity, id) = (entity.clone(), id.clone());
            alert.title(title.clone()).description("It won't send its message again.").button_props(DialogButtonProps::default().ok_text("Delete").show_cancel(true)).on_ok(move |_, window, cx| {
                let id = id.clone();
                // Asked from the editor: the editor goes too.
                let editing = entity.upgrade().is_some_and(|e| e.read(cx).schedule_editor_id(cx).as_deref() == Some(id.as_str()));
                if editing {
                    window.close_all_dialogs(cx);
                }
                entity
                    .update(cx, move |this, cx| {
                        if editing {
                            this.schedule_editor = None;
                        }
                        this.confirmed_delete_schedule(id, cx);
                    })
                    .ok();
                !editing
            })
        });
    }

    /// Gone at once, and back where it was if the runner refuses.
    pub(crate) fn confirmed_delete_schedule(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(index) = self.scheduled.list.iter().position(|s| s.id == id) else { return };
        let schedule = self.scheduled.list.remove(index);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "schedules.delete", json!({ "scheduleId": id })).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(_) => this.scheduled.list.retain(|s| s.id != id),
                    Err(e) => {
                        if this.schedule(&id).is_none() {
                            let at = index.min(this.scheduled.list.len());
                            this.scheduled.list.insert(at, schedule);
                        }
                        this.show_error(e.message, cx);
                        // It may have gone through and only the reply been lost.
                        this.load_schedules(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The row's right-click menu (`ScheduleRowView.iconContextMenu`).
    fn schedule_menu_rows(&self, schedule: &Schedule) -> Vec<MenuRow> {
        let id = schedule.id.clone();
        let mut rows = vec![MenuRow::item("Edit…", "pencil", MenuAction::EditSchedule(id.clone()))];
        let run = MenuRow::item("Run now", "play", MenuAction::RunSchedule(id.clone()));
        rows.push(if self.scheduled.busy.contains(&format!("run:{id}")) { run.disabled() } else { run });
        if !schedule.is_finished() {
            let (title, symbol) = if schedule.paused { ("Resume", "playpause") } else { ("Pause", "pause") };
            let pause = MenuRow::item(title, symbol, MenuAction::PauseSchedule(id.clone(), !schedule.paused));
            rows.push(if self.scheduled.busy.contains(&format!("pause:{id}")) { pause.disabled() } else { pause });
        }
        rows.push(MenuRow::separator());
        rows.push(MenuRow::item("Delete…", "trash", MenuAction::DeleteSchedule(id)));
        rows
    }

    fn open_schedule_menu(&mut self, id: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(schedule) = self.schedule(&id).cloned() else { return };
        let rows = self.schedule_menu_rows(&schedule);
        // The menu's "thread" is the schedule here: it keeps the row's buttons out.
        self.open_menu(Some(id), rows, position, false, window, cx);
    }

    // Drawing the sidebar's view.

    /// `ScheduledList`: the view's name and count, New schedule, then the
    /// headings and their rows.
    pub(crate) fn render_scheduled_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let now = now_ms();
        let groups = ScheduleSection::group(&self.scheduled.list);
        let mut list = v_flex().px(px(OUTER_INSET)).pt(px(4.)).pb(px(32.));
        for (section, rows) in &groups {
            let key = section.key();
            let folded = self.scheduled.folded.contains(key);
            let heading = self.section_heading(
                &format!("schedule-{key}"),
                section.title(),
                section.symbol(),
                rows.len(),
                folded,
                true,
                None,
                &p,
                move |this, cx| {
                    if !this.scheduled.folded.remove(key) {
                        this.scheduled.folded.insert(key);
                    }
                    this.schedule_settings_changed(cx);
                },
                cx,
            );
            list = list.child(heading);
            if !folded {
                for schedule in rows {
                    list = list.child(self.render_schedule_row(schedule, now, &p, cx));
                }
            }
        }
        if self.scheduled.list.is_empty() {
            list = list.child(div().pl(px(LEADING)).pt(px(10.)).text_size(px(style::size::SIDEBAR_PREVIEW)).text_color(p.secondary).child("Nothing scheduled."));
        }
        let count = self.scheduled.active_count();
        let side = p.side;
        v_flex()
            .relative()
            .size_full()
            .bg(p.side)
            .child(mode_header("Scheduled", Some(count), &p))
            .child(div().px(px(OUTER_INSET)).child(self.render_new_schedule_button(&p, cx)))
            .child(
                // As the inbox's list: no scrollbar, fading out at the foot.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(div().id("scheduled-list").size_full().overflow_y_scroll().track_scroll(&self.scheduled.scroll).child(list))
                    .child(div().absolute().left_0().right_0().bottom_0().h(px(34.)).bg(linear_gradient(180., linear_color_stop(side.opacity(0.), 0.), linear_color_stop(side, 0.88)))),
            )
            .into_any_element()
    }

    /// `SidebarButtonRow`: the plus where a row's title starts.
    fn render_new_schedule_button(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, secondary, fg) = (p.hover, p.secondary, p.fg);
        tipped!(h_flex().gap(px(8.)).pl(px(LEADING - 2.)).pr(px(TRAILING)).py(px(7.)).rounded(px(CORNER)).hover(move |s| s.bg(hover)), "new-schedule", "Schedule a message for later")
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.new_schedule(None, window, cx)))
            .child(div().w(px(16.)).flex().justify_center().child(icon("plus", style::size::SIDEBAR_BUTTON, secondary)))
            .child(div().text_size(px(style::size::SIDEBAR_BUTTON)).line_height(px(17.)).text_color(fg).child("New schedule"))
    }

    /// `ScheduleRowView`: its title and when it next runs; when it runs, the
    /// Mac it runs on if that's another, and where it sends; then a run that
    /// didn't send, or another Mac's Run now still waiting.
    fn render_schedule_row(&mut self, schedule: &Schedule, now: i64, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let id = schedule.id.clone();
        let running = self.scheduled.busy.contains(&format!("run:{id}"));
        let hovered = self.sidebar.hover_override.as_deref().or(self.scheduled.hovered.as_deref()) == Some(id.as_str());
        let menu_open = self.sidebar.menu.as_ref().is_some_and(|m| m.thread.as_deref() == Some(id.as_str()));
        // Stays out while Run now is on its way, so its spinner shows
        // wherever it was asked from (the row's menu, or a pointer that's
        // moved on).
        let show_actions = hovered || running || menu_open;
        let away = self.away_runner_name(schedule);
        let (fg, secondary, faint, failed) = (p.fg, p.secondary, p.faint, p.failed);

        let title = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(style::size::SIDEBAR_TITLE))
            .line_height(px(17.))
            // A paused or finished one is there to be found, not something coming up.
            .text_color(if schedule.is_active() { fg } else { secondary })
            .child(rules::title(schedule).to_owned());
        let state: Option<AnyElement> = if show_actions {
            // Room for the hover buttons, drawn over the row.
            Some(div().flex_none().w(px(3. * 24. - 6.)).h(px(1.)).into_any_element())
        } else if let Some(word) = rules::row_state(schedule) {
            let color = if schedule.paused { secondary } else { faint };
            Some(div().flex_none().text_size(px(style::size::SIDEBAR_META)).line_height(px(14.)).text_color(color).child(word).into_any_element())
        } else {
            rules::row_next(schedule, now).map(|next| {
                let full = schedule.next_ms().map(|at| rules::full_time(at, &Local)).unwrap_or_default();
                div()
                    .id(ElementId::Name(format!("schedule-next-{id}").into()))
                    .flex_none()
                    .text_size(px(style::size::SIDEBAR_META))
                    .line_height(px(14.))
                    .text_color(faint)
                    .tip(full.clone())
                    .child(next)
                    .into_any_element()
            })
        };

        let dot = || div().flex_none().text_color(faint).child("·");
        // When it runs stays whole before where it goes is cut short
        // (`layoutPriority(2)` in the Swift row), up to most of the line.
        let mut place = h_flex()
            .min_w_0()
            .items_center()
            .gap(px(4.))
            .text_size(px(style::size::SIDEBAR_PREVIEW))
            .line_height(px(16.))
            .text_color(secondary)
            .child(div().flex_none().max_w(relative(0.7)).truncate().child(schedule.when_text.clone()));
        if let Some(away) = away.clone() {
            // Quiet, like the rest of the line: most schedules are this Mac's,
            // and those say nothing about it. Its Mac's icon, so runners tell
            // apart at a glance.
            let tip = format!("Runs on {away}");
            place = place.child(dot()).child(
                h_flex()
                    .id(ElementId::Name(format!("schedule-runner-{id}").into()))
                    .flex_none()
                    .max_w(px(100.))
                    .gap(px(3.))
                    .items_center()
                    .tip(tip.clone())
                    .child(icon(self.schedule_runner_symbol(schedule), style::size::SIDEBAR_META, secondary))
                    .child(div().min_w_0().truncate().child(away)),
            );
        }
        place = place.child(dot()).child(self.render_schedule_target(schedule, p, cx));

        // Another Mac's Run now, until that Mac picks it up; else a last run
        // that didn't send.
        let note: Option<(String, Hsla, String)> = match (away.as_ref(), schedule.run_requested_ms()) {
            (Some(away), Some(asked)) => {
                let asked = rules::time_of_day(&threadbear_core::chrono::TimeZone::timestamp_millis_opt(&Local, asked).single().unwrap_or_else(Local::now));
                Some((format!("Waiting for {away} to run it"), faint, format!("Asked at {asked}. It runs as soon as {away} picks it up.")))
            }
            _ => rules::row_problem(schedule).map(|(text, is_failure)| (text.clone(), if is_failure { failed } else { faint }, text)),
        };
        let note = note.map(|(text, color, tip)| {
            div()
                .id(ElementId::Name(format!("schedule-note-{id}").into()))
                .truncate()
                .text_size(px(style::size::SIDEBAR_META))
                .line_height(px(14.))
                .text_color(color)
                .tip(tip.clone())
                .child(text)
        });

        let buttons = show_actions.then(|| self.render_schedule_actions(schedule, away.as_deref(), running, p, cx));
        // What it sends, which the row doesn't show. Click to edit it.
        let tip: String = schedule.text.chars().take(300).collect();
        let (hover_id, menu_id, edit_id) = (id.clone(), id.clone(), id.clone());
        let hover_bg = p.hover;
        v_flex()
            .id(ElementId::Name(format!("schedule-{id}").into()))
            .test_support()
            // Named by its tip, as the Swift app's rows are found.
            .aria_label(tip.clone())
            .tip(tip.clone())
            .relative()
            .gap(px(3.))
            .pt(px(8.))
            .pb(px(8.))
            .pl(px(LEADING))
            .pr(px(TRAILING))
            .rounded(px(CORNER))
            .when(show_actions, |s| s.bg(hover_bg))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.scheduled.hovered = Some(hover_id.clone());
                } else if this.scheduled.hovered.as_deref() == Some(hover_id.as_str()) {
                    this.scheduled.hovered = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.edit_schedule(&edit_id, window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| this.open_schedule_menu(menu_id.clone(), ev.position, window, cx)))
            .child(h_flex().items_baseline().gap(px(6.)).child(title).children(state))
            .child(place)
            .children(note)
            .when_some(buttons, |s, b| s.child(h_flex().absolute().top(px(5.)).right(px(TRAILING - 5.)).gap(px(2.)).child(b)))
            .into_any_element()
    }

    /// `TargetButton`: where it sends, which opens that thread (or, for one
    /// that starts a new thread each time, the last one it started).
    fn render_schedule_target(&mut self, schedule: &Schedule, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let label = self.schedule_target(schedule);
        let Some(opens) = self.schedule_opens(schedule) else {
            return div().min_w_0().truncate().child(label).into_any_element();
        };
        let tip = if schedule.thread_id.is_none() { "Open the last thread it started" } else { "Open the thread" };
        let underline = p.secondary;
        tipped!(div().min_w_0().truncate().cursor_pointer().hover(move |s| s.underline().text_decoration_color(underline)), ElementId::Name(format!("schedule-target-{}", schedule.id).into()), tip)
            // The row under it opens the editor; this opens the thread instead.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                this.open(opens.clone(), window, cx);
            }))
            .child(label)
            .into_any_element()
    }

    /// The row's buttons on hover: Run now, Pause or Resume, Delete.
    fn render_schedule_actions(&mut self, schedule: &Schedule, away: Option<&str>, running: bool, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let id = schedule.id.clone();
        let mut row = h_flex().gap(px(2.));
        if running {
            // On its way: a spinner in its place, and no more clicks.
            row = row.child(div().flex_none().size(px(22.)).flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(p.secondary).with_size(px(13.))));
        } else {
            let tip = away.map_or_else(|| "Run now".to_owned(), |away| format!("Ask {away} to run it now"));
            let run_id = id.clone();
            row = row.child(icon_button(ElementId::Name(format!("run-{id}").into()), icon("paperplane", 13., p.secondary), tip, p, cx.listener(move |this, _: &ClickEvent, _, cx| this.run_schedule(run_id.clone(), cx))));
        }
        if !schedule.is_finished() {
            let paused = schedule.paused;
            let (symbol, tip) = if paused { ("play", "Resume") } else { ("pause", "Pause") };
            let pausing = self.scheduled.busy.contains(&format!("pause:{id}"));
            let pause_id = id.clone();
            let button = icon_button(ElementId::Name(format!("pause-{id}").into()), icon(symbol, 13., p.secondary), tip, p, cx.listener(move |this, _: &ClickEvent, _, cx| this.set_schedule_paused(pause_id.clone(), !paused, cx)));
            // Until the last one lands (it already shows), so no second click.
            row = row.child(div().when(pausing, |d| d.opacity(0.4).occlude()).child(button));
        }
        let delete_id = id.clone();
        row = row.child(icon_button(ElementId::Name(format!("delete-{id}").into()), icon("trash", 13., p.secondary), "Delete", p, cx.listener(move |this, _: &ClickEvent, window, cx| this.delete_schedule(delete_id.clone(), window, cx))));
        row.into_any_element()
    }

    // The page beside it.

    /// `ScheduledHome`: what clicking Scheduled in the activity bar shows in
    /// the main panel, beside the sidebar's list.
    pub(crate) fn render_scheduled_home(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let now = now_ms();
        let local = Local::now();
        let next = rules::next(&self.scheduled.list);
        let status = next.map_or_else(|| "Nothing coming up".to_owned(), |s| format!("Next: {}, {}", rules::title(s), rules::when(s, &local)));
        let stats = if self.schedules_supported { rules::stats(&self.scheduled.list, now) } else { Vec::new() };
        let about = v_flex()
            .gap(px(12.))
            .child(home_title("Scheduled", "Messages that send themselves later, once or on repeat.", SCHEDULED_MORE, cx))
            .child(home_status_line(&status, next.is_some(), cx));
        self.render_home_page(Art::Scheduled, about.into_any_element(), &stats, None, cx)
    }

    /// The Scheduled view in the dump, in the Swift app's shape: its
    /// headings as the sidebar's sections and the schedules as their rows,
    /// their state the status (AppDump's `.scheduled`).
    #[cfg(feature = "automation")]
    pub(crate) fn scheduled_dump(&self) -> Value {
        let now = now_ms();
        let groups = ScheduleSection::group(&self.scheduled.list);
        let sections: Vec<Value> = ScheduleSection::ALL
            .into_iter()
            .map(|section| {
                let rows = groups.iter().find(|(s, _)| *s == section).map(|(_, rows)| rows.as_slice()).unwrap_or(&[]);
                let threads: Vec<Value> = rows
                    .iter()
                    .map(|s| {
                        let hovered = self.sidebar.hover_override.as_deref() == Some(s.id.as_str());
                        let away = self.away_runner_name(s);
                        let waiting = away.as_ref().filter(|_| s.run_requested_at.is_some()).map(|away| format!("Waiting for {away} to run it"));
                        let mut buttons = vec![away.as_ref().map_or_else(|| "Run now".to_owned(), |a| format!("Ask {a} to run it now"))];
                        if !s.is_finished() {
                            buttons.push(if s.paused { "Resume" } else { "Pause" }.to_owned());
                        }
                        buttons.push("Delete".to_owned());
                        let place = [Some(s.when_text.clone()), away.clone(), Some(self.schedule_target(s))].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        json!({
                            "id": s.id, "title": s.title,
                            "status": if s.paused { "paused" } else if s.is_finished() { "finished" } else { "active" },
                            "needsYou": false, "unread": false, "hovered": hovered,
                            "trailing": rules::row_state(s).map(str::to_owned).or_else(|| rules::row_next(s, now)).unwrap_or_default(),
                            "place": place,
                            "waiting": waiting.or_else(|| rules::row_problem(s).map(|(text, _)| text)),
                            "hoverButtons": hovered.then_some(buttons),
                        })
                    })
                    .collect();
                json!({ "name": section.title(), "collapsed": self.scheduled.folded.contains(section.key()), "threads": threads })
            })
            .collect();
        json!({ "sections": sections, "count": self.scheduled.active_count() })
    }

    /// The page in the main panel, for the dump (`homePage`).
    #[cfg(feature = "automation")]
    pub(crate) fn scheduled_home_dump(&self) -> Value {
        let local = Local::now();
        let next = rules::next(&self.scheduled.list);
        let status = next.map_or_else(|| "Nothing coming up".to_owned(), |s| format!("Next: {}, {}", rules::title(s), rules::when(s, &local)));
        let stats = if self.schedules_supported { rules::stats(&self.scheduled.list, now_ms()) } else { Vec::new() };
        json!({
            "title": "Scheduled",
            "status": status,
            "poke": self.scheduled.pokes.line,
            "stats": stats.iter().map(|s| json!({ "value": s.value, "label": s.label })).collect::<Vec<_>>(),
        })
    }
}

/// What the ⓘ beside the page's title says.
const SCHEDULED_MORE: &str = "Once at a set time, or over and over, like every weekday at 9. Each goes to a thread, or starts a new thread each time. Agents book them too, to check back on something. Make one with New schedule in the list, from Schedule\u{2026} in a composer, or by asking an agent. A schedule that sends to a thread puts it under Waiting until then. A schedule that starts a new thread can run on another of your Macs (Runs on). Schedules live in your Convex backend, so with no internet they wait.";

/// Scripted commands for the Scheduled view, as the Swift app's: a heading
/// folded or opened (`scheduleSection`). A row's buttons are clicked by their
/// tips once it's hovered (`hover` with its id, sidebar.rs).
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    let name = cmd["cmd"].as_str().unwrap_or("");
    match name {
        "expand" | "collapse" => {
            let raw = cmd["scheduleSection"].as_str().unwrap_or("");
            let key = ScheduleSection::ALL.into_iter().map(ScheduleSection::key).find(|k| *k == raw).ok_or_else(|| format!("scheduleSection must be one of active, paused, done, not {raw:?}"))?;
            app.update(cx, |t, cx| {
                if name == "expand" {
                    t.scheduled.folded.remove(key);
                } else {
                    t.scheduled.folded.insert(key);
                }
                t.schedule_settings_changed(cx);
                cx.notify();
            });
            Ok(json!("ok"))
        }
        _ => Err(format!("unknown scheduled command {name}")),
    }
}

/// The Swift app's `schedule` command: the editor, on a schedule by its id,
/// or a new one (`"from": "none"`, as the list's New schedule; otherwise
/// from the open thread). And `close-schedule`, which closes it.
#[cfg(feature = "automation")]
pub(crate) fn editor_automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    if cmd["cmd"].as_str() == Some("close-schedule") {
        if app.read(cx).schedule_editor.is_some() {
            window.close_all_dialogs(cx);
            app.update(cx, |t, cx| {
                t.schedule_editor = None;
                cx.notify();
            });
        }
        return Ok(json!("ok"));
    }
    match cmd["id"].as_str() {
        Some(id) => {
            if app.read(cx).schedule(id).is_none() {
                return Err(format!("no schedule {id}"));
            }
            app.update(cx, |t, cx| t.edit_schedule(id, window, cx));
        }
        None => {
            let from = if cmd["from"].as_str() == Some("none") { None } else { app.read(cx).selected.clone() };
            app.update(cx, |t, cx| t.new_schedule(from, window, cx));
        }
    }
    Ok(json!("ok"))
}
