//! Files only this user's account may read, written whole: what's typed in a
//! box (drafts, review drafts), the automation port's token, the sign-in
//! script, crash and hang records, the app's own settings and caches.
//!
//! Each is written into a new file of its own beside the real one, made with
//! owner-only permissions from the start (never written first and tightened
//! after), under a name nobody can guess or make first, and then renamed over
//! the real one. Daybreak, 11 Nov (findings 88 and 94): drafts and review
//! drafts went to `<name>.json.tmp` with `fs::write`, so under umask 022 the
//! file was 0644 until the chmod after it, and another account on the machine
//! that opened it in that moment kept reading it after the chmod and rename.
//! A fixed temp name had two more holes: a file someone else made there first
//! was written into (theirs, and open to them), and a symlink there was
//! followed. A new file with `O_EXCL` has neither.
//!
//! Every private file in `desktop/` goes through here, so there's one place
//! to get this right (tests/private_file_source.rs fails on a writer that
//! doesn't). Off Unix there are no modes: a file and any folder made for it
//! take the access of the folder they're in, which in the user's own profile
//! is that user's.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Writes `data` as `path`, readable and writable by this user only.
pub fn write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    write_with_mode(path, data, 0o600)
}

/// `write`, with the file's mode given: 0o700 for a script only this user
/// may run. Group and other bits are never set, whatever's asked.
pub fn write_with_mode(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    };
    make_dir(&dir)?;
    let (mut file, temp) = new_file(&dir, path, mode & 0o700)?;
    let written = file.write_all(data).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written.and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(())
}

/// A folder for this user only, made if it isn't there yet, for files that
/// go through `write` and `append` (the activity log's).
pub fn dir(dir: &Path) -> std::io::Result<()> {
    make_dir(dir)
}

/// Opens `path` to add to its end, for a log written a line at a time that
/// another process adds to as well (the Swift app writes the same activity
/// files; the update script's output goes into the update log), so it can't
/// be written whole and renamed like the rest. A new one is this user's only
/// from its first byte. A symlink there is refused, not followed, and one
/// made before this that others could read is made owner-only first.
pub fn append(path: &Path) -> std::io::Result<std::fs::File> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    };
    make_dir(&dir)?;
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} is a symlink", path.display())));
    }
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
        // A link put there after the check above isn't followed either
        // (Codex's review, 12 Nov): the open itself refuses one.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    // The file just opened, not the name, so it's the one being written.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} isn't a file", path.display())));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(file)
}

/// A copy of what the open file `from` holds, as `name` in a new folder of
/// its own under `folder`, only for this user: for handing a file to
/// another app (a film dragged out of the pane) without handing it a path
/// someone could swap before that app reads it (Codex's second review of
/// the media views, 12 Nov). It's an APFS clone where the disk can make
/// one, which is instant whatever the size; elsewhere the bytes are copied,
/// and only up to `copy_limit`. Copies older than a day there are cleared
/// out first.
pub fn snapshot(from: &std::fs::File, folder: &Path, name: &str, copy_limit: u64) -> std::io::Result<PathBuf> {
    let name = Path::new(name).file_name().filter(|n| !n.is_empty()).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no name to give the copy"))?.to_owned();
    make_dir(folder)?;
    // The folder itself, not a link someone put there first to a folder of
    // theirs, and this user's only.
    let meta = std::fs::symlink_metadata(folder)?;
    #[cfg(unix)]
    let shared = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o077 != 0;
    #[cfg(not(unix))]
    let shared = false;
    if !meta.is_dir() || shared {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} isn't a folder of this user's own", folder.display())));
    }
    clear_old(folder, std::time::Duration::from_secs(24 * 60 * 60));
    let dir = new_dir(folder)?;
    let path = dir.join(&name);
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::ffi::OsStrExt as _;
        let target = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
        // The clone takes the file's own mode, but its folder is this user's
        // alone; it's still made the owner's only, and read-only.
        if unsafe { libc::fclonefileat(from.as_raw_fd(), libc::AT_FDCWD, target.as_ptr(), 0) } == 0 {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))?;
            return Ok(path);
        }
    }
    let len = from.metadata()?.len();
    if len > copy_limit {
        let _ = std::fs::remove_dir(&dir);
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "too big to copy"));
    }
    let (mut file, temp) = new_file(&dir, &path, 0o600)?;
    // Not synced: it's a hand-over, read straight away, not kept.
    let copied = copy_all(from, &mut file, len);
    drop(file);
    if let Err(e) = copied.and_then(|()| std::fs::rename(&temp, &path)) {
        let _ = std::fs::remove_file(&temp);
        let _ = std::fs::remove_dir(&dir);
        return Err(e);
    }
    Ok(path)
}

