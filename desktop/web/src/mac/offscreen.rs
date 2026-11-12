// A sealed page drawn with no window (Chromium's windowless rendering), for
// the transcript's diagrams and pages: laid out at the size asked for, asked
// things and pictured through DevTools, which needs no window and no open
// port (the same in-process channel Ghostex uses). As the Swift app's
// offscreen WKWebViews in RichRenderer, with sealed.rs's rules.
use super::sealed;
use cef::*;
use futures::channel::oneshot;
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

/// An answer still to come from Chromium.
pub type Reply<T> = oneshot::Receiver<Result<T, String>>;

/// What the page's handlers share with it. They're called on Chromium's own
/// threads, so it's all behind locks, and none is held while calling CEF.
struct Shared {
    size: Mutex<(i32, i32)>,
    scale: Mutex<f32>,
    /// The one address the page may show; anything else is cancelled.
    allowed: Arc<Mutex<String>>,
    calls: Mutex<HashMap<i32, oneshot::Sender<Result<Json, String>>>>,
    load: Mutex<Option<oneshot::Sender<Result<(), String>>>>,
    /// Waiting for Chromium to paint the view at this size, in pixels.
    painted: Mutex<Option<((i32, i32), oneshot::Sender<Result<(), String>>)>>,
    gone: AtomicBool,
}

impl Shared {
    /// A frame Chromium painted: the one waited for, if it's that size (a
    /// pixel either way, for a scale that doesn't come out whole).
    fn paint(&self, width: i32, height: i32) {
        let Ok(mut waiting) = self.painted.lock() else { return };
        if let Some(((w, h), _)) = waiting.as_ref() {
            if (width - w).abs() <= 1 && (height - h).abs() <= 1 {
                if let Some((_, tx)) = waiting.take() {
                    let _ = tx.send(Ok(()));
                }
            }
        }
    }

    fn fail_all(&self, why: &str) {
        self.gone.store(true, Ordering::Release);
        let calls: Vec<_> = self.calls.lock().map(|mut c| c.drain().map(|(_, tx)| tx).collect()).unwrap_or_default();
        for tx in calls {
            let _ = tx.send(Err(why.to_owned()));
        }
        if let Some(tx) = self.load.lock().ok().and_then(|mut l| l.take()) {
            let _ = tx.send(Err(why.to_owned()));
        }
        if let Some((_, tx)) = self.painted.lock().ok().and_then(|mut p| p.take()) {
            let _ = tx.send(Err(why.to_owned()));
        }
    }

    fn finish_load(&self, result: Result<(), String>) {
        if let Some(tx) = self.load.lock().ok().and_then(|mut l| l.take()) {
            let _ = tx.send(result);
        }
    }
}

/// How a page is made.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Off for an agent's html, as the Swift app has `allowsContentJavaScript`
    /// false; on for the app's own diagram page, which runs Pele.
    pub javascript: bool,
    pub width: i32,
    pub height: i32,
    /// Pixels a CSS pixel, as the screen it's for (2 on Retina).
    pub scale: f32,
}

pub struct Offscreen {
    browser: Browser,
    shared: Arc<Shared>,
    next: AtomicI32,
    _observer: Option<Registration>,
}

