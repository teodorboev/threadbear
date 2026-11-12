// Windows must not follow a reply's symlink into an SMB share just to find
// out what it points at. Open each component without following reparse points,
// and keep its handle until resolution ends. Reads and listings reuse that
// handle; only callers handing a name on need to prevent replacement too.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf, Prefix};

#[link(name = "kernel32")]
extern "system" {
    fn GetFullPathNameW(path: *const u16, size: u32, out: *mut u16, part: *mut *mut u16) -> u32;
    fn QueryDosDeviceW(name: *const u16, out: *mut u16, size: u32) -> u32;
    fn GetFinalPathNameByHandleW(file: *mut std::ffi::c_void, out: *mut u16, size: u32, flags: u32) -> u32;
    fn GetFileInformationByHandleEx(file: *mut std::ffi::c_void, class: u32, out: *mut std::ffi::c_void, size: u32) -> i32;
    fn ReOpenFile(file: *mut std::ffi::c_void, access: u32, share: u32, flags: u32) -> *mut std::ffi::c_void;
    fn DeviceIoControl(
        file: *mut std::ffi::c_void,
        code: u32,
        input: *const std::ffi::c_void,
        input_len: u32,
        output: *mut std::ffi::c_void,
        output_len: u32,
        returned: *mut u32,
        overlapped: *mut std::ffi::c_void,
    ) -> i32;
}

#[repr(C)]
struct UnicodeString {
    len: u16,
    capacity: u16,
    text: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    len: u32,
    root: *mut std::ffi::c_void,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut std::ffi::c_void,
    quality: *mut std::ffi::c_void,
}
#[repr(C)]
struct IoStatus {
    status: usize,
    information: usize,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtCreateFile(
        file: *mut *mut std::ffi::c_void,
        access: u32,
        object: *const ObjectAttributes,
        io: *mut IoStatus,
        allocation: *const i64,
        attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const std::ffi::c_void,
        ea_len: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "a network share, device or unsupported reparse point")
}

/// The resolved local path, with every ancestor held against replacement.
/// The caller keeps these handles while inspecting the resolved file.
pub(crate) struct LocalPath {
    pub(crate) path: PathBuf,
    pub(crate) linked: bool,
    _held: Vec<File>,
}

#[derive(Clone, Copy)]
struct Access {
    final_access: u32,
    share: u32,
}

impl LocalPath {
    pub(crate) fn new(path: &Path) -> io::Result<Self> {
        Self::with_access(path, Access { final_access: 0x81, share: 3 })
    }

    pub(crate) fn stat(path: &Path) -> io::Result<Self> {
        Self::with_access(path, Access { final_access: 0x80, share: 7 })
    }

    pub(crate) fn for_read(path: &Path) -> io::Result<Self> {
        Self::with_access(path, Access { final_access: 0x81, share: 7 })
    }

    fn with_access(path: &Path, access: Access) -> io::Result<Self> {
        let mut held = Vec::new();
        let path = walk(path, &mut held, 0, false, access)?;
        let linked = held.iter().any(|f| f.metadata().is_ok_and(|m| m.file_type().is_symlink()));
        Ok(Self { path, linked, _held: held })
    }

    pub(crate) fn file(&self) -> io::Result<&File> {
        let file = self._held.last().ok_or_else(refused)?;
        if file.metadata()?.file_type().is_symlink() {
            return Err(refused());
        }
        Ok(file)
    }

    pub(crate) fn anchored_name(&self) -> io::Result<PathBuf> {
        let native = final_name(self.file()?, 2)?; // VOLUME_NAME_NT
        Ok(PathBuf::from(format!(r"\\?\GLOBALROOT{}", native.to_string_lossy())))
    }

    pub(crate) fn identity(&self) -> io::Result<[u64; 3]> {
        // FILE_ID_INFO: volume serial number, then the opaque 128-bit ID.
        let mut id = [0u64; 3];
        if unsafe { GetFileInformationByHandleEx(self.file()?.as_raw_handle(), 18, id.as_mut_ptr().cast(), 24) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(id)
    }

    pub(crate) fn writable(&self) -> io::Result<File> {
        let file = self.reopen(0x4000_0000)?;
        if !file.metadata()?.is_file() {
            return Err(refused());
        }
        Ok(file)
    }

    pub(crate) fn readable(&self) -> io::Result<File> {
        if self.file()?.metadata()?.file_attributes() & 0x400 != 0 {
            self.reopen(0x8000_0000)
        } else {
            self.file()?.try_clone()
        }
    }

    fn reopen(&self, access: u32) -> io::Result<File> {
        let file = self.file()?;
        // Reopen the same file object, without resolving its name again and
        // without truncating it. A non-link reparse point needs its owning
        // filter (OneDrive, WOF or dedup) for data, rather than its raw stream.
        // Name-surrogate links have already been followed or refused.
        let flags = if file.metadata()?.file_attributes() & 0x400 != 0 { 0x0200_0000 } else { 0x0220_0000 };
        let raw = unsafe { ReOpenFile(file.as_raw_handle(), access, 7, flags) };
        if raw as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_handle(raw) };
        let meta = file.metadata()?;
        if meta.file_type().is_symlink() {
            return Err(refused());
        }
        Ok(file)
    }

