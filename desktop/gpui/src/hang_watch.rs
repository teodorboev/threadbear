// The hang watch (the Swift app's HangWatch.swift): a thread of its own asks
// the main thread for a beat each second, and when one hasn't come back after
// 4 seconds it runs macOS's `sample` on this app, in a sandbox, while it's
// still stuck. The rules and the files are core's hang_watch.rs; this is the
// thread, the beat and the sample.
//
// Only the installed app watches itself, into ~/Library/Logs/Threadbear/
// hangs/desktop (the Swift app's own are the folder above, and each app only
// reads its own), and it sends the long ones to Sentry at its next launch
// (crash_reporting.rs). A scripted app watches into a folder beside its
// automation file, so a spec can see it work (`stall`), and sends nothing.
//
// The beat goes through GPUI's foreground executor, which runs on the main
// thread's run loop: while that's stuck, the beat can't come back. The watch
// thread never waits on anything the main thread could hold.
use crate::installed;
use futures::StreamExt as _;
use gpui_kit::App;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use threadbear_core::hang_watch::{self as rules, Beats, Context, Record, Tick};

/// Where this launch's stalls go, if it watches at all.
pub(crate) fn folder() -> Option<PathBuf> {
    let launch = installed::launch();
    if launch.scripted {
        let file = std::env::var_os("THREADBEAR_AUTOMATION")?;
        return Some(Path::new(&file).parent()?.join("hangs"));
    }
    launch.installed.then(|| installed::logs_dir().join("hangs").join("desktop"))
}

/// From the app's start, on the main thread.
pub(crate) fn start(cx: &mut App) {
    let Some(folder) = folder() else { return };
    let build = installed::launch().build.as_ref().map(|b| b.short.clone());
    let main_thread = main_thread_port();
    let (beat_tx, mut beat_rx) = futures::channel::mpsc::unbounded::<()>();
    let (back_tx, back_rx) = mpsc::channel::<()>();
    cx.spawn(async move |_| {
        while beat_rx.next().await.is_some() {
            if back_tx.send(()).is_err() {
                break;
            }
        }
    })
    .detach();
    let _ = std::thread::Builder::new().name("hang watch".into()).spawn(move || watch(folder, build, beat_tx, back_rx, main_thread));
}

