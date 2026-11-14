// Finish setting up: what's left after `./threadbear setup` for a new
// install, in a panel under the threads (like Usage) with an icon on the
// activity bar while anything's left, a port of the Swift app's SetupPanel,
// SetupRow and SetupActivityButton (Views/SetupChecklist.swift). Each item
// ticks itself off from what the app already knows (threadbear_core::tour's
// `SetupChecklist`), so there's nothing to mark done by hand. Hide this list
// puts it away for good, and Help > Finish Setting Up brings it back.
use crate::activity_panels::{panel_column, text_line};
use crate::app::Threadbear;
use crate::push_button::PushButton;
use crate::shell;
use crate::style::{self, px};
use gpui_kit::component::{h_flex, v_flex, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use threadbear_core::devices::{self as devices_rules, DeviceList};
use threadbear_core::tour::{SetupChecklist, SetupItem};


/// What the checklist needs to know that the app doesn't keep anyway: the
/// paired devices and whether the backend is on this Mac only, from
/// `devices.list` and `runner.status` (Swift's `devices*` and `localOnly`).
#[derive(Default)]
pub(crate) struct SetupState {
    devices: DeviceList,
    /// `devices.list` has answered since the runner connected.
    loaded: bool,
    /// A runner from before devices (-32601).
    unsupported: bool,
    local_only: bool,
    /// Open signed in, while its token is on its way.
    opening: bool,
    pub(crate) view: Option<Entity<crate::activity_panels::PanelView>>,
}

impl Threadbear {
    pub(crate) fn setup_checklist(&self) -> SetupChecklist {
        let s = &self.setup;
        // Nothing to pair with: no mirror (the list's reason), or a runner
        // that doesn't know devices.
        let can_pair = s.loaded && s.devices.reason.is_none() && !s.unsupported;
        SetupChecklist::build(self.threads.values().map(|t| &**t), &self.logins.accounts, &s.devices.devices, can_pair, s.local_only)
    }

    /// The activity bar's icon shows while something's left and Mike hasn't
    /// hidden the list.
    pub(crate) fn setup_shows_icon(&self) -> bool {
        self.settings.saved.setup_hidden != Some(true) && self.setup_checklist().left() > 0
    }

    /// Help > Finish Setting Up, and the tour once the first thread's sent:
    /// back from hidden, and open.
    pub(crate) fn show_setup(&mut self, cx: &mut Context<Self>) {
        if self.settings.saved.setup_hidden.take().is_some() {
            self.save_settings_soon(cx);
        }
        if !self.activity_panel_showing(shell::SETUP, cx) {
            self.toggle_under_threads(shell::SETUP, cx);
        }
        cx.notify();
    }

    /// The devices, each time the runner connects (the list's phone and
    /// website items need them) and when the panel comes on show. Not while
    /// the list is hidden: nothing shows it.
    pub(crate) fn load_setup_devices(&mut self, cx: &mut Context<Self>) {
        if self.settings.saved.setup_hidden == Some(true) {
            return;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let list = crate::call(client.clone(), "devices.list", json!({})).await;
            let status = crate::call(client, "runner.status", json!({})).await;
            this.update(cx, |this, cx| {
                match list {
                    Ok(v) => {
                        this.setup.devices = DeviceList::decode(&v);
                        this.setup.loaded = true;
                        this.setup.unsupported = false;
                    }
                    Err(e) if e.code == -32601 => this.setup.unsupported = true,
                    Err(_) => {}
                }
                if let Ok(status) = status {
                    this.setup.local_only = devices_rules::local_only(&status);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Open signed in: a browser token, and the website opened with it, as
    /// Settings › Phone & browser's button does.
    fn setup_open_website(&mut self, cx: &mut Context<Self>) {
        if self.setup.opening {
            return;
        }
        self.setup.opening = true;
        cx.notify();
        let name = crate::open_gate::default_browser().map_or_else(|| "Browser".to_owned(), |b| devices_rules::browser_name(&b));
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = crate::call(client, "devices.pair", json!({ "kind": "browser", "name": name })).await;
            this.update(cx, |this, cx| {
                this.setup.opening = false;
                match res {
                    Ok(v) => {
                        if let Some(link) = v["signInLink"].as_str() {
                            crate::open_gate::open_web(link, cx);
                        }
                        this.load_setup_devices(cx);
                    }
                    Err(e) => this.error_message = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The panel: one row per item, ticked or with what to press.
    pub(crate) fn render_setup_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let list = self.setup_checklist();
        let fg = style::label_text(cx);
        let secondary = style::secondary_text(cx);
        let mut children: Vec<AnyElement> = vec![];
        if list.left() == 0 {
            children.push(
                h_flex()
                    .items_center()
                    .gap(px(10.))
                    .child(crate::tour::waving_bear(36., secondary))
                    .child(div().text_size(px(style::size::CARD_TITLE)).font_semibold().text_color(fg).child("All set."))
                    .into_any_element(),
            );
        }
        for (item, done) in list.items.clone() {
            children.push(self.render_setup_row(item, done, list.no_login, cx));
        }
        if list.left() > 0 {
            children.push(
                div()
                    .id("setup-hide").role(Role::Button)
                    .cursor_pointer()
                    .text_size(px(style::size::DETAIL))
                    .text_color(secondary)
                    .child("Hide this list")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.saved.setup_hidden = Some(true);
                        this.save_settings_soon(cx);
                        if this.activity_panel_showing(shell::SETUP, cx) {
                            this.toggle_under_threads(shell::SETUP, cx);
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let column = panel_column("setup-panel", 14., children);
        div().size_full().relative().child(column).into_any_element()
    }

    fn render_setup_row(&mut self, item: SetupItem, done: bool, no_login: bool, cx: &mut Context<Self>) -> AnyElement {
        let fg = style::label_text(cx);
        let secondary = style::secondary_text(cx);
        let mut text = v_flex().gap(px(4.)).min_w_0().flex_1().child(div().text_size(px(style::size::LABEL)).font_medium().text_color(if done { secondary } else { fg }).child(item.title()));
        if !done {
            text = text.child(text_line(item.detail(no_login, self.setup.local_only), style::size::DETAIL, secondary));
            let action: Option<AnyElement> = match item {
                SetupItem::FirstThread if !no_login => Some(PushButton::new("setup-new-thread", "New thread", "A new thread, in your threads folder unless you pick another").render(cx, {
                    let app = cx.entity().downgrade();
                    move |_, _, cx| {
                        app.update(cx, |this, cx| this.new_thread(cx)).ok();
                    }
                })),
                SetupItem::FirstThread => None,
                SetupItem::Phone => Some(PushButton::new("setup-phone", "Open Phone & browser", "Settings › Phone & browser, where Show the code… gives the iPhone app a code to scan").render(cx, {
                    let app = cx.entity().downgrade();
                    move |_, _, cx| {
                        app.update(cx, |this, cx| this.open_settings(Some(crate::settings_window::Tab::Devices), cx)).ok();
                    }
                })),
                SetupItem::Website => {
                    let browser = crate::open_gate::default_browser().map(|b| devices_rules::browser_name(&b));
                    let tip = format!("Opens your Threadbear website in {}, signed in for 90 days", browser.as_deref().unwrap_or("your browser"));
                    Some(PushButton::new("setup-website", "Open signed in", tip).busy(self.setup.opening).render(cx, {
                        let app = cx.entity().downgrade();
                        move |_, _, cx| {
                            app.update(cx, |this, cx| this.setup_open_website(cx)).ok();
                        }
                    }))
                }
            };
            if let Some(action) = action {
                // As wide as its words, as Swift's small button is.
                text = text.child(h_flex().pt(px(2.)).child(action));
            }
        }
        let tick = crate::icons::symbol(if done { "checkmark.circle.fill" } else { "circle" }).with_size(px(style::size::LABEL)).text_color(secondary);
        h_flex().items_start().gap(px(10.)).child(div().pt(px(1.)).child(tick)).child(text).into_any_element()
    }

    /// The automation's `setup`, as the Swift command: {"show": bool} opens or
    /// closes the panel, {"hidden": bool} as Hide this list and Help's item
    /// set it; says each item, [x] when done.
    #[cfg(feature = "automation")]
    pub(crate) fn setup_command(&mut self, cmd: &Value, cx: &mut Context<Self>) -> String {
        if let Some(hidden) = cmd["hidden"].as_bool() {
            self.settings.saved.setup_hidden = hidden.then_some(true);
            self.save_settings_soon(cx);
        }
        if let Some(show) = cmd["show"].as_bool() {
            if show != self.activity_panel_showing(shell::SETUP, cx) {
                self.toggle_under_threads(shell::SETUP, cx);
            }
        }
        cx.notify();
        let list = self.setup_checklist();
        let items: Vec<String> = list.items.iter().map(|(item, done)| format!("[{}] {}", if *done { "x" } else { " " }, item.name())).collect();
        format!(
            "ok {} left, icon {}, panel {}: {}",
            list.left(),
            if self.setup_shows_icon() { "shown" } else { "hidden" },
            if self.activity_panel_showing(shell::SETUP, cx) { "open" } else { "closed" },
            items.join(", ")
        )
    }

    /// Finish setting up in `dump`.
    pub(crate) fn setup_dump(&self, cx: &App) -> Value {
        let list = self.setup_checklist();
        json!({
            "icon": self.setup_shows_icon(),
            "shown": self.activity_panel_showing(shell::SETUP, cx),
            "left": list.left(),
            "noLogin": list.no_login,
            "items": list.items.iter().map(|(item, done)| json!({ "item": item.name(), "title": item.title(), "done": done })).collect::<Vec<_>>(),
        })
    }
}
