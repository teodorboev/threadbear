//! macOS representations that GPUI's clipboard and filename-only drops omit.
//! The app captures temporary bytes during the drop. Ordinary file opens
//! and copies run in this app's worker; fallback bytes are kept before the
//! drop returns. Promises and URL-only browser
//! images get a dim chip while they are loading.
use std::path::PathBuf;
use futures::channel::oneshot;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Owns a completed handover even while it sits unread in a channel. If a
/// timeout wins concurrently, dropping the queued reply deletes its copies.
#[derive(Default)]
pub(crate) struct PreparedPictures { pub(crate) paths: Vec<PathBuf> }
impl Drop for PreparedPictures {
    fn drop(&mut self) { for path in &self.paths { crate::composer::unstage(path); } }
}
struct Pending {
    rx: oneshot::Receiver<Result<PreparedPictures, String>>,
    #[cfg(target_os = "macos")]
    _promise: Option<PromiseLease>,
    #[cfg(target_os = "macos")]
    alternative: Option<PromiseAlternative>,
    #[cfg(target_os = "macos")]
    image: Option<ImageFallback>,
    #[cfg(target_os = "macos")]
    opened: Option<oneshot::Receiver<()>>,
}
#[cfg(target_os = "macos")]
struct PromiseAlternative {
    path: PathBuf,
    rx: oneshot::Receiver<Result<PreparedPictures, String>>,
    _promise: Option<PromiseLease>,
}
#[cfg(target_os = "macos")]
impl PromiseAlternative {
    fn new(path: PathBuf, pending: Pending) -> Self { Self { path, rx: pending.rx, _promise: pending._promise } }
}
#[cfg(target_os = "macos")]
impl Drop for PromiseAlternative {
    fn drop(&mut self) { crate::composer::unstage(&self.path); }
}
static PENDING: LazyLock<Mutex<HashMap<PathBuf, Pending>>> = LazyLock::new(Default::default);

pub(crate) async fn ready(path: &std::path::Path, grace: impl std::future::Future<Output = ()>) -> Result<PreparedPictures, String> {
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
    let Some(mut pending) = pending else { return Ok(PreparedPictures { paths: vec![path.to_owned()] }) };
    #[cfg(target_os = "macos")]
    if pending.image.is_some() || pending.alternative.is_some() {
        let mut grace = Some(grace);
        if let Some(opened) = pending.opened.take() {
            let open = async { opened.await.is_ok() };
            let deadline = grace.take().unwrap();
            futures::pin_mut!(open, deadline);
            if let futures::future::Either::Left((true, _)) = futures::future::select(open, deadline).await {
                // Once the file is open, let its full-quality copy finish.
                // A slow copy mustn't silently select a smaller preview.
                return match receipt(&mut pending.rx).await {
                    Ok(pictures) => Ok(pictures),
                    Err(error) => read_alternative(&mut pending.image, pending.alternative.as_mut(), futures::future::ready(())).await.or(Err(error)),
                };
            }
        }
        let file = receipt(&mut pending.rx);
        let alternative = read_alternative(&mut pending.image, pending.alternative.as_mut(), async { if let Some(grace) = grace { grace.await; } });
        futures::pin_mut!(file, alternative);
        // Prefer the original file briefly. A stalled volume mustn't hide
        // bytes or a promise already captured during the drop. If both fail,
        // retain the file's named error instead of a generic promise error.
        return match futures::future::select(file, alternative).await {
            futures::future::Either::Left((Ok(pictures), _)) => Ok(pictures),
            futures::future::Either::Left((Err(error), alternative)) => alternative.await.or(Err(error)),
            futures::future::Either::Right((Ok(pictures), _)) => Ok(pictures),
            futures::future::Either::Right((Err(_), file)) => file.await,
        };
    }
    let _ = grace;
    receipt(&mut pending.rx).await
}

async fn receipt(rx: &mut oneshot::Receiver<Result<PreparedPictures, String>>) -> Result<PreparedPictures, String> {
    rx.await.unwrap_or_else(|_| Err("The picture source closed before it finished".into()))
}

#[cfg(target_os = "macos")]
async fn read_alternative(image: &mut Option<ImageFallback>, promise: Option<&mut PromiseAlternative>, grace: impl std::future::Future<Output = ()>) -> Result<PreparedPictures, String> {
    grace.await;
    let mut image_error = None;
    if let Some(image) = image.take() {
        let result = receipt(&mut copy_in_background(move || save_image(image))).await;
        match result {
            Ok(pictures) => return Ok(pictures),
            Err(error) => image_error = Some(error),
        }
    }
    if let Some(promise) = promise {
        let result = receipt(&mut promise.rx).await;
        return match image_error { Some(error) => result.or(Err(error)), None => result };
    }
    Err(image_error.unwrap_or_else(|| "Couldn't read the picture".into()))
}

