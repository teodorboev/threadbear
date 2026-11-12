//! Crash and error reports to Sentry (project `threadbear`, tag `component:
//! desktop`), from the installed app only, as the Swift app's CrashReporting
//! and runner/src/sentry.ts have it. Agents run debug builds, tests and
//! scripted runs of this app all day, and an agent watcher opens a thread for
//! anything that reaches Sentry, so only a release build running as the
//! installed app, on the real data folder and built from Mike's own repo,
//! reports (`wanted`). THREADBEAR_SENTRY=off turns it off.
//!
//! What's sent is built here, with no network: a panic (its message with any
//! quoted words and paths taken out, where it was, and the frames' names), a
//! crash macOS wrote a report of (its exception and the crashed thread's
//! frames), and a freeze the hang watch caught (hang_watch.rs). Never what's
//! typed, a thread's words, a file's contents or a path; no breadcrumbs, no
//! sessions, no machine name. The GPUI app's crash_reporting.rs sends them.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// A public client key (it ships in every Sentry client); fine to commit.
/// The Swift app's and the runner's.
pub const DSN: &str = "https://c69460318b1f938fd1703c6578a059f6@o4508736647266304.ingest.us.sentry.io/4512336956293120";
/// This app's tag beside the Swift app's `mac`, the runner's and the phone's.
pub const COMPONENT: &str = "desktop";

/// Whether this launch reports: the installed app (a release build in its
/// place in Applications, not scripted), built from Mike's repo, on the real
/// data folder, and not switched off.
pub fn wanted(installed: bool, mikes_repo: bool, default_home: bool, sentry_env: Option<&str>) -> bool {
    installed && mikes_repo && default_home && sentry_env != Some("off")
}

/// The DSN's parts, for the envelope endpoint and its auth header.
#[derive(Clone, Debug, PartialEq)]
pub struct Dsn {
    pub key: String,
    pub host: String,
    pub project: String,
    pub scheme: String,
}

impl Dsn {
    pub fn parse(dsn: &str) -> Option<Dsn> {
        let (scheme, rest) = dsn.split_once("://")?;
        let (key, rest) = rest.split_once('@')?;
        let (host, project) = rest.split_once('/')?;
        let project = project.trim_end_matches('/');
        (!key.is_empty() && !host.is_empty() && !project.is_empty() && !project.contains('/')).then(|| Dsn { key: key.into(), host: host.into(), project: project.into(), scheme: scheme.into() })
    }

    pub fn envelope_url(&self) -> String {
        format!("{}://{}/api/{}/envelope/", self.scheme, self.host, self.project)
    }

    pub fn auth_header(&self, client: &str) -> String {
        format!("Sentry sentry_version=7, sentry_key={}, sentry_client={client}", self.key)
    }
}

/// What every event has: who it's from and when. `os` is the Mac's version
/// ("27.2.1"), the only thing about the machine that goes.
pub fn base_event(event_id: &str, timestamp: &str, release: &str, os: Option<&str>) -> Map<String, Value> {
    let mut event = Map::new();
    event.insert("event_id".into(), json!(event_id));
    event.insert("timestamp".into(), json!(timestamp));
    event.insert("platform".into(), json!("native"));
    event.insert("logger".into(), json!("threadbear-desktop"));
    event.insert("release".into(), json!(release));
    event.insert("environment".into(), json!("production"));
    event.insert("tags".into(), json!({ "component": COMPONENT }));
    event.insert("sdk".into(), json!({ "name": "threadbear-desktop", "version": "1" }));
    if let Some(os) = os {
        event.insert("contexts".into(), json!({ "os": { "name": "macOS", "version": os } }));
    }
    event
}

/// `threadbear-desktop@0.1.0+abc1234`, as the Swift app's
/// `threadbear-mac@<version>+<short>`.
pub fn release(version: &str, short: Option<&str>) -> String {
    format!("threadbear-desktop@{version}+{}", short.unwrap_or("unknown"))
}

/// A past freeze the hang watch caught: "Main thread stuck in …", grouped by
/// the app's own frame so each cause is one issue (`reportHang`).
pub fn hang_event(mut event: Map<String, Value>, stall: &crate::hang_watch::PastStall) -> Value {
    let stuck = crate::hang_watch::stuck_in(&stall.frames);
    event.insert("level".into(), json!("error"));
    event.insert("message".into(), json!({ "formatted": format!("Main thread stuck in {stuck}") }));
    event.insert("fingerprint".into(), json!(["main-thread-hang", stuck]));
    event.insert("extra".into(), stall.record.extras(&stall.frames));
    Value::Object(event)
}

