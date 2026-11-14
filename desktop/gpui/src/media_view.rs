// Videos and PDFs in the Mac's own views (native/media_views.m): AVKit's
// player and PDFKit's view, over a GPUI layout slot, as the Swift app's pane
// (FilePreviewView, Quick Look) and review window (ReviewVideo) show them.
// GPUI has no video player, and the web runtime's Chromium has no H.264, so
// on a Mac the system's own views do it; on Windows and Linux a video or a
// PDF still opens in its own app, through the open gate.
//
// A view is made the first time its slot is drawn, through the open gate
// (`open_gate::show_media`, the only way one is made), and then placed by its
// window once a frame after everything's laid out (`sweeper`), as the web
// panels' pages are (web_panels.rs): where its slot was drawn that frame, and
// hidden while a dialog, a sheet or one of the app's menus could be over it,
// as GPUI can't draw over a native view. A slot that isn't drawn in a frame
// has gone (the preview closed, another thread or shot is showing, the pane
// was put away), so its view stops and goes then, as a SwiftUI view's
// dismantle stops the Swift app's.
//
// Kept in thread-locals rather than an entity: the views are the main
// thread's, and a slot drawn while an entity is being updated never has to
// read that entity back (a double lease).
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

/// How a view is used, as native/media_views.m numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Kind {
    /// A film in the pane: the player's floating controls, its sound on,
    /// paused on its first frame, sized to its own shape.
    PaneVideo = 0,
    /// A clip on the review window's stage: controls and sound, looping from
    /// the start, at the mute and volume the clip before it was left at.
    StageVideo = 1,
    /// A clip among them all in the review window: muted, no controls,
    /// looping, and a click goes to the cell under it.
    CellVideo = 2,
    /// A PDF in the pane, in place of Quick Look.
    PaneDocument = 3,
    /// Read-only Quick Look thumbnail, with no links or document actions.
    PaneQuickLook = 4,
}

/// What a view is showing, for the scripted app's dump.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MediaState {
    /// The film's shape once its track has said (0 until then), or a PDF's
    /// first page, in points.
    pub width: f64,
    pub height: f64,
    pub playing: bool,
    pub muted: bool,
    pub controls: bool,
    pub looping: bool,
    pub volume: f32,
    pub pages: i32,
    /// A PDF still being read, with the pane's spinner where it will be.
    pub loading: bool,
}

/// native/media_views.m's TBMediaState, field for field.
#[repr(C)]
#[derive(Default)]
struct NativeState {
    width: f64,
    height: f64,
    playing: bool,
    muted: bool,
    controls: bool,
    looping: bool,
    volume: f32,
    pages: i32,
    failed: bool,
    loading: bool,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn tb_media_place(view: *mut c_void, x: f64, y: f64, width: f64, height: f64);
    fn tb_media_set_hidden(view: *mut c_void, hidden: bool);
    fn tb_media_close(view: *mut c_void);
    fn tb_media_state(view: *mut c_void, out: *mut NativeState);
    fn tb_media_failure(view: *mut c_void, buffer: *mut std::ffi::c_char, capacity: i32) -> bool;
    #[cfg(feature = "automation")]
    fn tb_media_play(view: *mut c_void) -> bool;
    #[cfg(feature = "automation")]
    fn tb_media_click_link(view: *mut c_void) -> bool;
}

/// One of those views, owned: dropped, it stops and leaves its window.
pub(crate) struct Media {
    view: *mut c_void,
    kind: Kind,
    hidden: Cell<bool>,
    placed: Cell<Option<Bounds<Pixels>>>,
}

impl Media {
    /// The view `tb_media_open` handed back, owned from here. Only the open
    /// gate makes them (`open_gate::show_media`).
    ///
    /// # Safety
    /// `view` is null or a view from `tb_media_open` that nothing else owns.
    pub(crate) unsafe fn from_native(view: *mut c_void, kind: Kind) -> Option<Media> {
        (!view.is_null()).then(|| Media { view, kind, hidden: Cell::new(true), placed: Cell::new(None) })
    }

