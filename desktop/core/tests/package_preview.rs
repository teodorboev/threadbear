#![cfg(unix)]
use std::fs::{self, File};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use threadbear_core::private_file;

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("tb-package-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        private_file::dir(&path).unwrap();
        Fixture(path)
    }
    fn package(&self) -> PathBuf { self.0.join("Brief.pages") }
    fn cache(&self) -> PathBuf { self.0.join("previews") }
    fn prepare(&self) -> File {
        fs::create_dir_all(self.package().join("Index")).unwrap();
        fs::write(self.package().join("Index/Document.iwa"), b"the checked document").unwrap();
        File::open(self.package()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

#[test]
fn a_package_snapshot_stays_on_the_open_directory_after_its_path_is_replaced() {
    let f = Fixture::new("replaced");
    let checked = f.prepare();
    fs::rename(f.package(), f.0.join("previous.pages")).unwrap();
    fs::create_dir_all(f.package().join("Index")).unwrap();
    fs::write(f.package().join("Index/Document.iwa"), b"replacement").unwrap();
    // Repeated copies have independent directory listing offsets.
    for _ in 0..2 {
        let snapshot = private_file::snapshot_package(&checked, &f.cache(), "Brief.pages", 1024).unwrap();
        let document = snapshot.join("Index/Document.iwa");
        assert_eq!(fs::read(&document).unwrap(), b"the checked document");
        assert_eq!(fs::metadata(document).unwrap().permissions().mode() & 0o277, 0);
        assert_eq!(fs::metadata(snapshot).unwrap().permissions().mode() & 0o077, 0);
    }
}

#[test]
fn a_package_snapshot_refuses_nested_links_and_removes_its_incomplete_copy() {
    let f = Fixture::new("links");
    let checked = f.prepare();
    fs::write(f.0.join("outside"), b"must not be copied").unwrap();
    symlink(f.0.join("outside"), f.package().join("Index/linked.iwa")).unwrap();
    assert!(private_file::snapshot_package(&checked, &f.cache(), "Brief.pages", 1024).is_err());
    assert_eq!(fs::read_dir(f.cache()).unwrap().count(), 0);
}

#[test]
fn a_package_snapshot_bounds_bytes_and_does_not_wait_on_a_pipe() {
    let f = Fixture::new("limits");
    let checked = f.prepare();
    let error = private_file::snapshot_package(&checked, &f.cache(), "Brief.pages", 2).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
    assert_eq!(fs::read_dir(f.cache()).unwrap().count(), 0);
    use std::os::unix::ffi::OsStrExt;
    let pipe = std::ffi::CString::new(f.package().join("Index/pipe.iwa").as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    assert!(private_file::snapshot_package(&checked, &f.cache(), "Brief.pages", 1024).is_err());
    assert_eq!(fs::read_dir(f.cache()).unwrap().count(), 0);
}

#[cfg(target_os = "macos")]
struct Unlock(std::ffi::CString, u32);
#[cfg(target_os = "macos")]
impl Drop for Unlock { fn drop(&mut self) { unsafe { libc::lchflags(self.0.as_ptr(), self.1); } } }

#[cfg(target_os = "macos")]
#[test]
fn finder_locked_files_can_be_snapshotted_without_inheriting_the_lock() {
    use std::os::macos::fs::MetadataExt;
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new("locked");
    let checked = f.prepare();
    let original = f.package().join("Index/Document.iwa");
    let unlock = Unlock(std::ffi::CString::new(original.as_os_str().as_bytes()).unwrap(), 0);
    assert_eq!(unsafe { libc::lchflags(unlock.0.as_ptr(), libc::UF_IMMUTABLE) }, 0);
    let file = File::open(&original).unwrap();
    let single = private_file::snapshot(&file, &f.cache(), "Locked.docx", 1024).unwrap();
    let package = private_file::snapshot_package(&checked, &f.cache(), "Locked.pages", 1024).unwrap();
    for copy in [single.clone(), package.join("Index/Document.iwa")] {
        assert_eq!(fs::read(&copy).unwrap(), b"the checked document");
        assert_eq!(fs::metadata(&copy).unwrap().st_flags() & libc::UF_IMMUTABLE, 0);
    }
    assert_ne!(file.metadata().unwrap().st_flags() & libc::UF_IMMUTABLE, 0, "The original must stay Locked");
    fs::remove_dir_all(single.parent().unwrap()).unwrap();
    fs::remove_dir_all(package.parent().unwrap()).unwrap();
    drop(unlock);
}

#[cfg(target_os = "macos")]
#[test]
fn a_snapshot_keeps_the_data_and_compression_flag_of_a_ditto_copy() {
    let f = Fixture::new("compressed");
    let original = f.0.join("original");
    let compressed = f.0.join("compressed");
    let bytes = b"compressed snapshot data\n".repeat(10_000);
    fs::write(&original, &bytes).unwrap();
    assert!(std::process::Command::new("/usr/bin/ditto").arg("--hfsCompression").arg(&original).arg(&compressed).status().unwrap().success());
    let file = File::open(&compressed).unwrap();
    use std::os::macos::fs::MetadataExt;
    let original_flags = file.metadata().unwrap().st_flags();
    let compressed_flag = original_flags & libc::UF_COMPRESSED;
    use std::os::unix::ffi::OsStrExt;
    let unlock = Unlock(std::ffi::CString::new(compressed.as_os_str().as_bytes()).unwrap(), original_flags);
    assert_eq!(unsafe { libc::lchflags(unlock.0.as_ptr(), original_flags | libc::UF_IMMUTABLE) }, 0);
    let snapshot = private_file::snapshot(&file, &f.cache(), "Brief.docx", bytes.len() as u64).unwrap();
    assert_eq!(fs::read(&snapshot).unwrap(), bytes);
    let flags = fs::metadata(&snapshot).unwrap().st_flags();
    assert_eq!(flags & libc::UF_COMPRESSED, compressed_flag);
    assert_eq!(flags & libc::UF_IMMUTABLE, 0);
    fs::remove_dir_all(snapshot.parent().unwrap()).unwrap();
    drop(unlock);
}
