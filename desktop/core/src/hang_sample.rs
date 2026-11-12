//! Reads the text `/usr/bin/sample` writes, for what the hang watch sends on:
//! the frames the main thread spent most of its time in, and nothing else of
//! the file. Symbols only: the file's image list has paths, and those stay
//! here. A port of the Swift app's `HangSample`, with what a Rust binary's
//! symbols need on top (`tidy`).

/// The arguments for `/usr/bin/sandbox-exec` that run `sample` on `pid` for
/// `seconds`, refused the places macOS would ask about. It hunts for debug
/// symbols through Spotlight wherever that says they are, which on 31 Oct
/// 2026 took the runner's copy of it deep into a Mac's external drive: a
/// prompt left waiting there held up other apps on that drive for nearly
/// nine hours (docs/protocol.md, "A dialog on a Mac's screen"). The stack
/// it's after doesn't need those places. Same list as `sampleSandbox` in
/// runner/src/frozen-cli.ts and `HangSample.sandboxProfile`.
pub fn sandbox_profile(home: &str) -> String {
    let mut paths: Vec<String> = vec!["/Volumes".into(), "/System/Volumes/Data/Volumes".into()];
    for dir in ["Desktop", "Documents", "Downloads", "Dropbox", "Library/CloudStorage", "Library/Mobile Documents"] {
        paths.push(format!("{home}/{dir}"));
    }
    let denied: Vec<String> = paths.iter().map(|p| format!("(subpath \"{p}\")")).collect();
    format!("(version 1)(allow default)(deny file* {})", denied.join(" "))
}

/// `sandbox-exec`'s arguments for a sample of `pid` written to `file`.
pub fn sample_arguments(home: &str, pid: u32, seconds: u32, file: &str) -> Vec<String> {
    vec!["-p".into(), sandbox_profile(home), "/usr/bin/sample".into(), pid.to_string(), seconds.to_string(), "-file".into(), file.into()]
}

/// The main thread's heaviest path, outermost frame first, each as
/// "symbol (in Image)". Empty when the text has no main thread.
///
/// The call graph is a tree with each frame's children sorted heaviest
/// first, so the heaviest path is the run of lines from the thread's own
/// that keeps getting deeper. The first line that doesn't is a sibling.
pub fn heaviest_main_thread_path(text: &str) -> Vec<String> {
    let mut path = Vec::new();
    let mut depth: Option<usize> = None;
    let mut in_graph = false;
    for line in text.split('\n') {
        if !in_graph {
            in_graph = line.starts_with("Call graph:");
            continue;
        }
        let Some(current) = depth else {
            if line.contains("com.apple.main-thread") {
                depth = Some(prefix_length(line));
            }
            continue;
        };
        let line_depth = prefix_length(line);
        let Some(frame) = frame(line).filter(|_| line_depth > current) else { break };
        path.push(frame);
        depth = Some(line_depth);
    }
    path
}

/// The main thread only waiting for its next event: nothing was stuck when
/// the sample ran. A stall that was over by then, or App Nap holding back the
/// beat the hang watch listens for.
pub fn is_idle(path: &[String]) -> bool {
    let has = |prefix: &str| path.iter().any(|f| f.starts_with(prefix));
    has("_DPSNextEvent")
        && has("__CFRunLoopServiceMachPort")
        && !["__CFRunLoopDoSource", "__CFRunLoopDoObservers", "__CFRunLoopDoTimers", "_dispatch_main_queue"].iter().any(|p| has(p))
}

/// The deepest frame in the app's own code: the name a report goes by, as the
/// frame at the very top is usually AppKit's or the kernel's. The whole app is
/// one image, GPUI and the kit included, so it goes by the symbol: one of this
/// app's crates (`threadbear_gpui::sidebar::…`, or a trait impl on one of its
/// types, `<threadbear_gpui::app::Threadbear as …>::render`). `main` is always
/// there, and says nothing.
pub fn deepest_app_frame(path: &[String]) -> Option<String> {
    path.iter()
        .rev()
        .find(|f| (f.starts_with("threadbear_") || f.starts_with("<threadbear_")) && f.split(" (in ").next() != Some("threadbear_gpui::main"))
        .cloned()
}

