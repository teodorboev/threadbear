// The composer's login picker (AccountPickerView and LoginRow in
// ComposerControls.swift): the logins grouped by provider, each by its address
// with its note and the room it has left under it (coloured when it's running
// out), a tick on the one chosen, and where the numbers came from at the foot.
// A popover rather than a menu, like the model picker beside it, so a row can
// say more than a title. Only without pools: with them the runner picks the
// login, and the model chip says which. What it lists is
// threadbear_core::login_picker's.
use crate::composer::Composer;
use crate::style;
use gpui_kit::component::popover::PopoverState;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use threadbear_core::login_picker::{login_lines, login_sections, LoginItem, LoginSection};
use threadbear_core::new_thread::ChipBadge;
use crate::tips::Tip as _;

/// The picker's width (AccountPickerView.width).
const WIDTH: f32 = 360.;

impl Composer {
    /// The picker's sections and its last line, for the box's thread (or new
    /// one), read live so a pick shows at once.
    pub(crate) fn login_picker(&self, cx: &App) -> (Vec<LoginSection>, Option<String>) {
        let data = self.chip_data(cx);
        let selected = data.summary.as_ref().map(|t| t.account_id.clone()).unwrap_or_default();
        let now = threadbear_core::chrono::Local::now();
        let sections = login_sections(&data.logins.accounts, &selected, &now);
        let footer = threadbear_core::usage::source_line(&data.logins.accounts, now.timestamp_millis(), &threadbear_core::chrono::Local);
        (sections, footer)
    }
}

/// The popover's content: a heading a provider, its logins, and the line
/// about where the numbers came from under a divider.
pub(crate) fn picker(composer: WeakEntity<Composer>, window: &mut Window, cx: &mut Context<PopoverState>) -> AnyElement {
    let Some(entity) = composer.upgrade() else { return div().into_any_element() };
    let (sections, footer) = entity.read(cx).login_picker(cx);
    let popover = cx.entity();
    let tertiary = style::tertiary_label(cx);
    let mut list = v_flex().p(px(6.)).gap(px(1.));
    if sections.is_empty() {
        list = list.child(div().p(px(10.)).text_size(px(13.)).line_height(px(style::natural_line(13.))).text_color(style::secondary_label(cx)).child("No accounts yet"));
    }
    for (n, section) in sections.iter().enumerate() {
        // 11 pt medium, tertiary: 2 over the first, 10 over the rest.
        list = list.child(div().px(px(10.)).pt(px(if n == 0 { 2. } else { 10. })).pb(px(2.)).text_size(px(11.)).line_height(px(style::natural_line(11.))).font_weight(FontWeight::MEDIUM).text_color(tertiary).child(section.title.clone()));
        for item in &section.items {
            let (composer, popover, id) = (composer.clone(), popover.clone(), item.id.clone());
            list = list.child(login_row(item, window, cx, move |window, cx| {
                popover.update(cx, |s, cx| s.dismiss(window, cx));
                if let Some(c) = composer.upgrade() {
                    c.update(cx, |c, cx| c.set_account(id.clone(), cx));
                }
            }));
        }
    }
    // Never past the window: 420 at most, a scroll inside it.
    let room = (f32::from(window.viewport_size().height) - style::zoomed(300.)).clamp(style::zoomed(160.), style::zoomed(420.));
    v_flex()
        .w(px(WIDTH))
        // To the popover's edges, as the Swift divider over the footer goes:
        // the kit's popover pads its content 12 all round.
        .m(gpui_kit::px(-12.))
        .child(div().id("account-picker-list").max_h(gpui_kit::px(room)).overflow_y_scroll().child(list))
        .when_some(footer, |s, footer| {
            s.child(div().w_full().h(gpui_kit::px(1.)).bg(style::hairline(cx)))
                .child(div().px(px(16.)).py(px(8.)).text_size(px(11.)).line_height(px(style::natural_line(11.))).text_color(tertiary).child(footer))
        })
        .into_any_element()
}

/// A login (LoginRow): its address, its note and room under it in two lines
/// at most (red at its limit, orange running out, grey otherwise), and a
/// tick when it's the one chosen.
fn login_row(item: &LoginItem, _window: &mut Window, cx: &App, action: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    use gpui_kit::base::TestSupportExt as _;
    let subtitle_color = match item.badge {
        Some(ChipBadge::Blocked) => cx.theme().danger,
        Some(ChipBadge::Low(_)) => style::needs_you(cx),
        None => style::secondary_label(cx),
    };
    let tip = item.subtitle.clone().unwrap_or_else(|| item.title.clone());
    let name = if item.is_selected { format!("{}, chosen", item.title) } else { item.title.clone() };
    h_flex()
        .id(SharedString::from(format!("login-{}", item.id)))
        .test_support()
        .aria_label(name)
        .gap(px(8.))
        .px(px(10.))
        .py(px(5.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(style::primary_opacity(cx, 0.06)))
        .tip(tip.clone())
        .on_click(move |_, window, cx| action(window, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(div().truncate().text_size(px(13.)).line_height(px(style::natural_line(13.))).font_weight(FontWeight::MEDIUM).text_color(style::label_text(cx)).child(item.title.clone()))
                .when_some(item.subtitle.clone(), |s, subtitle| s.child(div().line_clamp(2).text_size(px(11.)).line_height(px(style::natural_line(11.))).text_color(subtitle_color).child(subtitle))),
        )
        // SF's checkmark at 11 pt semibold; kept in its place while unticked,
        // so the rows' words line up.
        .child(div().flex_none().child(gpui_kit::component::Icon::from(gpui_kit::assets::IconName::Check).with_size(px(12.)).text_color(style::label_text(cx)).when(!item.is_selected, |s| s.invisible())))
}

/// The automation's `composer-menu` for `account`: the picker as lines, or
/// `click` a login by its address or id, as a click on its row does.
#[cfg(feature = "automation")]
pub(crate) fn menu_command(composer: &Entity<Composer>, cmd: &serde_json::Value, cx: &mut App) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let (sections, footer) = composer.read(cx).login_picker(cx);
    if let Some(title) = cmd["click"].as_str() {
        let Some(item) = sections.iter().flat_map(|s| s.items.iter()).find(|i| i.title == title || i.id == title) else { return Err(format!("no item {title}")) };
        let id = item.id.clone();
        composer.update(cx, |c, cx| c.set_account(id, cx));
        return Ok(json!("ok"));
    }
    Ok(json!(login_lines(&sections, footer.as_deref())))
}
