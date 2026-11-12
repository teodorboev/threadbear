// A page in a native view of its own, a child of a GPUI window's view and
// placed from a GPUI layout slot each frame (Ghostex's CefElement: the slot's
// bounds set the view's frame in prepaint; the gpui-web-spike branch showed it
// following the layout). The full-size window's sealed diagram or page now,
// under sealed.rs's rules with no script of the page's; web panels next.
//
// GPUI's own menus and tooltips can't draw over it (it's a native view above
// GPUI's layer), so whatever holds one keeps its controls clear of it.
use super::host::{tb_view_set_frame, tb_view_set_hidden};
use super::sealed;
use cef::*;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

pub struct Page {
    browser: Browser,
    view: *mut c_void,
    frame: Cell<(f32, f32, f32, f32)>,
    hidden: Cell<bool>,
}

impl Page {
    /// A sealed page at `url` (a data: address the app made) in a child view
    /// of `parent`, a window's own NSView. JavaScript is off: a sealed page
    /// shown full size is the app's static copy of a diagram, or an agent's
    /// html, which never runs its scripts.
    pub fn sealed(parent: *mut c_void, url: &str) -> Result<Page, String> {
        if super::host::status() != super::host::Status::Ready {
            return Err("the web runtime isn't running".into());
        }
        if parent.is_null() {
            return Err("the window has no view to put the page in".into());
        }
        let mut context = sealed::context().ok_or("the sealed pages' context isn't there")?;
        let guard = sealed::Guard { allowed: Arc::new(Mutex::new(url.to_owned())), on_stop: Arc::new(|| {}) };
        // Alloy (no Chrome toolbar or menus), a native child view: the style
        // Ghostex ships and the spike ran.
        let window_info = WindowInfo { runtime_style: RuntimeStyle::ALLOY, ..WindowInfo::default() }.set_as_child(parent, &Rect { x: 0, y: 0, width: 1, height: 1 });
        let settings = BrowserSettings {
            javascript: State::DISABLED,
            javascript_access_clipboard: State::DISABLED,
            javascript_dom_paste: State::DISABLED,
            javascript_close_windows: State::DISABLED,
            local_storage: State::DISABLED,
            databases_deprecated: State::DISABLED,
            webgl: State::DISABLED,
            ..Default::default()
        };
        let mut client = PageClient::new(sealed::SealedRequests::new(guard), sealed::NoPopups::new(), sealed::QuietConsole::new(), sealed::NoMenu::new(), sealed::NoDownloads::new());
        let browser = browser_host_create_browser_sync(Some(&window_info), Some(&mut client), Some(&CefString::from(url)), Some(&settings), None, Some(&mut context))
            .ok_or("Chromium didn't make the page")?;
        let view = browser.host().ok_or("the page has no host")?.window_handle();
        Ok(Page { browser, view, frame: Cell::new((0., 0., 0., 0.)), hidden: Cell::new(false) })
    }

    /// Places it in a GPUI layout slot: window points, from the top left.
    pub fn set_bounds(&self, x: f32, y: f32, width: f32, height: f32) {
        if self.frame.get() == (x, y, width, height) {
            return;
        }
        self.frame.set((x, y, width, height));
        unsafe { tb_view_set_frame(self.view, f64::from(x), f64::from(y), f64::from(width), f64::from(height)) };
        if let Some(host) = self.browser.host() {
            host.was_resized();
        }
    }

    pub fn set_hidden(&self, hidden: bool) {
        if self.hidden.get() == hidden {
            return;
        }
        self.hidden.set(hidden);
        unsafe { tb_view_set_hidden(self.view, hidden) };
        if let Some(host) = self.browser.host() {
            host.was_hidden(i32::from(hidden));
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        if let Some(host) = self.browser.host() {
            host.close_browser(1);
        }
    }
}

wrap_client! {
    struct PageClient {
        requests: RequestHandler,
        popups: LifeSpanHandler,
        console: DisplayHandler,
        menu: ContextMenuHandler,
        downloads: DownloadHandler,
    }

    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.requests.clone())
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.popups.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.console.clone())
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(self.menu.clone())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(self.downloads.clone())
        }
    }
}