pub(crate) fn install(cx: &mut gpui_kit::App) {
    #[cfg(target_os = "macos")]
    cx.observe_new::<gpui_kit::base::Root>(|_, window, _| {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Some(window) = window {
            if let Ok(handle) = HasWindowHandle::window_handle(window) {
                if let RawWindowHandle::AppKit(handle) = handle.as_raw() {
                    // SAFETY: the new window's live NSView, on the main thread.
                    unsafe { tb_composer_install(handle.ns_view.as_ptr()); }
                }
            }
        }
    }).detach();
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

/// The image-only attachment picker. AppKit filters the sheet by type;
/// elsewhere GPUI's unfiltered picker is checked before anything is added.
pub(crate) fn choose_images(window: &gpui_kit::Window, cx: &mut gpui_kit::App) -> impl std::future::Future<Output = Result<Option<Vec<PathBuf>>, String>> {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let _ = cx;
        let (send, receive) = oneshot::channel::<Option<Vec<PathBuf>>>();
        match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::AppKit(handle)) => {
                // The sheet owns this sender until its single completion;
                // cancelling our task only closes the receiving end.
                let context = Box::into_raw(Box::new(send)).cast();
                unsafe { tb_composer_choose_images(handle.ns_view.as_ptr(), image_picker_finished, context); }
            }
            _ => { drop(send); }
        }
        async move { receive.await.map_err(|_| "Couldn't open the image picker".to_owned())?.map(image_selection).transpose() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        let selected = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Add".into()) });
        async move {
            selected.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string())?.map(image_selection).transpose()
        }
    }
}

fn image_selection(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    if paths.iter().any(|path| !crate::composer::is_picture(path)) {
        return Err("Choose image files to attach.".to_owned());
    }
    Ok(paths)
}

#[cfg(target_os = "macos")]
extern "C" fn image_picker_finished(context: *mut std::ffi::c_void, names: *const *const std::ffi::c_char, count: usize) {
    // AppKit calls once, while every UTF-8 name is still alive. Copy the
    // paths before releasing the sender, even if the composer went away.
    let send = unsafe { Box::from_raw(context.cast::<oneshot::Sender<Option<Vec<PathBuf>>>>()) };
    let paths = if names.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(names, count) }.iter().map(|name| {
            PathBuf::from(unsafe { std::ffi::CStr::from_ptr(*name) }.to_string_lossy().into_owned())
        }).collect())
    };
    let _ = send.send(paths);
}

pub(crate) fn read(drag: bool) -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        let mut captured = Captured::default();
        // SAFETY: read calls collect only before returning. Promises get
        // their own owned callback context, not this stack pointer.
        let taken = unsafe { tb_composer_read(drag, collect, &mut captured as *mut _ as *mut _) };
        taken.then_some(captured.paths)
    }
    #[cfg(not(target_os = "macos"))]
    { let _ = drag; None }
}

#[cfg(target_os = "macos")]
type Sink = extern "C" fn(*mut std::ffi::c_void, i32, *const std::ffi::c_char, *const std::ffi::c_void, usize) -> bool;
#[cfg(target_os = "macos")]
extern "C" {
    fn tb_composer_choose_images(view: *mut std::ffi::c_void, finished: extern "C" fn(*mut std::ffi::c_void, *const *const std::ffi::c_char, usize), context: *mut std::ffi::c_void);
    fn tb_composer_install(view: *mut std::ffi::c_void);
    fn tb_composer_read(drag: bool, sink: Sink, context: *mut std::ffi::c_void) -> bool;
    fn tb_composer_promise(object: *const std::ffi::c_void, destination: *const std::ffi::c_char, sink: Sink, context: *mut std::ffi::c_void);
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct Captured {
    paths: Vec<PathBuf>,
    // AppKit requires all receivers in a drop to share one destination.
    promise_folder: Option<std::sync::Arc<tempfile::TempDir>>,
}

#[cfg(target_os = "macos")]
extern "C" fn collect(context: *mut std::ffi::c_void, kind: i32, name: *const std::ffi::c_char, bytes: *const std::ffi::c_void, len: usize) -> bool {
    use crate::composer::{keep_picture, paste_base, stage_private};
    use std::ffi::CStr;
    // SAFETY: read owns Captured for this synchronous call. AppKit supplies
    // valid UTF-8 strings and byte buffers for the duration of the callback.
    let captured = unsafe { &mut *context.cast::<Captured>() };
    let name = if name.is_null() { String::new() } else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() };
    let path = match kind {
        0 => Some(keep_picture(PathBuf::from(name))),
        5 => {
            // SAFETY: AppKit keeps this struct and its strings/data alive
            // through this call only. Copy the fallback before spawning IO.
            let raw = if !bytes.is_null() && len == std::mem::size_of::<RawImageFallback>() {
                Some(unsafe { &*bytes.cast::<RawImageFallback>() })
            } else { None };
            let fallback = raw.and_then(|raw| {
                if raw.length > 0 && raw.length <= 100 * 1024 * 1024 && !raw.bytes.is_null() && !raw.extension.is_null() {
                    Some(ImageFallback {
                        extension: unsafe { CStr::from_ptr(raw.extension) }.to_string_lossy().into_owned(),
                        bytes: unsafe { std::slice::from_raw_parts(raw.bytes.cast::<u8>(), raw.length) }.into(),
                    })
                } else { None }
            });
            let original = std::path::Path::new(&name);
            let path = load_file(original, fallback);
            if let Some(object) = raw.map(|raw| raw.promise).filter(|p| !p.is_null()) {
                // Receipt must begin during this native callback. Keep the
                // receiver as an owned alternative, never an AppKit pointer
                // on the file worker. A good file wins even if its promise
                // is withdrawn; a failed open can still use the promise.
                if path.is_none() { return start_promise(captured, object).is_some_and(|path| { captured.paths.push(path); true }); }
                if !crate::composer::transient_picture(original) {
                    if let Some(promised) = start_promise(captured, object) {
                        let receiver = PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(&promised);
                        let mut alternative = receiver.map(|pending| PromiseAlternative::new(promised, pending));
                        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(file) = path.as_ref().and_then(|path| pending.get_mut(path)) {
                            file.alternative = alternative.take();
                        }
                        drop(pending);
                        drop(alternative); // clean an unused receiver outside the lock
                    }
                }
            }
            path
        },
        1 if len > 0 => {
            let ext = match name.to_ascii_lowercase().as_str() {
                "png" => "png", "jpeg" | "jpg" => "jpeg", "heic" => "heic", "heif" => "heif", "gif" => "gif", "webp" => "webp", _ => "png",
            };
            let data = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) };
            stage_private(&paste_base(), &format!("pasted.{ext}"), data)
        }
        2 => load_url(name),
        3 => start_promise(captured, bytes),
        _ => None,
    };
    if let Some(path) = path { captured.paths.push(path); true } else { false }
}