/// `len` bytes of `from` from its start into `to`, read at their offsets so
/// whatever else reads `from` keeps its place.
fn copy_all(from: &std::fs::File, to: &mut std::fs::File, len: u64) -> std::io::Result<()> {
    let mut buf = vec![0u8; 1024 * 1024];
    let mut at = 0u64;
    while at < len {
        let want = usize::try_from((len - at).min(buf.len() as u64)).unwrap_or(buf.len());
        #[cfg(unix)]
        let got = std::os::unix::fs::FileExt::read_at(from, &mut buf[..want], at)?;
        #[cfg(windows)]
        let got = std::os::windows::fs::FileExt::seek_read(from, &mut buf[..want], at)?;
        if got == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the file got shorter while it was copied"));
        }
        to.write_all(&buf[..got])?;
        at += got as u64;
    }
    Ok(())
}

/// A new folder under `folder`, this user's only, that wasn't there before.
fn new_dir(folder: &Path) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut last = None;
    for _ in 0..16 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = folder.join(format!("{}-{nanos:x}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("couldn't make a folder to copy into")))
}

/// Removes `snapshot`'s folders under `folder` older than `age`: by then
/// whatever they were dropped on has read them. Only folders with the names
/// `new_dir` gives, never a link (an entry's metadata here doesn't follow
/// one), and `remove_dir_all` doesn't follow one swapped in partway either.
fn clear_old(folder: &Path, age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        if !is_snapshot_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let old = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|d| d > age);
        if old && meta.is_dir() && !meta.file_type().is_symlink() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// `<pid>-<hex nanos>-<count>`, as `new_dir` names them.
fn is_snapshot_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('-').collect();
    parts.len() == 3
        && !parts.iter().any(|p| p.is_empty())
        && parts[0].bytes().all(|b| b.is_ascii_digit())
        && parts[1].bytes().all(|b| b.is_ascii_hexdigit())
        && parts[2].bytes().all(|b| b.is_ascii_digit())
}

