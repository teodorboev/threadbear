// Crash and freeze reports to Sentry, from the installed app only (core's
// crash_report.rs says what goes, what never does, and when it's wanted), as
// the Swift app's CrashReporting does.
//
// A panic is written down by the hook as it happens, scrubbed, in
// ~/Library/Logs/Threadbear/crashes/desktop (one on the main thread takes the
// app down with it), and sent from a thread of its own: at the next launch,
// or straight away when it was another thread's and the app carried on. A
// crash that never reached the hook (a fault in AppKit, Metal or Chromium) is
// read from the report macOS writes of it, at the next launch. The hang
// watch's freezes come through `past_stalls`. A scripted app keeps its panics
// beside its automation file and sends nothing; debug builds and anything
// else that isn't the installed app keep nothing at all.
use crate::installed;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use threadbear_core::crash_report::{self as report, NativeCrash, PanicRecord};
use threadbear_core::hang_watch::PastStall;

struct Reporting {
    folder: PathBuf,
    /// Sentry's, or only the folder (a scripted app, someone's fork).
    send: bool,
    release: String,
    build: Option<String>,
    os: Option<String>,
    /// Wakes the sender for a panic the app lived through.
    wake: Mutex<Option<mpsc::Sender<()>>>,
}

static REPORTING: OnceLock<Reporting> = OnceLock::new();

/// THREADBEAR_SENTRY_DSN points the reports somewhere else, for trying the
/// sending against a stand-in (a local listener) rather than Mike's Sentry.
fn dsn() -> String {
    std::env::var("THREADBEAR_SENTRY_DSN").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| report::DSN.to_owned())
}

/// As early as can be, before the window, to catch a panic at launch.
pub(crate) fn start() {
    let launch = installed::launch();
    let folder = if launch.scripted {
        match std::env::var_os("THREADBEAR_AUTOMATION").and_then(|f| Path::new(&f).parent().map(|p| p.join("crashes"))) {
            Some(folder) => folder,
            None => return,
        }
    } else if launch.installed {
        installed::logs_dir().join("crashes").join("desktop")
    } else {
        return;
    };
    let mikes = launch.build.as_ref().is_some_and(|b| b.is_mikes_repo());
    let send = !launch.scripted && report::wanted(launch.installed, mikes, installed::on_real_data(), std::env::var("THREADBEAR_SENTRY").ok().as_deref());
    let build = launch.build.as_ref().map(|b| b.short.clone()).filter(|s| !s.is_empty());
    let reporting = Reporting { folder, send, release: report::release(env!("CARGO_PKG_VERSION"), build.as_deref()), build, os: macos_version(), wake: Mutex::new(None) };
    if REPORTING.set(reporting).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        hook(info);
        previous(info);
    }));
    let (wake, woken) = mpsc::channel();
    if let Some(r) = REPORTING.get() {
        *r.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake);
        if r.send {
            eprintln!("threadbear: crash reporting on ({})", r.release);
        }
    }
    let _ = std::thread::Builder::new().name("crash reports".into()).spawn(move || sender(woken));
}

thread_local! {
    /// A panic inside the hook itself would come straight back here.
    static IN_HOOK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn hook(info: &std::panic::PanicHookInfo<'_>) {
    let Some(r) = REPORTING.get() else { return };
    if IN_HOOK.with(|h| h.replace(true)) {
        return;
    }
    let message = info.payload().downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic with no message".into());
    let location = info.location().map(|l| format!("{}:{}:{}", report::clean_location(l.file()), l.line(), l.column())).unwrap_or_default();
    let main_thread = std::thread::current().name() == Some("main");
    let frames = report::backtrace_frames(&std::backtrace::Backtrace::force_capture().to_string());
    let record = PanicRecord { at: threadbear_core::inbox::iso_time(crate::now_ms()), build: r.build.clone(), main_thread, message: report::safe_message(&message), location, frames, reported: false };
    let name = format!("panic-{}-{:03}", threadbear_core::chrono::Local::now().format("%Y-%m-%d-%H%M%S"), next_number());
    if write_record(&r.folder, &name, &record).is_ok() && !main_thread {
        // The app carries on: sent now rather than at the next launch.
        if let Some(wake) = r.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = wake.send(());
        }
    }
    IN_HOOK.with(|h| h.set(false));
}

