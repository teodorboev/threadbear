// What CEF's C++ wrapper would otherwise supply on a Mac
// (libcef_dll/wrapper/libcef_dll_dylib.cc and cef_library_loader_mac.mm):
// loading the framework at run time, and the lookup each forwarder in
// loader.rs does the first time it's called. See gen-loader.mjs for why the
// wrapper isn't built.
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicPtr, Ordering};

/// The framework, once `cef_load_library` has opened it. Never closed:
/// Chromium can't be started twice in one process, and the forwarders keep
/// the addresses they found.
static LIBRARY: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// A function's address in the framework, looked up once.
pub(crate) struct Slot {
    name: &'static CStr,
    found: AtomicPtr<c_void>,
}

impl Slot {
    pub(crate) const fn new(name: &'static CStr) -> Self {
        Slot { name, found: AtomicPtr::new(std::ptr::null_mut()) }
    }

    pub(crate) fn get(&self) -> *mut c_void {
        let found = self.found.load(Ordering::Acquire);
        if !found.is_null() {
            return found;
        }
        let library = LIBRARY.load(Ordering::Acquire);
        if library.is_null() {
            missing(self.name, "before the web runtime was loaded");
        }
        let found = unsafe { libc::dlsym(library, self.name.as_ptr()) };
        if found.is_null() {
            missing(self.name, "and isn't in the web runtime, so it isn't the build this app was made for");
        }
        self.found.store(found, Ordering::Release);
        found
    }
}

/// A call that can't be made. Carrying on would mean calling nothing, so the
/// process stops with the reason in its log, as the wrapper's own loader does.
fn missing(name: &CStr, why: &str) -> ! {
    eprintln!("threadbear-web: {} was called {why}", name.to_string_lossy());
    std::process::abort()
}

pub(crate) fn is_loaded() -> bool {
    !LIBRARY.load(Ordering::Acquire).is_null()
}

/// Opens the framework's library at `path`, as the wrapper's does:
/// RTLD_FIRST, so lookups search the framework alone and not everything it
/// links. Loading it twice is fine; the first stays.
#[no_mangle]
pub unsafe extern "C" fn cef_load_library(path: *const c_char) -> c_int {
    if is_loaded() {
        return 1;
    }
    if path.is_null() {
        return 0;
    }
    let handle = unsafe { libc::dlopen(path, libc::RTLD_LAZY | libc::RTLD_LOCAL | libc::RTLD_FIRST) };
    if handle.is_null() {
        let why = unsafe { libc::dlerror() };
        let why = if why.is_null() { "unknown".into() } else { unsafe { CStr::from_ptr(why) }.to_string_lossy() };
        eprintln!("threadbear-web: couldn't load {}: {why}", unsafe { CStr::from_ptr(path) }.to_string_lossy());
        return 0;
    }
    LIBRARY.store(handle, Ordering::Release);
    1
}

/// Kept loaded (see LIBRARY); said to have worked, as the `cef` crate's
/// loader complains otherwise.
#[no_mangle]
pub unsafe extern "C" fn cef_unload_library() -> c_int {
    1
}

/// The C side of CefScopedLibraryLoader: the framework from where a bundled
/// app or helper keeps it. Threadbear's don't (theirs is in the data folder,
/// loaded by path), but the C API has it, so it works the way it says.
#[no_mangle]
pub unsafe extern "C" fn cef_scoped_library_loader_create(helper: c_int) -> *mut c_void {
    let Ok(exe) = std::env::current_exe() else { return std::ptr::null_mut() };
    let Some(dir) = exe.parent() else { return std::ptr::null_mut() };
    let relative = if helper != 0 { "../../../Chromium Embedded Framework.framework/Chromium Embedded Framework" } else { "../Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework" };
    let Ok(path) = CString::new(dir.join(relative).to_string_lossy().into_owned()) else { return std::ptr::null_mut() };
    if unsafe { cef_load_library(path.as_ptr()) } == 1 {
        LIBRARY.load(Ordering::Acquire)
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_scoped_library_loader_free(_loader: *mut c_void) {}
