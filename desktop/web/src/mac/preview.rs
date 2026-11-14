// A local page previewed in the pane or on the review window's stage: the
// handlers that hold it to preview_rules.rs. The page is a PanelPage of kind
// Preview (panel.rs), so it's placed, hidden, kept and given the keyboard as a
// web panel's page is, but it lives in the sealed pages' request context (no
// network, keeps nothing: sealed.rs) and every request it makes is answered
// here or cancelled.
use super::panel::Shared;
use crate::preview_rules::{may_browse, may_request, own_path, policy, PreviewSite};
use cef::*;
use std::os::raw::c_int;
use std::sync::{Arc, Mutex};

wrap_request_handler! {
    pub(crate) struct PreviewRequests {
        preview: PreviewSite,
    }

    impl RequestHandler {
        // The main frame shows its page and nothing else; a frame inside it
        // its own origin, or what preview_rules allows.
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: c_int, _is_redirect: c_int) -> c_int {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let main = frame.is_some_and(|f| f.is_main() == 1);
            if may_browse(&url, &self.preview, main) {
                0
            } else {
                1
            }
        }

        // Every request from this view comes here, before the sealed
        // context's own handler: answered from the site, or cancelled.
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
            Some(PreviewResources::new(self.preview.clone()))
        }
    }
}

wrap_resource_request_handler! {
    struct PreviewResources {
        preview: PreviewSite,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _callback: Option<&mut Callback>) -> ReturnValue {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            if may_request(&url, &self.preview) {
                ReturnValue::CONTINUE
            } else {
                ReturnValue::CANCEL
            }
        }

        // The view's own origin is answered from its site, and never reaches
        // the network: a 404 for anything the site won't serve.
        fn resource_handler(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>) -> Option<ResourceHandler> {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let path = own_path(&url, &self.preview.origin)?;
            Some(Served::new(self.preview.clone(), path, Arc::new(Mutex::new(Answer::default()))))
        }

        // Nothing of the page's is kept, sent or read back as a cookie.
        fn cookie_access_filter(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>) -> Option<CookieAccessFilter> {
            Some(NoCookies::new())
        }
    }
}

wrap_cookie_access_filter! {
    struct NoCookies;

    impl CookieAccessFilter {
        fn can_send_cookie(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, _cookie: Option<&Cookie>) -> c_int {
            0
        }
        fn can_save_cookie(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, _response: Option<&mut Response>, _cookie: Option<&Cookie>) -> c_int {
            0
        }
    }
}

/// One answer being given: the bytes and how far they've been read.
#[derive(Default)]
struct Answer {
    status: i32,
    mime: String,
    data: Vec<u8>,
    sent: usize,
}

wrap_resource_handler! {
    struct Served {
        preview: PreviewSite,
        path: String,
        answer: Arc<Mutex<Answer>>,
    }

    impl ResourceHandler {
        // Read here, on Chromium's IO thread, never the app's main one: a
        // page of many megabytes is read for what it names (the Swift app's
        // serving held the app until it moved off the main thread).
        fn open(&self, _request: Option<&mut Request>, handle_request: Option<&mut c_int>, _callback: Option<&mut Callback>) -> c_int {
            let answer = match self.preview.site.serve(&self.path) {
                Some(served) => Answer { status: 200, mime: served.mime, data: served.data, sent: 0 },
                None => Answer { status: 404, mime: "text/plain".into(), data: b"Not available to this preview.".to_vec(), sent: 0 },
            };
            if let Ok(mut now) = self.answer.lock() {
                *now = answer;
            }
            if let Some(handle) = handle_request {
                *handle = 1;
            }
            1
        }

        fn response_headers(&self, response: Option<&mut Response>, response_length: Option<&mut i64>, _redirect_url: Option<&mut CefString>) {
            let Ok(answer) = self.answer.lock() else { return };
            if let Some(response) = response {
                response.set_status(answer.status);
                response.set_status_text(Some(&CefString::from(if answer.status == 200 { "OK" } else { "Not Found" })));
                let (mime, charset) = answer.mime.split_once("; charset=").unwrap_or((answer.mime.as_str(), ""));
                response.set_mime_type(Some(&CefString::from(mime)));
                if !charset.is_empty() {
                    response.set_charset(Some(&CefString::from(charset)));
                }
                let header = |name: &str, value: &str| response.set_header_by_name(Some(&CefString::from(name)), Some(&CefString::from(value)), 1);
                // The page can't loosen it, and a 404's carries the stricter one.
                header("Content-Security-Policy", &policy(self.preview.scripts || answer.status != 200));
                header("Permissions-Policy", crate::preview_rules::PERMISSIONS);
                header("X-Content-Type-Options", "nosniff");
                // A rewritten picture is asked for again, and nothing's kept.
                header("Cache-Control", "no-store");
                header("Referrer-Policy", "no-referrer");
            }
            if let Some(length) = response_length {
                *length = answer.data.len() as i64;
            }
        }

        fn read(&self, data_out: *mut u8, bytes_to_read: c_int, bytes_read: Option<&mut c_int>, _callback: Option<&mut ResourceReadCallback>) -> c_int {
            let Ok(mut answer) = self.answer.lock() else { return 0 };
            let left = answer.data.len().saturating_sub(answer.sent);
            let n = left.min(usize::try_from(bytes_to_read).unwrap_or(0));
            if n > 0 && !data_out.is_null() {
                // Safety: Chromium hands a buffer of bytes_to_read bytes.
                unsafe { std::ptr::copy_nonoverlapping(answer.data.as_ptr().add(answer.sent), data_out, n) };
                answer.sent += n;
            }
            if let Some(read) = bytes_read {
                *read = n as c_int;
            }
            // Nothing left: the answer's complete.
            i32::from(n > 0)
        }

        fn cancel(&self) {}
    }
}

wrap_life_span_handler! {
    pub(crate) struct PreviewLife {
        shared: Arc<Shared>,
    }

    impl LifeSpanHandler {
        // No new windows: a preview has nowhere to send one.
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

        // Never the window it's in (PanelLife says why).
        fn do_close(&self, _browser: Option<&mut Browser>) -> c_int {
            1
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            self.shared.fail_calls("its page closed");
        }
    }
}

wrap_drag_handler! {
    pub(crate) struct NoDrops;

    impl DragHandler {
        // Nothing is dropped onto a preview: a file dropped on a page is
        // Chromium's to load, and a preview loads only its own page.
        fn on_drag_enter(&self, _browser: Option<&mut Browser>, _drag_data: Option<&mut DragData>, _mask: DragOperationsMask) -> c_int {
            1
        }
    }
}
