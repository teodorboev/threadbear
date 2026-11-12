// A text file open in the side pane, with no UI in it: ports of the Swift
// app's TextFileDocument (ThreadbearCore/Threads/TextFileDocument.swift), the
// editing half of its TextFileController (Views/TextFileView.swift) and the
// MarkdownView pick (Transcript/FileLinks.swift).
//
// Editing reads and writes the file on this machine, as the Swift app does:
// only a thread on this runner gets a pencil, as another Mac's file would only
// be a copy here. The runner has no call to write a file, and that's on
// purpose: an edit never goes anywhere but the file Mike is looking at.
use crate::highlights::is_markdown;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

/// Read no more than this; the pane is for a look and small edits, not a log
/// viewer. A file this long or longer is shown cut off and never written back.
pub const LIMIT: usize = 2_000_000;

/// How long after the last keystroke the file is written (`saveDelay`).
pub const SAVE_DELAY_MS: u64 = 800;

/// A write comes as a burst of events; one read a short while after the last.
pub const CHECK_DELAY_MS: u64 = 120;

/// What a change on disk means for what's on screen (`TextFileDocument.Change`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The file says what the view already has (its own write, a touch).
    Same,
    /// Nothing unsaved in the view, or the file changed to exactly what's
    /// typed: show the file's text.
    Reload(String),
    /// Someone else wrote the file while the view holds unsaved edits. Neither
    /// side is thrown away until Mike picks one.
    Conflict(String),
    /// The file is gone (renamed away, deleted).
    Gone,
}

struct ReadText {
    text: String,
    truncated: bool,
    lossy: bool,
}

/// The file on disk: what was last read from or written to it, and whether it
/// can be written back (`TextFileDocument`).
#[derive(Debug)]
pub struct TextFileDocument {
    path: PathBuf,
    saved: String,
    /// Only the start was read (`LIMIT`): shown, but never written back, which
    /// would cut the file off.
    truncated: bool,
    /// The bytes weren't UTF-8 (shown with replacement characters), so saving
    /// would change what the file holds: read only.
    lossy: bool,
}