    fn place(&self, bounds: Bounds<Pixels>) {
        if self.placed.get() == Some(bounds) {
            return;
        }
        self.placed.set(Some(bounds));
        #[cfg(target_os = "macos")]
        unsafe {
            tb_media_place(self.view, f64::from(bounds.origin.x), f64::from(bounds.origin.y), f64::from(bounds.size.width), f64::from(bounds.size.height))
        };
    }

    fn set_hidden(&self, hidden: bool) {
        if self.hidden.replace(hidden) == hidden {
            return;
        }
        #[cfg(target_os = "macos")]
        unsafe {
            tb_media_set_hidden(self.view, hidden)
        };
    }

    pub(crate) fn state(&self) -> MediaState {
        let mut out = NativeState::default();
        #[cfg(target_os = "macos")]
        unsafe {
            tb_media_state(self.view, &mut out)
        };
        MediaState { width: out.width, height: out.height, playing: out.playing, muted: out.muted, controls: out.controls, looping: out.looping, volume: out.volume, pages: out.pages, loading: out.loading }
    }

    /// Why AVFoundation couldn't play the film, once it's said.
    fn failure(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            let mut buffer = [0 as std::ffi::c_char; 512];
            if unsafe { tb_media_failure(self.view, buffer.as_mut_ptr(), buffer.len() as i32) } {
                return Some(unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned());
            }
        }
        None
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            tb_media_close(self.view)
        };
    }
}

// ---------------------------------------------------------------- slots

struct Slot {
    key: String,
    window: WindowId,
    /// The view, or why it couldn't be made (said in the slot's place, and
    /// not tried again every frame).
    media: Result<Media, String>,
    /// The frame it was last drawn in, and where.
    drawn: Rc<Cell<(u64, Bounds<Pixels>)>>,
}

thread_local! {
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
    /// Each window's frames, counted as it starts each (`sweeper`).
    static FRAMES: RefCell<HashMap<WindowId, u64>> = RefCell::new(HashMap::new());
    /// The mute and volume each review window's stage clip was left at, for
    /// the next: clips compared by ear play at the same level (ReviewVideo).
    static STAGE_AUDIO: RefCell<HashMap<WindowId, (bool, f32)>> = RefCell::new(HashMap::new());
    /// Every menu the kit has made that's still about (`init`), to see if
    /// one's open over a view.
    static MENUS: RefCell<Vec<WeakEntity<PopupMenu>>> = const { RefCell::new(Vec::new()) };
    /// Windows with a look again booked (`open_menus`).
    static RECHECK: RefCell<std::collections::HashSet<WindowId>> = RefCell::new(std::collections::HashSet::new());
    /// Where the views' calls back into Rust go (`tb_media_said_link`,
    /// `tb_media_said_changed`), for the app to act on (`init`).
    static SAID: RefCell<Option<futures::channel::mpsc::UnboundedSender<Said>>> = const { RefCell::new(None) };
}

/// What a view said, from inside AppKit's own handling of an event.
enum Said {
    /// A PDF's link was clicked: for the open gate, with the PDF's folder.
    Link { url: String, base: String },
    /// A view changed in a way that needs the window drawn again (a film
    /// that failed, to say why in its place).
    Changed,
}