/// A panic the hook kept (`PanicRecord`). On the main thread it took the app
/// down with it; elsewhere that thread stopped and the app carried on.
pub fn panic_event(mut event: Map<String, Value>, panic: &PanicRecord) -> Value {
    event.insert("level".into(), json!(if panic.main_thread { "fatal" } else { "error" }));
    // Again here, not only when it was kept: a record an older build kept
    // (and scrubbed less well) goes out by the same rule (Codex, 12 Nov).
    let message = safe_message(&panic.message);
    let value = if panic.location.is_empty() { message } else { format!("{message} at {}", panic.location) };
    event.insert(
        "exception".into(),
        json!({ "values": [{
            "type": "panic",
            "value": value,
            "mechanism": { "type": "panic", "handled": false },
            "stacktrace": { "frames": sentry_frames(&panic.frames) },
        }] }),
    );
    event.insert("extra".into(), json!({ "thread": if panic.main_thread { "main" } else { "background" }, "build": panic.build.as_deref().unwrap_or("unknown") }));
    Value::Object(event)
}

/// A crash macOS wrote a report of (`NativeCrash`).
pub fn crash_event(mut event: Map<String, Value>, crash: &NativeCrash) -> Value {
    event.insert("level".into(), json!("fatal"));
    event.insert(
        "exception".into(),
        json!({ "values": [{
            "type": crash.exception,
            "value": crash.detail.clone().unwrap_or_else(|| crash.exception.clone()),
            "mechanism": { "type": "mach", "handled": false },
            "stacktrace": { "frames": sentry_frames(&crash.frames) },
        }] }),
    );
    Value::Object(event)
}

/// "symbol (in image)" frames, outermost first, as Sentry's stack frames.
/// This app's own code is the part that's "in app".
fn sentry_frames(frames: &[String]) -> Value {
    let from = frames.len().saturating_sub(100);
    Value::Array(
        frames[from..]
            .iter()
            .map(|f| {
                let (function, package) = match f.rsplit_once(" (in ") {
                    Some((function, image)) => (function, Some(image.trim_end_matches(')'))),
                    None => (f.as_str(), None),
                };
                let mut frame = json!({ "function": function, "in_app": function.starts_with("threadbear_") || function.starts_with("<threadbear_") });
                if let Some(package) = package {
                    frame["package"] = json!(package);
                }
                frame
            })
            .collect(),
    )
}

/// The envelope Sentry's endpoint takes: its header, the item's, and the
/// event, a line each.
pub fn envelope(dsn: &str, event: &Value, sent_at: &str) -> Vec<u8> {
    let payload = serde_json::to_vec(event).unwrap_or_default();
    let header = json!({ "event_id": event["event_id"], "sent_at": sent_at, "dsn": dsn });
    let item = json!({ "type": "event", "length": payload.len(), "content_type": "application/json" });
    let mut out = Vec::with_capacity(payload.len() + 256);
    out.extend_from_slice(header.to_string().as_bytes());
    out.push(b'\n');
    out.extend_from_slice(item.to_string().as_bytes());
    out.push(b'\n');
    out.extend_from_slice(&payload);
    out.push(b'\n');
    out
}

/// A panic as the hook keeps it until it's sent, in the crashes folder, a
/// file each (`panic-<time>.json`), with nothing in it that isn't safe to
/// send: `message` is already `safe_message`'s.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PanicRecord {
    pub at: String,
    pub build: Option<String>,
    pub main_thread: bool,
    pub message: String,
    pub location: String,
    /// Outermost first, each "symbol (in image)" or just the symbol.
    pub frames: Vec<String>,
    pub reported: bool,
}