impl TextFileDocument {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), saved: String::new(), truncated: false, lossy: false }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The text as last read from or written to the file.
    pub fn saved(&self) -> &str {
        &self.saved
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn lossy(&self) -> bool {
        self.lossy
    }

    pub fn is_editable(&self) -> bool {
        !self.truncated && !self.lossy
    }

    fn read(&self) -> io::Result<ReadText> {
        let file = crate::open_policy::local_open(&self.path)?;
        let mut data = Vec::new();
        file.take(LIMIT as u64).read_to_end(&mut data)?;
        let truncated = data.len() == LIMIT;
        Ok(match String::from_utf8(data) {
            Ok(text) => ReadText { text, truncated, lossy: false },
            Err(bad) => ReadText { text: String::from_utf8_lossy(bad.as_bytes()).into_owned(), truncated, lossy: true },
        })
    }

    /// Reads the file. An error if it can't be read at all.
    pub fn load(&mut self) -> io::Result<&str> {
        let read = self.read()?;
        self.truncated = read.truncated;
        self.lossy = read.lossy;
        self.saved = read.text;
        Ok(&self.saved)
    }

    /// Writes `text` over the file, in place: the same inode, mode and owner
    /// (an atomic replace would give it the temp file's, and lose an
    /// executable bit). Nothing happens when the file is read only or `text`
    /// is what's there already, and a file that's gone isn't made again.
    pub fn save(&mut self, text: &str) -> io::Result<()> {
        if !self.is_editable() || text == self.saved {
            return Ok(());
        }
        let mut file = crate::open_policy::local_open_for_write(&self.path)?;
        file.set_len(0)?;
        file.write_all(text.as_bytes())?;
        self.saved = text.to_owned();
        Ok(())
    }

    /// The file's text if it no longer says what was last read or written
    /// (someone else wrote it), else none. Read just before a save, so an edit
    /// the watch hasn't reported yet isn't written over. An error if it can't
    /// be read, as then nobody can say it's safe to write. A change to
    /// something too big or not UTF-8 makes the file read only, as a load
    /// would, so nothing cut off or mangled is ever written back.
    pub fn changed_on_disk(&mut self) -> io::Result<Option<String>> {
        let read = self.read()?;
        // Even when it decodes to the same text: bytes that aren't UTF-8 can
        // read as what was saved and still be mangled by a write.
        self.truncated = read.truncated;
        self.lossy = read.lossy;
        Ok((read.text != self.saved).then_some(read.text))
    }

    /// What to do when the file on disk has changed: `on_disk` is its text
    /// now, `shown` what the view holds (which differs from `saved` only by
    /// unsaved edits).
    pub fn change(on_disk: &str, saved: &str, shown: &str) -> Change {
        if on_disk == saved {
            Change::Same
        } else if shown == saved || on_disk == shown {
            Change::Reload(on_disk.to_owned())
        } else {
            Change::Conflict(on_disk.to_owned())
        }
    }

    /// Takes the file's text as the saved one: for a conflict, when Mike keeps
    /// the file's (the view then shows it) or the view's (its next save then
    /// writes over it). Whether it can be written stays as the read that found
    /// it said: a conflict over a file that's become too big or not UTF-8
    /// mustn't make it editable.
    pub fn adopt(&mut self, text: impl Into<String>) {
        self.saved = text.into();
    }

    /// Reads the file after the watch saw it change, and says what that means
    /// for `shown`, what the view holds now (`check`).
    pub fn check(&mut self, shown: &str) -> Change {
        let Ok(read) = self.read() else { return Change::Gone };
        // Whether it can be written changed, even with the same text (a binary
        // file put back as UTF-8).
        let flags_changed = read.truncated != self.truncated || read.lossy != self.lossy;
        let change = if shown == self.saved {
            // No unsaved edits: show the file as it is now.
            if read.text == self.saved && !flags_changed {
                Change::Same
            } else {
                Change::Reload(read.text.clone())
            }
        } else if !self.is_editable() || read.truncated || read.lossy {
            // Unsaved edits, and a file that can't take them now (too big, not
            // UTF-8) or couldn't before: they're never thrown away by the
            // watch. Whether it can be written follows this read, so no save
            // writes it back cut off or mangled.
            self.truncated = read.truncated;
            self.lossy = read.lossy;
            if read.text == shown {
                Change::Reload(read.text.clone())
            } else if read.text == self.saved && !flags_changed {
                Change::Same
            } else {
                Change::Conflict(read.text.clone())
            }
        } else {
            Self::change(&read.text, &self.saved, shown)
        };
        if let Change::Reload(text) = &change {
            self.truncated = read.truncated;
            self.lossy = read.lossy;
            self.saved = text.clone();
        }
        change
    }
}

// ---------------------------------------------------------------- editing

/// The bars' words, so the pane and the tests say the same.
pub mod words {
    pub const UNREADABLE: &str = "This file couldn't be read.";
    pub const CUT: &str = "Only the start of this file is shown.";
    pub const CUT_NO_EDIT: &str = "Only the start of this file is shown, so it can't be edited here.";
    pub const NOT_UTF8: &str = "This file isn't UTF-8 text, so it can't be edited here.";
    pub const GONE: &str = "This file isn't there any more.";
    pub const CONFLICT: &str = "Changed on disk while you were editing.";
    pub const TAKE_FILES: &str = "Take the file's";
    pub const TAKE_FILES_TIP: &str = "Show what's in the file now. What you typed since your last save goes.";
    pub const KEEP_MINE: &str = "Keep mine";
    pub const KEEP_MINE_TIP: &str = "Save what's here over the file.";
    pub const EDIT_TIP: &str = "Edit the file here. It's saved as you type.";
    pub const DONE_TIP: &str = "Done. What you typed is saved.";
    /// Put at the end of a cut-off file's text.
    pub const CUT_TAIL: &str = "\n\n… the rest is cut off here. Open it in its app to see it all.";

    pub fn cant_save(truncated: bool) -> String {
        format!(
            "{}, so what you typed can't be saved into it. Copy it out, or go Back to leave it.",
            if truncated { "The file is now too big to edit here" } else { "The file is no longer UTF-8 text" }
        )
    }
}

