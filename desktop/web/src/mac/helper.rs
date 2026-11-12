// The processes Chromium starts besides the app's own (renderers, the GPU
// process and so on): this, as the threadbear-web-helper binary, copied into
// the five "Threadbear Helper*.app" bundles beside the framework in the data
// folder (threadbear_core::web_runtime::make_helpers). As CEF's own
// process_helper_mac.cc: the sandbox if Chromium asks for one, then the
// framework, relative to the helper, then CEF takes over.
use cef::{args::Args, library_loader::LibraryLoader, sandbox::Sandbox, *};

pub fn run() -> i32 {
    let args = Args::new();
    // The browser runs without Chromium's sandbox for now (host.rs says why)
    // and says so with --no-sandbox; set up otherwise, it has to come before
    // the framework loads. Both are found from this binary:
    // ../../../Chromium Embedded Framework.framework.
    let sandboxed = !std::env::args().any(|a| a == "--no-sandbox");
    let sandbox = sandboxed.then(|| {
        let mut sandbox = Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    });
    let Ok(exe) = std::env::current_exe() else { return 1 };
    let loader = LibraryLoader::new(&exe, true);
    if !loader.load() {
        eprintln!("threadbear-web-helper: couldn't load the web runtime beside {}", exe.display());
        return 1;
    }
    // Kept loaded until the process ends (loader_support.rs).
    std::mem::forget(loader);
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let code = execute_process(Some(args.as_main_args()), None::<&mut App>, std::ptr::null_mut());
    drop(sandbox);
    code
}
