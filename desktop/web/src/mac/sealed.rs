// The rules every page an agent wrote is shown under: the transcript's
// ```html and ```mermaid boxes and their full-size windows. They follow the
// Swift app's (PreviewSandbox.swift, RichRenderer.swift and
// LocalPreviewSchemeHandler.swift, security-reviewed there), moved onto
// Chromium:
//
// - A request context of its own that keeps nothing (no cache folder), so
//   these pages share no cookies or storage with the panels' sign-ins.
// - No network: every request in that context that isn't a data: or about:
//   address is cancelled before it starts, in one place (the context's
//   handler), and the context's proxy is a port on this Mac nothing listens
//   on, so a connection Chromium makes before asking (a preconnect, a DNS
//   lookup for a link) goes nowhere rather than out. Network prediction is
//   off as well, which covers `<link rel=dns-prefetch>`, as a page's own
//   Content-Security-Policy doesn't.
// - No navigation: each page may show the one address it was given and
//   nothing else, in any frame (Page::allowed); no popups, downloads or new
//   windows.
// - The page's own policy (threadbear_core::rich_pages::POLICY) and, for an
//   html block, JavaScript off altogether are set where the page is made.
use super::host::{set_status, Status};
use cef::*;
use std::cell::RefCell;
use std::os::raw::c_int;
use std::sync::{Arc, Mutex};

thread_local! {
    static CONTEXT: RefCell<Option<RequestContext>> = const { RefCell::new(None) };
}

/// Where a sealed page's connections go: the discard port on this Mac,
/// nothing listening, as WebPanelPool's previewProxy in the Swift app.
pub const DEAD_PROXY: &str = "127.0.0.1:9";

/// Made once Chromium is up; the runtime counts as ready when its rules are on.
pub(crate) fn create_context() {
    // No cache_path: kept in memory, gone with the process.
    let settings = RequestContextSettings::default();
    let mut handler = SealedContextHandler::new();
    match request_context_create_context(Some(&settings), Some(&mut handler)) {
        Some(context) => CONTEXT.with(|c| *c.borrow_mut() = Some(context)),
        None => set_status(Status::Failed("the sealed pages' request context couldn't be made".into())),
    }
}

pub(crate) fn context() -> Option<RequestContext> {
    CONTEXT.with(|c| c.borrow().clone())
}

/// Whether a sealed page may load `url` at all, as a subresource or a frame:
/// only what's written into the page itself.
pub fn may_load(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("data:") || lower == "about:blank" || lower == "about:srcdoc"
}

fn value_string(text: &str) -> Option<Value> {
    let value = value_create()?;
    value.set_string(Some(&CefString::from(text)));
    Some(value)
}

fn set_pref(context: &mut RequestContext, name: &str, value: Option<Value>) {
    let Some(mut value) = value else { return };
    let mut error = CefString::from("");
    if context.set_preference(Some(&CefString::from(name)), Some(&mut value), Some(&mut error)) != 1 {
        // The page's policy and the request handler still stand; this is the
        // third layer, so it's said and not fatal.
        eprintln!("threadbear-web: couldn't set {name} on the sealed context: {error}");
    }
}

fn apply_rules(context: &mut RequestContext) {
    if let (Some(mut proxy), Some(value)) = (dictionary_value_create(), value_create()) {
        proxy.set_string(Some(&CefString::from("mode")), Some(&CefString::from("fixed_servers")));
        proxy.set_string(Some(&CefString::from("server")), Some(&CefString::from(DEAD_PROXY)));
        value.set_dictionary(Some(&mut proxy));
        set_pref(context, "proxy", Some(value));
    }
    // NETWORK_PREDICTION_NEVER: no DNS prefetching or preconnecting.
    if let Some(value) = value_create() {
        value.set_int(2);
        set_pref(context, "net.network_prediction_options", Some(value));
    }
    // WebRTC finds its own way round proxies; only a diagram page runs a
    // script, and it's the app's, but it costs nothing to close.
    set_pref(context, "webrtc.ip_handling_policy", value_string("disable_non_proxied_udp"));
}

wrap_request_context_handler! {
    struct SealedContextHandler;

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, request_context: Option<&mut RequestContext>) {
            if let Some(context) = request_context {
                apply_rules(context);
            }
            set_status(Status::Ready);
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            Some(Blocker::new())
        }
    }
}

wrap_resource_request_handler! {
    pub(crate) struct Blocker;

    impl ResourceRequestHandler {
        fn on_before_resource_load(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _callback: Option<&mut Callback>) -> ReturnValue {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            if may_load(&url) {
                ReturnValue::CONTINUE
            } else {
                ReturnValue::CANCEL
            }
        }
    }
}

/// What a sealed page's handlers share with whoever made it.
#[derive(Clone)]
pub(crate) struct Guard {
    /// The one address the page may show in its main frame: the app's own.
    pub(crate) allowed: Arc<Mutex<String>>,
    /// Said when the page's process stops (a page too big, a crash).
    pub(crate) on_stop: Arc<dyn Fn() + Send + Sync>,
}

wrap_request_handler! {
    pub(crate) struct SealedRequests {
        guard: Guard,
    }

    impl RequestHandler {
        // The page the app gave, in the main frame; in a frame only what the
        // page carries in itself (its policy has frame-src 'none' anyway).
        // Never a link, a form, a refresh or a script's navigation.
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: c_int, _is_redirect: c_int) -> c_int {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let main = frame.is_some_and(|f| f.is_main() == 1);
            let allowed = if main { self.guard.allowed.lock().is_ok_and(|a| *a == url) || url == "about:blank" } else { may_load(&url) };
            if allowed {
                0
            } else {
                1
            }
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: c_int,
            _is_download: c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut c_int>,
        ) -> Option<ResourceRequestHandler> {
            Some(Blocker::new())
        }

        fn on_render_process_terminated(&self, _browser: Option<&mut Browser>, _status: TerminationStatus, _error_code: c_int, _error_string: Option<&CefString>) {
            (self.guard.on_stop)();
        }
    }
}

wrap_life_span_handler! {
    pub(crate) struct NoPopups;

    impl LifeSpanHandler {
        // Nothing opens a window: a sealed page has nowhere to send one.
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: c_int,
            _target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut c_int>,
        ) -> c_int {
            1
        }
    }
}

wrap_display_handler! {
    pub(crate) struct QuietConsole;

    impl DisplayHandler {
        // What a page says to its console stays out of Chromium's log: it can
        // carry the agent's words.
        fn on_console_message(&self, _browser: Option<&mut Browser>, _level: LogSeverity, _message: Option<&CefString>, _source: Option<&CefString>, _line: c_int) -> c_int {
            1
        }
    }
}

wrap_context_menu_handler! {
    pub(crate) struct NoMenu;

    impl ContextMenuHandler {
        // Chromium's own menu has Back, Reload, Print and View Source, none
        // of which a sealed page should offer.
        fn on_before_context_menu(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _params: Option<&mut ContextMenuParams>, model: Option<&mut MenuModel>) {
            if let Some(model) = model {
                model.clear();
            }
        }
    }
}

wrap_download_handler! {
    pub(crate) struct NoDownloads;

    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> c_int {
            0
        }
    }
}