/// Two panics in one second on different threads get different files.
fn next_number() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNT: AtomicU32 = AtomicU32::new(0);
    COUNT.fetch_add(1, Ordering::Relaxed) % 1000
}

/// A panic's message can carry any text the app had, so it's this user's
/// only (`private_file`).
fn write_record(folder: &Path, name: &str, record: &PanicRecord) -> std::io::Result<()> {
    threadbear_core::private_file::write(&folder.join(format!("{name}.json")), &serde_json::to_vec(record).map_err(std::io::Error::other)?)
}

/// The sending thread: what's waiting at launch, a moment after the app's
/// up, then each panic the app lives through.
fn sender(woken: mpsc::Receiver<()>) {
    let Some(r) = REPORTING.get() else { return };
    std::thread::sleep(Duration::from_secs(5));
    native_crashes(r);
    loop {
        pending_panics(r);
        prune(&r.folder);
        if woken.recv().is_err() {
            return;
        }
    }
}

/// The newest of each kind kept, like the hang watch's ten.
fn prune(folder: &Path) {
    let mut names: Vec<String> = std::fs::read_dir(folder).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with("panic-") && n.ends_with(".json")).collect();
    names.sort();
    for name in names.iter().take(names.len().saturating_sub(10)) {
        let _ = std::fs::remove_file(folder.join(name));
    }
}

fn pending_panics(r: &Reporting) {
    let mut names: Vec<String> = std::fs::read_dir(&r.folder).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with("panic-") && n.ends_with(".json")).collect();
    names.sort();
    for name in names {
        let path = r.folder.join(&name);
        let Some(mut record) = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice::<PanicRecord>(&d).ok()) else { continue };
        if record.reported {
            continue;
        }
        // Marked first: a report that fails to go isn't tried again and
        // again at every launch.
        record.reported = true;
        let _ = write_record(&r.folder, name.trim_end_matches(".json"), &record);
        let event = report::panic_event(base(r, &record.at), &record);
        send(r, &event, &format!("panic {}", report::safe_message(&record.message)));
    }
}

/// Crashes macOS wrote reports of: the installed app's, since the last
/// launch that looked, once each, none from before reporting began and none
/// from a panic (the hook kept those).
fn native_crashes(r: &Reporting) {
    if !installed::launch().installed {
        return;
    }
    let reports = installed::home_dir().join("Library").join("Logs").join("DiagnosticReports");
    let seen_file = r.folder.join("crash-reports-seen.json");
    let mut seen: Value = std::fs::read(&seen_file).ok().and_then(|d| serde_json::from_slice::<Value>(&d).ok()).filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    let now = crate::now_ms();
    // The first launch with reporting only notes where it began.
    let since = seen["since"].as_i64().unwrap_or(now);
    seen["since"] = since.into();
    let mut names: Vec<String> = seen["names"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_owned)).collect();
    let exe = installed::install_dir().join("Contents").join("MacOS").join("threadbear-gpui");
    let files = std::fs::read_dir(&reports).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok());
    for name in report::report_files(files, "threadbear-gpui") {
        if names.contains(&name) {
            continue;
        }
        names.push(name.clone());
        let Some(crash) = std::fs::read_to_string(reports.join(&name)).ok().and_then(|t| report::parse_ips(&t)) else { continue };
        let at = threadbear_core::inbox::parse_ms(&crash.at).unwrap_or(0);
        // A week's enough: Sentry drops what's much older, and an old one
        // has been fixed or will come again.
        if at < since || now - at > 7 * 86_400_000 || report::is_panic(&crash) || !same_program(&crash, &exe) {
            continue;
        }
        let event = report::crash_event(base(r, &crash.at), &crash);
        send(r, &event, &format!("crash {}", crash.exception));
    }
    // The newest few hundred names are plenty to remember.
    let from = names.len().saturating_sub(300);
    seen["names"] = Value::from(names[from..].to_vec());
    let _ = threadbear_core::private_file::write(&seen_file, seen.to_string().as_bytes());
}

