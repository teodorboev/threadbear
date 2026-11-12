// Settings › Phone & browser (DevicesSettings.swift, DevicesModel.swift): pair
// the iPhone app with a QR code it scans, open the website signed in, and the
// list of what's paired, each with Revoke. The runner issues every token
// (docs/protocol.md, "Devices"), and a token is only ever inside the QR code on
// screen or the link handed to the browser: the link is drawn into the code and
// dropped, never kept, logged or put in the dump. The words and the code are
// threadbear_core::devices.
use super::{row_with, section, SettingsWindow};
use crate::app::Threadbear;
use crate::highlights::tipped;
use crate::push_button::PushButton;
use crate::{call, icons, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::HashSet;
use threadbear_core::devices::{self as rules, DeviceList, Qr};

/// Phone & browser's state while the window is open (AppModel's `devices*`
/// and the view's own `pairing` and `opening`).
#[derive(Default)]
pub(crate) struct DevicesDraft {
    list: DeviceList,
    /// The runner has answered `devices.list` since the window opened.
    loaded: bool,
    /// A runner from before devices (-32601).
    unsupported: bool,
    error: Option<String>,
    /// runner.status's sync: the backend is on this Mac only, and it sends
    /// the phone no notifications.
    local_only: bool,
    push_off: bool,
    /// Show the code… and Open signed in, while their token is on its way.
    pairing: bool,
    opening: bool,
    /// Revoked here and not yet answered: gone from the list at once, and
    /// kept out of a list that comes back in the meantime.
    revoking: HashSet<String>,
    /// The pairing code is up.
    pub(crate) showing_code: bool,
    /// And drawn, as the last frame the automation looked at had it, so the
    /// dump says what's on screen, not only what was asked for.
    pub(crate) code_drawn: bool,
    /// Asked for since the section last came on show, and whether the runner
    /// was there then, so it asks again when the runner comes back.
    asked: bool,
    was_connected: bool,
}

impl SettingsWindow {
    fn devices_connected(&self, cx: &App) -> bool {
        self.app.upgrade().is_some_and(|a| matches!(a.read(cx).status, threadbear_client::runner::Status::Connected { .. }))
    }

    /// The section came on show: the list afresh (`onAppear`), and again
    /// whenever the runner comes back (`onChange(of: connection)`).
    fn devices_on_show(&mut self, cx: &mut Context<Self>) {
        let connected = self.devices_connected(cx);
        if connected && (!self.devices.asked || !self.devices.was_connected) {
            self.devices.asked = true;
            self.load_devices(true, cx);
        }
        self.devices.was_connected = connected;
    }

    /// Off show: the next showing asks again.
    pub(super) fn devices_hidden(&mut self) {
        self.devices.asked = false;
    }

    /// `loadDevices`, with `runner.status` for whether a phone can pair.
    /// `fresh` is a new look, which clears what last went wrong; after a pair
    /// or a revoke the list comes back without taking away why one failed
    /// (the Swift app's reload cleared it as soon as it was said).
    fn load_devices(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let list = call(client.clone(), "devices.list", json!({})).await;
            let status = call(client, "runner.status", json!({})).await;
            this.update(cx, |this, cx| {
                match list {
                    Ok(v) => {
                        this.receive_devices(DeviceList::decode(&v));
                        if fresh {
                            this.devices.error = None;
                        }
                    }
                    Err(e) if e.code == -32601 => this.devices.unsupported = true,
                    Err(e) => this.devices.error = Some(e.message),
                }
                if let Ok(status) = status {
                    this.devices.local_only = rules::local_only(&status);
                    this.devices.push_off = rules::push_off(&status);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn receive_devices(&mut self, mut list: DeviceList) {
        list.devices.retain(|d| !self.devices.revoking.contains(&d.id));
        self.devices.list = list;
        self.devices.unsupported = false;
        self.devices.loaded = true;
    }

    /// Show the code…: a token for a phone, drawn as a QR code over the
    /// window. The button spins until the runner answers.
    fn pair_phone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.devices.pairing {
            return;
        }
        self.devices.pairing = true;
        cx.notify();
        let client = self.client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = call(client, "devices.pair", json!({ "kind": "phone", "name": "iPhone" })).await;
            this.update_in(cx, |this, window, cx| {
                this.devices.pairing = false;
                match res {
                    Ok(v) => {
                        this.devices.error = None;
                        this.load_devices(false, cx);
                        match v["pairingLink"].as_str() {
                            // The link goes into the code and no further.
                            Some(link) => this.show_code(rules::pairing_qr(link), window, cx),
                            None => this.devices.error = Some("The runner gave no pairing link back.".into()),
                        }
                    }
                    // Only that it failed: the answer would have held the token.
                    Err(e) => this.devices.error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Open signed in: a token for the browser web links open in, named
    /// after it, and the website opened there with it.
    fn open_website(&mut self, cx: &mut Context<Self>) {
        if self.devices.opening {
            return;
        }
        self.devices.opening = true;
        cx.notify();
        let name = crate::open_gate::default_browser().map_or_else(|| "Browser".to_owned(), |b| rules::browser_name(&b));
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "devices.pair", json!({ "kind": "browser", "name": name })).await;
            this.update(cx, |this, cx| {
                this.devices.opening = false;
                match res {
                    Ok(v) => {
                        this.devices.error = None;
                        this.load_devices(false, cx);
                        match v["signInLink"].as_str() {
                            Some(link) => {
                                crate::open_gate::open_web(link, cx);
                            }
                            None => this.devices.error = Some("The runner gave no sign-in link back.".into()),
                        }
                    }
                    Err(e) => this.devices.error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Revoke: off the list at once; if the runner says no, the list comes
    /// back as the runner has it, with why (`revoke`).
    fn revoke_device(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.devices.revoking.insert(id.clone()) {
            return;
        }
        let before = self.devices.list.devices.clone();
        self.devices.list.devices.retain(|d| d.id != id);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "devices.revoke", json!({ "deviceId": id })).await;
            this.update(cx, |this, cx| {
                this.devices.revoking.remove(&id);
                match res {
                    Ok(v) => {
                        this.receive_devices(DeviceList::decode(&v));
                        this.devices.error = None;
                    }
                    Err(e) => {
                        // Back where it was, with why; then the runner's
                        // list, which has the last word.
                        if let Some(at) = before.iter().position(|d| d.id == id).filter(|_| !this.devices.list.devices.iter().any(|d| d.id == id)) {
                            let at_most = at.min(this.devices.list.devices.len());
                            this.devices.list.devices.insert(at_most, before[at].clone());
                        }
                        this.devices.error = Some(e.message);
                        this.load_devices(false, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The pairing code over the window (`PairPhoneSheet`), until Done, Esc or
    /// Return. Only the code's squares are kept for it, never the link.
    fn show_code(&mut self, qr: Option<Qr>, window: &mut Window, cx: &mut Context<Self>) {
        self.devices.showing_code = true;
        let closed = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog: Dialog, _, cx| {
            let (bg, secondary) = (cx.theme().background, style::secondary_text(cx));
            let (on_ok, on_close) = (closed.clone(), closed.clone());
            dialog
                .w(px(360.))
                .p(px(24.))
                .bg(bg)
                .rounded(px(16.))
                .close_button(false)
                .overlay_closable(false)
                // Return is Done, as the sheet's default button.
                .on_ok(move |_, _, cx| {
                    code_closed(&on_ok, cx);
                    true
                })
                .on_close(move |_, _, cx| code_closed(&on_close, cx))
                .child(code_sheet(qr.clone(), secondary, closed.clone(), cx))
        });
    }

    pub(super) fn render_devices(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.devices_on_show(cx);
        let connected = self.devices_connected(cx);
        let mut sections = Vec::new();
        if let Some(lines) = rules::blocked(self.devices.unsupported, connected, &self.devices.list) {
            let mut note = v_flex().gap(px(6.));
            for line in lines {
                note = note.child(div().text_color(style::secondary_text(cx)).child(line));
            }
            sections.push(group_note(note.into_any_element(), cx));
        } else {
            sections.push(self.add_section(cx));
            sections.push(self.paired_section(cx));
        }
        // The last thing that went wrong, under the form.
        if let Some(error) = self.devices.error.clone() {
            sections.push(
                h_flex()
                    .px(px(4.))
                    .gap(px(6.))
                    .items_center()
                    .text_size(px(13.))
                    .text_color(style::failed(cx))
                    .child(icons::symbol("exclamationmark.triangle").with_size(px(13.)))
                    .child(div().flex_1().child(error))
                    .into_any_element(),
            );
        }
        sections
    }

    /// "Add a phone or browser": the iPhone's code and the website, or why a
    /// phone can't pair with a backend on this Mac only.
    fn add_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (local_only, secondary) = (self.devices.local_only, style::secondary_text(cx));
        let mut rows: Vec<AnyElement> = Vec::new();
        if local_only {
            rows.push(row_with(div().child("iPhone").into_any_element(), div().text_color(secondary).child(rules::LOCAL_ONLY_IPHONE).into_any_element(), cx));
            rows.push(row_with(div().text_size(px(12.)).line_height(px(style::natural_line(12.))).text_color(secondary).child(rules::LOCAL_ONLY_NOTE).into_any_element(), div().into_any_element(), cx));
        } else {
            if self.devices.push_off {
                let note = h_flex().gap(px(8.)).items_start().text_color(secondary).child(div().pt(px(1.)).child(icons::symbol("bell.slash").with_size(px(13.)))).child(div().flex_1().child(rules::PUSH_OFF_NOTE));
                rows.push(row_with(note.into_any_element(), div().into_any_element(), cx));
            }
            let pair = self.busy_button("settings-pair-phone", "Show the code…", rules::PHONE_BUTTON_TIP.to_owned(), self.devices.pairing, cx, |this, window, cx| this.pair_phone(window, cx));
            rows.push(row_with(div().child("iPhone").into_any_element(), pair, cx));
        }
        let browser = crate::open_gate::default_browser().map(|b| rules::browser_name(&b));
        let open = self.busy_button("settings-open-website", "Open signed in", rules::website_tip(browser.as_deref()), self.devices.opening, cx, |this, _, cx| this.open_website(cx));
        rows.push(row_with(div().child("Website").into_any_element(), open, cx));
        let site = self.devices.list.site.clone();
        let about = rules::add_about(local_only, site.as_deref());
        section("settings-devices-add", rules::add_title(local_only), &about, rules::ADD_ASK, rows, cx)
    }

    /// "Paired": each phone and browser, with Revoke.
    fn paired_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let secondary = style::secondary_text(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        if self.devices.list.devices.is_empty() {
            rows.push(row_with(div().text_color(secondary).child(rules::empty_line(self.devices.loaded)).into_any_element(), div().into_any_element(), cx));
        }
        let now = threadbear_core::sidebar::local_now();
        for device in self.devices.list.devices.clone() {
            let expired = device.is_expired(now.timestamp_millis());
            let label = h_flex()
                .gap(px(10.))
                .items_center()
                .min_w_0()
                .child(div().flex_none().w(px(20.)).flex().justify_center().child(icons::symbol(device.kind.symbol()).with_size(px(14.)).text_color(secondary)))
                .child(
                    v_flex()
                        .min_w_0()
                        .gap(px(2.))
                        .child(div().child(device.title()))
                        .child(div().text_size(px(12.)).line_height(px(style::natural_line(12.))).text_color(if expired { crate::activity_panels::system_orange(cx) } else { secondary }).child(device.detail(&now))),
                );
            let id = device.id.clone();
            let revoke = PushButton::new(SharedString::from(format!("settings-revoke-{}", device.id)), "Revoke", device.revoke_tip()).render(cx, {
                let me = cx.entity().downgrade();
                move |_, _, cx| {
                    let id = id.clone();
                    me.update(cx, |this, cx| this.revoke_device(id, cx)).ok();
                }
            });
            // Its name for the specs, as each browser's Revoke has the same tip.
            let revoke = div().id(SharedString::from(format!("settings-revoke-holder-{}", device.id))).test_support().aria_label(format!("Revoke {}", device.title())).flex_none().child(revoke);
            rows.push(row_with(label.into_any_element(), revoke.into_any_element(), cx));
        }
        section("settings-devices-paired", "Paired", rules::PAIRED_ABOUT, rules::PAIRED_ASK, rows, cx)
    }

    /// A push button that, while its work is on the way, keeps its room but
    /// shows a spinner in its place and takes no clicks (`.busy`).
    fn busy_button(&self, id: &'static str, label: &'static str, tip: String, busy: bool, cx: &mut Context<Self>, action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> AnyElement {
        let me = cx.entity().downgrade();
        let button = PushButton::new(id, label, tip.clone()).render(cx, move |_, window, cx| {
            me.update(cx, |this, cx| action(this, window, cx)).ok();
        });
        let secondary = style::secondary_text(cx);
        div()
            .id(SharedString::from(format!("{id}-busy")))
            .relative()
            .flex_none()
            .child(div().when(busy, |s| s.opacity(0.)).child(button))
            .when(busy, |s| {
                // Over the button, so it can't be pressed twice.
                s.child(tipped(div().id(SharedString::from(format!("{id}-spinner"))).absolute().inset_0().flex().items_center().justify_center().occlude(), tip).child(Spinner::new().with_size(px(12.)).color(secondary)))
            })
            .into_any_element()
    }

    /// The section in the dump, as the Swift `settings` command says it: each
    /// device a line, the site, why nothing can pair, the error, and whether
    /// the code is up. Never a token.
    #[cfg(feature = "automation")]
    pub(super) fn devices_dump(&self, cx: &App) -> Value {
        let now = threadbear_core::sidebar::local_now();
        let blocked = rules::blocked(self.devices.unsupported, self.devices_connected(cx), &self.devices.list);
        json!({
            "rows": self.devices.list.devices.iter().map(|d| d.line(&now)).collect::<Vec<_>>(),
            "heading": blocked.is_none().then(|| rules::add_title(self.devices.local_only)),
            "note": blocked.map(|lines| lines.join(" ")),
            "site": self.devices.list.site,
            "loaded": self.devices.loaded,
            "pairing": self.devices.showing_code && self.devices.code_drawn,
            "error": self.devices.error,
        })
    }
}

/// A line on its own in a group (`Section { Text(...) }`).
fn group_note(body: AnyElement, cx: &App) -> AnyElement {
    div()
        .rounded(px(12.))
        .bg(style::primary_opacity(cx, 0.03))
        .px(px(super::ROW_INSET))
        .py(px(10.))
        .text_size(px(13.))
        .line_height(px(17.))
        .child(body)
        .into_any_element()
}

/// The code's gone (Done, Return or Esc): the window says so, and with the
/// dialog went the only copy of its squares.
fn code_closed(window: &WeakEntity<SettingsWindow>, cx: &mut App) {
    window
        .update(cx, |this, cx| {
            this.devices.showing_code = false;
            cx.notify();
        })
        .ok();
}

/// `PairPhoneSheet`: the title, the code on white, how to scan it, and Done.
fn code_sheet(qr: Option<Qr>, secondary: Hsla, closed: WeakEntity<SettingsWindow>, cx: &App) -> AnyElement {
    let label = style::label_text(cx);
    let code: AnyElement = match qr {
        Some(qr) => div()
            .id("settings-pairing-code")
            .test_support()
            .aria_label("Pairing code")
            .p(px(10.))
            .rounded(px(10.))
            .bg(white())
            .child(div().size(px(220.)).child(qr_canvas(qr)))
            .into_any_element(),
        None => div().text_color(style::failed(cx)).child("Couldn't draw the code.").into_any_element(),
    };
    let done = prominent_button("settings-pairing-done", "Done", "Close the code. The phone stays paired once it has scanned it.", cx, move |window, cx| {
        code_closed(&closed, cx);
        window.close_dialog(cx);
    });
    v_flex()
        .w_full()
        .items_center()
        .gap(px(16.))
        .text_color(label)
        .child(div().text_size(px(17.)).line_height(px(style::natural_line(17.))).font_semibold().child("Pair your iPhone"))
        .child(code)
        .child(
            v_flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(13.))
                .line_height(px(style::natural_line(13.)))
                .child(div().child("Scan it with the Threadbear app on your iPhone."))
                .child(div().text_size(px(12.)).line_height(px(style::natural_line(12.))).text_color(secondary).text_center().child("Don't have the app yet? ios/README.md in the repo says how to put it on your phone.")),
        )
        .child(done)
        .into_any_element()
}

/// The code's squares, painted a run of them at a time, each square a whole
/// number of the screen's pixels so the edges stay sharp, and centred in the
/// 220 points the Swift sheet gives it.
fn qr_canvas(qr: Qr) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let scale = window.scale_factor();
            let (width, height) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
            let square = qr.scale(width.min(height) * scale) / scale;
            let drawn = square * qr.size as f32;
            // On the screen's pixel grid, so no square smears into the next.
            let snap = |v: f32| (v * scale).round() / scale;
            let left = snap(bounds.origin.x.as_f32() + (width - drawn) / 2.);
            let top = snap(bounds.origin.y.as_f32() + (height - drawn) / 2.);
            for y in 0..qr.size {
                for (x, run) in qr.runs(y) {
                    let origin = point(gpui_kit::px(left + x as f32 * square), gpui_kit::px(top + y as f32 * square));
                    window.paint_quad(fill(Bounds::new(origin, size(gpui_kit::px(run as f32 * square), gpui_kit::px(square))), black()));
                }
            }
        },
    )
    .size_full()
}

/// A default button (`.keyboardShortcut(.defaultAction)`, or
/// `.borderedProminent`): the accent behind white words, as macOS draws it in
/// the window in front.
pub(super) fn prominent_button(id: &'static str, label: &'static str, tip: impl Into<SharedString>, cx: &App, on_click: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
    let accent = style::control_accent(cx);
    tipped(
        div()
            .id(id)
            .flex_none()
            .h(px(24.))
            .px(px(12.5))
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .bg(accent)
            .active(move |s| s.bg(accent.opacity(0.8)))
            .text_size(px(13.))
            .line_height(px(style::natural_line(13.)))
            .whitespace_nowrap()
            .text_color(white())
            .cursor_pointer(),
        tip,
    )
    .on_click(move |_, window, cx| on_click(window, cx))
    .child(label)
    .into_any_element()
}

/// The automation's devices commands, as the Swift app's `settings` command
/// takes them: `pair` presses Show the code…, `pairDone` presses Done on the
/// code, and `revoke` (a device's name) presses that one's Revoke, each
/// through a real click in the Settings window.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Option<Value>, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    let target = if cmd["pair"].as_bool() == Some(true) {
        rules::PHONE_BUTTON_TIP.to_owned()
    } else if cmd["pairDone"].as_bool() == Some(true) {
        "Close the code. The phone stays paired once it has scanned it.".to_owned()
    } else if let Some(name) = cmd["revoke"].as_str() {
        format!("Revoke {name}")
    } else {
        return Ok(None);
    };
    let handle = app.read(cx).settings_window.as_ref().map(|(h, _)| *h).ok_or("the Settings window isn't open")?;
    handle
        .update(cx, |_, window, cx| -> Result<(), String> {
            window.render_frame(cx);
            let found = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| l.to_string() == target)).last().ok_or_else(|| format!("no control {target:?} in Settings"))?;
            let at = found.bounds().center();
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Left, cx);
            window.pointer_up(at, MouseButton::Left, cx);
            // And away, as a hand would go, so the button's tip doesn't
            // stay up over the code.
            window.pointer_move(point(gpui_kit::px(1.), gpui_kit::px(1.)), None, cx);
            window.render_frame(cx);
            Ok(())
        })
        .map_err(|e| e.to_string())??;
    Ok(Some(json!("ok")))
}

/// What the Settings window draws now that its state alone can't say: the
/// pairing code really on screen. The automation's `dump` calls it after
/// drawing the window.
#[cfg(feature = "automation")]
pub(crate) fn note_drawn(view: &Entity<SettingsWindow>, window: &mut Window, cx: &mut App) {
    use gpui_kit::test::TestQueryExt as _;
    let drawn = window.elements().into_iter().any(|e| e.visible() && e.label() == Some("Pairing code"));
    view.update(cx, |this, _| this.devices.code_drawn = drawn);
}
