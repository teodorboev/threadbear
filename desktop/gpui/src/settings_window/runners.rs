// Settings › Runners (RunnersSettings.swift): where new threads start, which is
// this Mac's own choice, and every runner in Mike's order, which is everyone's:
// the phone sets it too, and its new threads start on the first one online in
// it (backend API 58). Deliberately thin, as a runner is set up on its own
// machine, which the ⓘs say. The words and the order are
// threadbear_core::runners; the order is saved as the Runners panel saves it
// (`set_runner_order`), here at once.
use super::{plain_note, row_with, section, SettingsWindow};
use crate::app::Threadbear;
use crate::highlights::tipped;
use crate::{icons, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use threadbear_core::runners::{self as rules, SettingsRow};
use crate::tips::Tip as _;

/// A Form row's height on the Mac, which the ordered rows keep so the list
/// matches the sections round it (`.frame(minHeight: 38)`).
const ROW_HEIGHT: f32 = 38.;

/// What Settings › Runners shows, read from the app in one go.
struct Shown {
    rows: Vec<SettingsRow>,
    orderable: bool,
    default: Option<String>,
    /// The picker's choices: this Mac, then each other runner.
    choices: Vec<(Option<String>, String, &'static str)>,
}

/// A row being dragged to a new place, and what the lifted copy shows.
#[derive(Clone)]
struct DraggedRow {
    id: String,
    name: SharedString,
    symbol: &'static str,
    status: SharedString,
    width: Pixels,
}

/// The row lifted under the pointer while it's dragged (ReorderList's lifted
/// row): solid, a little lighter than the window, with an edge and a shadow.
struct RowGhost(DraggedRow);

impl Render for RowGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.0;
        let (secondary, tertiary) = (style::secondary_text(cx), style::tertiary_label(cx));
        h_flex()
            .w(row.width)
            .h(px(ROW_HEIGHT))
            .px(px(super::ROW_INSET))
            .gap(px(8.))
            .items_center()
            .rounded(px(8.))
            .bg(cx.theme().background)
            .child(div().absolute().inset_0().rounded(px(8.)).bg(style::primary_opacity(cx, 0.1)).border_1().border_color(style::hairline(cx)))
            .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.4), offset: point(gpui_kit::px(0.), px(5.)), blur_radius: px(10.), spread_radius: gpui_kit::px(0.), inset: false }])
            .text_size(px(13.))
            .line_height(px(style::natural_line(13.)))
            .child(handle_icon(tertiary))
            .child(runner_icon(row.symbol, secondary))
            .child(div().flex_none().child(row.name.clone()))
            .child(div().flex_1())
            .child(div().flex_none().text_color(secondary).child(row.status.clone()))
    }
}

/// `line.3.horizontal` in the tertiary colour, as wide as SF's at 13 points.
fn handle_icon(color: Hsla) -> Div {
    div().flex_none().w(px(16.)).flex().justify_center().child(icons::symbol("line.3.horizontal").with_size(px(15.)).text_color(color))
}

/// The runner's icon in a 20 point column, so the names line up.
fn runner_icon(symbol: &'static str, color: Hsla) -> Div {
    div().flex_none().w(px(20.)).flex().justify_center().child(icons::symbol(symbol).with_size(px(16.)).text_color(color))
}

impl SettingsWindow {
    fn runners_shown(&self, cx: &App) -> Option<Shown> {
        let app = self.app.upgrade()?;
        let t = app.read(cx);
        let this_mac = t.remote.all_runners.iter().find(|r| r.is_self).cloned();
        let others = t.remote.runners.clone();
        let order = t.runners_panel.order.clone();
        let default = t.new_thread.prefs.remembered.default_runner.clone();
        let orderable = rules::can_order(order.is_some(), &others, &t.remote.all_runners);
        // The direct link isn't in this app: other machines' threads come
        // through Convex, so nothing waits on a token being copied in.
        let rows = rules::settings_rows(this_mac.as_ref(), &others, order.as_deref(), default.as_deref(), orderable, false);
        let mut choices = vec![(None, "This Mac".to_owned(), t.self_runner_symbol())];
        choices.extend(others.iter().map(|r| (Some(r.id.clone()), r.name.clone(), icons::runner_symbol(r.icon.as_deref()))));
        Some(Shown { rows, orderable, default, choices })
    }