fn same_program(crash: &NativeCrash, exe: &Path) -> bool {
    let reported = Path::new(&crash.proc_path);
    reported == exe || matches!((reported.canonicalize(), exe.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// The hang watch's past freezes (core's `take_past`), sent from a thread of
/// their own so the watch starts watching straight away.
pub(crate) fn past_stalls(stalls: Vec<PastStall>) {
    let Some(r) = REPORTING.get() else { return };
    if stalls.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new().name("hang reports".into()).spawn(move || {
        for stall in stalls {
            let event = report::hang_event(base(r, &stall.record.at), &stall);
            send(r, &event, &format!("hang {}", threadbear_core::hang_watch::stuck_in(&stall.frames)));
        }
    });
}

fn base(r: &Reporting, at: &str) -> serde_json::Map<String, Value> {
    let id: String = threadbear_client::auth::new_nonce().chars().take(32).collect();
    report::base_event(&id, at, &r.release, r.os.as_deref())
}

/// Sends one event, or for an app that doesn't send, says what it would have.
fn send(r: &Reporting, event: &Value, what: &str) {
    if !r.send {
        eprintln!("threadbear: would report {what}");
        return;
    }
    let dsn_text = dsn();
    let Some(dsn) = report::Dsn::parse(&dsn_text) else { return };
    let body = report::envelope(&dsn_text, event, &threadbear_core::inbox::iso_time(crate::now_ms()));
    match post(&dsn.envelope_url(), dsn.auth_header("threadbear-desktop/1"), body) {
        Ok(()) => eprintln!("threadbear: reported {what}"),
        Err(e) => eprintln!("threadbear: couldn't report {what}: {e}"),
    }
}

/// One POST of an envelope to Sentry's endpoint (or THREADBEAR_SENTRY_DSN's).
fn post(url: &str, auth: String, body: Vec<u8>) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).user_agent("threadbear-desktop").build().into();
    agent.post(url).header("X-Sentry-Auth", auth).header("Content-Type", "application/x-sentry-envelope").send(body).map(|_| ()).map_err(|e| e.to_string())
}

/// The panics a scripted app has kept beside its automation file, for its
/// `crashes`: each as it would be sent.
#[cfg(feature = "automation")]
pub(crate) fn automation() -> Result<Value, String> {
    let r = REPORTING.get().ok_or("this app keeps no crash reports")?;
    let mut names: Vec<String> = std::fs::read_dir(&r.folder).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with("panic-") && n.ends_with(".json")).collect();
    names.sort();
    let panics: Vec<Value> = names.iter().filter_map(|n| std::fs::read(r.folder.join(n)).ok().and_then(|d| serde_json::from_slice::<Value>(&d).ok())).collect();
    Ok(Value::from(format!("ok {}", serde_json::json!({ "panics": panics }))))
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};

    /// The envelope reaches the endpoint the DSN names, with its key, as
    /// Sentry's own SDKs send one.
    #[test]
    fn an_envelope_reaches_the_dsns_endpoint_with_its_key() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            // Headers, then a body of the length they say.
            while !String::from_utf8_lossy(&seen).contains("\r\n\r\nhello") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        let dsn = threadbear_core::crash_report::Dsn::parse(&format!("http://k123@127.0.0.1:{port}/7")).unwrap();
        super::post(&dsn.envelope_url(), dsn.auth_header("threadbear-desktop/1"), b"hello".to_vec()).unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /api/7/envelope/ HTTP/1.1"), "{request}");
        assert!(request.to_lowercase().contains("x-sentry-auth: sentry sentry_version=7, sentry_key=k123"), "{request}");
        assert!(request.ends_with("hello"));
    }
}

/// "27.2.1", from the system's own version file (no program run for it).
fn macos_version() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let text = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let after = &text[text.find("<key>ProductVersion</key>")?..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")?;
    Some(after[start..start + end].trim().to_owned())
}
