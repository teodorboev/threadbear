// Every control's tooltip goes through here, as the Swift app's `.tip`
// (Views/Tips.swift): `Tip::tip` gives an element its tooltip and an anchor
// the activity log names a click by (activity.rs), the tooltip of the smallest
// control under the pointer, as `Tips.text(at:in:)` has it, so nothing else in
// the app needs to say what a control is. highlights.rs's `tipped` is the same
// with the label scripts find a control by. A kit component's own
// `.tooltip(text)` (a Button, a Switch) has no anchor: the kit draws those
// itself, and a child put in a Button changes how it looks.
//
// The anchor is an invisible layer over the control. While the log is off it
// does nothing; while it's on, it notes a press over it as the press begins
// (the capture phase, which nothing stops), whatever the control does with it.
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

pub(crate) trait Tip: ParentElement + StatefulInteractiveElement + Sized {
    /// A tooltip, shown after a short hover, and the name of a click on it.
    fn tip(self, text: impl Into<SharedString>) -> Self {
        let text: SharedString = text.into();
        let shown = text.clone();
        self.tooltip(move |window, cx| {
            if MENU_UP.with(|m| m.get() != 0) {
                return cx.new(|_| NoTip).into();
            }
            // The pointer stopped on it to find out what it is.
            crate::activity::hover(&shown, window, cx);
            Tooltip::new(shown.clone()).build(window, cx)
        })
        .child(anchor(text))
    }
}

impl<E: ParentElement + StatefulInteractiveElement + Sized> Tip for E {}

thread_local! {
    /// Which of the app's own menus are up, one bit each (`menu_up`).
    static MENU_UP: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// A menu of the app's is up (or has gone): no tooltip shows meanwhile, as
/// on macOS, where a menu's tracking holds them back. The sort button's tip
/// came up over its own menu, the pointer still on the button (12 Nov).
/// `which` is the menu's bit: 1, the sidebar's (its row and heading menus).
pub(crate) fn menu_up(which: u8, up: bool) {
    MENU_UP.with(|m| m.set(if up { m.get() | which } else { m.get() & !which }));
}

/// Whether any of those menus is up, which an update's restart waits for.
pub(crate) fn any_menu_up() -> bool {
    MENU_UP.with(|m| m.get() != 0)
}

/// What a tooltip shows while a menu is up: nothing.
struct NoTip;

impl Render for NoTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

fn anchor(text: SharedString) -> impl IntoElement {
    canvas(
        |bounds, window, _| crate::activity::is_on().then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal)),
        move |bounds, hitbox, window, _| {
            #[cfg(feature = "automation")]
            placed(&text, bounds, window);
            let Some(hitbox) = hitbox else { return };
            let area = f32::from(bounds.size.width) * f32::from(bounds.size.height);
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
                    crate::activity::press(event, Some((area, text.clone())), window, cx);
                }
            });
        },
    )
    // Over the whole control, as gpui-kit's `on_prepaint` places its layer.
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The last child of a window's root: every press in the window, named by an
/// anchor or not, goes in the log. Last, so its part of the press comes after
/// every anchor's in the window's own content.
pub(crate) fn window_presses() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            if !crate::activity::is_on() {
                return;
            }
            window.on_mouse_event(|event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    crate::activity::press(event, None, window, cx);
                    // A script's press only asks what's there: nothing under
                    // it is pressed (`activity` with `click`).
                    #[cfg(feature = "automation")]
                    if SIMULATING.with(std::cell::Cell::get) {
                        cx.stop_propagation();
                    }
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(feature = "automation")]
thread_local! {
    /// Where each tip was last drawn, by its text, for a script to find one.
    static PLACED: std::cell::RefCell<std::collections::HashMap<(u64, SharedString), Bounds<Pixels>>> = Default::default();
    static SIMULATING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "automation")]
fn placed(text: &SharedString, bounds: Bounds<Pixels>, window: &Window) {
    let key = (window.window_handle().window_id().as_u64(), text.clone());
    PLACED.with(|p| p.borrow_mut().insert(key, bounds));
}

/// A press on the control with this tip, as the activity log sees one, with
/// nothing pressed (the Swift app's `activity` with `click`).
#[cfg(feature = "automation")]
pub(crate) fn simulate_press(tip: &str, button: MouseButton, window: &mut Window, cx: &mut App) -> Result<(), String> {
    use gpui_kit::test::TestWindowExt as _;
    // Everything drawn afresh, cached views too, so where each tip is is now.
    window.refresh();
    window.render_frame(cx);
    let key = (window.window_handle().window_id().as_u64(), SharedString::from(tip.to_owned()));
    let Some(bounds) = PLACED.with(|p| p.borrow().get(&key).copied()) else { return Err(format!("no control with the tip {tip}")) };
    let at = bounds.center();
    let down = MouseDownEvent { button, position: at, modifiers: Modifiers::default(), click_count: 1, first_mouse: false };
    SIMULATING.with(|s| s.set(true));
    window.dispatch_event(down.to_platform_input(), cx);
    SIMULATING.with(|s| s.set(false));
    Ok(())
}
