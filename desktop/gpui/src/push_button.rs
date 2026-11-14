// Swift's regular push button: a SwiftUI `Button` with the default bordered
// style, as on the connection bar and in "Couldn't load this thread". 24 tall
// with a 13 pt label in label colour, and a fill of white at 8% in the dark
// look and black at 8% in the light one, with no edge or shadow, so it takes
// on what's behind it (on the yellow connection bar it's a lighter or darker
// yellow, not a grey box), as AppKit's does. Measured against the Swift app's
// pictures (features/tools/pixel-overlay.swift): 80 wide for "Try again",
// #2e2e2e on #1c1c1c, #ebebeb on white, #474231 on the dark bar's #38321f.
use crate::style::{self, px};
use crate::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub(crate) struct PushButton {
    id: ElementId,
    label: SharedString,
    tip: SharedString,
    busy: bool,
    small: bool,
}

impl PushButton {
    /// `tip` is its tooltip, and the name the specs click it by.
    pub(crate) fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, tip: impl Into<SharedString>) -> Self {
        PushButton { id: id.into(), label: label.into(), tip: tip.into(), busy: false, small: false }
    }

    /// Spinning, and not taking clicks, while what it started is under way.
    pub(crate) fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// SwiftUI's `.controlSize(.small)`: 20 tall with an 11 pt label, as
    /// the Browser pane's buttons are.
    pub(crate) fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub(crate) fn render(self, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
        let dark = cx.theme().is_dark();
        let label = style::label_text(cx);
        let busy = self.busy;
        let (height, inset, size) = if self.small { (20., 8., 11.) } else { (24., 12.5, 13.) };
        let base = div()
            .id(self.id)
            .flex_none()
            .h(px(height))
            .px(px(inset))
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(5.))
            .text_size(px(size))
            .line_height(px(style::natural_line(size)))
            .whitespace_nowrap()
            .text_color(label);
        // White over the dark look, black over the light one, at 8% either
        // way, and twice that while it's pressed.
        let ink = if dark { 1. } else { 0. };
        let filled = base.bg(hsla(0., 0., ink, 0.08)).active(move |s| s.bg(hsla(0., 0., ink, 0.16)));
        let button = filled.when(busy, |s| s.opacity(0.6).child(Spinner::new().color(label).with_size(px(11.)))).when(!busy, |s| s.on_click(on_click)).child(self.label);
        crate::highlights::tipped(button, self.tip).into_any_element()
    }
}