impl Offscreen {
    /// Opens a sealed page at `url` (a data: address the app made). The
    /// receiver says when it has loaded, or why it didn't.
    pub fn open(url: &str, options: Options) -> Result<(Offscreen, Reply<()>), String> {
        if super::host::status() != super::host::Status::Ready {
            return Err("the web runtime isn't running".into());
        }
        let mut context = sealed::context().ok_or("the sealed pages' context isn't there")?;
        let (loaded, load) = oneshot::channel();
        let shared = Arc::new(Shared {
            size: Mutex::new((options.width.max(1), options.height.max(1))),
            scale: Mutex::new(options.scale.max(1.)),
            allowed: Arc::new(Mutex::new(url.to_owned())),
            calls: Mutex::new(HashMap::new()),
            load: Mutex::new(Some(loaded)),
            painted: Mutex::new(None),
            gone: AtomicBool::new(false),
        });
        let stopped = shared.clone();
        let guard = sealed::Guard { allowed: shared.allowed.clone(), on_stop: Arc::new(move || stopped.fail_all("its page stopped")) };
        let window_info = WindowInfo::default().set_as_windowless(std::ptr::null_mut());
        let off = if options.javascript { State::ENABLED } else { State::DISABLED };
        let settings = BrowserSettings {
            // Enough to settle a layout quickly; nothing here animates.
            windowless_frame_rate: 30,
            javascript: off,
            // The rest a page could use to keep or pass on anything.
            javascript_access_clipboard: State::DISABLED,
            javascript_dom_paste: State::DISABLED,
            javascript_close_windows: State::DISABLED,
            local_storage: State::DISABLED,
            databases_deprecated: State::DISABLED,
            webgl: State::DISABLED,
            // Transparent, so a diagram sits on the box's own fill.
            background_color: 0,
            ..Default::default()
        };
        let mut client = OffscreenClient::new(
            OffscreenRender::new(shared.clone()),
            sealed::SealedRequests::new(guard),
            Loads::new(shared.clone()),
            sealed::NoPopups::new(),
            sealed::QuietConsole::new(),
            sealed::NoMenu::new(),
            sealed::NoDownloads::new(),
        );
        let browser = browser_host_create_browser_sync(Some(&window_info), Some(&mut client), Some(&CefString::from(url)), Some(&settings), None, Some(&mut context))
            .ok_or("Chromium didn't make the page")?;
        let host = browser.host().ok_or("the page has no host")?;
        let mut observer = Answers::new(shared.clone());
        let registration = host.add_dev_tools_message_observer(Some(&mut observer));
        Ok((Offscreen { browser, shared, next: AtomicI32::new(1), _observer: registration }, load))
    }

    /// A DevTools method on the page (chromedevtools.github.io/devtools-protocol),
    /// its result when it comes.
    pub fn devtools(&self, method: &str, params: Json) -> Reply<Json> {
        let (tx, rx) = oneshot::channel();
        if self.shared.gone.load(Ordering::Acquire) {
            let _ = tx.send(Err("its page stopped".into()));
            return rx;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut calls) = self.shared.calls.lock() {
            calls.insert(id, tx);
        }
        let message = json!({ "id": id, "method": method, "params": params }).to_string();
        let sent = self.browser.host().map(|h| h.send_dev_tools_message(Some(message.as_bytes()))).unwrap_or(0);
        if sent != 1 {
            if let Some(tx) = self.shared.calls.lock().ok().and_then(|mut c| c.remove(&id)) {
                let _ = tx.send(Err(format!("DevTools didn't take {method}")));
            }
        }
        rx
    }

    /// Lays the page out at a new size, in points.
    pub fn resize(&self, width: i32, height: i32) {
        if let Ok(mut size) = self.shared.size.lock() {
            *size = (width.max(1), height.max(1));
        }
        if let Some(host) = self.browser.host() {
            host.was_resized();
        }
    }

    /// Lays the page out `width` by `height` points at `scale` pixels each,
    /// and says when Chromium has painted it that size. A picture is only
    /// ever taken of what's inside the view, so the view is made the size of
    /// the picture first (a page measured in a view a point tall came out
    /// blank before this).
    pub fn resize_and_paint(&self, width: i32, height: i32, scale: f32) -> Reply<()> {
        let (width, height, scale) = (width.max(1), height.max(1), scale.max(0.1));
        let (tx, rx) = oneshot::channel();
        let expected = ((width as f32 * scale).round() as i32, (height as f32 * scale).round() as i32);
        if let Ok(mut waiting) = self.shared.painted.lock() {
            *waiting = Some((expected, tx));
        }
        let scale_changed = self.shared.scale.lock().map(|mut s| std::mem::replace(&mut *s, scale) != scale).unwrap_or(false);
        if let Ok(mut size) = self.shared.size.lock() {
            *size = (width, height);
        }
        if let Some(host) = self.browser.host() {
            if scale_changed {
                host.notify_screen_info_changed();
            }
            host.was_resized();
            // Painted again even if Chromium thinks nothing changed.
            host.invalidate(PaintElementType::VIEW);
        }
        rx
    }

    /// Shows another of the app's own pages in it.
    pub fn load(&self, url: &str) -> Reply<()> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut allowed) = self.shared.allowed.lock() {
            *allowed = url.to_owned();
        }
        if let Ok(mut load) = self.shared.load.lock() {
            *load = Some(tx);
        }
        if let Some(frame) = self.browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
        rx
    }

    pub fn stopped(&self) -> bool {
        self.shared.gone.load(Ordering::Acquire)
    }
}

impl Drop for Offscreen {
    fn drop(&mut self) {
        self.shared.gone.store(true, Ordering::Release);
        if let Some(host) = self.browser.host() {
            host.close_browser(1);
        }
    }
}