/// A panic's message, as much as is safe to send: Rust's own wording for
/// the panics it raises itself, with their numbers, and "panic" for any
/// other. A panic's text can hold anything the code put in it (a title, a
/// path, a reply's words), and scrubbing free text can't be sure it got it
/// all: "couldn't open /Users/mike/Secret Project/clients.csv" kept
/// "Project/clients.csv" (Codex's review, 12 Nov). Where it happened, the
/// location and the frames, says the rest.
pub fn safe_message(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    // Fixed wording, kept up to where the panic's own values would start.
    const FIXED: &[&str] = &[
        "called `Option::unwrap()` on a `None` value",
        "called `Result::unwrap()` on an `Err` value",
        "called `Result::unwrap_err()` on an `Ok` value",
        "called `Option::expect()` on a `None` value",
        "attempt to add with overflow",
        "attempt to subtract with overflow",
        "attempt to multiply with overflow",
        "attempt to divide by zero",
        "attempt to divide with overflow",
        "attempt to calculate the remainder with a divisor of zero",
        "attempt to calculate the remainder with overflow",
        "attempt to negate with overflow",
        "attempt to shift left with overflow",
        "attempt to shift right with overflow",
        "already borrowed",
        "already mutably borrowed",
        "explicit panic",
        "internal error: entered unreachable code",
        "not yet implemented",
        "not implemented",
        "assertion `left == right` failed",
        "assertion `left != right` failed",
        "assertion failed",
    ];
    if let Some(fixed) = FIXED.iter().find(|f| first.starts_with(**f)) {
        return (*fixed).to_owned();
    }
    // Wording with numbers in it, where `#` is a number.
    const NUMBERED: &[&str] = &[
        "index out of bounds: the len is # but the index is #",
        "range end index # out of range for slice of length #",
        "range start index # out of range for slice of length #",
        "slice index starts at # but ends at #",
        "byte index # is not a char boundary",
        "byte index # is out of bounds",
    ];
    NUMBERED.iter().find_map(|pattern| numbered_prefix(first, pattern)).unwrap_or_else(|| "panic".to_owned())
}

/// `text`'s start if it's `pattern` with a number at each `#`.
fn numbered_prefix(text: &str, pattern: &str) -> Option<String> {
    let mut rest = text;
    let mut parts = pattern.split('#').peekable();
    while let Some(part) = parts.next() {
        rest = rest.strip_prefix(part)?;
        if parts.peek().is_some() {
            let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if digits == 0 {
                return None;
            }
            rest = &rest[digits..];
        }
    }
    Some(text[..text.len() - rest.len()].to_owned())
}

/// Where a panic happened, without the build machine's folders: this repo's
/// files from `desktop/`, a crate from the registry by its name and version.
pub fn clean_location(file: &str) -> String {
    let file = file.replace('\\', "/");
    if let Some(at) = file.find("/registry/src/") {
        let rest = &file[at + "/registry/src/".len()..];
        return rest.split_once('/').map_or(rest, |(_, r)| r).to_owned();
    }
    if let Some(at) = file.find("/library/") {
        if file.contains("/rustc/") || file.contains("/rustlib/") {
            return file[at + 1..].to_owned();
        }
    }
    if let Some(at) = file.rfind("/desktop/") {
        return file[at + 1..].to_owned();
    }
    if file.starts_with('/') {
        let parts: Vec<&str> = file.rsplitn(3, '/').collect();
        return format!("{}/{}", parts.get(1).unwrap_or(&""), parts[0]);
    }
    file
}

/// The frames of `std::backtrace::Backtrace`'s text, outermost first, with
/// the panic machinery and the hook itself left off the top. Its "at" lines
/// (the build machine's paths) are never read.
pub fn backtrace_frames(text: &str) -> Vec<String> {
    let mut frames: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (number, symbol) = line.split_once(": ")?;
            number.parse::<u32>().ok()?;
            Some(crate::hang_sample::tidy(symbol.trim()))
        })
        .collect();
    // Innermost first here: everything up to where the panic began goes (the
    // backtrace's own frames, the hook, std's panic handling), and the frame
    // after it is the code that panicked. Only near the top: further down,
    // `std::sys::backtrace` is a thread's start, which stays.
    let entry = |f: &String| {
        ["core::panicking::", "std::panicking::", "rust_begin_unwind", "__rustc::rust_begin_unwind", "core::option::unwrap_failed", "core::option::expect_failed", "core::result::unwrap_failed"].iter().any(|m| f.starts_with(m))
    };
    let hook = |f: &String| ["std::backtrace", "threadbear_gpui::crash_reporting"].iter().any(|m| f.starts_with(m));
    if let Some(last) = frames.iter().take(30).rposition(entry) {
        frames.drain(..=last);
    } else {
        let mine = frames.iter().take_while(|f| hook(*f)).count();
        frames.drain(..mine);
    }
    frames.reverse();
    frames
}