#[cfg(target_os = "macos")]
fn start_promise(captured: &mut Captured, object: *const std::ffi::c_void) -> Option<PathBuf> {
    if captured.promise_folder.is_none() {
        captured.promise_folder = crate::composer::private_folder(&crate::composer::paste_base()).map(std::sync::Arc::new);
    }
    let source = captured.promise_folder.clone()?;
    let path = crate::composer::stage_private(&crate::composer::paste_base(), "promised.png", b"")?;
    let (id, pending) = register_promise(path.clone(), source.clone());
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), pending);
    let folder = std::ffi::CString::new(source.path().to_string_lossy().as_bytes()).unwrap();
    // SAFETY: collect's synchronous native callback keeps the object live.
    unsafe { tb_composer_promise(object, folder.as_ptr(), promised, id as *mut _); }
    Some(path)
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct RawImageFallback {
    extension: *const std::ffi::c_char,
    bytes: *const std::ffi::c_void,
    length: usize,
    promise: *const std::ffi::c_void,
}
#[cfg(target_os = "macos")]
struct ImageFallback { extension: String, bytes: std::sync::Arc<[u8]> }

#[cfg(target_os = "macos")]
fn load_file(path: &std::path::Path, fallback: Option<ImageFallback>) -> Option<PathBuf> {
    // A screenshot's source can disappear as the drop returns. Other files
    // may be on an unavailable share: even canonicalize/open must run off
    // the UI thread. Both opens still happen in this app, not the runner.
    if crate::composer::transient_picture(path) {
        return prepare_file(path, fallback, None).ok();
    }
    #[cfg(feature = "automation")]
    let slow = FIXTURE_SLOW_FILE.lock().unwrap().as_deref() == Some(path);
    let original = path.to_owned();
    let (opened, opened_rx) = oneshot::channel();
    let path = file_later(move || {
        #[cfg(feature = "automation")]
        if slow { std::thread::sleep(std::time::Duration::from_secs(8)); }
        prepare_file(&original, None, Some(opened))
    })?;
    // A blocked open only owns a path. Cancellation can release these bytes
    // immediately, and the ready future can use them before the open returns.
    if let Some(pending) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&path) {
        pending.image = fallback;
        pending.opened = Some(opened_rx);
    }
    Some(path)
}

#[cfg(target_os = "macos")]
fn prepare_file(path: &std::path::Path, fallback: Option<ImageFallback>, opened: Option<oneshot::Sender<()>>) -> Result<PathBuf, String> {
    match threadbear_core::open_policy::local_open(path) {
        Ok(file) => {
            if !file.metadata().map_err(|e| file_read_error(path, e))?.is_file() { return Err(format!("Not a file: {}", path.file_name().unwrap_or_default().to_string_lossy())); }
            if let Some(opened) = opened { let _ = opened.send(()); }
            crate::composer::snapshot_picture(&file, path).map_err(|e| e.to_string())
        }
        Err(error) => match fallback {
            Some(image) => save_image(image),
            None => Err(file_read_error(path, error)),
        },
    }
}