/// Hears the views' calls back, and keeps a list of the kit's menus. A view
/// calls back from inside AppKit's handling of a click, with no way to the
/// app's state there, so what it says is acted on as a task.
pub(crate) fn init(cx: &mut App) {
    use futures::StreamExt as _;
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    SAID.with(|s| *s.borrow_mut() = Some(tx));
    cx.spawn(async move |cx| {
        while let Some(said) = rx.next().await {
            cx.update(|cx| match said {
                // Through the gate as any link in a thread is: a web page in
                // the browser, a file asking first if it's a program, any
                // other scheme asking. Never PDFKit's own opener.
                Said::Link { url, base } => crate::open_gate::open_link(&url, crate::open_gate::Origin::ThisMac, &base, cx),
                Said::Changed => cx.refresh_windows(),
            });
        }
    })
    .detach();
    cx.observe_new(|_: &mut PopupMenu, _, cx: &mut Context<PopupMenu>| {
        let menu = cx.entity().downgrade();
        MENUS.with(|m| m.borrow_mut().push(menu));
    })
    .detach();
}

fn say(said: Said) {
    SAID.with(|s| {
        if let Some(tx) = s.borrow().as_ref() {
            let _ = tx.unbounded_send(said);
        }
    });
}

/// native/media_views.m: a PDF's link was clicked.
#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn tb_media_said_link(url: *const std::ffi::c_char, base: *const std::ffi::c_char) {
    let text = |p: *const std::ffi::c_char| if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    say(Said::Link { url: text(url), base: text(base) });
}

/// native/media_views.m: a view's state changed.
#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn tb_media_said_changed() {
    say(Said::Changed);
}

/// A film dragged out is an APFS clone of the open file, which is instant,
/// or not dragged at all (a beep): copying it byte by byte from another disk
/// would hold the main thread as the drag starts for as long as that disk
/// takes (Codex's third and fourth reviews, 12 Nov).
const DRAG_COPY_LIMIT: u64 = 0;

/// Where films dragged out are copied to: the app's own Caches, which an
/// agent working in its own folders doesn't write to.
fn drag_folder() -> Option<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").filter(|h| !h.is_empty())?);
    Some(home.join("Library/Caches/Threadbear/drags"))
}

/// native/media_views.m: a private copy of the open file `fd` (still the
/// view's, not closed here) named `name`, for a drag out, its path written
/// into `out` (`threadbear_core::private_file::snapshot`).
#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn tb_media_snapshot(fd: i32, name: *const std::ffi::c_char, out: *mut std::ffi::c_char, capacity: i32) -> bool {
    let Some(folder) = drag_folder() else { return false };
    snapshot_fd(fd, name, out, capacity, &folder, DRAG_COPY_LIMIT).is_ok()
}

/// Quick Look reads a private copy too, made off the main thread. It is
/// removed after thumbnail generation; interrupted copies expire in a day.
#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn tb_preview_snapshot(fd: i32, name: *const std::ffi::c_char, out: *mut std::ffi::c_char, capacity: i32, limit: u64) -> i32 {
    if limit == 0 || limit > 256 * 1024 * 1024 { return -1 }
    let Some(folder) = drag_folder().and_then(|p| p.parent().map(|p| p.join("previews"))) else { return 0 };
    match snapshot_fd(fd, name, out, capacity, &folder, limit) {
        Ok(()) => 1,
        Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => -1,
        Err(_) => 0,
    }
}

#[cfg(target_os = "macos")]
fn snapshot_fd(fd: i32, name: *const std::ffi::c_char, out: *mut std::ffi::c_char, capacity: i32, folder: &std::path::Path, limit: u64) -> std::io::Result<()> {
    use std::os::fd::FromRawFd as _;
    use std::os::unix::ffi::OsStrExt as _;
    if fd < 0 || name.is_null() || out.is_null() || capacity <= 0 {
        return Err(std::io::Error::other("No snapshot destination"));
    }
    let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned();
    // Borrowed: the view still owns and closes it.
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    let meta = file.metadata()?;
    // Drag exports use an unlimited clone (limit 0); previews have a hard
    // byte cap even on APFS, where a clone itself needn't copy any bytes.
    if limit > 0 && meta.is_file() && meta.len() > limit {
        return Err(std::io::Error::new(std::io::ErrorKind::FileTooLarge, "The document is too big to preview"));
    }
    let path = if meta.is_dir() {
        threadbear_core::private_file::snapshot_package(&file, folder, &name, limit)
    } else {
        threadbear_core::private_file::snapshot(&file, folder, &name, limit)
    }?;
    let too_big = limit > 0 && std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() > limit);
    let bytes = path.as_os_str().as_bytes();
    if too_big || bytes.len() + 1 > capacity as usize || bytes.contains(&0) {
        if let Some(parent) = path.parent() { let _ = std::fs::remove_dir_all(parent); }
        return Err(std::io::Error::new(if too_big { std::io::ErrorKind::FileTooLarge } else { std::io::ErrorKind::InvalidInput }, "The document couldn't be previewed"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), out, bytes.len());
        *out.add(bytes.len()) = 0;
    }
    Ok(())
}