/// A crash macOS's ReportCrash wrote down (a `.ips` file in
/// ~/Library/Logs/DiagnosticReports): what killed it and where, and nothing
/// else of the file.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeCrash {
    /// When it happened, ISO 8601 in UTC.
    pub at: String,
    /// The program that crashed, to tell the installed app's from a dev build's.
    pub proc_path: String,
    /// "EXC_BAD_ACCESS (SIGSEGV)".
    pub exception: String,
    /// "KERN_INVALID_ADDRESS at 0x…": the subtype, its addresses taken out so
    /// one fault is one issue.
    pub detail: Option<String>,
    /// The crashed thread's, outermost first, each "symbol (in image)".
    pub frames: Vec<String>,
}

/// A `.ips` report (macOS 12 and later: a line of JSON, then the report's
/// JSON), or none if it isn't one.
pub fn parse_ips(text: &str) -> Option<NativeCrash> {
    let (header, body) = text.split_once('\n')?;
    let header: Value = serde_json::from_str(header).ok()?;
    let body: Value = serde_json::from_str(body).ok()?;
    let images: Vec<&str> = body["usedImages"].as_array().into_iter().flatten().map(|i| i["name"].as_str().unwrap_or("???")).collect();
    let threads = body["threads"].as_array()?;
    let crashed = threads.iter().find(|t| t["triggered"].as_bool() == Some(true)).or_else(|| threads.get(body["faultingThread"].as_u64()? as usize))?;
    let mut frames: Vec<String> = crashed["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let symbol = f["symbol"].as_str().map_or_else(|| "???".to_owned(), crate::hang_sample::tidy);
            match f["imageIndex"].as_u64().and_then(|i| images.get(i as usize)) {
                Some(image) => format!("{symbol} (in {image})"),
                None => symbol,
            }
        })
        .collect();
    frames.reverse();
    let exception = &body["exception"];
    let kind = exception["type"].as_str().unwrap_or("crash");
    let exception_text = match exception["signal"].as_str() {
        Some(signal) => format!("{kind} ({signal})"),
        None => kind.to_owned(),
    };
    let detail = exception["subtype"].as_str().map(mask_addresses);
    let when = body["captureTime"].as_str().or_else(|| header["timestamp"].as_str()).unwrap_or("");
    Some(NativeCrash { at: ips_time(when).unwrap_or_default(), proc_path: body["procPath"].as_str().unwrap_or("").to_owned(), exception: exception_text, detail, frames })
}

/// A crash that came from a panic: the panic hook kept that one already.
pub fn is_panic(crash: &NativeCrash) -> bool {
    crash.frames.iter().any(|f| f.contains("panicking") || f.contains("rust_panic") || f.contains("panic_abort"))
}

/// "0x00005a2aec4600a8" to "0x…".
fn mask_addresses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 2..].chars().take_while(char::is_ascii_hexdigit).count();
        out.push_str(if digits > 0 { "0x…" } else { "0x" });
        rest = &rest[at + 2 + digits..];
    }
    out.push_str(rest);
    out
}

/// "2026-11-11 13:09:22.0574 +0800" to ISO 8601 in UTC.
fn ips_time(text: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f %z").ok()?;
    Some(crate::inbox::iso_time(parsed.timestamp_millis()))
}