    pub(crate) fn entries(&self) -> io::Result<Vec<io::Result<super::open_policy::LocalDirEntry>>> {
        let file = self.readable()?;
        if !file.metadata()?.is_dir() {
            return Err(io::Error::other("Not a folder"));
        }
        let mut entries = Vec::new();
        // FILE_ID_EXTD_DIR_INFO (class 19), aligned to eight bytes. Enumerate
        // the checked directory handle, never a new lookup of `path\*`.
        let mut storage = vec![0u64; 8192];
        let mut class = 19;
        loop {
            if unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), class, storage.as_mut_ptr().cast(), (storage.len() * 8) as u32) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(18) {
                    break;
                }
                // FAT and older filesystems need the 64-bit directory-info
                // layout (FILE_ID_BOTH_DIR_INFO), not the 128-bit one.
                if class == 19 && matches!(error.raw_os_error(), Some(1 | 50 | 87)) {
                    class = 10;
                    continue;
                }
                return Err(error);
            }
            let bytes = unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), storage.len() * 8) };
            let mut at = 0usize;
            loop {
                let read = |offset| -> io::Result<u32> { Ok(u32::from_le_bytes(bytes.get(at + offset..at + offset + 4).ok_or_else(refused)?.try_into().unwrap())) };
                let next = read(0)? as usize;
                let attributes = read(56)?;
                let len = read(60)? as usize;
                let tag = if class == 19 { read(68)? } else { 0 };
                let name_at = if class == 19 { 88 } else { 104 };
                if len % 2 != 0 {
                    return Err(refused());
                }
                let raw = bytes.get(at + name_at..at + name_at + len).ok_or_else(refused)?;
                let name = OsString::from_wide(&raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>());
                if name != "." && name != ".." {
                    let folder = attributes & 0x10 != 0 && (attributes & 0x400 == 0 || tag & 0x2000_0000 == 0);
                    entries.push(Ok(super::open_policy::LocalDirEntry { path: self.path.join(&name), name, is_folder: folder }));
                    if entries.len() > 20_000 {
                        return Ok(entries);
                    }
                }
                if next == 0 {
                    break;
                }
                if next < name_at || at + next >= bytes.len() {
                    return Err(refused());
                }
                at += next;
            }
        }
        Ok(entries)
    }
}

fn final_name(file: &File, flags: u32) -> io::Result<PathBuf> {
    let mut out = vec![0u16; 32768];
    let len = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), out.as_mut_ptr(), out.len() as u32, flags) } as usize;
    if len == 0 || len >= out.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&out[..len])))
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn volume_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    let Some(guid) = name.strip_prefix("Volume{").and_then(|s| s.strip_suffix('}')) else { return false };
    guid.len() == 36 && guid.bytes().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == b'-' } else { c.is_ascii_hexdigit() })
}

fn ambiguous_name(name: &OsStr) -> bool {
    let chars: Vec<u16> = name.encode_wide().collect();
    chars.last().is_some_and(|c| matches!(*c, 32 | 46)) || chars.iter().any(|c| matches!(*c, 0 | 58)) || super::open_policy::is_device_name(&name.to_string_lossy())
}

/// Windows removes dot segments before following links. Normalize once,
/// then open every prefix verbatim so Windows never trims a different last
/// component at each step. Ambiguous trailing dots/spaces are refused.
fn normalize(path: &Path, from_link: bool) -> io::Result<PathBuf> {
    let internal_volume = from_link && matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Verbatim(v) if volume_name(v)));
    if !path.is_absolute() || (!internal_volume && super::open_policy::on_a_share(path)) {
        return Err(refused());
    }
    for part in path.components() {
        if let Component::Normal(name) = part {
            if ambiguous_name(name) {
                return Err(refused());
            }
        }
    }
    let verbatim = matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::VerbatimDisk(_) | Prefix::Verbatim(_)));
    if internal_volume {
        let mut normalized = PathBuf::new();
        for c in path.components() {
            match c {
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::CurDir => {}
                _ => normalized.push(c),
            }
        }
        return Ok(normalized);
    }
    if verbatim && path.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir)) {
        return Err(refused());
    }
    let raw = wide(path.as_os_str());
    let raw = if verbatim { &raw[4..] } else { &raw[..] };
    let mut out = vec![0u16; 32768];
    let len = unsafe { GetFullPathNameW(raw.as_ptr(), out.len() as u32, out.as_mut_ptr(), std::ptr::null_mut()) } as usize;
    if len == 0 || len >= out.len() {
        return Err(refused());
    }
    let mut extended = vec![92, 92, 63, 92]; // \\?\
    extended.extend_from_slice(&out[..len]);
    Ok(PathBuf::from(OsString::from_wide(&extended)))
}