fn frame_of(window: WindowId) -> u64 {
    FRAMES.with(|f| f.borrow().get(&window).copied().unwrap_or(0))
}

/// What a slot's view is given when it's made: the window's own view, and
/// the review stage's mute and volume to carry on with.
pub(crate) struct Opening {
    pub parent: *mut c_void,
    pub audio: Option<(bool, f32)>,
}

/// The view for `key`, placed where this element's drawn in `window`: made
/// now the first time, by `open` (which goes through the open gate), or why
/// it couldn't be, for the caller to say instead.
pub(crate) fn slot(key: &str, window: &Window, open: impl FnOnce(Opening) -> Result<Media, String>) -> Result<AnyElement, String> {
    let id = window.window_handle().window_id();
    // Only a window that places its views every frame (one with a
    // `sweeper`) can show one: elsewhere it would sit unplaced and never go
    // (a panel in a window of its own, Codex's review, 12 Nov).
    if !FRAMES.with(|f| f.borrow().contains_key(&id)) {
        return Err("Videos, PDFs and document previews show here when the pane is in the main window.".to_owned());
    }
    // A film AVFoundation couldn't play says why in its place, from then on.
    let failed = SLOTS.with(|s| s.borrow().iter().find(|s| s.key == key && s.window == id).and_then(|s| s.media.as_ref().ok()).and_then(Media::failure));
    if let Some(why) = failed {
        // Every key ends with the file's path.
        let name = threadbear_core::highlights::last_component(key).to_owned();
        let pdf = SLOTS.with(|s| s.borrow().iter().any(|s| s.key == key && s.window == id && s.media.as_ref().is_ok_and(|m| matches!(m.kind, Kind::PaneDocument | Kind::PaneQuickLook))));
        let said = format!("Threadbear couldn't {} {name}: {why}", if pdf { "show" } else { "play" });
        // Taken out of the list before it's dropped, which closes it.
        let gone = SLOTS.with(|s| s.borrow_mut().iter_mut().find(|s| s.key == key && s.window == id).map(|slot| std::mem::replace(&mut slot.media, Err(said))));
        drop(gone);
    }
    let found = SLOTS.with(|s| {
        let slots = s.borrow();
        let slot = slots.iter().find(|s| s.key == key && s.window == id)?;
        // One that couldn't be made has no element to say it's still
        // wanted, so asking for it does.
        if slot.media.is_err() {
            slot.drawn.set((frame_of(id), Bounds::default()));
        }
        Some(slot.media.as_ref().map(|_| slot.drawn.clone()).map_err(Clone::clone))
    });
    let drawn = match found {
        Some(found) => found?,
        None => {
            let media = match parent_view(window) {
                Some(parent) => {
                    // The clip on the stage now (on its way out, as this one
                    // takes its place), else the last that went: the one just
                    // left is where this one starts (Codex's review, 12 Nov).
                    let live: Vec<(WindowId, Kind, (bool, f32))> = SLOTS.with(|s| {
                        s.borrow()
                            .iter()
                            .filter_map(|s| s.media.as_ref().ok().filter(|m| m.kind == Kind::StageVideo).map(|m| (s.window, m.kind, m.state())))
                            .map(|(w, k, st)| (w, k, (st.muted, st.volume)))
                            .collect()
                    });
                    let audio = stage_audio(live, id, STAGE_AUDIO.with(|a| a.borrow().get(&id).copied()));
                    open(Opening { parent, audio })
                }
                None => Err("This window has no view to show it in.".to_owned()),
            };
            let drawn = Rc::new(Cell::new((if media.is_err() { frame_of(id) } else { 0 }, Bounds::default())));
            let made = media.as_ref().map(|_| drawn.clone()).map_err(Clone::clone);
            SLOTS.with(|s| s.borrow_mut().push(Slot { key: key.to_owned(), window: id, media, drawn }));
            made?
        }
    };
    Ok(canvas(move |bounds, window, _| drawn.set((frame_of(window.window_handle().window_id()), bounds)), |_, _, _, _| {}).size_full().into_any_element())
}