/// What a save came to (`saveNow`'s answer, and what it asks of the view).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Saved {
    /// Nothing typed is waiting: written, or there was nothing to write.
    Done,
    /// Done, and the view now shows this: undone back to what was saved while
    /// a conflict waited, so the file's side won without asking.
    DoneShowing(String),
    /// What's typed is still unsaved: a conflict waits on Mike, or the file
    /// can't take it (the note says why).
    Kept,
}

/// One file being shown and perhaps edited in the pane: the document, whether
/// it's taking typing, what's typed and not saved, and the bars over it (the
/// editing half of `TextFileController`). The view hands it what it holds and
/// shows what it hands back.
#[derive(Debug)]
pub struct EditSession {
    pub document: TextFileDocument,
    editing: bool,
    /// The file's text, while it's changed under unsaved edits.
    pub conflict: Option<String>,
    /// The grey line over the text: cut off, can't be edited, gone.
    pub note: Option<String>,
    /// Typed since the last save or load: the view holds text the file doesn't.
    drafted: bool,
}

impl EditSession {
    /// Reads `path` for the pane. A file that can't be read says so in its note.
    pub fn open(path: impl Into<PathBuf>, editing: bool) -> Self {
        let mut document = TextFileDocument::new(path);
        let read = document.load().is_ok();
        let note = match read {
            false => Some(words::UNREADABLE.to_owned()),
            true if document.truncated() => Some(words::CUT.to_owned()),
            true => None,
        };
        let mut session = Self { document, editing: false, conflict: None, note, drafted: false };
        if editing {
            session.editing = true;
            session.apply_editing();
        }
        session
    }

    pub fn editing(&self) -> bool {
        self.editing
    }

    pub fn drafted(&self) -> bool {
        self.drafted
    }

    /// Whether the view takes typing now.
    pub fn takes_typing(&self) -> bool {
        self.editing && self.document.is_editable()
    }

    /// The text to put in the view for the file as it is: a cut-off file says
    /// so at its end.
    pub fn view_text(&self, text: &str) -> String {
        if self.document.truncated() {
            format!("{text}{}", words::CUT_TAIL)
        } else {
            text.to_owned()
        }
    }

    /// What the view holds now, as the file's text: what's typed while there's
    /// any, else what was loaded (`shownText`).
    pub fn shown<'a>(&'a self, in_view: &'a str) -> &'a str {
        if self.editing && self.drafted {
            in_view
        } else {
            self.document.saved()
        }
    }

    /// The view's side of `editing`: why a file that can't be written back
    /// can't (`applyEditing`).
    fn apply_editing(&mut self) {
        if self.editing && !self.document.is_editable() {
            self.note = Some(if self.document.truncated() { words::CUT_NO_EDIT } else { words::NOT_UTF8 }.to_owned());
        }
    }

    /// Something was typed (`textDidChange`).
    pub fn typed(&mut self) {
        if self.editing {
            self.drafted = true;
        }
    }

    /// The pencil, Done or the Markdown menu. Done while what's typed can't be
    /// saved stays editing, so it's still there for the conflict bar or
    /// another try: false then, and the bar's state goes back to editing.
    pub fn set_editing(&mut self, editing: bool, in_view: &str) -> bool {
        if self.editing == editing {
            return true;
        }
        if !editing && matches!(self.save_now(in_view), Saved::Kept) {
            return false;
        }
        if !editing {
            // Saved, or a file that never took typing: nothing's waiting.
            self.drafted = false;
        }
        self.editing = editing;
        self.apply_editing();
        true
    }

