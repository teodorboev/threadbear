// The Connectors panel, a port of the Swift app's ConnectorsPanel.swift and
// ConnectorsModel.swift: each of the Connectors gateway's services with its
// state, and Connect for one that's signed out, which opens its sign-in page
// in the browser. The header's arrow opens the gateway's admin page and a
// signed-in row's opens that service's page, where it can be signed out (30
// Oct: Mike wanted to edit connectors from here, and the gateway already has
// the pages for it). The runner asks the gateway; the app only asks the
// runner, which also looks every few minutes by itself. The words are
// threadbear_core::connectors.
use crate::activity_panels::{about_button, dot, icon_button, panel_column, push_button, small_heading, system_orange, text_line};
use crate::app::Threadbear;
use crate::highlights::tipped;
use crate::{call, icons, shell, style};
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use threadbear_core::connectors::{self as rules, ConnectorService, ConnectorsStatus};

/// The Connectors panel's state in the app (`Threadbear::connectors_panel`).
#[derive(Default)]
pub(crate) struct ConnectorsPanelState {
    /// The view the dock shows it in, made the first time it's drawn.
    pub(crate) view: Option<Entity<crate::activity_panels::PanelView>>,
    /// What the runner last said, once it has (`connectors.status`, then
    /// `connectors.updated`). None from a runner before Connectors, which has
    /// no icon and no panel.
    pub(crate) status: Option<ConnectorsStatus>,
    /// The refresh button's look at the gateway is on its way.
    pub(crate) refreshing: bool,
}

impl Threadbear {
    /// After hello (`loadConnectors`); `connectors.updated` keeps it current.
    pub(crate) fn load_connectors(&mut self, cx: &mut Context<Self>) {
        self.fetch_connectors(false, false, cx);
    }

    /// A fresh look at the gateway: the panel coming on show, or its refresh
    /// button (`shown`), whose icon spins and which says if the gateway's
    /// state couldn't be read. The runner skips the ask if it made one a
    /// moment ago.
    pub(crate) fn refresh_connectors(&mut self, shown: bool, cx: &mut Context<Self>) {
        if shown {
            if self.connectors_panel.refreshing {
                return;
            }
            self.connectors_panel.refreshing = true;
            cx.notify();
        }
        self.fetch_connectors(true, shown, cx);
    }

