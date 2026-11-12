// Which file a path leads to, to tell later whether it still leads there: a
// file dragged out of the app is the one that was grabbed, not whatever's at
// its path by the time it's dropped (a path swapped for a link to something
// private while the drag was under way: Codex's review, 12 Nov). The GPUI
// app notes it as a drag begins (desktop/gpui/src/file_drag.rs).
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    /// Where every link in the path ends.
    real: PathBuf,
    /// Which file that is on its disk, where the system says.
    #[cfg(unix)]
    node: (u64, u64),
    #[cfg(windows)]
    node: [u64; 3],
}

impl FileIdentity {
    /// The file `path` leads to now; None when it leads nowhere.
    pub fn of(path: &Path) -> Option<FileIdentity> {
        #[cfg(windows)]
        let _local = crate::local_path::LocalPath::stat(path).ok()?;
        // By its drive path on Windows, as the open gate names files.
        #[cfg(windows)]
        let real = crate::open_policy::system_path(&_local.path).to_path_buf();
        #[cfg(not(windows))]
        let real = crate::open_policy::real_path(path).ok()?;
        #[cfg(not(windows))]
        let meta = crate::open_policy::local_metadata(path).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            Some(FileIdentity { real, node: (meta.dev(), meta.ino()) })
        }
        #[cfg(windows)]
        {
            Some(FileIdentity { real, node: _local.identity().ok()? })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = meta;
            Some(FileIdentity { real })
        }
    }

    /// Where the file is, every link followed.
    pub fn real(&self) -> &Path {
        &self.real
    }

    /// Whether `path` still leads to this same file.
    pub fn still_at(&self, path: &Path) -> bool {
        FileIdentity::of(path).is_some_and(|now| now == *self)
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    #[test]
    fn a_hard_link_swap_does_not_keep_the_grabbed_file_identity() {
        let dir = std::env::temp_dir().join(format!("tb-win-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (shot, private) = (dir.join("shot.png"), dir.join("private.txt"));
        std::fs::write(&shot, b"picture").unwrap();
        std::fs::write(&private, b"private bytes").unwrap();
        let grabbed = FileIdentity::of(&shot).unwrap();
        std::fs::write(&shot, b"updated picture").unwrap();
        assert!(grabbed.still_at(&shot));
        std::fs::rename(&shot, dir.join("old.png")).unwrap();
        std::fs::hard_link(&private, &shot).unwrap();
        assert!(!grabbed.still_at(&shot));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A folder of the test's own, gone when it's dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("tb-file-identity-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_path_still_leads_to_its_file_until_its_swapped() {
        let dir = Scratch::new("swap");
        let (shot, private) = (dir.path().join("shot.png"), dir.path().join("private.txt"));
        std::fs::write(&shot, b"picture").unwrap();
        std::fs::write(&private, b"secret").unwrap();
        let grabbed = FileIdentity::of(&shot).unwrap();
        assert!(grabbed.still_at(&shot));
        assert_eq!(grabbed.real(), std::fs::canonicalize(&shot).unwrap());
        // Its contents changing is still the same file.
        std::fs::write(&shot, b"picture, edited").unwrap();
        assert!(grabbed.still_at(&shot));
        // Swapped for a link to something else: not the file grabbed. (The
        // grabbed file's moved aside, not deleted, so a new file can't be
        // given its number on a disk that reuses them.)
        std::fs::rename(&shot, dir.path().join("aside.png")).unwrap();
        std::os::unix::fs::symlink(&private, &shot).unwrap();
        assert!(!grabbed.still_at(&shot));
        // Nor is a new file put at its path, nor nothing at all.
        std::fs::remove_file(&shot).unwrap();
        std::fs::write(&shot, b"picture").unwrap();
        assert!(!grabbed.still_at(&shot));
        std::fs::remove_file(&shot).unwrap();
        assert!(!grabbed.still_at(&shot));
    }

    #[test]
    fn a_link_grabbed_is_the_file_it_led_to() {
        let dir = Scratch::new("link");
        let (file, link) = (dir.path().join("real.png"), dir.path().join("link.png"));
        std::fs::write(&file, b"picture").unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let grabbed = FileIdentity::of(&link).unwrap();
        assert_eq!(grabbed.real(), std::fs::canonicalize(&file).unwrap());
        assert!(grabbed.still_at(&link));
        assert!(grabbed.still_at(&file));
    }
}