fn walk(path: &Path, held: &mut Vec<File>, links: usize, from_link: bool, access: Access) -> io::Result<PathBuf> {
    if links > 40 {
        return Err(refused());
    }
    let path = normalize(path, from_link)?;
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else { return Err(refused()) };
    let device = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => OsString::from(format!("{}:", drive as char)),
        Prefix::Verbatim(name) if from_link && volume_name(name) => name.to_owned(),
        _ => return Err(refused()),
    };
    if components.next() != Some(Component::RootDir) {
        return Err(refused());
    }
    // A mapped share is just as remote as a UNC path. Ask about the drive
    // before opening its root. Unknown and unavailable drives fail closed.
    let mut at = PathBuf::from(prefix.as_os_str());
    at.push(r"\");
    // SUBST/DefineDosDevice aliases can hide another namespace behind a
    // local-looking drive. Inspect the mapping without opening that drive.
    let mut mapping = vec![0u16; 32768];
    if unsafe { QueryDosDeviceW(wide(&device).as_ptr(), mapping.as_mut_ptr(), mapping.len() as u32) } == 0 {
        return Err(refused());
    }
    let len = mapping.iter().position(|c| *c == 0).ok_or_else(refused)?;
    let mapped = PathBuf::from(OsString::from_wide(&mapping[..len]));
    let text = mapped.to_string_lossy();
    if let Some(local) = text.strip_prefix(r"\??\") {
        let mut target = PathBuf::from(local);
        target.extend(components);
        return walk(&target, held, links + 1, false, access);
    }
    if !super::open_policy::is_local_volume_device(&text) {
        return Err(refused());
    }
    // Anchor opens to the checked physical volume, so redefining a DOS drive
    // letter cannot redirect a later step to a share.
    at = PathBuf::from(format!(r"{}\", text));
    if hold(&at, held, components.as_path().as_os_str().is_empty(), access)? {
        return Err(refused());
    }
    while let Some(component) = components.next() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(refused());
            }
            Component::Normal(name) => {
                // Alternate streams and NT namespace spellings aren't files
                // this gate can hand to another app without changing meaning.
                if name.encode_wide().any(|c| c == b':' as u16) {
                    return Err(refused());
                }
                let parent = held.last().ok_or_else(refused)?;
                let file = child(parent, name, components.as_path().as_os_str().is_empty(), access)?;
                held.push(file);
            }
            _ => return Err(refused()),
        }
        let reparse = held.last().ok_or_else(refused)?.metadata()?.file_type().is_symlink();
        if reparse {
            // read_link opens the reparse point itself, never its target.
            // Only name-surrogate reparse points are links. Cloud placeholders
            // and deduplicated files remain ordinary files (FileType's rule).
            let target = link_target(held.last().ok_or_else(refused)?)?;
            let mut target = if target.is_absolute() {
                target
            } else {
                let link = final_name(held.last().ok_or_else(refused)?, 1)?; // VOLUME_NAME_GUID
                let parent = link.parent().ok_or_else(refused)?;
                // A relative symlink target may contain `..`. Apply normal
                // Windows normalization to that substitution before walking
                // it, rather than treating it as a verbatim input path.
                let parent = if matches!(parent.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::VerbatimDisk(_))) {
                    let wide: Vec<u16> = parent.as_os_str().encode_wide().collect();
                    PathBuf::from(OsString::from_wide(&wide[4..]))
                } else {
                    parent.to_path_buf()
                };
                parent.join(target)
            };
            target.extend(components);
            return walk(&target, held, links + 1, true, access);
        }
    }
    // Read the checked object's name from its handle. A last-component
    // reparse-point swap must never trigger another pathname resolution.
    let file = held.last().ok_or_else(refused)?;
    let path = final_name(file, 0).or_else(|_| final_name(file, 1))?;
    // An 8.3 alias can conceal a reserved or trimmed long name. Refuse the
    // resolved spelling too, before anyone simplifies it for the shell.
    if path.components().any(|c| matches!(c, Component::Normal(name) if ambiguous_name(name))) {
        return Err(refused());
    }
    Ok(path)
}