fn watch(folder: PathBuf, build: Option<String>, beat: futures::channel::mpsc::UnboundedSender<()>, back: mpsc::Receiver<()>, main_thread: u32) {
    // What earlier launches caught, sent (or, scripted, only noted) and
    // tidied before anything new is written.
    crate::crash_reporting::past_stalls(rules::take_past(&folder, crate::now_ms()));
    rules::prune(&folder);
    // Instant stands still while the Mac sleeps, so a night's sleep isn't a stall.
    let started = Instant::now();
    let now = || started.elapsed().as_millis() as u64;
    let mut beats = Beats::default();
    let mut next_tick = Instant::now() + Duration::from_secs(1);
    loop {
        match back.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(()) => {
                if let Some((name, seconds)) = beats.arrived(now()) {
                    eprintln!("threadbear: main thread back after {seconds:.1} s");
                    let mut record = rules::read(&folder, &name).unwrap_or_else(|| Record { at: threadbear_core::inbox::iso_time(crate::now_ms()), build: build.clone(), ..Default::default() });
                    record.recovered_after = Some(seconds);
                    let _ = rules::write(&folder, &name, &record);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                next_tick = Instant::now() + Duration::from_secs(1);
                match beats.tick(now(), || rules::file_name(chrono_now())) {
                    Tick::Post => {
                        // The app has gone.
                        if beat.unbounded_send(()).is_err() {
                            return;
                        }
                    }
                    Tick::Stalled(name) => begin_sample(&folder, &name, build.clone(), main_thread),
                    Tick::Wait => {}
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn chrono_now() -> threadbear_core::chrono::DateTime<threadbear_core::chrono::Local> {
    threadbear_core::chrono::Local::now()
}

fn begin_sample(folder: &Path, name: &str, build: Option<String>, main_thread: u32) {
    let record = Record { at: threadbear_core::inbox::iso_time(crate::now_ms()), build, ..Default::default() };
    let _ = rules::write(folder, name, &record);
    let file = folder.join(format!("{name}.sample"));
    eprintln!("threadbear: main thread stuck for {} s, sampling to {}", rules::STALL_AFTER_MS / 1000, file.display());
    run_sample(&file);
    // After the sample's away: asking for the idle time goes through the
    // window server, and whatever that does, the sample is what counts.
    if let Some(mut record) = rules::read(folder, name) {
        record.context = Some(context(main_thread));
        let _ = rules::write(folder, name, &record);
    }
}

#[cfg(target_os = "macos")]
fn run_sample(file: &Path) {
    use std::process::{Command, Stdio};
    let home = std::env::var("HOME").unwrap_or_default();
    let arguments = threadbear_core::hang_sample::sample_arguments(&home, std::process::id(), rules::SAMPLE_SECONDS, &file.to_string_lossy());
    let mut sample = Command::new("/usr/bin/sandbox-exec");
    match sample.args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        // Waited for on a thread of its own, so it doesn't linger as a zombie;
        // this one carries on watching.
        Ok(mut child) => {
            let _ = std::thread::Builder::new().name("hang sample".into()).spawn(move || child.wait());
        }
        Err(e) => eprintln!("threadbear: couldn't run sample: {e}"),
    }
}

/// There's no `sample` here: the stall is noted without one.
#[cfg(not(target_os = "macos"))]
fn run_sample(_file: &Path) {}

/// The main thread, as the kernel knows it, taken while on it: looking it up
/// during a stall would take a lock the stuck thread might be holding.
#[cfg(target_os = "macos")]
fn main_thread_port() -> u32 {
    // SAFETY: both only read the calling thread's own identity.
    unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) }
}

#[cfg(not(target_os = "macos"))]
fn main_thread_port() -> u32 {
    0
}

#[cfg(target_os = "macos")]
fn context(main_thread: u32) -> Context {
    Context { main_priority: priority(main_thread), load_average: load_average(), idle_seconds: idle_seconds() }
}

#[cfg(not(target_os = "macos"))]
fn context(_main_thread: u32) -> Context {
    Context { main_priority: None, load_average: load_average(), idle_seconds: None }
}

/// The thread's scheduling priority now: 46 or so normally, 4 when something
/// has clamped it to background.
#[cfg(target_os = "macos")]
fn priority(thread: u32) -> Option<i64> {
    let mut info: libc::thread_extended_info = unsafe { std::mem::zeroed() };
    let mut count = libc::THREAD_EXTENDED_INFO_COUNT;
    // SAFETY: `info` is the size `count` says, and thread_info writes no more.
    let result = unsafe { libc::thread_info(thread, libc::THREAD_EXTENDED_INFO as libc::thread_flavor_t, (&mut info as *mut libc::thread_extended_info).cast(), &mut count) };
    (result == libc::KERN_SUCCESS).then_some(i64::from(info.pth_curpri))
}

#[cfg(unix)]
fn load_average() -> Option<f64> {
    let mut load = [0.0f64; 1];
    // SAFETY: room for the one number asked for.
    (unsafe { libc::getloadavg(load.as_mut_ptr(), 1) } == 1).then_some(load[0])
}

#[cfg(not(unix))]
fn load_average() -> Option<f64> {
    None
}

/// How long since the last key or click anywhere on the Mac: just how long.
#[cfg(target_os = "macos")]
pub(crate) fn idle_seconds() -> Option<f64> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType (~0).
    // SAFETY: a plain query with no pointers.
    let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    seconds.is_finite().then_some(seconds)
}

/// What the watch has noted, for a scripted app's `hangs`: each stall, how
/// long it lasted, and where its sample found the main thread.
#[cfg(feature = "automation")]
pub(crate) fn automation() -> Result<serde_json::Value, String> {
    use serde_json::json;
    let folder = folder().ok_or("this app keeps no hang watch")?;
    let stalls: Vec<serde_json::Value> = rules::names(&folder)
        .into_iter()
        .filter_map(|name| {
            let record = rules::read(&folder, &name)?;
            let sample = std::fs::read_to_string(folder.join(format!("{name}.sample"))).unwrap_or_default();
            let frames = threadbear_core::hang_sample::heaviest_main_thread_path(&sample);
            Some(json!({ "name": name, "recoveredAfter": record.recovered_after, "sampled": !frames.is_empty(), "stuckIn": (!frames.is_empty()).then(|| rules::stuck_in(&frames)), "frames": frames }))
        })
        .collect();
    Ok(json!(format!("ok {}", json!({ "stalls": stalls }))))
}