/// A folder that isn't there yet is made for this user only. One that is
/// keeps the permissions it has.
fn make_dir(dir: &Path) -> std::io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// A file that wasn't there before, beside `path`, with only `mode`'s owner
/// bits. `create_new` is `O_CREAT | O_EXCL`, which also refuses to follow a
/// symlink at that name; the name has the process, the time and a count in
/// it, and a clash (someone made it first) just tries another.
fn new_file(dir: &Path, path: &Path, mode: u32) -> std::io::Result<(std::fs::File, PathBuf)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let mut last = None;
    for _ in 0..16 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let temp = dir.join(format!(".{name}.{}-{nanos:x}-{}.tmp", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
        #[cfg(not(unix))]
        let _ = mode;
        match options.open(&temp) {
            Ok(file) => return Ok((file, temp)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("couldn't make a file to write into")))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-private-file-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // As a runner home can be: listable and openable by everyone.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_log_is_added_to_and_only_for_its_owner() {
        use std::io::Write as _;
        let dir = scratch("append");
        // Its folder made too, owner-only.
        let path = dir.join("activity").join("2026-11-11.jsonl");
        append(&path).unwrap().write_all(b"one\n").unwrap();
        append(&path).unwrap().write_all(b"two\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\n");
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir.join("activity")), 0o700);
        // One made before, readable by others, is tightened, its lines kept.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        append(&path).unwrap().write_all(b"three\n").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\nthree\n");
        // A symlink isn't followed.
        let elsewhere = dir.join("elsewhere");
        std::fs::write(&elsewhere, b"").unwrap();
        let link = dir.join("activity").join("2026-11-12.jsonl");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        assert!(append(&link).is_err());
        assert_eq!(std::fs::read(&elsewhere).unwrap(), b"");
        // Nor is a folder where the file goes written.
        assert!(append(&dir.join("activity")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn written_whole_and_only_for_its_owner() {
        let dir = scratch("whole");
        let path = dir.join("desktop-drafts.json");
        write(&path, b"{\"t\":{\"text\":\"secret\"}}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"t\":{\"text\":\"secret\"}}");
        assert_eq!(mode(&path), 0o600);
        // An older file someone made readable is replaced, not written into.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(mode(&path), 0o600);
        // Nothing left behind.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        write_with_mode(&dir.join("sign-in.sh"), b"#!/bin/sh\n", 0o755).unwrap();
        assert_eq!(mode(&dir.join("sign-in.sh")), 0o700, "never group or other bits");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A copy for handing to another app holds what the open file held when
    /// it was made, at a path of its own that's the owner's only: swapping
    /// or rewriting the original afterwards changes nothing in it.
    #[test]
    fn a_snapshot_is_the_open_files_bytes_and_the_owners_only() {
        let dir = scratch("snapshot");
        let original = dir.join("hero.mp4");
        std::fs::write(&original, b"the film that was checked").unwrap();
        let held = std::fs::File::open(&original).unwrap();
        let folder = dir.join("drags");
        let copy = snapshot(&held, &folder, "hero.mp4", 1 << 20).unwrap();
        assert_eq!(copy.file_name().unwrap(), "hero.mp4");
        assert_ne!(copy, original);
        assert!(copy.starts_with(&folder));
        assert_eq!(mode(copy.parent().unwrap()), 0o700);
        assert_eq!(mode(&copy) & 0o077, 0);
        // The original swapped for a link to something private, then
        // rewritten where it is: the copy still holds the checked bytes.
        std::fs::remove_file(&original).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", &original).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"the film that was checked");
        assert!(!std::fs::symlink_metadata(&copy).unwrap().file_type().is_symlink());
        // Each copy has a folder of its own, and a name can't climb out.
        let again = snapshot(&held, &folder, "../../escape.mp4", 1 << 20).unwrap();
        assert_ne!(again.parent(), copy.parent());
        assert_eq!(again.file_name().unwrap(), "escape.mp4");
        assert!(again.starts_with(&folder));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A drags folder that's a link to somewhere else, or one others can
    /// write to, isn't copied into: a copy there could be swapped.
    #[test]
    fn a_snapshot_isnt_made_in_a_folder_that_isnt_the_users_own() {
        let dir = scratch("snapshot-folder");
        let original = dir.join("hero.mp4");
        std::fs::write(&original, b"film").unwrap();
        let held = std::fs::File::open(&original).unwrap();
        let theirs = dir.join("theirs");
        std::fs::create_dir(&theirs).unwrap();
        std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o700)).unwrap();
        let linked = dir.join("drags");
        std::os::unix::fs::symlink(&theirs, &linked).unwrap();
        assert!(snapshot(&held, &linked, "hero.mp4", 1 << 20).is_err());
        assert_eq!(std::fs::read_dir(&theirs).unwrap().count(), 0);
        // Cleaning up touches only its own folders, old as they may be.
        let own = dir.join("own");
        std::fs::create_dir(&own).unwrap();
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o700)).unwrap();
        let project = own.join("OldProject");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("notes.txt"), b"keep").unwrap();
        let stale = own.join("123-abc-0");
        std::fs::create_dir(&stale).unwrap();
        let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 60 * 60);
        for old in [&project, &stale] {
            std::fs::File::open(old).unwrap().set_modified(day_ago).unwrap();
        }
        snapshot(&held, &own, "hero.mp4", 1 << 20).unwrap();
        assert!(project.join("notes.txt").exists());
        assert!(!stale.exists());
        let open = dir.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(snapshot(&held, &open, "hero.mp4", 1 << 20).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_folder_is_the_owners_only() {
        let dir = scratch("folder");
        let path = dir.join("a/b/desktop-review-drafts.json");
        write(&path, b"x").unwrap();
        assert_eq!(mode(&dir.join("a")), 0o700);
        assert_eq!(mode(&dir.join("a/b")), 0o700);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The old writers' fixed `<name>.json.tmp`, made first by someone else:
    /// theirs to read, or a symlink to somewhere of theirs. Neither is
    /// touched, and the real file is still only the owner's.
    #[test]
    fn a_file_or_symlink_made_first_at_the_old_temp_name_is_left_alone() {
        let dir = scratch("planted");
        let path = dir.join("desktop-drafts.json");
        let planted = dir.join("desktop-drafts.json.tmp");
        std::fs::write(&planted, b"").unwrap();
        std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o666)).unwrap();
        let elsewhere = dir.join("theirs");
        std::os::unix::fs::symlink(&elsewhere, dir.join("desktop-review-drafts.json.tmp")).unwrap();
        write(&path, b"secret").unwrap();
        write(&dir.join("desktop-review-drafts.json"), b"secret").unwrap();
        assert_eq!(std::fs::read(&planted).unwrap(), b"");
        assert!(!elsewhere.exists(), "the symlink was followed");
        assert_eq!(mode(&path), 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The race itself: another account opening each temp file as it
    /// appears. Played first against the old way (write, then chmod), to
    /// show the watcher can catch it here, then against `write`, which it
    /// must never catch with group or other bits set. A umask that already
    /// keeps them off (077) can't show the old way's hole, so only the
    /// second half is checked then.
    #[test]
    fn no_temp_file_is_ever_readable_by_anyone_else() {
        fn watch(dir: PathBuf, rounds: usize, writer: impl Fn(&Path, usize)) -> usize {
            let stop = Arc::new(AtomicBool::new(false));
            let caught = Arc::new(AtomicUsize::new(0));
            let watcher = {
                let (stop, caught, dir) = (stop.clone(), caught.clone(), dir.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                            if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
                                if meta.permissions().mode() & 0o077 != 0 {
                                    caught.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                })
            };
            let path = dir.join("desktop-drafts.json");
            for i in 0..rounds {
                writer(&path, i);
            }
            stop.store(true, Ordering::Relaxed);
            watcher.join().unwrap();
            caught.load(Ordering::Relaxed)
        }
        let old = scratch("race-old");
        let caught_old = watch(old.clone(), 3000, |path, i| {
            let temp = path.with_extension("json.tmp");
            std::fs::write(&temp, format!("secret {i}")).unwrap();
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::rename(&temp, path).unwrap();
        });
        if caught_old == 0 {
            eprintln!("the old way wasn't caught here (a strict umask?), so only today's is checked");
        }
        let new = scratch("race-new");
        let caught_new = watch(new.clone(), 1000, |path, i| write(path, format!("secret {i}").as_bytes()).unwrap());
        assert_eq!(caught_new, 0, "a temp file was readable by others");
        let _ = std::fs::remove_dir_all(&old);
        let _ = std::fs::remove_dir_all(&new);
    }
}
