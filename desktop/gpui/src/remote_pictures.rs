// Another Mac's pictures in a transcript (a tool's screenshots there, a
// picture pasted into one of its messages): their paths are that Mac's, so a
// tile never draws the file here that has the same path, which isn't the one
// meant (Daybreak, 7 Nov). This Mac's runner is asked for it instead
// (`files.read` with the thread's `runnerId`), as the pane asks for that Mac's
// files, and the tile draws the bytes. A tile with no picture says so
// quietly, as the Swift app's ImageGridView does: "Loading…", "Couldn't get it
// from bruce" (why, on hover; a click asks again), or "On bruce" for one too
// big to send whole.
//
// Through Convex, the only way this app reaches another Mac so far, the
// runner sends no pictures at all, just a line saying how many it kept there,
// and answers a `files.read` for another runner with "Files on another runner
// are kept there for now." So these tiles turn up for the mock runner's bruce,
// which plays a direct link, and for whatever comes next.
use crate::call;
use gpui_kit::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use threadbear_client::runner::Client;
use threadbear_core::folders::FileRead;

/// Biggest picture asked for whole (`files.read`'s own default).
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone)]
pub(crate) enum Picture {
    Loading,
    Ready(Arc<Image>),
    /// Over the size cap: only its start came, which won't draw.
    Truncated,
    Failed(String),
}

/// Every remote picture asked for since launch, by runner and path.
#[derive(Default)]
struct RemotePictures {
    pictures: HashMap<(String, String), Picture>,
}

impl Global for RemotePictures {}

/// The picture as far as it's got, asking for it the first time. `redraw`
/// is told when it comes.
pub(crate) fn picture(runner: &str, path: &str, client: &Arc<Client>, redraw: AnyWeakEntity, cx: &mut App) -> Picture {
    let key = (runner.to_owned(), path.to_owned());
    if let Some(found) = cx.default_global::<RemotePictures>().pictures.get(&key) {
        return found.clone();
    }
    cx.default_global::<RemotePictures>().pictures.insert(key.clone(), Picture::Loading);
    let (client, params) = (client.clone(), json!({ "path": path, "runnerId": runner, "maxBytes": MAX_BYTES }));
    cx.spawn(async move |cx| {
        let answer = call(client, "files.read", params).await;
        let picture = match answer {
            Ok(value) => match FileRead::decode(&value) {
                Some(read) if read.truncated => Picture::Truncated,
                Some(read) => match crate::viewer::format_of(&key.1, &read.data) {
                    Some(format) => Picture::Ready(Arc::new(Image::from_bytes(format, read.data))),
                    None => Picture::Failed("It isn't a picture this app can show.".into()),
                },
                None => Picture::Failed("Couldn't read the runner's answer.".into()),
            },
            Err(e) => Picture::Failed(e.message),
        };
        cx.update(|cx| {
            cx.default_global::<RemotePictures>().pictures.insert(key, picture);
            if let Some(entity) = redraw.upgrade() {
                cx.notify(entity.entity_id());
            }
        });
    })
    .detach();
    Picture::Loading
}

/// A click on one that couldn't be got: asked for again on the next draw.
pub(crate) fn retry(runner: &str, path: &str, cx: &mut App) {
    let key = (runner.to_owned(), path.to_owned());
    let pictures = &mut cx.default_global::<RemotePictures>().pictures;
    if matches!(pictures.get(&key), Some(Picture::Failed(_))) {
        pictures.remove(&key);
    }
}

/// What a tile with no picture says (`remoteWords`).
pub(crate) fn words(picture: &Picture, runner_name: &str) -> Option<String> {
    match picture {
        Picture::Loading => Some("Loading…".into()),
        Picture::Truncated => Some(format!("On {runner_name}")),
        Picture::Failed(_) => Some(format!("Couldn't get it from {runner_name}")),
        Picture::Ready(_) => None,
    }
}

/// The picture as far as it's got, without asking for it.
#[cfg(feature = "automation")]
fn peek(runner: &str, path: &str, cx: &App) -> Option<Picture> {
    cx.try_global::<RemotePictures>()?.pictures.get(&(runner.to_owned(), path.to_owned())).cloned()
}

/// `tiles`, as the Swift app's: what each picture of the last row of at
/// least `count` says, "picture" for one that's drawn, else its words.
#[cfg(feature = "automation")]
pub(crate) fn tiles_command(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, cx: &mut App) -> Result<serde_json::Value, String> {
    let count = cmd["count"].as_u64().unwrap_or(1) as usize;
    let (images, runner) = app.update(cx, |t, cx| t.update_pane(cx, |p, _| p.last_row_images(count).map(|images| (images, p.summary.runner_id.clone()))))??;
    let name = runner.as_deref().map(|id| app.read(cx).runner_name_of(id)).unwrap_or_default();
    let said: Vec<String> = images
        .iter()
        .map(|image| match &runner {
            None => "picture".to_owned(),
            // Not asked for yet is on its way, as the Swift tile says it.
            Some(runner) => peek(runner, &image.path, cx).map_or_else(|| Some("Loading…".to_owned()), |p| words(&p, &name)).unwrap_or_else(|| "picture".to_owned()),
        })
        .collect();
    Ok(json!(format!("ok {}", said.join(" | "))))
}

/// `tile-point`: where a picture tile's middle is (`index` of the last row of
/// at least `count`), with its file's name: "ok x y name.png".
#[cfg(feature = "automation")]
pub(crate) fn tile_point_command(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    let count = cmd["count"].as_u64().unwrap_or(1) as usize;
    let index = cmd["index"].as_u64().unwrap_or(0) as usize;
    let images = app.update(cx, |t, cx| t.update_pane(cx, |p, _| p.last_row_images(count)))??;
    let image = images.get(index).ok_or("no such picture")?;
    let name = std::path::Path::new(&image.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    window.render_frame(cx);
    // A tile's label is its file's name (rows/images.rs).
    let tile = window.find_all_by_label(&name).into_iter().filter(|e| e.visible()).last().ok_or("that picture isn't on screen")?;
    let at = tile.bounds().center();
    Ok(json!(format!("ok {} {} {name}", f32::from(at.x).round(), f32::from(at.y).round())))
}
