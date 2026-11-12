// The Updates panel, a port of the Swift app's UpdatesPanel.swift: what's on
// its way for the app, the runner and the agent CLIs, with the one or two
// things Mike can do about each. The runner's side comes from hello's
// `updates` and then `runner.updates`; the CLIs from hello's `clis` (or
// `clis.list`) and then `clis.updated`. The words are threadbear_core::updates.
//
// The app's own update (app_update.rs) is its App section, and a right-click
// on the icon puts it in a popover beside it with Restart to Update, the quick
// way to take one (Swift's AppUpdatePopover).
use crate::activity_panels::{button_row, panel_column, push_button, small_heading, system_orange, text_button, text_line, MenuLabel};
use crate::app::Threadbear;
use crate::{call, icons, now_ms, shell, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::new_thread::{Draft, NewThreadPicks};
use threadbear_core::updates::{self as rules, AppSide, CliStatus, RunnerUpdates};
use crate::tips::Tip as _;


/// How long Copy says it copied.
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// The Updates panel's state in the app (`Threadbear::updates_panel`).
#[derive(Default)]
pub(crate) struct UpdatesPanelState {
    /// The view the dock shows it in, made the first time it's drawn.
    pub(crate) view: Option<Entity<crate::activity_panels::PanelView>>,
    /// The runner's own update (protocol "Runner updates"); None from a
    /// runner that doesn't say.
    pub(crate) runner: Option<RunnerUpdates>,
    pub(crate) clis: Vec<CliStatus>,
    /// Some(false): a runner from before the CLIs' requests. None: not known
    /// yet (one that put none in its hello, before `clis.list` answers).
    pub(crate) clis_supported: Option<bool>,
    /// The buttons' calls on their way: "check", "update/<provider>",
    /// "policy/<provider>" (the Swift app's `BusyKey`s).
    pub(crate) busy: HashSet<String>,
    /// Bring Them In or Let's Discuss is starting its thread.
    pub(crate) bringing_in: bool,
    /// The launcher's command was copied a moment ago (counts the copies, so
    /// an older one's timer leaves a newer one's word alone).
    copied: Option<u64>,
    copies: u64,
    /// The icon's right-click popover is up, and its keyboard (Return
    /// restarts, Esc puts it away).
    pub(crate) popover: bool,
    popover_focus: Option<FocusHandle>,
}

impl Threadbear {
    // What the runner says.

    /// From hello: a runner since step 4 puts its CLIs there; an older one
    /// has none, and `clis.list` tells which.
    pub(crate) fn updates_from_hello(&mut self, hello: &Value) {
        let panel = &mut self.updates_panel;
        panel.clis = rules::decode_clis(&hello["clis"]);
        panel.clis_supported = if panel.clis.is_empty() { None } else { Some(true) };
        panel.runner = RunnerUpdates::decode(&hello["updates"]);
        panel.busy.clear();
    }

    /// After hello, for a runner that didn't put its CLIs in it.
    pub(crate) fn load_clis(&mut self, cx: &mut Context<Self>) {
        if !self.updates_panel.clis.is_empty() {
            return;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "clis.list", json!({})).await;
            this.update(cx, |t, cx| {
                match res {
                    Ok(v) => {
                        t.updates_panel.clis = rules::decode_clis(&v["clis"]);
                        t.updates_panel.clis_supported = Some(true);
                    }
                    Err(e) if e.code == -32601 => {
                        t.updates_panel.clis.clear();
                        t.updates_panel.clis_supported = Some(false);
                    }
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `clis.updated`: the latest list wins.
    pub(crate) fn clis_updated(&mut self, params: &Value) {
        self.updates_panel.clis = rules::decode_clis(&params["clis"]);
        self.updates_panel.clis_supported = Some(true);
    }

    /// `runner.updates`: replaces the one from hello.
    pub(crate) fn runner_updates_changed(&mut self, params: &Value) {
        self.updates_panel.runner = RunnerUpdates::decode(&params["updates"]);
    }

    /// The app's own update (app_update.rs).
    fn app_side(&self) -> AppSide {
        self.app_update_side()
    }

    /// Something on its way for the runner or a CLI: the icon's grey dot.
    pub(crate) fn updates_quiet_dot(&self) -> bool {
        rules::shown(&self.app_side(), self.updates_panel.runner.as_ref()) || self.updates_panel.clis.iter().any(rules::cli_pending)
    }

    /// Something only Mike can do: the accent dot.
    pub(crate) fn updates_dot(&self) -> bool {
        rules::needs_you(&self.app_side(), self.updates_panel.runner.as_ref())
    }

    pub(crate) fn updates_tooltip(&self) -> String {
        rules::icon_tooltip(&self.app_side(), self.updates_panel.runner.as_ref(), &self.updates_panel.clis)
    }

    // The buttons.

    /// A CLI call: the button spins until the runner takes it, and the list it
    /// answers with is the new one; progress comes through `clis.updated`.
    /// `undo` puts back a change shown before the answer (the policy).
    fn cli_call(&mut self, key: String, method: &'static str, params: Value, undo: Option<Box<dyn FnOnce(&mut Threadbear)>>, cx: &mut Context<Self>) {
        if !self.updates_panel.busy.insert(key.clone()) {
            return;
        }
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, method, params).await;
            this.update(cx, |t, cx| {
                t.updates_panel.busy.remove(&key);
                match res {
                    Ok(v) => {
                        t.updates_panel.clis = rules::decode_clis(&v["clis"]);
                        t.updates_panel.clis_supported = Some(true);
                    }
                    Err(e) => {
                        if let Some(undo) = undo {
                            undo(t);
                        }
                        if e.code == -32601 {
                            t.updates_panel.clis_supported = Some(false);
                        } else {
                            t.show_error(e.message, cx);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Check now: both CLIs looked at again.
    pub(crate) fn check_clis(&mut self, cx: &mut Context<Self>) {
        self.cli_call("check".into(), "clis.check", json!({}), None, cx);
    }

    /// Update, Update anyway (`force`, past a hold) or Go back (`version`).
    pub(crate) fn update_cli(&mut self, provider: &str, version: Option<String>, force: bool, cx: &mut Context<Self>) {
        let mut params = json!({ "provider": provider });
        if let Some(version) = version {
            params["version"] = json!(version);
        }
        if force {
            params["force"] = json!(true);
        }
        self.cli_call(format!("update/{provider}"), "clis.update", params, None, cx);
    }

    /// The menu says the new setting at once, and goes back if it's refused.
    pub(crate) fn set_cli_policy(&mut self, provider: &str, policy: &str, cx: &mut Context<Self>) {
        let before = self.updates_panel.clis.iter().find(|c| c.provider == provider).map(|c| c.policy.clone());
        if let Some(cli) = self.updates_panel.clis.iter_mut().find(|c| c.provider == provider) {
            cli.policy = policy.to_owned();
        }
        let (p, guess) = (provider.to_owned(), policy.to_owned());
        let undo: Box<dyn FnOnce(&mut Threadbear)> = Box::new(move |t| {
            if let (Some(before), Some(cli)) = (before, t.updates_panel.clis.iter_mut().find(|c| c.provider == p)) {
                if cli.policy == guess {
                    cli.policy = before;
                }
            }
        });
        self.cli_call(format!("policy/{provider}"), "clis.setPolicy", json!({ "provider": provider, "policy": policy }), Some(undo), cx);
    }

    /// Bring Them In, or Let's Discuss: a thread in the checkout the runner
    /// follows about Mike's newest release, that merges it, or talks it
    /// through first and changes nothing until he says
    /// (`bringInUpstream`). On the new-thread box's login or pool and model,
    /// with full access; it spins until the runner has made it, then opens.
    pub(crate) fn bring_in_upstream(&mut self, discuss: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.updates_panel.bringing_in || self.new_thread.creating {
            return;
        }
        let Some(upstream) = self.updates_panel.runner.as_ref().and_then(|r| r.upstream.clone()) else { return };
        let Some(release) = upstream.release.clone() else { return };
        let prompt = if discuss { rules::discuss_prompt(&release) } else { rules::bring_in_prompt(&release) };
        // This Mac's logins: the thread starts on this runner, whatever the
        // new-thread box has picked for another.
        let picks = if self.new_thread.picks.runner.is_some() { NewThreadPicks::default() } else { self.new_thread.picks.clone() };
        let home = std::env::var("HOME").unwrap_or_default();
        let draft = Draft { logins: &self.logins, picks: &picks, mem: &self.new_thread.prefs.remembered, home: &home, now_ms: now_ms() };
        let mut params = draft.create_params(&prompt, vec![], false);
        if let Some(checkout) = upstream.checkout.as_deref().filter(|c| !c.is_empty()) {
            params["cwd"] = json!(checkout);
        }
        params["permissionMode"] = json!("full");
        params["title"] = json!(rules::bring_in_title(&release, discuss));
        self.updates_panel.bringing_in = true;
        cx.notify();
        let client = self.client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = call(client, "threads.create", params).await;
            this.update_in(cx, |t, window, cx| {
                t.updates_panel.bringing_in = false;
                match res.map(|v| serde_json::from_value::<ThreadSummary>(v["thread"].clone())) {
                    Ok(Ok(thread)) if !thread.id.is_empty() => {
                        let id = thread.id.clone();
                        t.threads.insert(id.clone(), thread);
                        t.open(id, window, cx);
                    }
                    Ok(_) => t.show_error("The runner didn't say what thread it made.".to_owned(), cx),
                    Err(e) => t.show_error(e.message, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Copy: the command, to run in Terminal. The button says Copied for a
    /// moment, as nothing else shows the click took.
    fn copy_install_command(&mut self, command: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(command));
        self.updates_panel.copies += 1;
        let n = self.updates_panel.copies;
        self.updates_panel.copied = Some(n);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |t, cx| {
                if t.updates_panel.copied == Some(n) {
                    t.updates_panel.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // What it shows.

    /// The command for a changed launch.mjs, from the checkout the runner
    /// follows when it says, else where the README's fork puts one.
    fn install_command(&self) -> String {
        let checkout = self.updates_panel.runner.as_ref().and_then(|r| r.upstream.as_ref()).and_then(|u| u.checkout.clone());
        rules::install_command(checkout.as_deref(), &std::env::var("HOME").unwrap_or_default())
    }

    /// A thread's title for a link to it, or the start of its id when it
    /// isn't listed here.
    fn thread_title(&self, id: &str) -> String {
        match self.threads.get(id) {
            Some(t) if !t.title.is_empty() => t.title.clone(),
            _ => id.chars().take(8).collect(),
        }
    }

    /// `UpdatesPanel`: App, Runner, From Mike when there's a release to say
    /// anything about, and the CLIs.
    pub(crate) fn render_updates_panel(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fg = style::label_text(cx);
        let line = |text: String| text_line(text, 12., fg).into_any_element();
        let app = self.render_app_update(cx);
        let mut sections = vec![section("App", app, cx)];
        sections.push(section("Runner", self.render_runner_updates(cx), cx));
        let upstream = self.updates_panel.runner.as_ref().and_then(|r| r.upstream.clone());
        if let Some(text) = rules::upstream_line(upstream.as_ref()) {
            let mut content = vec![line(text)];
            if let Some(release) = upstream.as_ref().filter(|u| u.available()).and_then(|u| u.release.clone()) {
                content.extend(self.render_release(&release, cx));
            }
            sections.push(section("From Mike", content, cx));
        }
        // The CLIs' versions and whether they update by themselves. In Usage
        // until 28 Oct; they're updates, not room.
        sections.push(section("CLIs", vec![self.render_clis(cx)], cx));
        panel_column("updates-panel", 14., sections)
    }

    /// The Updates icon with its right-click popover (`onRightClick` and
    /// `AppUpdatePopover`): the app's update in a line, with Restart to Update
    /// once a new version is ready, or the log once a build failed. It
    /// follows the update as it goes, so one opened mid-build gets its
    /// button once the build is staged.
    pub(crate) fn with_update_popover(&mut self, icon: AnyElement, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let popover = self.updates_panel.popover.then(|| self.render_update_popover(window, cx));
        div()
            .id("rail-updates-wrap")
            .relative()
            .flex_none()
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.toggle_update_popover(window, cx);
            }))
            .child(icon)
            .children(popover)
            .into_any_element()
    }

    pub(crate) fn toggle_update_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = &mut self.updates_panel;
        panel.popover = !panel.popover;
        if panel.popover {
            let focus = panel.popover_focus.get_or_insert_with(|| cx.focus_handle()).clone();
            window.focus(&focus, cx);
            crate::web_panels::keyboard_to_app(window);
        }
        cx.notify();
    }

    fn render_update_popover(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let side = self.app_side();
        let staged = self.updater.staged.as_ref().map(|s| s.short.clone()).filter(|_| side.staged.is_some());
        let secondary = style::secondary_text(cx);
        let line = h_flex()
            .gap(px(7.))
            .items_center()
            .when(side.building && side.staged.is_none(), |s| s.child(div().size(px(11.)).child(gpui_kit::component::spinner::Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(secondary).with_size(px(11.)))))
            .child(div().whitespace_nowrap().text_size(px(12.)).text_color(style::label_text(cx)).child(rules::popover_line(&side)));
        let button = match &staged {
            // Return takes it, as the popover is there for this.
            Some(short) => Some(push_button("updates-popover-restart", "Restart to Update", format!("Quit and reopen on {short}. Running threads carry on in the runner."), false, false, cx, |t, _, cx| {
                t.updates_panel.popover = false;
                t.restart_to_update(cx);
            })),
            // Not while a newer main builds: the line is about that.
            None if side.failed && !side.building => Some(push_button("updates-popover-log", "Show the Log", "Open the update build's log, to see why it failed", false, false, cx, |t, _, cx| {
                t.updates_panel.popover = false;
                cx.notify();
                crate::open_gate::open_own(&crate::app_update::log_path(), cx);
            })),
            None => None,
        };
        let focus = self.updates_panel.popover_focus.clone();
        let (bg, border) = (cx.theme().popover, cx.theme().border);
        let body = v_flex()
            .id("updates-popover")
            .occlude()
            .p(px(14.))
            .gap(px(10.))
            .rounded(px(10.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .shadow_lg()
            .when_some(focus, |s, f| s.track_focus(&f))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                "enter" if this.updater.staged.is_some() => {
                    this.updates_panel.popover = false;
                    this.restart_to_update(cx);
                }
                "escape" => {
                    this.updates_panel.popover = false;
                    cx.notify();
                }
                _ => {}
            }))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.updates_panel.popover = false;
                cx.notify();
            }))
            .child(line)
            .children(button.map(|b| h_flex().child(b)));
        // Beside the icon, its foot level with the icon's, kept in the window.
        div()
            .absolute()
            .left(px(crate::rails::BUTTON + 8.))
            .bottom_0()
            .child(deferred(anchored().anchor(Anchor::BottomLeft).snap_to_window_with_margin(px(8.)).child(body)).with_priority(2))
            .into_any_element()
    }

    /// The app's own update, as the Swift panel's App section: what's on its
    /// way, with Restart to Update once it's ready, or the log once it failed.
    fn render_app_update(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let side = self.app_side();
        let mut out = vec![text_line(rules::app_line(&side), 12., style::label_text(cx)).into_any_element()];
        if side.staged.is_some() {
            out.push(h_flex().child(push_button("updates-restart", "Restart to Update", "Quit and reopen on the new version. Running threads carry on in the runner.", false, false, cx, |t, _, cx| t.restart_to_update(cx))).into_any_element());
        } else if side.failed {
            out.push(h_flex().child(push_button("updates-log", "Show the Log", "Open the update build's log, to see why it failed", false, false, cx, |_, _, cx| crate::open_gate::open_own(&crate::app_update::log_path(), cx))).into_any_element());
        }
        out
    }

    /// The runner's lines: not following main, its build, the threads still
    /// on the previous version (which open), and the LaunchAgent's command.
    fn render_runner_updates(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let fg = style::label_text(cx);
        let line = |text: String| text_line(text, 12., fg).into_any_element();
        let Some(runner) = self.updates_panel.runner.clone() else { return rules::runner_lines(None).into_iter().map(line).collect() };
        let (build, old, launcher) = (rules::build_line(&runner), rules::old_runners_line(&runner), rules::launcher_line(&runner));
        let not_following = rules::follow_lines(&runner);
        let mut out = Vec::new();
        if build.is_none() && old.is_none() && launcher.is_none() && not_following.is_empty() {
            out.push(line("The runner is up to date.".to_owned()));
        }
        out.extend(not_following.into_iter().map(line));
        out.extend(build.map(line));
        if let Some(old) = old {
            out.push(line(old));
            for id in runner.old_runners.iter().flat_map(|o| o.threads.iter()) {
                let open = id.clone();
                out.push(text_button(SharedString::from(format!("updates-old-{id}")), self.thread_title(id), "Open this thread", 12., true, false, cx, move |t, window, cx| t.open(open.clone(), window, cx)));
            }
        }
        if let Some(launcher) = launcher {
            out.push(line(launcher));
            let command = self.install_command();
            let copy = command.clone();
            let copied = self.updates_panel.copied.is_some();
            let mono = cx.theme().mono_font_family.clone();
            out.push(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(text_line(command, 11., fg).font_family(mono).flex_1().min_w_0().line_clamp(2))
                    .child(push_button("updates-copy", if copied { "Copied" } else { "Copy" }, "Copy the command, to run in Terminal", false, false, cx, move |t, _, cx| t.copy_install_command(copy.clone(), cx)))
                    .into_any_element(),
            );
        }
        out
    }

    /// Mike's plain-words notes, or commit subjects from a release made
    /// before there were any, then Bring Them In and Let's Discuss.
    fn render_release(&mut self, release: &rules::Release, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (fg, secondary) = (style::label_text(cx), style::secondary_text(cx));
        let mut out = Vec::new();
        for (heading, lines) in rules::release_lines(release) {
            if let Some(heading) = heading {
                out.push(text_line(heading, 11., fg).font_weight(FontWeight::MEDIUM).pt(px(2.)).into_any_element());
            }
            out.extend(lines.into_iter().map(|l| text_line(l, 11., secondary).line_clamp(2).into_any_element()));
        }
        let busy = self.updates_panel.bringing_in || self.new_thread.creating;
        let buttons = vec![
            push_button("updates-bring-in", "Bring Them In", "Start a thread that merges Mike's release into your main. It asks before it lands.", false, false, cx, |t, window, cx| t.bring_in_upstream(false, window, cx)),
            push_button("updates-discuss", "Let's Discuss", "Start a thread that talks the release through against your own changes first, and changes nothing until you say", false, false, cx, |t, window, cx| t.bring_in_upstream(true, window, cx)),
        ];
        // Both stay put and take no clicks while the thread is on its way.
        let row = button_row(buttons, busy, cx);
        out.push(if busy { div().opacity(0.5).child(row).into_any_element() } else { row });
        out
    }

    /// `CliSection`: each CLI's row, then when the runner last looked and
    /// Check now.
    fn render_clis(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (secondary, tertiary) = (style::secondary_text(cx), style::tertiary_label(cx));
        if self.updates_panel.clis_supported == Some(false) || (self.updates_panel.clis_supported.is_none() && self.updates_panel.clis.is_empty()) {
            return text_line("The CLI versions need a newer runner.", 12., secondary).into_any_element();
        }
        let mut column = v_flex().gap(px(12.));
        for cli in self.updates_panel.clis.clone() {
            column = column.child(self.render_cli_row(&cli, cx));
        }
        let checked = rules::checked_line(&self.updates_panel.clis, &threadbear_core::sidebar::local_now());
        let busy = self.updates_panel.busy.contains("check");
        column
            .child(
                h_flex()
                    .items_center()
                    .children(checked.map(|c| text_line(c, 11., tertiary)))
                    .child(div().flex_1())
                    .child(text_button("updates-check", "Check now", "Look for new versions of the CLIs now", 11., false, busy, cx, |t, _, cx| t.check_clis(cx))),
            )
            .into_any_element()
    }

    /// `CliRow`: its version and Update (or Update anyway), what an update is
    /// doing, what a held one lacks, and whether updates install by
    /// themselves, with Go back for a kept version.
    fn render_cli_row(&mut self, cli: &CliStatus, cx: &mut Context<Self>) -> AnyElement {
        let (fg, secondary, tertiary) = (style::label_text(cx), style::secondary_text(cx), style::tertiary_label(cx));
        let provider = cli.provider.clone();
        let updating = self.updates_panel.busy.contains(&format!("update/{provider}"));
        let title = rules::update_title(cli);
        // Its first line's baseline on the button's, as the Swift row's
        // `.firstTextBaseline` puts it.
        let status = text_line(rules::status_line(cli), 12., if cli.status == "held" { system_orange(cx) } else { fg }).flex_1().min_w_0().when(title.is_some(), |s| s.mt(px(2.)));
        let mut top = h_flex().items_start().gap(px(6.)).child(status);
        if let Some(title) = title {
            let tip = if cli.can_update_now {
                "Ask it to update itself"
            } else if cli.status == "held" {
                "Install it even though the check held it back"
            } else {
                "Install the new version now"
            };
            let (p, force) = (provider.clone(), cli.status == "held");
            top = top.child(push_button(SharedString::from(format!("updates-update-{provider}")), title, tip, true, updating, cx, move |t, _, cx| t.update_cli(&p, None, force, cx)));
        }
        let mut row = v_flex().gap(px(6.)).child(top);
        if let (true, Some(text)) = (rules::is_updating(cli), rules::banner_text(cli)) {
            row = row.child(text_line(text, 11., secondary));
        }
        if let Some(missing) = cli.hold.as_ref().map(|h| &h.missing).filter(|m| !m.is_empty()) {
            row = row.child(text_line(format!("Missing: {}", missing.join(", ")), 11., tertiary).line_clamp(2));
        }
        if let Some(managed) = rules::managed_line(cli) {
            row = row.child(text_line(managed, 11., tertiary));
        }
        let mut bottom = h_flex().items_center().gap(px(8.));
        // A menu, not a segmented control: two rows of three buttons were
        // most of the popover's noise, and the choice is set once and left.
        if !cli.can_update_now {
            bottom = bottom.child(self.render_policy_menu(cli, cx));
        }
        bottom = bottom.child(div().flex_1());
        if let (Some(back), Some(previous)) = (rules::go_back_title(cli), cli.previous_version.clone()) {
            let p = provider.clone();
            bottom = bottom.child(text_button(SharedString::from(format!("updates-back-{provider}")), back, "Go back to the version you had before the last update", 11., false, updating, cx, move |t, _, cx| t.update_cli(&p, Some(previous.clone()), false, cx)));
        }
        row.child(bottom).into_any_element()
    }

    /// "Updates: Automatic", and the three choices with a tick on the one set.
    /// It already says the new one while the runner takes it, and takes no
    /// other choice meanwhile.
    fn render_policy_menu(&mut self, cli: &CliStatus, cx: &mut Context<Self>) -> AnyElement {
        let tip = "Whether new versions install by themselves";
        let provider = cli.provider.clone();
        let busy = self.updates_panel.busy.contains(&format!("policy/{provider}"));
        let current = cli.policy.clone();
        let app = cx.entity().downgrade();
        let words = format!("Updates: {}", rules::policy_title(&cli.policy));
        let holder = div().id(SharedString::from(format!("updates-policy-{}", cli.provider))).flex_none().test_support().aria_label(tip).tip(tip);
        if busy {
            // Taking no other choice until the runner has this one.
            return holder.opacity(0.5).child(MenuLabel::new(SharedString::from(format!("updates-policy-{provider}-label")), words, 11., style::label_text(cx))).into_any_element();
        }
        let menu = MenuLabel::new(SharedString::from(format!("updates-policy-{provider}-label")), words, 11., style::label_text(cx))
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                let mut menu = menu;
                for policy in rules::POLICIES {
                    let (app, p) = (app.clone(), provider.clone());
                    let checked = current == policy;
                    let mut item = PopupMenuItem::new(rules::policy_title(policy)).checked(checked).on_click(move |_, _, cx| {
                        app.update(cx, |t, cx| t.set_cli_policy(&p, policy, cx)).ok();
                    });
                    // A real tick beside the icon: the tick used to be the
                    // icon, so it was hidden along with all of them.
                    if !checked {
                        item = item.icon(icons::symbol(rules::policy_symbol(policy)));
                    }
                    menu = menu.item(item);
                }
                menu
            });
        holder.child(menu).into_any_element()
    }

    // For scripts and the specs.

    /// What the panel says, a line each, in the Swift app's dump shape.
    pub(crate) fn updates_dump(&self, cx: &App) -> Value {
        let mut lines = vec!["# App".to_owned(), rules::app_line(&self.app_side()), "# Runner".to_owned()];
        match &self.updates_panel.runner {
            Some(runner) => {
                let (build, old, launcher) = (rules::build_line(runner), rules::old_runners_line(runner), rules::launcher_line(runner));
                let not_following = rules::follow_lines(runner);
                if build.is_none() && old.is_none() && launcher.is_none() && not_following.is_empty() {
                    lines.push("The runner is up to date.".to_owned());
                }
                lines.extend(not_following);
                lines.extend(build);
                if let Some(old) = old {
                    lines.push(old);
                    lines.extend(runner.old_runners.iter().flat_map(|o| o.threads.iter()).map(|id| format!("  > {}", self.thread_title(id))));
                }
                if let Some(launcher) = launcher {
                    lines.push(launcher);
                    lines.push(format!("  $ {} [Copy]", self.install_command()));
                }
            }
            None => lines.extend(rules::runner_lines(None)),
        }
        let upstream = self.updates_panel.runner.as_ref().and_then(|r| r.upstream.as_ref());
        if let Some(text) = rules::upstream_line(upstream) {
            lines.push("# From Mike".to_owned());
            lines.push(text);
            if let Some(release) = upstream.filter(|u| u.available()).and_then(|u| u.release.as_ref()) {
                for (heading, group) in rules::release_lines(release) {
                    lines.extend(heading.map(|h| format!("## {h}")));
                    lines.extend(group.into_iter().map(|l| format!("  {l}")));
                }
                lines.push("[Bring Them In] [Let's Discuss]".to_owned());
            }
        }
        lines.push("# CLIs".to_owned());
        let clis = &self.updates_panel.clis;
        if self.updates_panel.clis_supported == Some(false) || (self.updates_panel.clis_supported.is_none() && clis.is_empty()) {
            lines.push("The CLI versions need a newer runner.".to_owned());
        } else {
            for cli in clis {
                lines.push(rules::dump_line(cli));
                if let Some(missing) = cli.hold.as_ref().map(|h| &h.missing).filter(|m| !m.is_empty()) {
                    lines.push(format!("  Missing: {}", missing.join(", ")));
                }
                lines.extend(rules::managed_line(cli).map(|m| format!("  {m}")));
            }
            let checked = rules::checked_line(clis, &threadbear_core::sidebar::local_now());
            lines.push(checked.map_or_else(|| "[Check now]".to_owned(), |c| format!("{c} [Check now]")));
        }
        json!({ "shown": self.activity_panel_showing(shell::UPDATES, cx), "lines": lines })
    }
}

/// Small headings, as the Usage panel groups its logins (`section`).
fn section(title: &'static str, content: Vec<AnyElement>, cx: &App) -> AnyElement {
    v_flex().gap(px(7.)).child(small_heading(title, cx)).children(content).into_any_element()
}

/// The automation's `updates` (the Swift app's): `show` opens or closes the
/// panel; with nothing, whether the icon has its dot, its tooltip, and what
/// the panel says about the app and the runner. `preview` makes up the app's
/// own update first (`staged`, `building`, `failed`, `waiting` or `none`), as
/// the Swift app's; this app has no right-click popover (`rightClick`).
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    // What a restart onto an update leaves for the next launch, put back as
    // that launch does (app_update.rs, `restore_after_update`): a scripted
    // app never restarts itself.
    if let Some(saved) = cmd.get("restore").filter(|v| v.is_object()) {
        app.update(cx, |t, cx| {
            t.settings.saved.update_restore = Some(saved.clone());
            t.restore_after_update(cx);
        });
        return Ok(json!("ok"));
    }
    if let Some(state) = cmd["preview"].as_str() {
        if !["staged", "building", "failed", "waiting", "none"].contains(&state) {
            return Err("preview is staged, building, failed, waiting or none".into());
        }
        app.update(cx, |t, cx| t.preview_update(state, cx));
    }
    // {"rightClick": true}: the icon's right-click, which opens or closes
    // the popover; {"popover": false} closes it.
    if cmd["rightClick"].as_bool() == Some(true) {
        let shown = app.update(cx, |t, cx| {
            t.toggle_update_popover(window, cx);
            t.updates_panel.popover
        });
        return Ok(json!(format!("ok popover {}", if shown { "shown" } else { "closed" })));
    }
    if let Some(popover) = cmd["popover"].as_bool() {
        app.update(cx, |t, cx| {
            t.updates_panel.popover = popover;
            cx.notify();
        });
        return Ok(json!(format!("ok popover {}", if popover { "shown" } else { "closed" })));
    }
    if let Some(show) = cmd["show"].as_bool() {
        let shown = app.update(cx, |t, cx| {
            if t.activity_panel_showing(shell::UPDATES, cx) != show {
                t.toggle_under_threads(shell::UPDATES, cx);
            }
            t.activity_panel_showing(shell::UPDATES, cx)
        });
        return match (show, shown) {
            (false, _) => Ok(json!("ok closed")),
            (true, true) => Ok(json!("ok shown")),
            (true, false) => Err("the panel didn't open".into()),
        };
    }
    let t = app.read(cx);
    let side = t.app_side();
    let runner = t.updates_panel.runner.as_ref();
    let popover = format!("popover: {}, {}", if t.updates_panel.popover { "shown" } else { "closed" }, rules::popover_line(&side));
    let mut lines = vec![format!("shown: {}", rules::shown(&side, runner)), format!("tip: {}", t.updates_tooltip().replace('\n', " / ")), popover, rules::app_line(&side)];
    lines.extend(rules::runner_lines(runner));
    Ok(json!(format!("ok {}", serde_json::to_string(&lines).unwrap_or_default())))
}