fn hold(path: &Path, held: &mut Vec<File>, final_part: bool, access: Access) -> io::Result<bool> {
    // FILE_READ_DATA (LIST_DIRECTORY on a folder) | FILE_READ_ATTRIBUTES;
    // FILE_SHARE_READ | FILE_SHARE_WRITE; FILE_FLAG_OPEN_REPARSE_POINT |
    // FILE_FLAG_BACKUP_SEMANTICS (directories can be opened as handles too).
    // Attribute-only access does NOT participate in Windows sharing locks.
    let rights = if final_part { access.final_access } else { 0xa0 };
    let file = native_file(std::ptr::null_mut(), path.as_os_str(), rights, access.share)?;
    let reparse = file.metadata()?.file_type().is_symlink();
    held.push(file);
    Ok(reparse)
}

/// One component relative to the checked parent object, with no reparse
/// processing. No ancestor name is looked up again, even if its tag changes.
/// This is the same single-component strategy as Go's Windows openat fallback.
fn child(parent: &File, name: &OsStr, final_part: bool, access: Access) -> io::Result<File> {
    let text: Vec<u16> = name.encode_wide().collect();
    if text.is_empty() || text.len() > 32767 || text.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92)) || name == "." || name == ".." {
        return Err(refused());
    }
    native_file(parent.as_raw_handle(), name, if final_part { access.final_access } else { 0xa0 }, access.share)
}

fn native_file(root: *mut std::ffi::c_void, name: &OsStr, access: u32, share: u32) -> io::Result<File> {
    let mut text: Vec<u16> = name.encode_wide().collect();
    if text.len() > 32767 || text.contains(&0) {
        return Err(refused());
    }
    let mut name = UnicodeString { len: (text.len() * 2) as u16, capacity: (text.len() * 2) as u16, text: text.as_mut_ptr() };
    let attrs = ObjectAttributes { len: std::mem::size_of::<ObjectAttributes>() as u32, root, name: &mut name, attributes: 0x40, security: std::ptr::null_mut(), quality: std::ptr::null_mut() };
    let mut status = IoStatus { status: 0, information: 0 };
    let mut raw = std::ptr::null_mut();
    let access = 0x0010_0000 | access;
    let code = unsafe { NtCreateFile(&mut raw, access, &attrs, &mut status, std::ptr::null(), 0, share, 1, 0x0020_4020, std::ptr::null(), 0) };
    if code < 0 {
        return Err(io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(code) } as i32));
    }
    Ok(unsafe { File::from_raw_handle(raw) })
}

/// Read reparse data from the held link itself, never via its pathname.
fn link_target(file: &File) -> io::Result<PathBuf> {
    let mut data = vec![0u8; 16384];
    let mut size = 0u32;
    if unsafe { DeviceIoControl(file.as_raw_handle(), 0x0009_00a8, std::ptr::null(), 0, data.as_mut_ptr().cast(), data.len() as u32, &mut size, std::ptr::null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    data.truncate(size as usize);
    let u16_at = |at: usize| -> io::Result<usize> { Ok(u16::from_le_bytes(data.get(at..at + 2).ok_or_else(refused)?.try_into().unwrap()) as usize) };
    let tag = u32::from_le_bytes(data.get(..4).ok_or_else(refused)?.try_into().unwrap());
    let base = match tag {
        0xa000_0003 => 16,
        0xa000_000c => 20,
        _ => return Err(refused()),
    };
    let start = base + u16_at(8)?;
    let len = u16_at(10)?;
    if len % 2 != 0 {
        return Err(refused());
    }
    let mut text = data.get(start..start + len).ok_or_else(refused)?.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>();
    if text.starts_with(&[92, 63, 63, 92]) {
        // NT \??\ to Win32 verbatim form
        text.drain(..4);
        if text.len() >= 4 && text[..3].iter().copied().map(|c| (c as u8).to_ascii_uppercase()).eq(b"UNC".iter().copied()) && text[3] == 92 {
            text.splice(..4, [92, 92]);
        } else {
            text.splice(..0, [92, 92, 63, 92]);
        }
    }
    let path = PathBuf::from(OsString::from_wide(&text));
    let relative = if tag == 0xa000_000c {
        let flags = u32::from_le_bytes(data.get(16..20).ok_or_else(refused)?.try_into().unwrap());
        if flags > 1 {
            return Err(refused());
        }
        flags == 1
    } else {
        false
    };
    if (relative && (path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_))))) || (!relative && !path.is_absolute()) {
        return Err(refused());
    }
    Ok(path)
}