#[cfg(target_os = "macos")]
fn file_read_error(path: &std::path::Path, error: std::io::Error) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match error.kind() {
        std::io::ErrorKind::NotFound => format!("The file went away before it could be added: {name}"),
        std::io::ErrorKind::PermissionDenied => format!("Threadbear isn't allowed to read {name}"),
        _ => format!("Couldn't read {name}: {error}"),
    }
}

#[cfg(target_os = "macos")]
fn save_image(image: ImageFallback) -> Result<PathBuf, String> {
    crate::composer::stage_private(&crate::composer::paste_base(), &format!("pasted.{}", image.extension), &image.bytes)
        .ok_or_else(|| "Couldn't keep the pasted picture".into())
}

#[cfg(target_os = "macos")]
fn copy_in_background(copy: impl FnOnce() -> Result<PathBuf, String> + Send + 'static) -> oneshot::Receiver<Result<PreparedPictures, String>> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let result = copy().map(|kept| PreparedPictures { paths: vec![kept] });
        // An unread reply owns its snapshot too, including after a timeout.
        let _ = tx.send(result);
    });
    rx
}

#[cfg(target_os = "macos")]
fn file_later(copy: impl FnOnce() -> Result<PathBuf, String> + Send + 'static) -> Option<PathBuf> {
    let path = crate::composer::stage_private(&crate::composer::paste_base(), "file.png", b"")?;
    let rx = copy_in_background(copy);
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), Pending { rx, _promise: None, alternative: None, image: None, opened: None });
    Some(path)
}

#[cfg(target_os = "macos")]
struct Promise {
    path: PathBuf,
    prepared: PreparedPictures,
    tx: Option<oneshot::Sender<Result<PreparedPictures, String>>>,
    _source: std::sync::Arc<tempfile::TempDir>,
}
#[cfg(target_os = "macos")]
static PROMISES: LazyLock<Mutex<HashMap<usize, Promise>>> = LazyLock::new(Default::default);
#[cfg(target_os = "macos")]
static NEXT_PROMISE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// The waiting future owns receipt, including any partial snapshots. A
/// timeout or closed window drops it even if the source never calls back.
#[cfg(target_os = "macos")]
struct PromiseLease(usize);
#[cfg(target_os = "macos")]
impl Drop for PromiseLease {
    fn drop(&mut self) {
        let promise = PROMISES.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
        drop(promise); // release the lock before removing private files
    }
}

#[cfg(target_os = "macos")]
fn register_promise(path: PathBuf, source: std::sync::Arc<tempfile::TempDir>) -> (usize, Pending) {
    let id = NEXT_PROMISE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    PROMISES.lock().unwrap_or_else(|e| e.into_inner()).insert(id, Promise { path, prepared: PreparedPictures::default(), tx: Some(tx), _source: source });
    (id, Pending { rx, _promise: Some(PromiseLease(id)), alternative: None, image: None, opened: None })
}

#[cfg(target_os = "macos")]
extern "C" fn promised(context: *mut std::ffi::c_void, kind: i32, name: *const std::ffi::c_char, _: *const std::ffi::c_void, _: usize) -> bool {
    // This opaque value is an ID, never a dereferenced pointer. AppKit can
    // deliver after cancellation; a removed ID simply ignores that callback.
    let id = context as usize;
    if kind == 4 {
        let promise = PROMISES.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        let Some(mut promise) = promise else { return false };
        let prepared = std::mem::take(&mut promise.prepared);
        let result = if prepared.paths.is_empty() { Err("Couldn't read the promised picture".into()) } else { Ok(prepared) };
        if let Some(tx) = promise.tx.take() { let _ = tx.send(result); }
        return true;
    }
    if kind == 6 && !name.is_null() {
        // SAFETY: AppKit's file path is valid UTF-8 for this reader callback.
        let original = PathBuf::from(unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned());
        return retain_promised(id, original, crate::composer::keep_picture);
    }
    false
}

#[cfg(target_os = "macos")]
fn retain_promised(id: usize, original: PathBuf, keep: impl FnOnce(PathBuf) -> PathBuf) -> bool {
    let placeholder = PROMISES.lock().unwrap_or_else(|e| e.into_inner()).get(&id).map(|p| p.path.clone());
    let Some(placeholder) = placeholder else { return false };
    if !crate::composer::is_staged(&placeholder) { return false; }
    // No registry lock spans IO: a slow source mustn't hold up the main
    // thread's cancellation or the registration of a different drop.
    let kept = keep(original.clone());
    if kept == original { return false; }
    let mut copy = PreparedPictures { paths: vec![kept] };
    let mut promises = PROMISES.lock().unwrap_or_else(|e| e.into_inner());
    let accepted = if let Some(promise) = promises.get_mut(&id) {
        promise.prepared.paths.append(&mut copy.paths);
        true
    } else { false };
    drop(promises);
    // If cancellation won during the copy, its unclaimed snapshot goes now.
    accepted
}