    /// Writes what's typed, unless a conflict is waiting on Mike. If the file
    /// changed on disk since it was read (an agent's edit the watch hasn't
    /// reported yet), it isn't written over: the conflict bar asks
    /// (`saveNow`). `in_view` is what the view holds.
    pub fn save_now(&mut self, in_view: &str) -> Saved {
        if !self.editing || !self.drafted {
            return Saved::Done;
        }
        if let Some(file_text) = self.conflict.clone() {
            // Undone back to what was saved: nothing of Mike's left to keep,
            // so the file's side wins without asking.
            if in_view != self.document.saved() {
                return Saved::Kept;
            }
            return Saved::DoneShowing(self.use_file(file_text));
        }
        // Typed, but the file can't take it now: it stays on screen, read
        // only, rather than go.
        if !self.document.is_editable() {
            self.apply_editing();
            self.note = Some(words::cant_save(self.document.truncated()));
            return Saved::Kept;
        }
        if in_view == self.document.saved() {
            self.drafted = false;
            return Saved::Done;
        }
        match self.document.changed_on_disk() {
            Err(error) => {
                self.note = Some(format!("Couldn't read the file to check it before saving: {error}"));
                return Saved::Kept;
            }
            // It turned too big or not UTF-8: never written back. What's typed
            // stays on screen, read only, to copy out.
            Ok(_) if !self.document.is_editable() => {
                self.apply_editing();
                self.note = Some(words::cant_save(self.document.truncated()));
                return Saved::Kept;
            }
            Ok(Some(on_disk)) if on_disk != in_view => {
                self.conflict = Some(on_disk);
                return Saved::Kept;
            }
            Ok(_) => {}
        }
        match self.document.save(in_view) {
            Ok(()) => {
                self.drafted = false;
                Saved::Done
            }
            Err(error) => {
                self.note = Some(format!("Couldn't save: {error}"));
                Saved::Kept
            }
        }
    }

    /// What the watch found. The text to show, when the view should change.
    pub fn file_changed(&mut self, change: Change) -> Option<String> {
        match change {
            Change::Same => None,
            Change::Reload(text) => {
                self.conflict = None;
                self.drafted = false;
                // It may have turned read only, or back.
                self.note = self.document.truncated().then(|| words::CUT.to_owned());
                self.apply_editing();
                Some(self.view_text(&text))
            }
            Change::Conflict(text) => {
                self.conflict = Some(text);
                // It may have turned read only: no more typing it can't take.
                self.apply_editing();
                None
            }
            Change::Gone => {
                self.conflict = None;
                self.note = Some(words::GONE.to_owned());
                None
            }
        }
    }

    /// The conflict bar's "Take the file's": the file wins. The text to show.
    pub fn use_file(&mut self, text: String) -> String {
        self.conflict = None;
        self.drafted = false;
        self.document.adopt(text.clone());
        // It may have become read only (too big, not UTF-8).
        self.apply_editing();
        self.view_text(&text)
    }

    /// Its "Keep mine": what's typed is written over the file. The file's text
    /// is taken as read first, so the save doesn't find it changed and ask
    /// again. A file that's turned too big or not UTF-8 isn't written: what's
    /// typed stays on screen, read only.
    pub fn keep_mine(&mut self, in_view: &str) -> Saved {
        if let Some(on_disk) = self.conflict.take() {
            self.document.adopt(on_disk);
        }
        if !self.document.is_editable() {
            self.apply_editing();
            self.note = Some(words::cant_save(self.document.truncated()));
            return Saved::Kept;
        }
        self.save_now(in_view)
    }
}

// ---------------------------------------------------------------- Preview or Markdown

/// How a Markdown file shows in the pane: Preview draws it as a page,
/// Markdown is its raw text, ready to type in. The menu in the bar switches
/// one file and remembers the pick for the next (`MarkdownView`; Wayne, 7 Nov:
/// he wanted to read a `.md` like a doc, and flip to its text from the same
/// place).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkdownView {
    #[default]
    Preview,
    Markdown,
}

impl MarkdownView {
    pub const ALL: [MarkdownView; 2] = [MarkdownView::Preview, MarkdownView::Markdown];