    fn fetch_connectors(&mut self, refresh: bool, shown: bool, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "connectors.status", json!({ "refresh": refresh })).await;
            this.update(cx, |t, cx| {
                if shown {
                    t.connectors_panel.refreshing = false;
                }
                match res {
                    Ok(v) => t.connectors_panel.status = Some(ConnectorsStatus::decode(&v["connectors"])),
                    // A runner from before Connectors: no icon, no panel.
                    Err(e) if e.code == -32601 => {
                        t.connectors_panel.status = None;
                        if t.activity_panel_showing(shell::CONNECTORS, cx) {
                            t.toggle_under_threads(shell::CONNECTORS, cx);
                        }
                    }
                    Err(e) if shown => t.show_error(e.message, cx),
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `connectors.updated`.
    pub(crate) fn connectors_updated(&mut self, params: &Value) {
        self.connectors_panel.status = Some(ConnectorsStatus::decode(&params["connectors"]));
    }

    /// Whether the activity bar has the icon: once the runner has said it
    /// watches a gateway. It says "off" when none is set up (most people, as
    /// the gateway is optional), and a panel saying only that is no use.
    pub(crate) fn connectors_on(&self) -> bool {
        self.connectors_panel.status.as_ref().is_some_and(|s| s.state != "off")
    }

    /// A service's page on the gateway in the browser: its sign-in when it's
    /// signed out, else where it can be signed out or retried.
    fn open_connect(&mut self, service: &ConnectorService, cx: &mut Context<Self>) {
        if let Some(url) = rules::service_url(self.connectors_panel.status.as_ref(), service) {
            crate::open_gate::open_web(&url, cx);
        }
    }

    /// The gateway's admin page, with every service on it.
    fn open_connectors_admin(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = rules::manage_url(self.connectors_panel.status.as_ref()) {
            crate::open_gate::open_web(&url, cx);
        }
    }

    /// `ConnectorsPanel`: where they live, the gateway's page, refresh and
    /// the ⓘ, then a row per service (or why there are none).
    pub(crate) fn render_connectors_panel(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let status = self.connectors_panel.status.clone();
        let host = rules::host(status.as_ref());
        let mut header = h_flex().gap(px(6.)).items_center().child(small_heading(host.clone().unwrap_or_else(|| "Gateway".to_owned()), cx)).child(div().flex_1());
        if rules::manage_url(status.as_ref()).is_some() {
            let tip = format!("Manage connectors on {}", host.as_deref().unwrap_or("the gateway"));
            header = header.child(icon_button("connectors-manage", "arrow.up.forward.square", tip, false, cx, |t, _, cx| t.open_connectors_admin(cx)));
        }
        header = header.child(icon_button("connectors-refresh", "arrow.clockwise", "Check the gateway again now", self.connectors_panel.refreshing, cx, |t, _, cx| t.refresh_connectors(true, cx)));
        let footnote = status.as_ref().and_then(|s| s.checked_ms).and_then(|at| {
            use threadbear_core::chrono::TimeZone as _;
            threadbear_core::chrono::Local.timestamp_millis_opt(at).single().map(|d| format!("Last checked {}.", threadbear_core::schedules::time_of_day(&d)))
        });
        header = header.child(about_button("connectors-about", rules::about(status.as_ref()).into_iter().collect(), footnote, cx));
        let mut children = vec![header.into_any_element()];
        if status.as_ref().is_none_or(|s| s.services.is_empty()) {
            if let Some(line) = rules::empty_line(status.as_ref()) {
                let color = if status.as_ref().is_some_and(|s| s.state == "unreachable") { style::label_text(cx) } else { style::secondary_text(cx) };
                children.push(text_line(line, 12., color).into_any_element());
            }
        }
        for service in status.iter().flat_map(|s| s.services.iter()) {
            children.push(self.render_connector_row(service, cx));
        }
        panel_column("connectors-panel", 12., children)
    }

    /// `ConnectorRow`: a mark for its state (filled when signed in, the
    /// accent only where Mike has something to do), its name and state, its
    /// message, and Connect, or a quiet arrow to its page.
    fn render_connector_row(&mut self, service: &ConnectorService, cx: &mut Context<Self>) -> AnyElement {
        let (fg, secondary, tertiary) = (style::label_text(cx), style::secondary_text(cx), style::tertiary_label(cx));
        let signed_out = service.state == "signed_out";
        let mark = match service.state.as_str() {
            "signed_out" => style::accent(cx),
            "connected" => secondary.opacity(0.7),
            "error" => system_orange(cx),
            _ => secondary.opacity(0.3),
        };
        let mut words = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(text_line(service.title.clone(), 12., fg).font_weight(if signed_out { FontWeight::MEDIUM } else { FontWeight::NORMAL }).truncate())
            .child(text_line(rules::state_line(service), 11., secondary));
        if let Some(message) = service.message.clone().filter(|_| service.state != "connected") {
            words = words.child(text_line(message, 11., tertiary).line_clamp(3));
        }
        let group = SharedString::from(format!("connector-{}", service.id));
        let button = rules::button_title(service);
        let mut row = h_flex()
            .id(SharedString::from(format!("connector-row-{}", service.id)))
            .group(group.clone())
            .items_start()
            .gap(px(8.))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_start()
                    .gap(px(8.))
                    // Beside Connect, its name's baseline on the button's, as
                    // the Swift row's `.firstTextBaseline` puts it.
                    .when(button.is_some(), |s| s.mt(px(2.)))
                    // The dot on the name's line, a point under its middle, as
                    // the Swift row puts it on the text's baseline.
                    .child(div().flex_none().h(px(15.)).flex().items_center().child(dot(mark).mt(px(1.))))
                    .child(words),
            );
        if let Some(title) = button {
            let s = service.clone();
            row = row.child(push_button(SharedString::from(format!("connector-connect-{}", service.id)), title, format!("Open {}'s sign-in page in your browser", service.title), true, false, cx, move |t, _, cx| t.open_connect(&s, cx)));
        } else if rules::service_url(self.connectors_panel.status.as_ref(), service).is_some() {
            // A signed-in service has nothing to press, but Mike may want to
            // sign it out or switch accounts: its page on the gateway has
            // Disconnect. Quiet until the row's hovered.
            let s = service.clone();
            let tip = format!("Open {}'s page on {} to sign out or reconnect", service.title, rules::host(self.connectors_panel.status.as_ref()).as_deref().unwrap_or("the gateway"));
            let quiet = secondary.opacity(0.35);
            row = row.child(
                tipped(
                    style::group_hover_text(
                        div()
                            .id(SharedString::from(format!("connector-page-{}", service.id)))
                            .flex_none()
                            .h(px(15.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_color(quiet),
                        group,
                        secondary,
                    )
                    .hover(move |st| st.text_color(fg)),
                    tip,
                )
                .on_click(cx.listener(move |t, _: &ClickEvent, _, cx| t.open_connect(&s, cx)))
                // Lucide's arrow is drawn smaller in its box than SF's.
                .child(icons::symbol("arrow.up.forward").with_size(px(14.))),
            );
        }
        row.into_any_element()
    }

    // For scripts and the specs.

    /// What the panel says, a line each, in the Swift app's dump shape.
    pub(crate) fn connectors_dump(&self, cx: &App) -> Value {
        let status = self.connectors_panel.status.as_ref();
        let mut lines = vec![rules::host(status).unwrap_or_else(|| "Gateway".to_owned())];
        if status.is_none_or(|s| s.services.is_empty()) {
            lines.extend(rules::empty_line(status));
        }
        for s in status.iter().flat_map(|s| s.services.iter()) {
            lines.push(format!("{}: {}{}", s.title, rules::state_line(s), rules::button_title(s).map_or_else(String::new, |b| format!(" [{b}]"))));
            if let Some(message) = s.message.as_ref().filter(|_| s.state != "connected") {
                lines.push(format!("  {message}"));
            }
        }
        json!({ "shown": self.activity_panel_showing(shell::CONNECTORS, cx), "lines": lines })
    }
}

/// The automation's `connectors` (the Swift app's): `show` opens or closes
/// the panel, `click` presses a service's Connect (which opens its page in
/// the browser); with neither, the icon's dots and tooltip and each
/// service's line.
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    if let Some(show) = cmd["show"].as_bool() {
        let shown = app.update(cx, |t, cx| {
            if t.activity_panel_showing(shell::CONNECTORS, cx) != show {
                t.toggle_under_threads(shell::CONNECTORS, cx);
            }
            t.activity_panel_showing(shell::CONNECTORS, cx)
        });
        return match (show, shown) {
            (false, _) => Ok(json!("ok closed")),
            (true, true) => Ok(json!("ok shown")),
            (true, false) => Err("the panel didn't open".into()),
        };
    }
    let status = app.read(cx).connectors_panel.status.clone();
    if let Some(id) = cmd["click"].as_str() {
        let service = status.as_ref().and_then(|s| s.services.iter().find(|x| x.id == id)).cloned().ok_or_else(|| format!("no service {id}"))?;
        if rules::button_title(&service).is_none() {
            return Err(format!("{id} has no button"));
        }
        app.update(cx, |t, cx| t.open_connect(&service, cx));
        return Ok(json!(format!("ok opened {}", service.connect_url.unwrap_or_default())));
    }
    let Some(s) = status else { return Err("the runner has no connectors".into()) };
    let mut lines = vec![format!("state: {}", s.state), format!("dot: {}", rules::needs_you(Some(&s))), format!("quietDot: {}", rules::quiet(Some(&s))), format!("tip: {}", rules::tooltip(Some(&s)).replace('\n', " / "))];
    lines.extend(rules::empty_line(Some(&s)));
    lines.extend(s.services.iter().map(|x| format!("{}: {}{}", x.id, rules::state_line(x), rules::button_title(x).map_or_else(String::new, |b| format!(" [{b}]")))));
    Ok(json!(format!("ok {}", serde_json::to_string(&lines).unwrap_or_default())))
}