/// The mute and volume a new stage clip in `window` starts at: that of the
/// stage clip up there now, about to go as this one takes its place, or
/// failing that the last one's that went (`left`).
fn stage_audio<W: PartialEq>(live: impl IntoIterator<Item = (W, Kind, (bool, f32))>, window: W, left: Option<(bool, f32)>) -> Option<(bool, f32)> {
    live.into_iter().find(|(w, kind, _)| *w == window && *kind == Kind::StageVideo).map(|(_, _, audio)| audio).or(left)
}

/// Whether `key`'s view is up in some window, and what it's showing.
pub(crate) fn state_of(key: &str) -> Option<MediaState> {
    SLOTS.with(|s| s.borrow().iter().find(|s| s.key == key).and_then(|s| s.media.as_ref().ok().map(Media::state)))
}

/// The play button of `key`'s film, for the scripted app: false when there's
/// no film up by that key.
#[cfg(feature = "automation")]
pub(crate) fn play(key: &str) -> bool {
    SLOTS.with(|s| {
        s.borrow().iter().find(|s| s.key == key).and_then(|s| s.media.as_ref().ok()).is_some_and(|m| {
            #[cfg(target_os = "macos")]
            return unsafe { tb_media_play(m.view) };
            #[cfg(not(target_os = "macos"))]
            {
                let _ = m;
                false
            }
        })
    })
}

/// A click on the first link in `key`'s PDF, for the scripted app, as a
/// person's would come through the window: false with no PDF or no link.
#[cfg(feature = "automation")]
pub(crate) fn click_link(key: &str) -> bool {
    SLOTS.with(|s| {
        s.borrow().iter().find(|s| s.key == key).and_then(|s| s.media.as_ref().ok()).is_some_and(|m| {
            #[cfg(target_os = "macos")]
            return unsafe { tb_media_click_link(m.view) };
            #[cfg(not(target_os = "macos"))]
            {
                let _ = m;
                false
            }
        })
    })
}

/// Why `key`'s view couldn't be made, if it couldn't.
pub(crate) fn failure_of(key: &str) -> Option<String> {
    SLOTS.with(|s| s.borrow().iter().find(|s| s.key == key).and_then(|s| s.media.as_ref().err().cloned()))
}

/// What could be over a window's views this frame.
#[derive(Default)]
pub(crate) struct Cover {
    /// A dialog, a sheet, a popover or the picture viewer: over everything.
    pub everything: bool,
    /// Where the app's own menus are open: one reaching a view's column
    /// covers it.
    pub menus: Vec<Point<Pixels>>,
}

/// Made as a window's frame starts (its root view's render) and drawn after
/// everything else in it: counts the frame, then once everything's laid out
/// places the views drawn in it and closes the ones that weren't.
pub(crate) fn sweeper(window: &Window, cover: Cover) -> AnyElement {
    let id = window.window_handle().window_id();
    let frame = FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let n = f.entry(id).or_insert(0);
        *n += 1;
        *n
    });
    canvas(move |_, window, cx| sweep(id, frame, &cover, window, cx), |_, _, _, _| {}).absolute().size_0().into_any_element()
}