    pub(super) fn render_runners(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(shown) = self.runners_shown(cx) else { return vec![plain_note("Not connected to the runner.", cx)] };
        let mut sections = Vec::new();

        // Where new threads start.
        let picker = self.default_runner_picker(&shown);
        sections.push(section("settings-default-runner", "Default runner", rules::DEFAULT_ABOUT, rules::DEFAULT_ASK, vec![row("New threads start on", picker, cx)], cx));

        // The runners, in Mike's order once it can be changed.
        let width = row_width(window);
        let rows: Vec<AnyElement> = shown.rows.iter().map(|r| self.runner_row(r, shown.orderable, width, cx)).collect();
        if shown.orderable {
            // The Swift list is one Form row holding them all, so the group
            // has a row's room above and below it (96 tall for two).
            let body = div().py(px(10.)).child(super::rows_with_dividers(rows, cx)).into_any_element();
            sections.push(super::section_with_body("settings-runners", "Runners", rules::runners_about(true), rules::RUNNERS_ASK, body, cx));
        } else {
            sections.push(section("settings-runners", "Runners", rules::runners_about(false), rules::RUNNERS_ASK, rows, cx));
        }

        // How other machines' threads get here.
        let tip = "This app opens other machines' threads through Convex for now. Connecting directly is in the Swift app.";
        let switch = div()
            .id("settings-direct-link-holder")
            .test_support()
            .aria_label(tip)
            .tip(tip)
            .flex_none()
            .child(crate::mac_switch::MacSwitch::new("settings-direct-link", false).disabled(true));
        sections.push(section("settings-other-machines", "Other machines' threads", rules::OTHERS_ABOUT, rules::OTHERS_ASK, vec![row("Connect to other machines directly", switch, cx)], cx));
        sections
    }

    /// `Picker("New threads start on")`: this Mac or another runner, each
    /// with its icon, the current one ticked.
    fn default_runner_picker(&self, shown: &Shown) -> AnyElement {
        let current = shown.choices.iter().find(|(id, _, _)| *id == shown.default);
        let (label, symbol) = match current {
            Some((_, name, symbol)) => (name.clone(), *symbol),
            None => (shown.default.clone().unwrap_or_else(|| "This Mac".to_owned()), "desktopcomputer"),
        };
        let app = self.app.clone();
        let choices = shown.choices.clone();
        let default = shown.default.clone();
        let button = Button::new("settings-default-runner-menu").label(label).icon(icons::symbol(symbol)).small().secondary().dropdown_caret(true).tooltip(rules::DEFAULT_TIP).dropdown_menu(move |menu: PopupMenu, _, _| {
            let mut menu = menu;
            for (id, name, symbol) in &choices {
                let checked = *id == default;
                let (app, id) = (app.clone(), id.clone());
                menu = menu.item(PopupMenuItem::new(name.clone()).icon(icons::symbol(symbol)).checked(checked).on_click(move |_, _, cx| {
                    let id = id.clone();
                    app.update(cx, |t, cx| t.set_default_runner(id, cx)).ok();
                }));
            }
            menu
        });
        div().id("settings-default-runner-holder").test_support().aria_label(rules::DEFAULT_TIP).flex_none().child(button).into_any_element()
    }