/// The crash reports of `exe_name` in macOS's folder of them, by file name.
pub fn report_files(names: impl IntoIterator<Item = String>, exe_name: &str) -> Vec<String> {
    let prefix = format!("{exe_name}-");
    let mut out: Vec<String> = names.into_iter().filter(|n| n.starts_with(&prefix) && n.ends_with(".ips")).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_installed_app_of_mikes_repo_on_the_real_data_reports() {
        assert!(wanted(true, true, true, None));
        assert!(!wanted(false, true, true, None), "a dev build or a scripted run");
        assert!(!wanted(true, false, true, None), "someone's fork");
        assert!(!wanted(true, true, false, None), "pointed at a dev runner or the mock");
        assert!(!wanted(true, true, true, Some("off")));
    }

    #[test]
    fn the_dsn_gives_the_envelope_endpoint() {
        let dsn = Dsn::parse(DSN).unwrap();
        assert_eq!(dsn.envelope_url(), "https://o4508736647266304.ingest.us.sentry.io/api/4512336956293120/envelope/");
        assert_eq!(dsn.auth_header("threadbear-desktop/1"), "Sentry sentry_version=7, sentry_key=c69460318b1f938fd1703c6578a059f6, sentry_client=threadbear-desktop/1");
        assert_eq!(Dsn::parse("not a dsn"), None);
        // A local stand-in, for trying the sending without Mike's Sentry.
        assert_eq!(Dsn::parse("http://k@127.0.0.1:9999/1").unwrap().envelope_url(), "http://127.0.0.1:9999/api/1/envelope/");
    }

    #[test]
    fn only_rusts_own_wording_goes() {
        assert_eq!(safe_message("index out of bounds: the len is 3 but the index is 5"), "index out of bounds: the len is 3 but the index is 5");
        assert_eq!(safe_message(r#"called `Result::unwrap()` on an `Err` value: Custom { kind: Other, error: "can't read \"Fix the share button\"" }"#), "called `Result::unwrap()` on an `Err` value");
        assert_eq!(safe_message("byte index 5 is not a char boundary; it is inside 'é' (bytes 4..6) of `Fix the share button`"), "byte index 5 is not a char boundary");
        assert_eq!(safe_message("assertion `left == right` failed\n  left: \"secret\"\n right: \"other\""), "assertion `left == right` failed");
        assert_eq!(safe_message("assertion failed: title.is_empty()"), "assertion failed");
        // Anything else is someone's words, or might be.
        assert_eq!(safe_message("couldn't open /Users/mike/Secret Project/clients.csv: denied"), "panic");
        assert_eq!(safe_message("no thread called Fix the share button"), "panic");
        assert_eq!(safe_message("index out of bounds: the len is x but the index is 5"), "panic");
        assert_eq!(safe_message(""), "panic");
    }

    #[test]
    fn a_location_keeps_only_the_files_own_part() {
        assert_eq!(clean_location("gpui/src/sidebar.rs"), "gpui/src/sidebar.rs");
        assert_eq!(clean_location("/Users/m5-mike/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-pre-0.3.11/src/window.rs"), "gpui-pre-0.3.11/src/window.rs");
        assert_eq!(clean_location("/Users/x/.cache/threadbear-check/gpui/src-ab/desktop/gpui/src/app.rs"), "desktop/gpui/src/app.rs");
        assert_eq!(clean_location("/rustc/90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf/library/core/src/option.rs"), "library/core/src/option.rs");
        assert_eq!(clean_location("/somewhere/else/thing.rs"), "else/thing.rs");
    }

    #[test]
    fn a_backtrace_loses_the_panic_machinery_and_its_paths() {
        let text = "   0: std::backtrace_rs::backtrace::libunwind::trace
             at /rustc/abc/library/std/src/../../backtrace/src/backtrace/libunwind.rs:116:5
   1: std::backtrace::Backtrace::force_capture
   2: threadbear_gpui::crash_reporting::install::{{closure}}::h0011223344556677
   3: std::panicking::rust_panic_with_hook
   4: std::panicking::begin_panic_handler::{{closure}}
   5: core::panicking::panic_fmt
   6: threadbear_gpui::sidebar::SidebarView::rows::hfedcba9876543210
             at ./gpui/src/sidebar.rs:12:5
   7: threadbear_gpui::main
   8: main
";
        assert_eq!(backtrace_frames(text), ["main", "threadbear_gpui::main", "threadbear_gpui::sidebar::SidebarView::rows"]);
        // A background thread's: its start, low down, stays.
        let thread = "   0: std::backtrace::Backtrace::force_capture
   1: threadbear_gpui::crash_reporting::hook
   2: __rustc::rust_begin_unwind
   3: core::panicking::panic_fmt
   4: core::option::unwrap_failed
   5: threadbear_gpui::hang_watch::watch
   6: std::sys::backtrace::__rust_begin_short_backtrace
   7: std::thread::lifecycle::spawn_unchecked::{{closure}}
";
        assert_eq!(backtrace_frames(thread), ["std::thread::lifecycle::spawn_unchecked::{{closure}}", "std::sys::backtrace::__rust_begin_short_backtrace", "threadbear_gpui::hang_watch::watch"]);
    }

    #[test]
    fn a_panic_event_says_what_and_where_and_no_more() {
        let panic = PanicRecord { at: "2026-11-11T10:00:00.000Z".into(), build: Some("abc1234".into()), main_thread: true, message: "attempt to subtract with overflow".into(), location: "gpui/src/sidebar.rs:12:5".into(), frames: vec!["main".into(), "threadbear_gpui::sidebar::SidebarView::rows".into()], reported: false };
        let event = panic_event(base_event("e1", &panic.at, &release("0.1.0", Some("abc1234")), Some("27.2.1")), &panic);
        assert_eq!(event["level"], "fatal");
        assert_eq!(event["release"], "threadbear-desktop@0.1.0+abc1234");
        assert_eq!(event["tags"]["component"], "desktop");
        let exception = &event["exception"]["values"][0];
        assert_eq!(exception["value"], "attempt to subtract with overflow at gpui/src/sidebar.rs:12:5");
        assert_eq!(exception["stacktrace"]["frames"][1], json!({ "function": "threadbear_gpui::sidebar::SidebarView::rows", "in_app": true }));
        // Nothing of the machine's but its macOS version.
        assert!(event.get("server_name").is_none() && event.get("user").is_none());
        assert_eq!(event["contexts"], json!({ "os": { "name": "macOS", "version": "27.2.1" } }));
    }

    #[test]
    fn a_hang_event_is_named_by_the_apps_own_frame() {
        let stall = crate::hang_watch::PastStall {
            name: "hang-1".into(),
            frames: vec!["main (in threadbear-gpui)".into(), "threadbear_gpui::thread::ThreadPane::load (in threadbear-gpui)".into(), "stat (in libsystem_kernel.dylib)".into()],
            record: crate::hang_watch::Record { at: "2026-11-11T10:00:00Z".into(), recovered_after: Some(17.2), ..Default::default() },
        };
        let event = hang_event(base_event("e2", "2026-11-11T10:00:00Z", "r", None), &stall);
        assert_eq!(event["message"]["formatted"], "Main thread stuck in threadbear_gpui::thread::ThreadPane::load (in threadbear-gpui)");
        assert_eq!(event["fingerprint"], json!(["main-thread-hang", "threadbear_gpui::thread::ThreadPane::load (in threadbear-gpui)"]));
        assert_eq!(event["extra"]["seconds"], 17);
    }

    #[test]
    fn the_envelope_is_three_lines_with_the_events_length() {
        let event = json!({ "event_id": "abc", "message": { "formatted": "hi" } });
        let body = String::from_utf8(envelope(DSN, &event, "2026-11-11T10:00:00Z")).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 3);
        let header: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(header["event_id"], "abc");
        let item: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(item["length"].as_u64().unwrap() as usize, lines[2].len());
    }

    const IPS: &str = r#"{"app_name":"threadbear-gpui","timestamp":"2026-11-11 13:09:25.00 +0800","name":"threadbear-gpui","bug_type":"309"}
{"procPath":"\/Users\/me\/Applications\/Threadbear GPUI.app\/Contents\/MacOS\/threadbear-gpui","captureTime":"2026-11-11 13:09:22.0574 +0800","exception":{"type":"EXC_BAD_ACCESS","signal":"SIGSEGV","subtype":"KERN_INVALID_ADDRESS at 0x00005a2aec4600a8"},"faultingThread":0,"asi":{"libsystem_c.dylib":["secret words"]},"threads":[{"triggered":true,"frames":[{"imageOffset":31740,"symbol":"objc_release","symbolLocation":16,"imageIndex":1},{"imageOffset":4000,"symbol":"threadbear_gpui::web_runtime::start::h0123456789abcdef","imageIndex":0},{"imageOffset":12,"imageIndex":0}]}],"usedImages":[{"name":"threadbear-gpui","path":"\/Users\/me\/x"},{"name":"libobjc.A.dylib"}]}"#;

    #[test]
    fn a_crash_report_gives_its_exception_and_frames_and_nothing_else() {
        let crash = parse_ips(IPS).unwrap();
        assert_eq!(crash.exception, "EXC_BAD_ACCESS (SIGSEGV)");
        assert_eq!(crash.detail.as_deref(), Some("KERN_INVALID_ADDRESS at 0x…"));
        assert_eq!(crash.at, "2026-11-11T05:09:22.057Z");
        assert_eq!(crash.frames, ["??? (in threadbear-gpui)", "threadbear_gpui::web_runtime::start (in threadbear-gpui)", "objc_release (in libobjc.A.dylib)"]);
        assert!(!is_panic(&crash));
        let event = crash_event(base_event("e3", &crash.at, "r", None), &crash);
        let text = event.to_string();
        assert!(!text.contains("secret words") && !text.contains("/Users"), "{text}");
        assert_eq!(parse_ips("not a report"), None);
    }

    #[test]
    fn only_this_apps_reports_are_looked_at() {
        let names = ["threadbear-gpui-2026-11-11-130925.ips", "Finder-2026-11-11-145504.ips", "threadbear-gpui-2026-11-10-100000.ips", "threadbear-gpui-x.txt"].map(String::from);
        assert_eq!(report_files(names, "threadbear-gpui"), ["threadbear-gpui-2026-11-10-100000.ips", "threadbear-gpui-2026-11-11-130925.ips"]);
    }
}