#[cfg(target_os = "macos")]
fn load_url(url: String) -> Option<PathBuf> {
    use crate::composer::{paste_base, stage_private};
    let path = stage_private(&paste_base(), "browser.png", b"")?;
    let (tx, rx) = oneshot::channel();
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), Pending { rx, _promise: None, alternative: None, image: None, opened: None });
    let destination = path.clone();
    #[cfg(feature = "automation")]
    let fixture = fixture_address(&url);
    std::thread::spawn(move || {
        let result = (|| {
            let resolver: crate::composer_url::PublicResolver = Default::default();
            #[cfg(feature = "automation")]
            let resolver = resolver.allow_fixture(fixture);
            let agent = crate::composer_url::agent(resolver);
            let mut response = agent.get(&url).call().map_err(|e| e.to_string())?;
            let data = response.body_mut().with_config().limit(25 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
            // Sniff the bytes before handing them on. A webpage's URL must
            // fail as a picture, rather than importing HTML under a PNG name.
            image::guess_format(&data).map_err(|_| "That link didn't return a supported picture".to_string())?;
            crate::composer::write_staged(&destination, &data).map_err(|e| e.to_string())
        })();
        let _ = tx.send(result.map(|()| PreparedPictures { paths: vec![destination] }));
    });
    Some(path)
}

