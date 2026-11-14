// A package is read through held directories, never by following a path an
// agent could replace while Quick Look reads it. This runs on a worker.
use std::ffi::{CStr, CString, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(super) fn snapshot(from: &File, folder: &Path, name: &str, limit: u64) -> io::Result<PathBuf> {
    if !from.metadata()?.is_dir() { return Err(io::Error::other("Not a package directory")); }
    let name = Path::new(name).file_name().filter(|n| !n.is_empty()).ok_or_else(|| io::Error::other("No package name"))?;
    super::make_dir(folder)?;
    let meta = std::fs::symlink_metadata(folder)?;
    if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 { return Err(io::Error::other("The preview cache isn't private")); }
    super::clear_old(folder, std::time::Duration::from_secs(24 * 60 * 60));
    let owned = super::new_dir(folder)?;
    let path = owned.join(name);
    let mut remaining = limit;
    let mut entries = 10_000;
    if let Err(error) = copy_directory(from, &path, &mut remaining, &mut entries, 0) {
        let _ = std::fs::remove_dir_all(&owned);
        return Err(error);
    }
    Ok(path)
}

fn copy_directory(from: &File, to: &Path, remaining: &mut u64, entries: &mut usize, depth: usize) -> io::Result<()> {
    if depth > 32 { return Err(io::Error::other("The package has too many nested folders")); }
    super::make_dir(to)?;
    let listing = Directory::open(from)?;
    loop {
        set_errno(0);
        // SAFETY: this stream is owned here, used on one thread, and each
        // name is copied before readdir advances to the next entry.
        let entry = unsafe { libc::readdir(listing.0) };
        if entry.is_null() {
            return if errno() == 0 { Ok(()) } else { Err(io::Error::last_os_error()) };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." { continue; }
        if *entries == 0 { return Err(io::Error::other("The package has too many files")); }
        *entries -= 1;
        let name = OsString::from_vec(name.to_vec());
        let child = open_at(from, &name)?;
        let meta = child.metadata()?;
        let target = to.join(&name);
        if meta.is_dir() {
            copy_directory(&child, &target, remaining, entries, depth + 1)?;
        } else if meta.is_file() {
            if meta.len() > *remaining { return Err(io::Error::new(io::ErrorKind::FileTooLarge, "The package is too big to preview")); }
            #[cfg(target_os = "macos")]
            if super::clone_file(&child, &target)? {
                // The source can grow between metadata and the atomic clone.
                // Charge the clone's actual bytes before accepting it.
                let length = std::fs::metadata(&target)?.len();
                if length > *remaining { return Err(io::Error::new(io::ErrorKind::FileTooLarge, "The package is too big to preview")); }
                *remaining -= length;
                continue;
            }
            *remaining -= meta.len();
            let (mut output, temporary) = super::new_file(to, &target, 0o400)?;
            super::copy_all(&child, &mut output, meta.len())?;
            drop(output);
            std::fs::rename(temporary, target)?;
        } else {
            return Err(io::Error::other("The package contains a special file"));
        }
    }
}

fn open_at(parent: &File, name: &std::ffi::OsStr) -> io::Result<File> {
    let name = CString::new(name.as_bytes()).map_err(io::Error::other)?;
    // A held parent and O_NOFOLLOW keep a link swap from reaching outside
    // the package. NONBLOCK makes a pipe fail the metadata check at once.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    Ok(unsafe { File::from_raw_fd(fd) })
}

struct Directory(*mut libc::DIR);
impl Directory {
    fn open(parent: &File) -> io::Result<Self> {
        // A new description of the held directory: dup would share its
        // listing offset with another snapshot of the same descriptor.
        let file = open_at(parent, std::ffi::OsStr::new("."))?;
        use std::os::fd::IntoRawFd;
        let fd = file.into_raw_fd();
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            unsafe { libc::close(fd); }
            return Err(error);
        }
        Ok(Directory(stream))
    }
}
impl Drop for Directory {
    fn drop(&mut self) { unsafe { libc::closedir(self.0); } }
}

#[cfg(target_os = "macos")]
fn errno_pointer() -> *mut libc::c_int { unsafe { libc::__error() } }
#[cfg(target_os = "linux")]
fn errno_pointer() -> *mut libc::c_int { unsafe { libc::__errno_location() } }
fn set_errno(value: libc::c_int) { unsafe { *errno_pointer() = value; } }
fn errno() -> libc::c_int { unsafe { *errno_pointer() } }