    /// `RunnerRow`: a handle to drag it by when the order can change, its
    /// icon, name and a star where new threads start, then how it is.
    fn runner_row(&self, r: &SettingsRow, movable: bool, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let (secondary, tertiary) = (style::secondary_text(cx), style::tertiary_label(cx));
        let symbol = icons::runner_symbol(r.icon.as_deref());
        let mut line = h_flex().w_full().gap(px(8.)).items_center();
        if movable {
            line = line.child(tipped(div().id(SharedString::from(format!("settings-runner-handle-{}", r.id))).flex_none().cursor_grab(), "Drag to change the order").child(handle_icon(tertiary)));
        }
        line = line.child(runner_icon(symbol, secondary)).child(div().flex_none().child(r.name.clone()));
        if r.is_default {
            // SF's star.fill at 9 points, on the name's line.
            line = line.child(tipped(div().id(SharedString::from(format!("settings-runner-star-{}", r.id))).flex_none(), "New threads start here").child(icons::symbol("star.fill").with_size(px(11.)).text_color(tertiary)));
        }
        line = line.child(div().flex_1().min_w(px(8.))).child(div().flex_none().text_color(secondary).child(r.status.clone()));
        if !movable {
            return row_with(line.into_any_element(), div().into_any_element(), cx);
        }
        // The whole row drags, as the Swift list's does; the slot it would
        // take shows dashed meanwhile, and a drop puts it there.
        let slot = style::tertiary_label(cx);
        let drag = DraggedRow { id: r.id.clone(), name: r.name.clone().into(), symbol, status: r.status.clone().into(), width };
        let onto = r.id.clone();
        div()
            .id(SharedString::from(format!("settings-runner-{}", r.id)))
            .test_support()
            .w_full()
            .rounded(px(8.))
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| RowGhost(drag.clone())))
            .drag_over::<DraggedRow>(move |s, _, _, _| s.border_1().border_dashed().border_color(slot).m(gpui_kit::px(-1.)))
            .on_drop(cx.listener(move |this, drag: &DraggedRow, _, cx| this.drop_runner_row(&drag.id, &onto, cx)))
            .child(h_flex().min_h(px(ROW_HEIGHT)).px(px(super::ROW_INSET)).items_center().text_size(px(13.)).line_height(px(17.)).child(line))
            .into_any_element()
    }

    /// A row dropped on another's place: it moves there and the rest shift
    /// round it, and the whole order is saved (`setRunnerOrder`).
    fn drop_runner_row(&mut self, dragged: &str, onto: &str, cx: &mut Context<Self>) {
        let Some(shown) = self.runners_shown(cx) else { return };
        let ids: Vec<String> = shown.rows.iter().map(|r| r.id.clone()).collect();
        let (Some(from), Some(to)) = (ids.iter().position(|id| id == dragged), ids.iter().position(|id| id == onto)) else { return };
        if from == to || !shown.orderable {
            return;
        }
        let next = rules::moving(&ids, from, to);
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |t, cx| t.set_runner_order(next, cx));
        }
        cx.notify();
    }

    /// The section in the dump: its rows as lines ("m5 *: This Mac"), the
    /// default's name, and whether the order can change.
    #[cfg(feature = "automation")]
    pub(super) fn runners_dump(&self, cx: &App) -> Value {
        let Some(shown) = self.runners_shown(cx) else { return Value::Null };
        let default = shown.choices.iter().find(|(id, _, _)| *id == shown.default).map(|(_, name, _)| name.clone());
        json!({
            "rows": shown.rows.iter().map(rules::settings_line).collect::<Vec<_>>(),
            "default": default,
            "canOrder": shown.orderable,
            "directLink": false,
        })
    }
}

/// A row's width while it's dragged: the form's, as wide as the window lets
/// it be (the pane less the sidebar and the form's margins, at most 700).
fn row_width(window: &Window) -> Pixels {
    let pane = window.viewport_size().width - px(super::SIDEBAR_WIDTH);
    (pane - px(40.)).min(px(660.)).max(px(240.))
}

/// A Form row with a plain label (`LabeledContent`).
fn row(label: &'static str, control: impl IntoElement, cx: &App) -> AnyElement {
    row_with(div().child(label).into_any_element(), control.into_any_element(), cx)
}

/// The automation's runner commands for Settings, as the Swift app's
/// `settings` command takes them: `defaultRunner` (a name, or "This Mac")
/// picks it from the menu, as a click there does; `drag` with `onto` (names)
/// drags one row onto another's place through real pointer events.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Option<Value>, String> {
    use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
    if let Some(name) = cmd["defaultRunner"].as_str() {
        let runner = if name == "This Mac" { None } else { Some(app.read(cx).remote.runners.iter().find(|r| r.name == name).ok_or_else(|| format!("no runner {name}"))?.id.clone()) };
        app.update(cx, |t, cx| t.set_default_runner(runner, cx));
        return Ok(Some(json!("ok")));
    }
    let (Some(from), Some(onto)) = (cmd["drag"].as_str(), cmd["onto"].as_str()) else { return Ok(None) };
    let id_of = |name: &str| app.read(cx).remote.all_runners.iter().map(|r| (r.id.clone(), r.name.clone())).chain(app.read(cx).remote.runners.iter().map(|r| (r.id.clone(), r.name.clone()))).find(|(_, n)| n == name).map(|(id, _)| id).ok_or_else(|| format!("no runner {name}"));
    let (a, b) = (id_of(from)?, id_of(onto)?);
    let handle = app.read(cx).settings_window.as_ref().map(|(h, _)| *h).ok_or("the Settings window isn't open")?;
    handle
        .update(cx, |_, window, cx| -> Result<(), String> {
            window.render_frame(cx);
            let center = |id: &str, window: &Window| window.try_find(SharedString::from(format!("settings-runner-{id}"))).map(|e| e.bounds().center()).ok_or_else(|| format!("no row for {id} on screen"));
            let (p, q) = (center(&a, window)?, center(&b, window)?);
            window.pointer_move(p, None, cx);
            window.pointer_down(p, MouseButton::Left, cx);
            for step in 1..=8 {
                let t = step as f32 / 8.;
                window.pointer_move(point(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t), Some(MouseButton::Left), cx);
            }
            window.pointer_up(q, MouseButton::Left, cx);
            Ok(())
        })
        .map_err(|e| e.to_string())??;
    Ok(Some(json!("ok")))
}