/// A Rust symbol as `sample`, a crash report or the panic hook's backtrace
/// gives it, made readable and the same from one build to the next. `sample`
/// leaves Rust's v0 mangling as it is (`_RNvCs…_15threadbear_gpui4main`), so
/// a mangled one is demangled, without its crate hashes; one the C++
/// demangler has been at has the legacy mangling's escapes (`$LT$`, `$u20$`,
/// `..`) put back. Either way the hash on the end of each name (`::h3f2a…`)
/// goes, as it changes with every build and would make each one a new issue.
pub fn tidy(symbol: &str) -> String {
    let mangled = symbol.strip_prefix('_').filter(|s| s.starts_with("_R") || s.starts_with("_ZN")).unwrap_or(symbol);
    if let Ok(demangled) = rustc_demangle::try_demangle(mangled) {
        // The alternate form leaves out the hashes and crate disambiguators.
        return strip_hashes(&format!("{demangled:#}"));
    }
    // A name that starts with an escape is mangled with a `_` in front of it.
    let mut out = symbol.replace("::_$", "::$");
    if out.starts_with("_$") {
        out.remove(0);
    }
    for (escape, plain) in [
        ("$LT$", "<"),
        ("$GT$", ">"),
        ("$u20$", " "),
        ("$C$", ","),
        ("$RF$", "&"),
        ("$BP$", "*"),
        ("$SP$", "@"),
        ("$LP$", "("),
        ("$RP$", ")"),
        ("$u7b$", "{"),
        ("$u7d$", "}"),
        ("$u5b$", "["),
        ("$u5d$", "]"),
        ("$u27$", "'"),
        ("$u7e$", "~"),
        ("$u3b$", ";"),
        ("$u2b$", "+"),
        ("$u22$", "\""),
    ] {
        out = out.replace(escape, plain);
    }
    // `..` is `::` in the legacy mangling, but only between names: a range
    // in a symbol isn't a thing, so every pair can go.
    if out.contains("..") && !out.contains("...") {
        out = out.replace("..", "::");
    }
    strip_hashes(&out)
}

