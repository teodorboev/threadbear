#![cfg(feature = "automation")]

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Context, ExternalPaths, FileDropEvent, InputEvent as _,
    InteractiveElement as _, IntoElement, Keystroke, Render, Styled, TestAppContext,
    TouchEvent, Window, div, point, px,
};
use std::{cell::Cell, path::PathBuf, rc::Rc};

struct FileDropTarget(Rc<Cell<usize>>);

impl Render for FileDropTarget {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let drops = self.0.clone();
        div().size_full().on_drop(move |_: &ExternalPaths, _, _| drops.set(drops.get() + 1))
    }
}

#[test]
fn file_drop_after_keyboard_input_reaches_its_target() {
    let mut cx = TestAppContext::single();
    let drops = Rc::new(Cell::new(0));
    let window = cx.add_window({
        let drops = drops.clone();
        move |_, _| FileDropTarget(drops)
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let position = point(px(10.), px(10.));
        for event in [
            FileDropEvent::Entered {
                position,
                paths: ExternalPaths(vec![PathBuf::from("/tmp/picture.png")].into()),
            },
            FileDropEvent::Pending { position },
            FileDropEvent::Submit { position },
        ] {
            window.dispatch_keystroke(Keystroke::parse("a").unwrap(), cx);
            assert!(window.last_input_was_keyboard());
            window.dispatch_event(event.to_platform_input(), cx);
            assert!(!window.last_input_was_keyboard());
        }
        assert_eq!(drops.get(), 1);
        assert!(!cx.has_active_drag());
        // End notifications can arrive after typing resumes. They only clean
        // up the drag; they must not turn keyboard hover highlights back on.
        for event in [FileDropEvent::Exited, FileDropEvent::Ended] {
            window.dispatch_keystroke(Keystroke::parse("a").unwrap(), cx);
            window.dispatch_event(event.to_platform_input(), cx);
            assert!(window.last_input_was_keyboard());
        }
    }).unwrap();
}

#[test]
fn file_drop_preserves_touch_input_mode() {
    let mut cx = TestAppContext::single();
    let drops = Rc::new(Cell::new(0));
    let window = cx.add_window({
        let drops = drops.clone();
        move |_, _| FileDropTarget(drops)
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(TouchEvent::default().to_platform_input(), cx);
        let position = point(px(10.), px(10.));
        for event in [
            FileDropEvent::Entered {
                position,
                paths: ExternalPaths(vec![PathBuf::from("/tmp/picture.png")].into()),
            },
            FileDropEvent::Pending { position },
            FileDropEvent::Submit { position },
        ] {
            window.dispatch_event(event.to_platform_input(), cx);
            assert!(window.last_input_was_touch());
        }
        assert_eq!(drops.get(), 1);
    }).unwrap();
}