/// A PNG of the top-left `width` by `height` CSS pixels, at the page's
/// scale, taken from what Chromium drew.
pub async fn picture(page: &Offscreen, width: f64, height: f64) -> Result<Vec<u8>, String> {
    picture_scaled(page, width, height, 1.).await
}

/// The same, at `scale` of the page's own (below 1 for a big one), taking in
/// what lies beyond the view as well.
pub(crate) async fn picture_scaled(page: &Offscreen, width: f64, height: f64, scale: f64) -> Result<Vec<u8>, String> {
    let shot = page
        .devtools("Page.captureScreenshot", json!({ "format": "png", "fromSurface": true, "captureBeyondViewport": true, "clip": { "x": 0, "y": 0, "width": width, "height": height, "scale": scale } }))
        .await
        .map_err(|_| "the page went away".to_owned())??;
    let data = shot["data"].as_str().ok_or("the picture came back empty")?;
    base64(data).ok_or_else(|| "the picture couldn't be read".to_owned())
}

/// Standard base64, for a page's bytes in a data: address.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A PNG's size, from its header.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((width, height))
}

/// Standard base64, as DevTools sends pictures.
fn base64(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let trimmed = bytes.iter().rposition(|b| *b != b'=').map_or(&bytes[..0], |end| &bytes[..=end]);
    let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in trimmed {
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

wrap_client! {
    struct OffscreenClient {
        render: RenderHandler,
        requests: RequestHandler,
        loads: LoadHandler,
        popups: LifeSpanHandler,
        console: DisplayHandler,
        menu: ContextMenuHandler,
        downloads: DownloadHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.requests.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.loads.clone())
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

wrap_render_handler! {
    struct OffscreenRender {
        shared: Arc<Shared>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            let (width, height) = self.shared.size.lock().map(|s| *s).unwrap_or((1, 1));
            if let Some(rect) = rect {
                *rect = Rect { x: 0, y: 0, width, height };
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, screen_info: Option<&mut ScreenInfo>) -> c_int {
            let Some(info) = screen_info else { return 0 };
            let (width, height) = self.shared.size.lock().map(|s| *s).unwrap_or((1, 1));
            info.device_scale_factor = self.shared.scale.lock().map(|s| *s).unwrap_or(2.);
            info.rect = Rect { x: 0, y: 0, width, height };
            info.available_rect = info.rect.clone();
            1
        }

        // The pictures come from DevTools; a frame only says the view has
        // been painted at the size a picture's waiting for.
        fn on_paint(&self, _browser: Option<&mut Browser>, type_: PaintElementType, _dirty_rects: Option<&[Rect]>, _buffer: *const u8, width: c_int, height: c_int) {
            if type_ == PaintElementType::VIEW {
                self.shared.paint(width, height);
            }
        }
    }
}

wrap_load_handler! {
    struct Loads {
        shared: Arc<Shared>,
    }

    impl LoadHandler {
        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _http_status_code: c_int) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                self.shared.finish_load(Ok(()));
            }
        }

        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _error_code: Errorcode, error_text: Option<&CefString>, _failed_url: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                let why = error_text.map(|t| t.to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| "it didn't load".into());
                self.shared.finish_load(Err(why));
            }
        }
    }
}

wrap_dev_tools_message_observer! {
    struct Answers {
        shared: Arc<Shared>,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, message_id: c_int, success: c_int, result: Option<&[u8]>) {
            let Some(tx) = self.shared.calls.lock().ok().and_then(|mut c| c.remove(&message_id)) else { return };
            let parsed = result.and_then(|bytes| serde_json::from_slice::<Json>(bytes).ok()).unwrap_or(Json::Null);
            let answer = if success == 1 { Ok(parsed) } else { Err(parsed["message"].as_str().unwrap_or("DevTools said no").to_owned()) };
            let _ = tx.send(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_base64_and_png_sizes() {
        for text in ["", "a", "ab", "abc", "hello world", "<p>ünïcode ✓</p>"] {
            assert_eq!(base64(&base64_encode(text.as_bytes())).unwrap(), text.as_bytes(), "{text}");
        }
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64("YQ==").unwrap(), b"a");
        assert!(base64("a$b").is_none());
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&800u32.to_be_bytes());
        png.extend_from_slice(&400u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((800, 400)));
        assert_eq!(png_size(b"nope"), None);
    }
}