/// Takes off each `::h` and 16 hex digits that ends a path segment.
fn strip_hashes(symbol: &str) -> String {
    let bytes = symbol.as_bytes();
    let mut out = String::with_capacity(symbol.len());
    let mut i = 0;
    while i < bytes.len() {
        if symbol[i..].starts_with("::h") && i + 19 <= bytes.len() && bytes[i + 3..i + 19].iter().all(u8::is_ascii_hexdigit) {
            let after = bytes.get(i + 19).copied();
            // The end of the name, or of the segment.
            if after.is_none_or(|c| !c.is_ascii_alphanumeric() && c != b'_') {
                i += 19;
                continue;
            }
        }
        let c = symbol[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Everything before the sample count: spaces and the tree's + ! : | marks.
fn prefix_length(line: &str) -> usize {
    line.chars().take_while(|c| " +!:|".contains(*c)).count()
}

/// "1497 main  (in Threadbear) + 64  [0x1042b1ab0]  ThreadbearApp.swift:0" to
/// "main (in Threadbear)", and a Rust name tidied (`tidy`).
fn frame(line: &str) -> Option<String> {
    let body = line.trim_start_matches(|c: char| " +!:|".contains(c));
    let space = body.find(' ')?;
    body[..space].parse::<u64>().ok()?;
    let mut symbol = body[space + 1..].to_owned();
    // "… + 64  [0x…" or "…  [0x…": the offset and address go.
    if let Some(at) = symbol.find("  [0x").or_else(|| symbol.find(" [0x")) {
        symbol.truncate(at);
    }
    let symbol = symbol.trim_end();
    let symbol = match symbol.rfind(" + ") {
        // The offset, when it's all digits after the last " + ".
        Some(at) if symbol[at + 3..].chars().all(|c| c.is_ascii_digit()) => &symbol[..at],
        _ => symbol,
    };
    let symbol = symbol.trim_end().replace("  (in ", " (in ");
    // The image stays as it is; the name in front of it is tidied.
    Some(match symbol.rfind(" (in ") {
        Some(at) => format!("{}{}", tidy(&symbol[..at]), &symbol[at..]),
        None => tidy(&symbol),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cut down from a real `sample` of the Swift app, with a branch at each
    /// of the places the walk could wander off the heaviest path.
    const STUCK: &str = "Analysis of sampling Threadbear (pid 64764) every 1 millisecond
Path:            /Users/USER/*/Threadbear.app/Contents/MacOS/Threadbear

Call graph:
    1513 Thread_41679326   DispatchQueue_1: com.apple.main-thread  (serial)
    + 1497 start  (in dyld) + 6992  [0x1890984e4]
    + ! 1497 main  (in Threadbear) + 64  [0x1042b1ab0]  ThreadbearApp.swift:0
    + !   1497 _DPSNextEvent  (in AppKit) + 576  [0x18d956084]
    + !     1353 __CFRunLoopRun  (in CoreFoundation) + 820  [0x18951f884]
    + !     : 830 __CFRunLoopDoSources0  (in CoreFoundation) + 232  [0x189520c60]
    + !     : | 830 static Files.load(_:)  (in Threadbear) + 12  [0x1042b1000]  Files.swift:12
    + !     : |   830 stat  (in libsystem_kernel.dylib) + 8  [0x18941fc34]
    + !     : 352 __CFRunLoopRun  (in CoreFoundation) + 1188  [0x18951f9f4]
    + !     144 __CFRunLoopServiceMachPort  (in CoreFoundation) + 160  [0x189521108]
    + 16 ???  (in <unknown binary>)  [0x10000]
    1513 Thread_41679408
    + 1513 start_wqthread  (in libsystem_pthread.dylib) + 8  [0x189461b78]
";

    const IDLE: &str = "Call graph:
    1513 Thread_1   DispatchQueue_1: com.apple.main-thread  (serial)
    + 1513 main  (in Threadbear) + 64  [0x1042b1ab0]  ThreadbearApp.swift:0
    + ! 1513 _DPSNextEvent  (in AppKit) + 576  [0x18d956084]
    + !   1513 __CFRunLoopRun  (in CoreFoundation) + 1188  [0x18951f9f4]
    + !   : 1513 __CFRunLoopServiceMachPort  (in CoreFoundation) + 160  [0x189521108]
    + !   : | 1513 mach_msg2_trap  (in libsystem_kernel.dylib) + 8  [0x18941fc34]
";

    /// The GPUI app's main thread stuck in its own code, as `sample` prints a
    /// Rust binary: legacy-mangled names, demangled as C++, hashes on.
    const RUST: &str = "Call graph:
    2632 Thread_6086107   DispatchQueue_1: com.apple.main-thread  (serial)
    + 2632 start  (in dyld) + 6076  [0x18a3b0274]
    + ! 2632 main  (in threadbear-gpui) + 36  [0x104d5e0c4]
    + !   2632 std::rt::lang_start_internal::h7b3f7c5a8e1d2f40  (in threadbear-gpui) + 1216  [0x10571c1d0]
    + !     2632 threadbear_gpui::main::h0123456789abcdef  (in threadbear-gpui) + 120  [0x104d5e000]
    + !       2632 _$LT$threadbear_gpui..app..Threadbear$u20$as$u20$gpui..Render$GT$::render::h00112233445566ff  (in threadbear-gpui) + 52  [0x104e00000]
    + !         2632 threadbear_gpui::sidebar::SidebarView::rows::{{closure}}::hfedcba9876543210  (in threadbear-gpui) + 8  [0x104e10000]
    + !           2632 __semwait_signal  (in libsystem_kernel.dylib) + 8  [0x18a6d0f10]
";

    #[test]
    fn follows_the_heaviest_path_and_stops_at_the_first_sibling() {
        assert_eq!(
            heaviest_main_thread_path(STUCK),
            [
                "start (in dyld)",
                "main (in Threadbear)",
                "_DPSNextEvent (in AppKit)",
                "__CFRunLoopRun (in CoreFoundation)",
                "__CFRunLoopDoSources0 (in CoreFoundation)",
                "static Files.load(_:) (in Threadbear)",
                "stat (in libsystem_kernel.dylib)",
            ]
        );
    }

    #[test]
    fn a_rust_apps_frames_are_tidied_and_named_by_its_own_code() {
        let path = heaviest_main_thread_path(RUST);
        assert_eq!(path[2], "std::rt::lang_start_internal (in threadbear-gpui)");
        assert_eq!(path[4], "<threadbear_gpui::app::Threadbear as gpui::Render>::render (in threadbear-gpui)");
        assert_eq!(deepest_app_frame(&path).as_deref(), Some("threadbear_gpui::sidebar::SidebarView::rows::{{closure}} (in threadbear-gpui)"));
    }

    #[test]
    fn main_alone_isnt_a_name() {
        let rust_main = vec!["main (in threadbear-gpui)".to_owned(), "threadbear_gpui::main (in threadbear-gpui)".to_owned(), "__semwait_signal (in libsystem_kernel.dylib)".to_owned()];
        // Stuck in what main itself runs is a place, though.
        let in_main = vec!["threadbear_gpui::main (in threadbear-gpui)".to_owned(), "threadbear_gpui::main::{{closure}} (in threadbear-gpui)".to_owned()];
        assert_eq!(deepest_app_frame(&in_main).as_deref(), Some("threadbear_gpui::main::{{closure}} (in threadbear-gpui)"));
        assert_eq!(deepest_app_frame(&rust_main), None);
        assert_eq!(deepest_app_frame(&heaviest_main_thread_path(IDLE)), None);
    }

    #[test]
    fn a_main_thread_waiting_for_events_is_idle() {
        assert!(is_idle(&heaviest_main_thread_path(IDLE)));
        assert!(!is_idle(&heaviest_main_thread_path(STUCK)));
    }

    #[test]
    fn paths_in_the_file_never_reach_the_result() {
        assert!(!heaviest_main_thread_path(STUCK).join("").contains("/Users"));
    }

    #[test]
    fn text_without_a_main_thread_has_no_path() {
        assert!(heaviest_main_thread_path("Call graph:\n    5 Thread_1\n").is_empty());
        assert!(heaviest_main_thread_path("").is_empty());
    }

    #[test]
    fn sample_is_kept_away_from_what_macos_asks_about() {
        let profile = sandbox_profile("/Users/me");
        for path in ["/Volumes", "/Users/me/Downloads", "/Users/me/Documents", "/Users/me/Desktop", "/Users/me/Library/Mobile Documents"] {
            assert!(profile.contains(&format!("(subpath \"{path}\")")), "{path}");
        }
    }

    /// Real frames from a sample of the GPUI debug app (11 Nov), which macOS
    /// leaves in Rust's v0 mangling.
    #[test]
    fn v0_mangled_names_are_demangled() {
        assert_eq!(tidy("_RNvCslrt7FU2RYJ4_15threadbear_gpui4main"), "threadbear_gpui::main");
        assert_eq!(tidy("_RNvNtCsdnanKjkF2cC_3std2rt19lang_start_internal"), "std::rt::lang_start_internal");
        assert_eq!(tidy("_RNvNtCs8lZASfzeS8H_10gpui_apple10dispatcher10trampoline"), "gpui_apple::dispatcher::trampoline");
        let line = "    + !     2632 _RNvCslrt7FU2RYJ4_15threadbear_gpui4main  (in threadbear-gpui) + 120  [0x104d5e000]";
        assert_eq!(heaviest_main_thread_path(&format!("Call graph:\n    2632 Thread_1   DispatchQueue_1: com.apple.main-thread  (serial)\n{line}\n")), ["threadbear_gpui::main (in threadbear-gpui)"]);
        // Not Rust at all: as it was.
        assert_eq!(tidy("-[NSApplication run]"), "-[NSApplication run]");
        assert_eq!(tidy("_dispatch_main_queue_drain.cold.6"), "_dispatch_main_queue_drain.cold.6");
    }

    #[test]
    fn hashes_go_but_names_that_look_like_them_stay() {
        assert_eq!(tidy("core::ops::function::FnOnce::call_once::h1a2b3c4d5e6f7a8b"), "core::ops::function::FnOnce::call_once");
        assert_eq!(tidy("a::h1a2b3c4d5e6f7a8b::b"), "a::b");
        // Not 16 hex digits, or more name after them: a real name.
        assert_eq!(tidy("threadbear_gpui::hang_watch::start"), "threadbear_gpui::hang_watch::start");
        assert_eq!(tidy("x::h1a2b3c4d5e6f7a8bc"), "x::h1a2b3c4d5e6f7a8bc");
    }
}
