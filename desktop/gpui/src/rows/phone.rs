// The small phone beside messages sent from the iPhone app (Cells.swift).
use crate::a11y::A11yRole as _;
use crate::style::{self, px};
use crate::tips::Tip as _;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::Selectable;
use gpui_kit::prelude::*;
use gpui_kit::*;

const WORDS: &str = "Sent from the phone app";

pub(super) fn mark(index: usize, cx: &App) -> AnyElement {
    let ink = style::faint_text(cx);
    let text = style::label_text(cx);
    Popover::new(("phone-origin", index))
        .anchor(Anchor::BottomLeft)
        .trigger(PhoneMark { index, ink, open: false })
        .content(move |_, _, _| {
            div().id("phone-origin-note").px(px(4.)).py(px(2.)).text_size(px(12.)).line_height(px(15.)).text_color(text)
                .test_support().a11y_role(Role::GenericContainer).aria_label("Phone message origin").child(WORDS)
        })
        .into_any_element()
}

#[derive(IntoElement)]
struct PhoneMark { index: usize, ink: Hsla, open: bool }

impl Selectable for PhoneMark {
    fn selected(mut self, selected: bool) -> Self { self.open = selected; self }
    fn is_selected(&self) -> bool { self.open }
}

impl RenderOnce for PhoneMark {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let ink = self.ink;
        div().id(("phone-origin-button", self.index)).flex_none().size(px(22.))
            .flex().items_center().justify_center().cursor_pointer()
            .when(!self.open, |d| d.tip(WORDS)).test_support().role(Role::Button).aria_expanded(self.open).aria_label(WORDS)
            .child(crate::icons::symbol("iphone").size(px(12.)).text_color(ink))
    }
}
