// A reply's diagram or page full size, in a window of its own taking most of
// the screen (rich.fullsize; RichBlockWindow.swift, which Mike asked for on
// 8 Nov as a big diagram was still too small to read in the pane). The page
// is the sealed one (threadbear_web's page.rs), in a native view filling the
// window, placed from a GPUI layout slot each frame. One window per block:
// opening it again brings that one forward. Until the runtime's installed the
// window shows the question and the block's source instead.
use crate::rich_boxes::{self, Drawing};
use crate::style::{self, px, size};
use crate::web_runtime::{self, Engine};
use gpui_kit::component::{v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
#[cfg(target_os = "macos")]
use std::rc::Rc;
use threadbear_core::rich_block::{RichBlock, RichKind};
use threadbear_core::web_runtime::State;

/// Whether a script drives this app: its windows stay where they are, as the
/// Swift app's tuck themselves behind.
fn scripted() -> bool {
    cfg!(feature = "automation") && std::env::var_os("THREADBEAR_AUTOMATION").is_some()
}

/// Opens the block full size, or brings its window forward.
pub(crate) fn open(block: &RichBlock, window: &mut Window, cx: &mut App) {
    let Some(store) = rich_boxes::store(cx) else { return };
    if let Some(handle) = store.read(cx).windows.get(&block.id).copied() {
        if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
            return;
        }
        store.update(cx, |s, _| s.windows.remove(&block.id));
    }
    // On the screen Threadbear's in, most of it (the review window's frame).
    let display = window.display(cx);
    let visible = display.as_ref().map(|d| d.visible_bounds()).unwrap_or(Bounds { origin: point(gpui_kit::px(0.), gpui_kit::px(0.)), size: size(gpui_kit::px(1240. / 0.9), gpui_kit::px(860. / 0.9)) });
    let frame = threadbear_core::review::window_frame(threadbear_core::zoom::Rect { x: f64::from(visible.origin.x), y: f64::from(visible.origin.y), width: f64::from(visible.size.width), height: f64::from(visible.size.height) });
    let bounds = Bounds { origin: point(gpui_kit::px(frame.x as f32), gpui_kit::px(frame.y as f32)), size: size(gpui_kit::px(frame.width as f32), gpui_kit::px(frame.height as f32)) };
    let block = block.clone();
    // Opened once this update is over, as Settings and reviews are.
    cx.defer(move |cx| {
        let id = block.id.clone();
        let opened = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some(block.window_title().into()), ..Default::default() }),
                focus: !scripted(),
                display_id: display.map(|d| d.id()),
                window_min_size: Some(size(gpui_kit::px(480.), gpui_kit::px(360.))),
                app_id: Some("threadbear".into()),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| RichWindow::new(block, window, cx)),
        );
        if let Ok((handle, _)) = opened {
            store.update(cx, |s, cx| {
                s.windows.insert(id, handle);
                cx.notify();
            });
        }
    });
}

struct RichWindow {
    block: RichBlock,
    #[cfg(target_os = "macos")]
    page: Option<Rc<threadbear_web::Page>>,
    failed: Option<String>,
    _subs: Vec<Subscription>,
}

impl RichWindow {
    fn new(block: RichBlock, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subs = Vec::new();
        // The runtime installed or started, or the diagram drawn: try again.
        if let Some(runtime) = web_runtime::runtime(cx) {
            subs.push(cx.observe_in(&runtime, window, |this, _, window, cx| this.make_page(window, cx)));
        }
        if let Some(store) = rich_boxes::store(cx) {
            subs.push(cx.observe_in(&store, window, |this, _, window, cx| this.make_page(window, cx)));
        }
        // Closed: its page goes with it, and the block opens a new one next time.
        let id = block.id.clone();
        window.on_window_should_close(cx, move |_, cx| {
            if let Some(store) = rich_boxes::store(cx) {
                store.update(cx, |s, cx| {
                    s.windows.remove(&id);
                    cx.notify();
                });
            }
            true
        });
        let mut this = RichWindow {
            block,
            #[cfg(target_os = "macos")]
            page: None,
            failed: None,
            _subs: subs,
        };
        this.make_page(window, cx);
        this
    }

    /// The page, once the runtime's running and (for a diagram) it's drawn.
    #[cfg(target_os = "macos")]
    fn make_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_some() || self.failed.is_some() {
            return;
        }
        let Some(runtime) = web_runtime::runtime(cx) else { return };
        if runtime.read(cx).state != State::Ready || !runtime.update(cx, |r, cx| r.ensure_engine(cx)) {
            if let Engine::Failed(why) = &runtime.read(cx).engine {
                self.failed = Some(why.clone());
                cx.notify();
            }
            return;
        }
        let Some(store) = rich_boxes::store(cx) else { return };
        let Some(html) = store.read(cx).window_page(&self.block) else { return };
        let Some(parent) = native_view(window) else {
            self.failed = Some("the window has no view to show it in".into());
            return;
        };
        match threadbear_web::Page::sealed(parent, &threadbear_web::data_url(&html)) {
            Ok(page) => self.page = Some(Rc::new(page)),
            Err(why) => self.failed = Some(why),
        }
        cx.notify();
    }

    #[cfg(not(target_os = "macos"))]
    fn make_page(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    #[cfg(target_os = "macos")]
    fn page_slot(&self) -> Option<AnyElement> {
        let page = self.page.clone()?;
        // The page's view follows this slot: its bounds set the view's frame
        // as the window lays out, so it resizes with the window.
        Some(
            canvas(
                move |bounds, _window, _cx| {
                    page.set_bounds(f32::from(bounds.origin.x), f32::from(bounds.origin.y), f32::from(bounds.size.width), f32::from(bounds.size.height));
                },
                |_, _, _, _| {},
            )
            .size_full()
            .into_any_element(),
        )
    }

    #[cfg(not(target_os = "macos"))]
    fn page_slot(&self) -> Option<AnyElement> {
        None
    }
}

/// The window's own NSView, the page's parent.
#[cfg(target_os = "macos")]
fn native_view(window: &Window) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()),
        _ => None,
    }
}

impl Render for RichWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        if let Some(slot) = self.page_slot() {
            return div().size_full().bg(background).child(slot).into_any_element();
        }
        // No page (yet): why, the question, or a note while it's drawn.
        let secondary = style::secondary_label(cx);
        let note = |text: String| div().text_size(px(size::LABEL)).text_color(secondary).child(text);
        let drawing = matches!(rich_boxes::store(cx).and_then(|s| s.read(cx).state(&self.block, style::zoom_percent()).cloned()), Some(Drawing::Queued | Drawing::Drawing));
        let wants = match self.block.kind {
            RichKind::Mermaid => "this diagram",
            RichKind::Html => "this page",
        };
        let top = match (&self.failed, web_runtime::prompt(&format!("rich-window-{}", self.block.id), wants, cx)) {
            (Some(why), _) => Some(note(format!("Couldn't show this: {why}")).into_any_element()),
            (None, Some(prompt)) => Some(prompt),
            (None, None) if drawing => Some(note("Drawing it…".into()).into_any_element()),
            _ => None,
        };
        v_flex()
            .id("rich-window-source")
            .size_full()
            .bg(background)
            .p(px(24.))
            .gap(px(16.))
            .overflow_y_scroll()
            .children(top)
            .child(div().font_family(cx.theme().mono_font_family.clone()).text_size(px(size::CODE)).line_height(px(size::CODE_LINE)).text_color(style::label_text(cx)).child(self.block.source.clone()))
            .into_any_element()
    }
}