fn sweep(id: WindowId, frame: u64, cover: &Cover, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::WindowExt as _;
    let kit_menu = menu_open(window, cx);
    let modal = cover.everything || window.has_active_dialog(cx) || window.has_active_sheet(cx) || kit_menu;
    if kit_menu && SLOTS.with(|s| s.borrow().iter().any(|slot| slot.window == id && slot.media.is_ok())) {
        look_again_soon(window, cx);
    }
    let gone = SLOTS.with(|s| {
        let mut slots = s.borrow_mut();
        let (keep, gone): (Vec<Slot>, Vec<Slot>) = std::mem::take(&mut *slots).into_iter().partition(|slot| slot.window != id || slot.drawn.get().0 == frame);
        *slots = keep;
        for slot in slots.iter().filter(|s| s.window == id) {
            let Ok(media) = &slot.media else { continue };
            let (_, b) = slot.drawn.get();
            // A menu opens down and to the right of where it was asked for,
            // about this wide (web_panels.rs's rule).
            let menu_over = cover.menus.iter().any(|at| at.x + gpui_kit::px(300.) > b.origin.x && at.x < b.origin.x + b.size.width);
            if !modal && !menu_over && f32::from(b.size.width) >= 1. && f32::from(b.size.height) >= 1. {
                media.place(b);
                media.set_hidden(false);
            } else {
                media.set_hidden(true);
            }
        }
        gone
    });
    // Dropped outside the borrow: each stops and leaves its window as it goes.
    for slot in gone {
        remember_audio(&slot);
    }
}

/// Whether one of the kit's menus is showing in this window (`open_menus`;
/// it used to go by the keyboard, which a hovered submenu never has). Where
/// it is isn't always the
/// app's to know (a panel bar's right-click menu is the dock's, Codex's
/// review, 12 Nov), and a native view would sit over it and take its clicks.
fn menu_open(window: &Window, cx: &App) -> bool {
    !open_menus(window, cx).is_empty()
}

/// Where the kit's menus showing in this window are, submenus included (a
/// zero-sized one hasn't been drawn yet: `PopupMenu::showing`). Not by focus:
/// a hovered submenu never has it, and a menu that lost it can still be up
/// (Codex's review, 13 Nov). web_panels.rs hides a page under one, as a
/// panel tab's right-click menu went behind a web page (Mike, 13 Nov).
pub(crate) fn open_menus(window: &Window, cx: &App) -> Vec<Bounds<Pixels>> {
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        menus.retain(|menu| menu.upgrade().is_some());
        menus.iter().filter_map(WeakEntity::upgrade).filter_map(|menu| menu.read(cx).showing(window, cx)).flatten().collect()
    })
}

/// While a menu hides a native view, the window's drawn again a moment
/// later: `showing` reads the last frame, so a menu whose host stopped
/// drawing it would otherwise keep the view hidden until something else
/// redraws. Only then, so an open menu over nothing native costs nothing.
pub(crate) fn look_again_soon(window: &Window, cx: &mut App) {
    let handle = window.window_handle();
    if !RECHECK.with(|r| r.borrow_mut().insert(handle.window_id())) {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
        RECHECK.with(|r| r.borrow_mut().remove(&handle.window_id()));
        let _ = handle.update(cx, |_, window, _| window.refresh());
    })
    .detach();
}

/// A stage clip going: its mute and volume are where the next one starts.
fn remember_audio(slot: &Slot) {
    if let Some(media) = slot.media.as_ref().ok().filter(|m| m.kind == Kind::StageVideo) {
        let state = media.state();
        STAGE_AUDIO.with(|a| a.borrow_mut().insert(slot.window, (state.muted, state.volume)));
    }
}

