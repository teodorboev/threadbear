// Dragging a file out of the app onto the Finder, X's upload box or anything
// else that takes files, as the Swift app's FileDrag does (Mike wanted to post
// a clip without opening the panel first, 29 Oct): the transcript's picture
// tiles, Highlights' file rows, the folder browser's rows and the picture open
// in the pane each start one. In the window it's GPUI's own drag, with a small
// picture of what's going under the pointer, which the thread's box takes as a
// dropped file; past the window's edge GPUI hands it to the system as the file
// itself (`external_drag_payload`, macOS and Wayland so far). Copy only: a drop
// never moves the file.
//
// Only ever this Mac's files: another Mac's thread names paths on that Mac,
// and a file here with the same path isn't the one meant (Daybreak, 7 Nov), so
// those never start one. And only the file that was grabbed: a path swapped
// for a link to something else while the drag's under way hands on nothing
// (Codex's review, 12 Nov), as the file's noted as the drag begins.
use crate::style::{self, px};
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::OnceCell;
use std::path::PathBuf;
use std::rc::Rc;
use threadbear_core::file_identity::FileIdentity;

/// A file on its way out of the app.
#[derive(Clone, Debug)]
pub(crate) struct DraggedFile {
    pub(crate) path: PathBuf,
    /// The file as the drag began, noted then (the element is made afresh
    /// every frame, the drag only once).
    grabbed: Rc<OnceCell<Option<FileIdentity>>>,
}

impl DraggedFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, grabbed: Rc::default() }
    }

    /// The drag's begun: which file it is now is the one it carries.
    fn note_grabbed(&self) {
        self.grabbed.get_or_init(|| FileIdentity::of(&self.path));
    }

    /// Where the grabbed file is, while its path still leads to that same
    /// file; None once it doesn't (gone, or swapped for something else).
    pub(crate) fn still_grabbed(&self) -> Option<PathBuf> {
        let then = self.grabbed.get_or_init(|| FileIdentity::of(&self.path)).clone()?;
        then.still_at(&self.path).then(|| then.real().to_path_buf())
    }
}

/// What follows the pointer in the window: a picture small, else the file's
/// name beside a document's symbol (`FileDrag.picture`). GPUI draws a drag
/// where the element was grabbed (`grabbed`, from its corner), so it's moved
/// back under the pointer: a picture centred on it, as the Swift drag's is,
/// a name held by its left end.
pub(crate) struct DragPicture {
    path: PathBuf,
    grabbed: Point<Pixels>,
}

/// As big as the Swift drag's thumbnail: 96 on its longer side.
const PICTURE: f32 = 96.;

impl Render for DragPicture {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.path.to_string_lossy().to_string();
        let zero = gpui_kit::px(0.);
        if threadbear_core::highlights::looks_like_picture(&path) {
            let side = px(PICTURE);
            return div()
                .pl((self.grabbed.x - side / 2.).max(zero))
                .pt((self.grabbed.y - side / 2.).max(zero))
                .child(
                    div()
                        .size(side)
                        .flex()
                        .items_center()
                        .justify_center()
                        // Read only if it really is a picture file, as every
                        // picture a thread names is (`picture_file`).
                        .child(img(crate::rows::markdown::file_picture(&path)).max_w(side).max_h(side).rounded(px(6.)).object_fit(ObjectFit::Contain).shadow(vec![BoxShadow::new(zero, px(2.), hsla(0., 0., 0., 0.3)).blur_radius(px(8.))])),
                )
                .into_any_element();
        }
        let name = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path);
        let chip = h_flex()
            .gap(px(6.))
            .px(px(8.))
            .py(px(5.))
            .rounded(px(6.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .shadow(vec![BoxShadow::new(gpui_kit::px(0.), px(2.), hsla(0., 0., 0., 0.25)).blur_radius(px(8.))])
            .text_size(px(style::size::LABEL))
            .child(crate::icons::symbol(if threadbear_core::open_policy::local_metadata(&self.path).is_ok_and(|m| m.is_dir()) { "folder" } else { "doc" }).with_size(px(14.)))
            .child(name);
        div().pl((self.grabbed.x - px(14.)).max(zero)).pt((self.grabbed.y - px(12.)).max(zero)).child(chip).into_any_element()
    }
}

/// Lets an element be dragged out as `path`, a file on this Mac. Nothing for
/// None (another Mac's file, or none to drag).
pub(crate) trait FileDragExt: StatefulInteractiveElement + Sized {
    fn file_drag(self, path: Option<String>) -> Self {
        let Some(path) = path else { return self };
        self.on_drag(DraggedFile::new(PathBuf::from(path)), |file, grabbed, _, cx| {
            file.note_grabbed();
            cx.new(|_| DragPicture { path: file.path.clone(), grabbed })
        })
        // Looked at as it leaves the window: a file that's gone since, or
        // isn't the one grabbed any more, goes nowhere, as `FileDrag.writer`
        // has it.
        .external_drag_payload(|file: &DraggedFile, _, _| {
            let real = file.still_grabbed()?;
            let is_dir = threadbear_core::open_policy::local_metadata(&real).ok()?.is_dir();
            Some(ExternalDragPayload::Files(FileDragPaths::new([(real, is_dir)])))
        })
    }
}

impl<E: StatefulInteractiveElement + Sized> FileDragExt for E {}

/// `file-drag`: the mouse pressed on something that drags out as a file and
/// moved to the window's point `x`, `y`, then let go, through the window's
/// own dispatch: `"tile": index` (a picture in the transcript, the lowest one
/// on screen at that place in its row), `"highlight": title` (a Highlights
/// row) or `"pane": true` (the picture open in the pane).
#[cfg(feature = "automation")]
pub(crate) async fn file_drag_command(app: &WeakEntity<crate::app::Threadbear>, window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    let app = app.upgrade().ok_or("the app has gone")?;
    let from = cx
        .update_window(window, |_, window, cx| -> Result<Point<Pixels>, String> {
            window.render_frame(cx);
            let found = if let Some(index) = cmd["tile"].as_u64() {
                // A tile's label is its file's name (rows/images.rs).
                let count = cmd["count"].as_u64().unwrap_or(1) as usize;
                let images = app.update(cx, |t, cx| t.update_pane(cx, |p, _| p.last_row_images(count)))??;
                let image = images.get(index as usize).ok_or("no such picture")?;
                let name = std::path::Path::new(&image.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                window.find_all_by_label(&name)
            } else if let Some(title) = cmd["highlight"].as_str() {
                window.find_all_by_label(title)
            } else if cmd["pane"].as_bool() == Some(true) {
                window.find_all("hl-picture")
            } else {
                return Err("file-drag takes tile, highlight or pane".into());
            };
            let at = found.into_iter().filter(|e| e.visible()).last().map(|e| e.bounds().center()).ok_or("nothing like that on screen to drag")?;
            crate::thread::scripted_input(|| {
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Left, cx);
            });
            Ok(at)
        })
        .map_err(|e| e.to_string())??;
    let to = point(gpui_kit::px(cmd["x"].as_f64().unwrap_or(650.) as f32), gpui_kit::px(cmd["y"].as_f64().unwrap_or(300.) as f32));
    let steps = 12;
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let at = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| window.pointer_move(at, Some(MouseButton::Left), cx))).map_err(|e| e.to_string())?;
        cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
    }
    let dragging = cx.update(|cx| cx.has_active_drag());
    cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| window.pointer_up(to, MouseButton::Left, cx))).map_err(|e| e.to_string())?;
    Ok(serde_json::json!(if dragging { "ok dropped" } else { "ok no drag started" }))
}