#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) fn fixture_begin(kind: &str, path: &std::path::Path) -> Result<(), String> {
    use std::ffi::CString;
    extern "C" { fn tb_composer_fixture_begin(kind: *const std::ffi::c_char, path: *const std::ffi::c_char, ended: extern "C" fn()); }
    fixture_finished();
    let permitted = (kind == "url-fixture").then(|| path.to_string_lossy().into_owned());
    let slow = matches!(kind, "slow-file" | "slow-file-bytes").then(|| path.to_owned());
    let (kind, path) = (CString::new(kind).map_err(|e| e.to_string())?, CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?);
    unsafe { tb_composer_fixture_begin(kind.as_ptr(), path.as_ptr(), fixture_finished); }
    *FIXTURE_URL.lock().unwrap() = permitted;
    *FIXTURE_SLOW_FILE.lock().unwrap() = slow;
    Ok(())
}
#[cfg(all(feature = "automation", target_os = "macos"))]
static FIXTURE_URL: Mutex<Option<String>> = Mutex::new(None);
#[cfg(all(feature = "automation", target_os = "macos"))]
static FIXTURE_SLOW_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(all(feature = "automation", target_os = "macos"))]
extern "C" fn fixture_finished() {
    *FIXTURE_URL.lock().unwrap() = None;
    *FIXTURE_SLOW_FILE.lock().unwrap() = None;
}
#[cfg(all(feature = "automation", target_os = "macos"))]
fn fixture_address(url: &str) -> Option<std::net::SocketAddr> {
    let fixture = FIXTURE_URL.lock().unwrap();
    if fixture.as_deref() != Some(url) { return None; }
    let uri: ureq::http::Uri = url.parse().ok()?;
    let address: std::net::SocketAddr = uri.authority()?.as_str().parse().ok()?;
    address.ip().is_loopback().then_some(address)
}
#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) fn fixture_end() {
    extern "C" { fn tb_composer_fixture_end(); }
    unsafe { tb_composer_fixture_end(); }
}
#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) fn fixture_drag(window: &gpui_kit::Window, phase: i32, vanish: Option<&std::path::Path>) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    extern "C" { fn tb_composer_fixture_drag(view: *mut std::ffi::c_void, phase: i32, vanish: *const std::ffi::c_char); }
    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return Err("no AppKit window".into()) };
    let vanish = vanish.map(|p| std::ffi::CString::new(p.to_string_lossy().as_bytes()).unwrap());
    unsafe { tb_composer_fixture_drag(handle.ns_view.as_ptr(), phase, vanish.as_ref().map_or(std::ptr::null(), |p| p.as_ptr())); }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn image_picker_does_not_turn_a_nonimage_into_a_file_chip() {
        let image = std::path::PathBuf::from("photo.HEIC");
        let document = std::path::PathBuf::from("notes.md");
        assert_eq!(super::image_selection(vec![image.clone()]).unwrap(), vec![image.clone()]);
        assert!(super::image_selection(vec![image, document]).is_err());
        assert!(super::image_selection(vec!["photo.png.command".into()]).is_err());
        assert!(super::image_selection(vec!["drawing.svg".into()]).is_err());
        assert!(super::image_selection(vec!["icon.ico".into()]).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn image_picker_cancellation_and_a_closed_composer_release_the_callback() {
        let (send, receive) = super::oneshot::channel::<Option<Vec<std::path::PathBuf>>>();
        super::image_picker_finished(Box::into_raw(Box::new(send)).cast(), std::ptr::null(), 0);
        assert_eq!(futures::executor::block_on(receive).unwrap(), None);

        let (send, receive) = super::oneshot::channel::<Option<Vec<std::path::PathBuf>>>();
        drop(receive);
        let name = std::ffi::CString::new("/picture.png").unwrap();
        let names = [name.as_ptr()];
        // The native sheet may outlive the composer that opened it.
        super::image_picker_finished(Box::into_raw(Box::new(send)).cast(), names.as_ptr(), names.len());
    }
    async fn wait_ready(path: &std::path::Path) -> Result<super::PreparedPictures, String> {
        super::ready(path, futures::future::pending()).await
    }

    #[cfg(target_os = "macos")]
    fn completed_alternative() -> (super::PromiseAlternative, std::path::PathBuf) {
        let source = std::sync::Arc::new(crate::composer::private_folder(&crate::composer::paste_base()).unwrap());
        let original = source.path().join("photo.png");
        std::fs::write(&original, b"promised picture").unwrap();
        let path = crate::composer::stage_private(&crate::composer::paste_base(), "promise.png", b"").unwrap();
        let (id, pending) = super::register_promise(path.clone(), source);
        assert!(super::retain_promised(id, original, crate::composer::keep_picture));
        let kept = super::PROMISES.lock().unwrap()[&id].prepared.paths[0].clone();
        super::promised(id as *mut _, 4, std::ptr::null(), std::ptr::null(), 0);
        (super::PromiseAlternative::new(path, pending), kept)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_readable_file_releases_the_unused_promises_copies() {
        let (alternative, promised) = completed_alternative();
        let placeholder = alternative.path.clone();
        let canonical = crate::composer::stage_private(&crate::composer::paste_base(), "canonical.png", b"canonical bytes").unwrap();
        let path = super::file_later(move || Ok(canonical)).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().alternative = Some(alternative);
        let pictures = futures::executor::block_on(wait_ready(&path)).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"canonical bytes");
        assert!(!promised.exists() && !placeholder.exists());
        drop(pictures);
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_file_can_use_its_owned_promise() {
        let (alternative, promised) = completed_alternative();
        let placeholder = alternative.path.clone();
        let path = super::file_later(|| Err("file open failed".into())).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().alternative = Some(alternative);
        let pictures = futures::executor::block_on(super::ready(&path, futures::future::ready(()))).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"promised picture");
        assert!(!placeholder.exists());
        drop(pictures);
        assert!(!promised.exists());
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cancelling_a_file_open_also_cancels_its_promise_alternative() {
        use futures::FutureExt;
        let source = std::sync::Arc::new(crate::composer::private_folder(&crate::composer::paste_base()).unwrap());
        let source_path = source.path().to_owned();
        let promised = crate::composer::stage_private(&crate::composer::paste_base(), "promise.png", b"").unwrap();
        let (id, pending) = super::register_promise(promised.clone(), source);
        let (release, wait) = std::sync::mpsc::channel();
        let path = super::file_later(move || { wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); Err("file unavailable".into()) }).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().alternative = Some(super::PromiseAlternative::new(promised.clone(), pending));
        let mut ready = Box::pin(wait_ready(&path));
        assert!(ready.as_mut().now_or_never().is_none());
        drop(ready);
        assert!(!super::PROMISES.lock().unwrap().contains_key(&id));
        assert!(!promised.exists() && !source_path.exists());
        release.send(()).unwrap();
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_waiting_open_can_use_its_captured_bytes_after_grace() {
        let (release, wait) = std::sync::mpsc::channel();
        let path = super::file_later(move || { wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); Err("file unavailable".into()) }).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().image = Some(super::ImageFallback { extension: "png".into(), bytes: std::sync::Arc::from(&b"pasted bytes"[..]) });
        let pictures = futures::executor::block_on(super::ready(&path, futures::future::ready(()))).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"pasted bytes");
        release.send(()).unwrap();
        drop(pictures);
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_open_file_keeps_its_original_bytes_even_if_the_copy_is_slow() {
        use futures::FutureExt;
        let (opened, opened_rx) = futures::channel::oneshot::channel();
        let (started, started_rx) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let path = super::file_later(move || {
            opened.send(()).unwrap();
            started.send(()).unwrap();
            wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            crate::composer::stage_private(&crate::composer::paste_base(), "original.png", b"full resolution").ok_or_else(|| "couldn't copy".into())
        }).unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        {
            let mut pending = super::PENDING.lock().unwrap();
            let pending = pending.get_mut(&path).unwrap();
            pending.opened = Some(opened_rx);
            pending.image = Some(super::ImageFallback { extension: "png".into(), bytes: std::sync::Arc::from(&b"smaller preview"[..]) });
        }
        let mut ready = Box::pin(super::ready(&path, futures::future::ready(())));
        assert!(ready.as_mut().now_or_never().is_none(), "the preview replaced a file whose open had succeeded");
        release.send(()).unwrap();
        let pictures = futures::executor::block_on(ready).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"full resolution");
        drop(pictures);
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_copy_can_still_use_captured_bytes_after_open_succeeds() {
        let (opened, opened_rx) = futures::channel::oneshot::channel();
        let path = super::file_later(move || { let _ = opened.send(()); Err("copy failed".into()) }).unwrap();
        {
            let mut pending = super::PENDING.lock().unwrap();
            let pending = pending.get_mut(&path).unwrap();
            pending.opened = Some(opened_rx);
            pending.image = Some(super::ImageFallback { extension: "png".into(), bytes: std::sync::Arc::from(&b"captured bytes"[..]) });
        }
        let pictures = futures::executor::block_on(wait_ready(&path)).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"captured bytes");
        drop(pictures);
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_waiting_open_can_use_its_ready_promise_after_grace() {
        let (alternative, _) = completed_alternative();
        let (release, wait) = std::sync::mpsc::channel();
        let path = super::file_later(move || { wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); Err("file unavailable".into()) }).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().alternative = Some(alternative);
        let pictures = futures::executor::block_on(super::ready(&path, futures::future::ready(()))).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"promised picture");
        release.send(()).unwrap();
        drop(pictures);
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cancelling_a_waiting_open_releases_its_captured_image_bytes() {
        use futures::FutureExt;
        let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"owned image bytes"[..]);
        let observed = std::sync::Arc::downgrade(&bytes);
        let (release, wait) = std::sync::mpsc::channel();
        let path = super::file_later(move || { wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); Err("file unavailable".into()) }).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().image = Some(super::ImageFallback { extension: "png".into(), bytes });
        let mut ready = Box::pin(wait_ready(&path));
        assert!(ready.as_mut().now_or_never().is_none());
        drop(ready);
        assert!(observed.upgrade().is_none(), "a blocked open kept its image bytes after cancellation");
        release.send(()).unwrap();
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn two_failed_sources_keep_the_files_named_error() {
        let source = std::sync::Arc::new(crate::composer::private_folder(&crate::composer::paste_base()).unwrap());
        let promised = crate::composer::stage_private(&crate::composer::paste_base(), "promise.png", b"").unwrap();
        let (id, pending) = super::register_promise(promised.clone(), source);
        super::promised(id as *mut _, 4, std::ptr::null(), std::ptr::null(), 0);
        let path = super::file_later(|| Err("The file went away before it could be added: photo.png".into())).unwrap();
        super::PENDING.lock().unwrap().get_mut(&path).unwrap().alternative = Some(super::PromiseAlternative::new(promised, pending));
        let error = futures::executor::block_on(super::ready(&path, futures::future::ready(()))).err().unwrap();
        assert_eq!(error, "The file went away before it could be added: photo.png");
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unavailable_files_error_keeps_its_name() {
        let path = std::path::Path::new("/missing/photo.png");
        assert_eq!(super::file_read_error(path, std::io::Error::from_raw_os_error(2)), "The file went away before it could be added: photo.png");
        for code in [1, 13] {
            assert_eq!(super::file_read_error(path, std::io::Error::from_raw_os_error(code)), "Threadbear isn't allowed to read photo.png");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_waiting_file_reader_keeps_its_fallback_off_the_caller_thread() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("unavailable.png");
        let (started, started_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let path = super::file_later(move || {
            started.send(std::thread::current().id()).unwrap();
            release_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            super::prepare_file(&original, Some(super::ImageFallback { extension: "png".into(), bytes: std::sync::Arc::from(&b"owned pasteboard bytes"[..]) }), None)
        }).unwrap();
        assert_ne!(started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), std::thread::current().id());
        use futures::FutureExt;
        let mut ready = Box::pin(wait_ready(&path));
        assert!(ready.as_mut().now_or_never().is_none());
        release.send(()).unwrap();
        let pictures = futures::executor::block_on(ready).unwrap();
        let kept = pictures.paths[0].clone();
        assert_eq!(std::fs::read(&kept).unwrap(), b"owned pasteboard bytes");
        drop(pictures);
        assert!(!kept.exists());
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_open_file_copy_preserves_its_error() {
        let file = tempfile::tempfile().unwrap();
        let path = super::file_later(move || crate::composer::snapshot_picture(&file, std::path::Path::new("")).map_err(|e| e.to_string())).unwrap();
        let error = futures::executor::block_on(wait_ready(&path)).err().unwrap();
        assert!(error.contains("filename"), "{error}");
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_open_file_can_be_copied_after_its_name_disappears() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("original.png");
        std::fs::write(&original, b"the original image bytes").unwrap();
        let file = threadbear_core::open_policy::local_open(&original).unwrap();
        std::fs::remove_file(&original).unwrap();
        let path = super::file_later(move || crate::composer::snapshot_picture(&file, &original).map_err(|e| e.to_string())).unwrap();
        let pictures = futures::executor::block_on(wait_ready(&path)).unwrap();
        assert_eq!(std::fs::read(&pictures.paths[0]).unwrap(), b"the original image bytes");
        drop(pictures);
        crate::composer::unstage(&path);
    }

    #[cfg(all(feature = "automation", target_os = "macos"))]
    #[test]
    fn ending_a_fixture_clears_its_url_exception() {
        let url = "http://127.0.0.1:12345/picture.png";
        *super::FIXTURE_URL.lock().unwrap() = Some(url.into());
        assert!(super::fixture_address(url).is_some());
        super::fixture_finished();
        assert!(super::fixture_address(url).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cancellation_during_a_copy_does_not_block_or_keep_the_copy() {
        let base = tempfile::tempdir().unwrap();
        let original = base.path().join("original.png");
        std::fs::write(&original, b"the user's original picture").unwrap();
        let source = std::sync::Arc::new(crate::composer::private_folder(base.path()).unwrap());
        let path = crate::composer::stage_private(base.path(), "promise.png", b"").unwrap();
        let (id, pending) = super::register_promise(path.clone(), source);
        let mut copied = None;
        assert!(!super::retain_promised(id, original.clone(), |file| {
            assert!(super::PROMISES.try_lock().is_ok(), "a copy held the promise registry lock");
            drop(pending); // cancellation between checking the ID and finishing IO
            let kept = crate::composer::keep_picture(file);
            copied = Some(kept.clone());
            kept
        }));
        assert!(!copied.unwrap().exists());
        assert!(original.exists());
        crate::composer::unstage(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_dragged_url_never_fetches_a_loopback_service() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/private.png", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while std::time::Instant::now() < until {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    let picture = include_bytes!("../../../features/support/sample.png");
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", picture.len()).unwrap();
                    stream.write_all(picture).unwrap();
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            false
        });
        let path = super::load_url(url).unwrap();
        let result = futures::executor::block_on(wait_ready(&path));
        let fetched = server.join().unwrap();
        drop(result);
        crate::composer::unstage(&path);
        assert!(!fetched, "dragging an image made a GET to a private service");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cancelling_a_stalled_promise_removes_partial_pictures() {
        use futures::FutureExt;
        use std::ffi::CString;
        let base = tempfile::tempdir().unwrap();
        let source = std::sync::Arc::new(crate::composer::private_folder(base.path()).unwrap());
        let folder = source.path().to_owned();
        let original = folder.join("first.png");
        std::fs::write(&original, b"part of a promised bundle").unwrap();
        let path = crate::composer::stage_private(base.path(), "promise.png", b"").unwrap();
        let (id, pending) = super::register_promise(path.clone(), source);
        super::PENDING.lock().unwrap().insert(path.clone(), pending);
        let name = CString::new(original.to_str().unwrap()).unwrap();
        super::promised(id as *mut _, 6, name.as_ptr(), std::ptr::null(), 0);
        let snapshots = super::PROMISES.lock().unwrap()[&id].prepared.paths.clone();
        assert_eq!(snapshots.len(), 1);
        let mut ready = Box::pin(wait_ready(&path));
        assert!(ready.as_mut().now_or_never().is_none());
        drop(ready); // the composer's timer wins, or its window closes
        let removed = !folder.exists() && snapshots.iter().all(|p| !p.exists());
        // Complete the fake source afterwards, also checking a late callback.
        super::promised(id as *mut _, 6, name.as_ptr(), std::ptr::null(), 0);
        super::promised(id as *mut _, 4, std::ptr::null(), std::ptr::null(), 0);
        crate::composer::unstage(&path);
        assert!(removed, "a cancelled receiver kept its source folder or partial copies");
    }

    #[test]
    fn an_unread_completion_removes_its_snapshots() {
        let base = tempfile::tempdir().unwrap();
        let path = crate::composer::stage_private(base.path(), "promise.png", b"private picture").unwrap();
        let (tx, rx) = futures::channel::oneshot::channel();
        tx.send(super::PreparedPictures { paths: vec![path.clone()] }).ok().unwrap();
        assert!(path.exists());
        drop(rx); // completion won send(), but the UI's timeout won select()
        assert!(!path.exists());
    }
}

/// native/composer_media.m's imageURL, from draggingEntered on the main
/// thread: whether a browser drag's HTML is an `<img>` of its own URL, read
/// in one bounded pass (html_tags.rs; Daybreak 120). Never panics across
/// the boundary: bad input is a plain no.
#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn tb_drag_html_shows_image(html: *const u8, html_length: usize, url: *const u8, url_length: usize) -> bool {
    if html.is_null() || url.is_null() || html_length > threadbear_core::html_tags::DRAG_HTML_BYTES {
        return false;
    }
    // SAFETY: the Objective-C caller hands NSData bytes it holds for the call.
    let (html, url) = unsafe { (std::slice::from_raw_parts(html, html_length), std::slice::from_raw_parts(url, url_length)) };
    let Ok(url) = std::str::from_utf8(url) else { return false };
    threadbear_core::html_tags::drag_html_shows_image(html, url)
}
