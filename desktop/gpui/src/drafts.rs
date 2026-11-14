// What's in each composer, kept in `<runner home>/desktop-drafts.json` so it
// outlives the app (Swift's DraftStore, `app-drafts.json`): a quit, an update
// or a crash used to throw away half-written messages. This app's own file,
// so the Swift app and this one running side by side don't write over each
// other's. Written a moment after typing stops and as the app quits; a
// thread's box gets its draft back when it's next made. Private words: the
// file is only readable by Mike's account.
use crate::app::Threadbear;
use crate::composer::{image_ref_json, RestoreDraft};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use threadbear_core::reducer::ImageRef;

/// The new-thread page's box.
pub(crate) const NEW_THREAD: &str = "new";

/// How long after the last keystroke it writes.
const PAUSE: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    pub(crate) text: String,
    pub(crate) images: Vec<ImageRef>,
}

impl Draft {
    fn is_empty(&self) -> bool {
        self.images.is_empty() && self.text.trim().is_empty()
    }
}

#[derive(Default)]
pub(crate) struct Drafts {
    path: Option<PathBuf>,
    /// What the file had, for boxes not made yet this run.
    saved: HashMap<String, Draft>,
    pending: Option<Task<()>>,
}

impl Drafts {
    pub(crate) fn load(home: &std::path::Path) -> Drafts {
        let path = home.join("desktop-drafts.json");
        Drafts { saved: read(&path), path: Some(path), pending: None }
    }
}

fn read(path: &std::path::Path) -> HashMap<String, Draft> {
    let Some(value) = std::fs::read(path).ok().and_then(|data| serde_json::from_slice::<Value>(&data).ok()) else { return HashMap::new() };
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, v)| {
            let draft = Draft { text: v["text"].as_str().unwrap_or("").to_owned(), images: v["images"].as_array().into_iter().flatten().filter_map(ImageRef::decode).collect() };
            (!draft.is_empty()).then(|| (key.clone(), draft))
        })
        .collect()
}

/// Written whole and renamed into place, so a crash mid-write can't leave
/// half a file, and only ever readable by this user, the temp file too
/// (`private_file`); none at all when every box is empty.
fn write(path: &std::path::Path, all: &HashMap<String, Draft>) -> std::io::Result<()> {
    if all.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let value: serde_json::Map<String, Value> = all.iter().map(|(k, d)| (k.clone(), json!({ "text": d.text, "images": d.images.iter().map(image_ref_json).collect::<Vec<_>>() }))).collect();
    let data = serde_json::to_vec(&Value::Object(value)).map_err(std::io::Error::other)?;
    threadbear_core::private_file::write(path, &data)
}

impl Threadbear {
    /// A box's draft from before, put back as the box is made (`restore`).
    pub(crate) fn restore_saved_draft(&self, key: &str, composer: &Entity<crate::composer::Composer>, cx: &mut App) {
        // A handover's incoming app: the drafts are the outgoing app's until
        // it takes over (`reload_drafts`), as he may still be typing there.
        if crate::handover::waiting() {
            return;
        }
        if let Some(draft) = self.drafts.saved.get(key).cloned() {
            composer.update(cx, |c, cx| c.restore_draft(RestoreDraft::plain(draft.text, draft.images), cx));
        }
    }

    /// A handover's incoming app taking over: the drafts the outgoing app
    /// wrote as it handed over, into every box (none had its draft put back
    /// while it waited: `restore_saved_draft`).
    pub(crate) fn reload_drafts(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.drafts.path.clone() else { return };
        self.drafts.saved = read(&path);
        let mut boxes: Vec<(String, Entity<crate::composer::Composer>)> = self.composers.iter().map(|(k, c)| (k.clone(), c.clone())).collect();
        if let Some(new) = &self.new_thread.composer {
            boxes.push((NEW_THREAD.to_owned(), new.clone()));
        }
        for (key, composer) in boxes {
            if !composer.read(cx).holds_anything(cx) {
                self.restore_saved_draft(&key, &composer, cx);
            }
        }
    }

    /// A box changed: written a moment later, once typing pauses.
    pub(crate) fn drafts_changed(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PAUSE).await;
            this.update(cx, |this, cx| this.save_drafts(cx)).ok();
        });
        self.drafts.pending = Some(task);
    }

    /// Writes now: what every box has, and what the file had for boxes not
    /// made yet this run.
    pub(crate) fn save_drafts(&mut self, cx: &App) {
        self.drafts.pending = None;
        if crate::handover::waiting() {
            return;
        }
        let Some(path) = self.drafts.path.clone() else { return };
        let mut all = self.drafts.saved.clone();
        let mut boxes: Vec<(String, Entity<crate::composer::Composer>)> = self.composers.iter().map(|(k, c)| (k.clone(), c.clone())).collect();
        if let Some(new) = &self.new_thread.composer {
            boxes.push((NEW_THREAD.to_owned(), new.clone()));
        }
        for (key, composer) in boxes {
            let (text, images) = composer.read(cx).draft_for_saving(cx);
            let draft = Draft { text, images };
            if draft.is_empty() {
                all.remove(&key);
            } else {
                all.insert(key, draft);
            }
        }
        if let Err(e) = write(&path, &all) {
            eprintln!("threadbear: couldn't save the drafts: {e}");
        }
        self.drafts.saved = all;
    }
}

#[cfg(test)]
mod tests {
    use super::{read, write, Draft};
    use std::collections::HashMap;
    use threadbear_core::reducer::ImageRef;

    #[test]
    fn drafts_go_round_and_an_empty_set_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop-drafts.json");
        let mut all = HashMap::new();
        all.insert("t1".to_owned(), Draft { text: "half a thought \u{FFFC}".into(), images: vec![ImageRef::new("/tmp/a.png", "image/png")] });
        write(&path, &all).unwrap();
        assert_eq!(read(&path), all);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        write(&path, &HashMap::new()).unwrap();
        assert!(!path.exists());
    }
}