/// A window's going: its views stop and go with it.
pub(crate) fn close_window(id: WindowId) {
    let gone: Vec<Slot> = SLOTS.with(|s| {
        let mut slots = s.borrow_mut();
        let (gone, keep) = std::mem::take(&mut *slots).into_iter().partition(|slot| slot.window == id);
        *slots = keep;
        gone
    });
    drop(gone);
    FRAMES.with(|f| f.borrow_mut().remove(&id));
    STAGE_AUDIO.with(|a| a.borrow_mut().remove(&id));
}

/// The window's own view, a media view's parent.
fn parent_view(window: &Window) -> Option<*mut c_void> {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()),
            _ => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// Whether this system has the views at all: on Windows and Linux a video or
/// a PDF opens in its own app.
pub(crate) const SHOWN_HERE: bool = cfg!(target_os = "macos");

/// The views up now, for the dump.
#[cfg(feature = "automation")]
pub(crate) fn dump() -> serde_json::Value {
    SLOTS.with(|s| {
        serde_json::Value::Array(
            s.borrow()
                .iter()
                .map(|slot| match &slot.media {
                    Ok(media) => {
                        let st = media.state();
                        serde_json::json!({ "key": slot.key, "kind": format!("{:?}", media.kind), "hidden": media.hidden.get(), "width": st.width, "height": st.height, "playing": st.playing, "muted": st.muted, "controls": st.controls, "looping": st.looping, "pages": st.pages, "loading": st.loading })
                    }
                    Err(why) => serde_json::json!({ "key": slot.key, "failed": why }),
                })
                .collect(),
        )
    })
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings gpui's own `test` macro in over std's.
    use super::{stage_audio, Kind};

    /// A PDF's right-click menu keeps what's safe and sends Open Link to the
    /// open gate (Codex's second review, 12 Nov): PDFKit's own Open Link and
    /// Open With hand a file link to the system, which would run a program
    /// it points at with no question. Anything it doesn't know goes too.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_pdf_links_menu_opens_nothing_itself() {
        extern "C" {
            fn tb_media_menu_for_tests(link: *const std::ffi::c_char, buffer: *mut std::ffi::c_char, capacity: i32) -> i32;
        }
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        super::SAID.with(|s| *s.borrow_mut() = Some(tx));
        let link = std::ffi::CString::new("file:///tmp/payload.command").unwrap();
        let mut buffer = [0 as std::ffi::c_char; 1024];
        unsafe { tb_media_menu_for_tests(link.as_ptr(), buffer.as_mut_ptr(), buffer.len() as i32) };
        let left = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned();
        assert_eq!(left, "Open Link|openLinkThroughGate:\nCopy Link|copyLink:\n-|\nZoom In|zoomIn:\n");
        // Choosing Open Link reached the gate's way in, with the link.
        match rx.try_recv() {
            Ok(super::Said::Link { url, base }) => {
                assert_eq!(url, "file:///tmp/payload.command");
                assert_eq!(base, "/tmp");
            }
            _ => panic!("Open Link didn't go to the open gate"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn quicklook_snapshots_use_the_descriptor_and_each_callers_byte_cap() {
        use std::{ffi::{CStr, CString}, os::{fd::AsRawFd, unix::{ffi::OsStrExt, fs::symlink}}};
        let work=tempfile::tempdir().unwrap();
        let source=work.path().join("source.pdf");
        std::fs::write(&source,b"held document").unwrap();
        let file=std::fs::File::open(&source).unwrap();
        std::fs::remove_file(&source).unwrap();
        symlink("/missing/replaced-source",&source).unwrap();
        let name=CString::new("document.pdf").unwrap();
        let mut path=[0 as std::ffi::c_char;4096];
        let folder=work.path().join("previews");
        super::snapshot_fd(file.as_raw_fd(),name.as_ptr(),path.as_mut_ptr(),path.len() as i32,&folder,16*1024*1024).unwrap();
        let copied=std::path::Path::new(std::ffi::OsStr::from_bytes(unsafe{CStr::from_ptr(path.as_ptr())}.to_bytes()));
        assert_eq!(std::fs::read(copied).unwrap(),b"held document");
        assert!(file.metadata().is_ok(),"snapshot borrows the gate's descriptor");
        let large=std::fs::File::create(work.path().join("large.pdf")).unwrap();
        large.set_len(16*1024*1024+1).unwrap();
        let error=super::snapshot_fd(large.as_raw_fd(),name.as_ptr(),path.as_mut_ptr(),path.len() as i32,&folder,16*1024*1024).unwrap_err();
        assert_eq!(error.kind(),std::io::ErrorKind::FileTooLarge);
        let large=std::fs::File::open(work.path().join("large.pdf")).unwrap();
        super::snapshot_fd(large.as_raw_fd(),name.as_ptr(),path.as_mut_ptr(),path.len() as i32,&folder,256*1024*1024).unwrap();
        let copied=std::path::Path::new(std::ffi::OsStr::from_bytes(unsafe{CStr::from_ptr(path.as_ptr())}.to_bytes()));
        assert_eq!(std::fs::metadata(copied).unwrap().len(),16*1024*1024+1);
        // Packages carry length zero through the pane; snapshot_fd still
        // walks held directory descriptors and enforces the total byte cap.
        let package=work.path().join("deck.key");
        std::fs::create_dir(&package).unwrap();
        std::fs::write(package.join("slide.bin"),b"held slide").unwrap();
        let directory=std::fs::File::open(&package).unwrap();
        let package_name=CString::new("deck.key").unwrap();
        super::snapshot_fd(directory.as_raw_fd(),package_name.as_ptr(),path.as_mut_ptr(),path.len() as i32,&folder,256*1024*1024).unwrap();
        let copied=std::path::Path::new(std::ffi::OsStr::from_bytes(unsafe{CStr::from_ptr(path.as_ptr())}.to_bytes()));
        assert_eq!(std::fs::read(copied.join("slide.bin")).unwrap(),b"held slide");
        let large_slide=std::fs::File::create(package.join("oversize.bin")).unwrap();
        large_slide.set_len(16*1024*1024+1).unwrap();
        assert_eq!(super::snapshot_fd(directory.as_raw_fd(),package_name.as_ptr(),path.as_mut_ptr(),path.len() as i32,&folder,16*1024*1024).unwrap_err().kind(),std::io::ErrorKind::FileTooLarge);
        std::fs::remove_file(package.join("oversize.bin")).unwrap();
        symlink("/missing/package-link",package.join("link")).unwrap();
        assert!(super::snapshot_fd(directory.as_raw_fd(),package_name.as_ptr(),path.as_mut_ptr(),path.len() as i32,&folder,256*1024*1024).is_err());
        assert_eq!(super::tb_preview_snapshot(-1,std::ptr::null(),std::ptr::null_mut(),0,0),-1);
        assert_eq!(super::tb_preview_snapshot(-1,std::ptr::null(),std::ptr::null_mut(),0,256*1024*1024+1),-1);
    }

    /// Stepping from one clip to the next on the stage: the next starts at
    /// the mute and volume just set on the one going, which is still up as
    /// the next is made, not at the one before that.
    #[test]
    fn the_next_stage_clip_starts_where_the_last_was_left() {
        let muted = (true, 0.4);
        let older = (false, 1.0);
        assert_eq!(stage_audio([(1, Kind::StageVideo, muted)], 1, Some(older)), Some(muted));
        // Only this window's stage clip, not a cell or another window's.
        assert_eq!(stage_audio([(1, Kind::CellVideo, (true, 1.0)), (2, Kind::StageVideo, muted)], 1, Some(older)), Some(older));
        assert_eq!(stage_audio(Vec::<(u8, Kind, (bool, f32))>::new(), 1, None), None);
    }
}