    /// The Swift app's raw value, kept in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            MarkdownView::Preview => "preview",
            MarkdownView::Markdown => "markdown",
        }
    }

    pub fn from_name(name: &str) -> Option<MarkdownView> {
        Self::ALL.into_iter().find(|v| v.name() == name)
    }

    pub fn title(self) -> &'static str {
        match self {
            MarkdownView::Preview => "Preview",
            MarkdownView::Markdown => "Markdown",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            MarkdownView::Preview => "doc.richtext",
            MarkdownView::Markdown => "chevron.left.forwardslash.chevron.right",
        }
    }

    /// The menu's tooltip while this is showing.
    pub fn tip(self) -> &'static str {
        match self {
            MarkdownView::Preview => "Showing it drawn as a page. Pick Markdown to edit the text.",
            MarkdownView::Markdown => "Showing the Markdown text, ready to type in. Pick Preview to read it as a page.",
        }
    }

    /// Whether a file opens with its raw text up for typing: a Markdown file
    /// once Markdown was the last pick. Anything else opens to read.
    pub fn opens_raw(path: &str, last_pick: MarkdownView) -> bool {
        is_markdown(path) && last_pick == MarkdownView::Markdown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("tb-textfile-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Folder(path)
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_change_on_disk_keeps_both_sides_apart() {
        // Its own write, or a touch: nothing to do.
        assert_eq!(TextFileDocument::change("a", "a", "a"), Change::Same);
        assert_eq!(TextFileDocument::change("a", "a", "a typed"), Change::Same);
        // Nothing unsaved: take the file's.
        assert_eq!(TextFileDocument::change("b", "a", "a"), Change::Reload("b".into()));
        // The file now says what's typed (the view's save, seen late).
        assert_eq!(TextFileDocument::change("a typed", "a", "a typed"), Change::Reload("a typed".into()));
        // Both changed: Mike picks.
        assert_eq!(TextFileDocument::change("b", "a", "a typed"), Change::Conflict("b".into()));
    }

    #[test]
    fn saves_in_place_and_leaves_no_old_tail() {
        let folder = Folder::new("save");
        let path = folder.file("run.sh", b"#!/bin/sh\necho hi\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut doc = TextFileDocument::new(&path);
        assert_eq!(doc.load().unwrap(), "#!/bin/sh\necho hi\n");
        assert!(doc.is_editable());
        doc.save("#!/bin/sh\necho bye\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "#!/bin/sh\necho bye\n");
        assert_eq!(doc.saved(), "#!/bin/sh\necho bye\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755, "the file keeps its mode");
        }
        // Shorter text doesn't leave the old tail behind.
        doc.save("x").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "x");
    }

    #[test]
    fn a_file_thats_gone_isnt_made_again() {
        let folder = Folder::new("gone");
        let path = folder.file("notes.txt", b"a");
        let mut doc = TextFileDocument::new(&path);
        doc.load().unwrap();
        fs::remove_file(&path).unwrap();
        assert!(doc.save("b").is_err());
        assert!(!path.exists());
    }

    /// Read just before a save: someone else's write the watch hasn't reported
    /// yet shows up, its own write doesn't.
    #[test]
    fn changed_on_disk_sees_someone_elses_write() {
        let folder = Folder::new("changed");
        let path = folder.file("notes.txt", b"a");
        let mut doc = TextFileDocument::new(&path);
        doc.load().unwrap();
        assert_eq!(doc.changed_on_disk().unwrap(), None);
        doc.save("b").unwrap();
        assert_eq!(doc.changed_on_disk().unwrap(), None, "its own save");
        fs::write(&path, "c").unwrap();
        assert_eq!(doc.changed_on_disk().unwrap().as_deref(), Some("c"));
        // Turned into something it can't write back: read only from then on.
        fs::write(&path, [0x68, 0x69, 0xFF, 0xFE]).unwrap();
        assert!(doc.changed_on_disk().unwrap().is_some());
        assert!(!doc.is_editable());
        doc.adopt("whatever");
        assert!(!doc.is_editable(), "taking the file's text doesn't make it editable");
        // Can't be read at all: nobody can say a save is safe.
        fs::remove_file(&path).unwrap();
        assert!(doc.changed_on_disk().is_err());
    }

    /// Bytes that aren't UTF-8 can decode to exactly the saved text: the check
    /// still marks it read only.
    #[test]
    fn changed_on_disk_sees_bytes_that_read_the_same() {
        let folder = Folder::new("same");
        let path = folder.file("same.txt", "hi\u{FFFD}".as_bytes());
        let mut doc = TextFileDocument::new(&path);
        doc.load().unwrap();
        assert!(doc.is_editable());
        fs::write(&path, [0x68, 0x69, 0xFF]).unwrap();
        assert_eq!(doc.changed_on_disk().unwrap(), None);
        assert!(!doc.is_editable());
    }

    #[test]
    fn binary_and_cut_off_files_are_read_only() {
        let folder = Folder::new("binary");
        let path = folder.file("blob.txt", &[0x68, 0x69, 0xFF, 0xFE]);
        let mut doc = TextFileDocument::new(&path);
        doc.load().unwrap();
        assert!(doc.lossy());
        assert!(!doc.is_editable());
        doc.save("changed").unwrap();
        assert_eq!(fs::read(&path).unwrap(), [0x68, 0x69, 0xFF, 0xFE], "a read-only file isn't written");

        let big = folder.file("big.txt", &vec![b'a'; LIMIT + 10]);
        let mut doc = TextFileDocument::new(&big);
        assert_eq!(doc.load().unwrap().len(), LIMIT);
        assert!(doc.truncated());
        assert!(!doc.is_editable());
    }

    /// The watch, when a file being edited turns binary: the edits aren't
    /// thrown away (a conflict, read only), and putting it back as UTF-8 with
    /// the same text makes it editable again.
    #[test]
    fn the_watch_keeps_edits_when_the_file_turns_binary() {
        let folder = Folder::new("turns");
        let path = folder.file("turns.txt", b"hi");
        let mut doc = TextFileDocument::new(&path);
        doc.load().unwrap();
        fs::write(&path, [0x68, 0x69, 0xFF]).unwrap();
        assert_eq!(doc.check("hi typed"), Change::Conflict("hi\u{FFFD}".into()));
        assert!(!doc.is_editable());
        // Checked again before anyone picks: the edits still aren't dropped.
        assert_eq!(doc.check("hi typed"), Change::Conflict("hi\u{FFFD}".into()));
        assert!(!doc.is_editable());
        // Take the file's, then it's put back as UTF-8 with the same text.
        doc.adopt("hi\u{FFFD}");
        fs::write(&path, "hi\u{FFFD}").unwrap();
        assert_eq!(doc.check("hi\u{FFFD}"), Change::Reload("hi\u{FFFD}".into()));
        assert!(doc.is_editable());
    }

    #[test]
    fn the_watch_reloads_what_someone_else_wrote() {
        let folder = Folder::new("watch");
        let path = folder.file("notes.md", b"# One\n");
        let mut doc = TextFileDocument::new(&path);
        doc.load().unwrap();
        // An agent's atomic write: a new file renamed over this one.
        let temp = folder.file("notes.md.tmp", b"# Two\n");
        fs::rename(&temp, &path).unwrap();
        assert_eq!(doc.check("# One\n"), Change::Reload("# Two\n".into()));
        assert_eq!(doc.saved(), "# Two\n");

        // Its own save isn't a change.
        doc.save("# Two\nmore\n").unwrap();
        assert_eq!(doc.check("# Two\nmore\n"), Change::Same);

        // Someone else writes while there are unsaved edits: a conflict, and
        // nothing adopted until Mike picks.
        fs::write(&path, "# Three\n").unwrap();
        assert_eq!(doc.check("# Two\nmore\ntyping"), Change::Conflict("# Three\n".into()));
        assert_eq!(doc.saved(), "# Two\nmore\n");

        fs::remove_file(&path).unwrap();
        assert_eq!(doc.check("# Two\nmore\ntyping"), Change::Gone);
    }

    #[test]
    fn typing_is_saved_and_a_write_under_it_asks_first() {
        let folder = Folder::new("session");
        let path = folder.file("notes.md", b"# Notes\n");
        let mut session = EditSession::open(&path, true);
        assert!(session.takes_typing());
        assert_eq!(session.save_now("# Notes\n"), Saved::Done, "nothing typed, nothing to write");

        session.typed();
        assert_eq!(session.save_now("# Notes\nmore\n"), Saved::Done);
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Notes\nmore\n");
        assert!(!session.drafted());

        // An agent writes the file before the watch says so: not written over.
        session.typed();
        fs::write(&path, "# Theirs\n").unwrap();
        assert_eq!(session.save_now("# Notes\nmore\nmine\n"), Saved::Kept);
        assert_eq!(session.conflict.as_deref(), Some("# Theirs\n"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Theirs\n");
        // Done can't leave while it waits.
        assert!(!session.set_editing(false, "# Notes\nmore\nmine\n"));
        assert!(session.editing());

        // Keep mine writes over it, and doesn't ask again.
        assert_eq!(session.keep_mine("# Notes\nmore\nmine\n"), Saved::Done);
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Notes\nmore\nmine\n");
        assert_eq!(session.conflict, None);
        assert!(session.set_editing(false, "# Notes\nmore\nmine\n"));
    }

    #[test]
    fn taking_the_files_side_drops_whats_typed() {
        let folder = Folder::new("take");
        let path = folder.file("a.txt", b"one");
        let mut session = EditSession::open(&path, true);
        session.typed();
        fs::write(&path, "theirs").unwrap();
        let shown = session.shown("one mine").to_owned();
        let change = session.document.check(&shown);
        assert_eq!(change, Change::Conflict("theirs".into()));
        assert_eq!(session.file_changed(change), None);
        assert_eq!(session.conflict.as_deref(), Some("theirs"));
        assert_eq!(session.use_file("theirs".into()), "theirs");
        assert_eq!(session.conflict, None);
        assert!(!session.drafted());
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs", "taking the file's writes nothing");
    }

    /// Undone back to what was saved while a conflict waits: nothing of Mike's
    /// is left, so the file's side wins without asking.
    #[test]
    fn undone_back_to_the_saved_text_takes_the_files() {
        let folder = Folder::new("undone");
        let path = folder.file("a.txt", b"one");
        let mut session = EditSession::open(&path, true);
        session.typed();
        fs::write(&path, "theirs").unwrap();
        assert_eq!(session.save_now("one typed"), Saved::Kept);
        assert_eq!(session.save_now("one"), Saved::DoneShowing("theirs".into()));
        assert_eq!(session.conflict, None);
    }

    #[test]
    fn a_file_that_cant_take_typing_says_why() {
        let folder = Folder::new("why");
        let binary = folder.file("blob.txt", &[0x68, 0x69, 0xFF]);
        let session = EditSession::open(&binary, true);
        assert!(!session.takes_typing());
        assert_eq!(session.note.as_deref(), Some(words::NOT_UTF8));

        let big = folder.file("big.md", &vec![b'a'; LIMIT + 1]);
        let session = EditSession::open(&big, false);
        assert_eq!(session.note.as_deref(), Some(words::CUT));
        assert!(session.view_text("aaa").ends_with("Open it in its app to see it all."));
        let session = EditSession::open(&big, true);
        assert_eq!(session.note.as_deref(), Some(words::CUT_NO_EDIT));

        let session = EditSession::open(folder.0.join("missing.txt"), true);
        assert_eq!(session.note.as_deref(), Some(words::UNREADABLE));
    }

    /// Typed, then the file turns binary under it: what's typed stays on
    /// screen, read only, and nothing is written.
    #[test]
    fn typing_into_a_file_that_turned_binary_isnt_written() {
        let folder = Folder::new("turned");
        let path = folder.file("a.txt", b"one");
        let mut session = EditSession::open(&path, true);
        session.typed();
        fs::write(&path, [0x6f, 0x6e, 0x65, 0xFF]).unwrap();
        assert_eq!(session.save_now("one typed"), Saved::Kept);
        assert_eq!(session.note, Some(words::cant_save(false)));
        assert!(!session.takes_typing());
        assert_eq!(fs::read(&path).unwrap(), [0x6f, 0x6e, 0x65, 0xFF]);
    }

    #[test]
    fn markdown_opens_as_a_page_until_markdown_is_picked() {
        assert_eq!(MarkdownView::default(), MarkdownView::Preview);
        assert!(!MarkdownView::opens_raw("/tmp/README.md", MarkdownView::Preview));
        assert!(!MarkdownView::opens_raw("/tmp/notes.MDX", MarkdownView::Preview));
        assert!(MarkdownView::opens_raw("/tmp/README.markdown", MarkdownView::Markdown), "the pick is remembered");
        assert!(!MarkdownView::opens_raw("/tmp/hello.ts", MarkdownView::Markdown), "code still opens to read");
        assert_eq!(MarkdownView::from_name("markdown"), Some(MarkdownView::Markdown));
        assert_eq!(MarkdownView::from_name("raw"), None);
    }
}
