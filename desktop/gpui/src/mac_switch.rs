// The switch a SwiftUI Toggle draws in a grouped Form on macOS 27: a 36 by 16
// track with a pill-shaped knob, 21 by 14, a point in from its edge. gpui-kit's
// small switch is 28 by 16 with a round knob, and its medium one 36 by 20, so
// Settings' switches read as another app's (the Usage helper's note, 11 Nov).
// Measured from the Swift Settings window on Tinker Desk: the track #393939
// over the group's #232323 when off (white at 10%), the knob #e1e1e1.
//
// Built on gpui-base's Switch, as the kit's own is, for its role, its click and
// Space/Return when it has the keyboard; the knob slides on the kit's spring.
use crate::style::{self, px};
use gpui_kit::base::{spring, Switch as BaseSwitch, SwitchThumb, SwitchTrack};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

const TRACK_W: f32 = 36.;
const TRACK_H: f32 = 16.;
const KNOB_W: f32 = 21.;
const KNOB_H: f32 = 14.;
const INSET: f32 = 1.;

#[derive(IntoElement)]
pub(crate) struct MacSwitch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    outside_form: bool,
    on_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
}

impl MacSwitch {
    pub(crate) fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        MacSwitch { id: id.into(), checked, disabled: false, outside_form: false, on_change: None }
    }

    /// Swift's `.controlSize(.small)` outside a grouped Form has a 44×20
    /// track and a 26×16 knob, two points inset. Measured from a hidden
    /// NSHostingView on macOS 27; its layout is 54×24 including the padding.
    /// Forms use the smaller dimensions above.
    pub(crate) fn outside_form(mut self) -> Self {
        self.outside_form = true;
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the value a click asks for; the owner sets it and redraws.
    pub(crate) fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MacSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let dark = cx.theme().is_dark();
        let on = self.checked;
        let (track_w, track_h, knob_w, knob_h, inset) = if self.outside_form { (44., 20., 26., 16., 2.) } else { (TRACK_W, TRACK_H, KNOB_W, KNOB_H, INSET) };
        let track = if on { style::control_accent(cx) } else { style::primary_opacity(cx, if dark { 0.1 } else { 0.09 }) };
        let knob = if dark { hsla(0., 0., 0.88, 1.) } else { hsla(0., 0., 1., 1.) };
        let travel = px(track_w - knob_w - inset * 2.);
        let x = spring((self.id.clone(), "knob"), if on { travel } else { gpui_kit::px(0.) }, cx.theme().motion_tokens().spring_move, window, cx);
        let on_change = self.on_change.clone();
        BaseSwitch::new(self.id.clone())
            .checked(on)
            .disabled(self.disabled)
            .when_some(on_change.filter(|_| !self.disabled), |s, f| s.on_change(move |next, _, window, cx| f(next, window, cx)))
            .flex_none()
            .when(self.outside_form, |s| s.px(px(5.)).py(px(2.)))
            .child(
                SwitchTrack::new((self.id.clone(), "track"))
                    .checked(on)
                    .disabled(self.disabled)
                    .w(px(track_w))
                    .h(px(track_h))
                    .flex_none()
                    .rounded_full()
                    .p(px(inset))
                    .bg(track)
                    .when(self.disabled, |s| s.opacity(0.5))
                    .cursor_pointer()
                    .child(
                        SwitchThumb::new(on)
                            .relative()
                            .left(x)
                            .w(px(knob_w))
                            .h(px(knob_h))
                            .rounded_full()
                            .bg(knob)
                            // A light knob on a light track needs its edge.
                            .when(!dark, |s| s.shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.25), offset: point(gpui_kit::px(0.), gpui_kit::px(0.5)), blur_radius: gpui_kit::px(1.5), spread_radius: gpui_kit::px(0.), inset: false }])),
                    ),
            )
    }
}
