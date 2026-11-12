//! The hang watch's rules, with no threads or clocks of its own: the GPUI app's
//! `hang_watch.rs` runs them. A port of the Swift app's HangWatch.
//!
//! A background thread asks the main thread for a beat each second, and when
//! one hasn't come back after `STALL_AFTER` it runs `/usr/bin/sample` on the
//! app (hang_sample.rs), which works however stuck the main thread is. A
//! freeze ends in a force-quit, which leaves no crash report, and by then
//! there's nothing left to sample: Mike hit several in a day (29 and 30 Oct)
//! and all the logs could say was where they began. Each stall is a record
//! (`hang-<time>.json`) beside its sample (`.sample`), with a note of how it
//! ended; the next launch sends only stalls long enough to be freezes, as
//! their frames (never the file).
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::Path;

/// Long enough that no ordinary busy moment gets sampled, short enough that
/// the sample is done before anyone reaches for Force Quit.
pub const STALL_AFTER_MS: u64 = 4_000;
pub const SAMPLE_SECONDS: u32 = 3;
/// What the next launch reports. Shorter ones stay in the folder.
pub const REPORT_AFTER_SECONDS: f64 = 15.;
/// Files older than this at launch are finished: a sample outlives the app
/// it's sampling, and may still be writing just after a force-quit.
pub const SETTLED_AFTER_MS: i64 = 30_000;
pub const KEEP: usize = 10;

/// One stall, as the Swift app's `HangWatch.Record` writes it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// When it was noticed, ISO 8601 in UTC.
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Seconds until the main thread came back; none if it never did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_after: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
}

/// How the Mac was when the stall was noticed, sent with the report. A stall
/// at low priority on an overloaded Mac with nobody there (THREADBEAR-1B: 33
/// s, display off, load 60 to 77) isn't the same bug as one under Mike's
/// hands, and afterwards nothing could say what the priority had been.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    /// The main thread's scheduling priority at that moment: 46 or so
    /// normally, 4 when something has clamped it to background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_priority: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_average: Option<f64>,
    /// How long since the last key or click anywhere on the Mac: just how
    /// long, never what.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<f64>,
}

impl Record {
    /// What goes to Sentry with the frames: symbol names and numbers, no user
    /// text (`Record.extras`).
    pub fn extras(&self, frames: &[String]) -> Value {
        let mut extras = Map::new();
        let from = frames.len().saturating_sub(60);
        extras.insert("frames".into(), json!(frames[from..]));
        extras.insert("seconds".into(), self.recovered_after.map_or_else(|| json!("never came back"), |s| json!(s as i64)));
        extras.insert("build".into(), json!(self.build.as_deref().unwrap_or("unknown")));
        if let Some(context) = &self.context {
            if let Some(p) = context.main_priority {
                extras.insert("mainPriority".into(), json!(p));
            }
            if let Some(l) = context.load_average {
                extras.insert("loadAverage".into(), json!(l));
            }
            if let Some(i) = context.idle_seconds {
                extras.insert("idleSeconds".into(), json!(i as i64));
            }
        }
        Value::Object(extras)
    }
}

/// The beat the watch thread keeps going to the main thread, and the stall
/// it's in, if any. Times are milliseconds of a clock that stands still while
/// the Mac sleeps (Rust's `Instant` on a Mac), so a night's sleep isn't a
/// stall.
#[derive(Debug, Default)]
pub struct Beats {
    posted_at: Option<u64>,
    stall: Option<(String, u64)>,
}

/// What the watch thread does on a tick.
#[derive(Debug, PartialEq)]
pub enum Tick {
    /// No beat out: send one.
    Post,
    /// The beat's been out too long: sample now, under this name.
    Stalled(String),
    Wait,
}

impl Beats {
    /// Once a second. `name` is the new stall's file name, asked for only
    /// when there is one.
    pub fn tick(&mut self, now: u64, name: impl FnOnce() -> String) -> Tick {
        let Some(posted) = self.posted_at else {
            self.posted_at = Some(now);
            return Tick::Post;
        };
        if self.stall.is_none() && now.saturating_sub(posted) >= STALL_AFTER_MS {
            let name = name();
            self.stall = Some((name.clone(), posted));
            return Tick::Stalled(name);
        }
        Tick::Wait
    }

    /// The beat came back. A stall it ends, with how long it lasted in
    /// seconds, from when the beat went out.
    pub fn arrived(&mut self, now: u64) -> Option<(String, f64)> {
        self.posted_at = None;
        let (name, started) = self.stall.take()?;
        Some((name, now.saturating_sub(started) as f64 / 1000.))
    }
}

/// "hang-2026-10-29-091502", by the local time it was noticed: the names sort
/// by time.
pub fn file_name(local: chrono::DateTime<chrono::Local>) -> String {
    local.format("hang-%Y-%m-%d-%H%M%S").to_string()
}

/// The records in `folder`, oldest first, by name without `.json`.
pub fn names(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut names: Vec<String> = entries.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).filter(|n| n.starts_with("hang-")).map(str::to_owned)).collect();
    names.sort();
    names
}

pub fn read(folder: &Path, name: &str) -> Option<Record> {
    let data = std::fs::read(folder.join(format!("{name}.json"))).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Written whole and renamed into place, so the next launch never reads half,
/// and this user's only, as a sample names what the app was doing.
pub fn write(folder: &Path, name: &str, record: &Record) -> std::io::Result<()> {
    let data = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    crate::private_file::write(&folder.join(format!("{name}.json")), &data)
}

/// A past stall to send: its main thread's heaviest path and its record.
#[derive(Debug, PartialEq)]
pub struct PastStall {
    pub name: String,
    pub frames: Vec<String>,
    pub record: Record,
}

/// The past stalls the next launch sends, each marked as reported so it's
/// sent once: settled, long enough (or never over), and with a sample that
/// shows the main thread busy (`reportPast`).
pub fn take_past(folder: &Path, now_ms: i64) -> Vec<PastStall> {
    let mut out = Vec::new();
    for name in names(folder) {
        let Some(mut record) = read(folder, &name) else { continue };
        if record.reported == Some(true) {
            continue;
        }
        let at = crate::inbox::parse_ms(&record.at).unwrap_or(i64::MIN);
        if now_ms.saturating_sub(at) < SETTLED_AFTER_MS {
            continue;
        }
        record.reported = Some(true);
        let _ = write(folder, &name, &record);
        if record.recovered_after.is_some_and(|s| s < REPORT_AFTER_SECONDS) {
            continue;
        }
        let sample = std::fs::read_to_string(folder.join(format!("{name}.sample"))).unwrap_or_default();
        let frames = crate::hang_sample::heaviest_main_thread_path(&sample);
        if frames.is_empty() || crate::hang_sample::is_idle(&frames) {
            continue;
        }
        out.push(PastStall { name, frames, record });
    }
    out
}

/// Keeps the newest `KEEP`, with their samples.
pub fn prune(folder: &Path) {
    let names = names(folder);
    for name in names.iter().take(names.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(folder.join(format!("{name}.json")));
        let _ = std::fs::remove_file(folder.join(format!("{name}.sample")));
    }
}

/// What a report's title names the stall by: the app's own deepest frame, or
/// the top of the stack.
pub fn stuck_in(frames: &[String]) -> String {
    crate::hang_sample::deepest_app_frame(frames).or_else(|| frames.last().cloned()).unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-hang-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const BUSY: &str = "Call graph:
    3000 Thread_1   DispatchQueue_1: com.apple.main-thread  (serial)
    + 3000 main  (in threadbear-gpui) + 36  [0x104d5e0c4]
    + ! 3000 threadbear_gpui::sidebar::SidebarView::rows::h0123456789abcdef  (in threadbear-gpui) + 8  [0x104e10000]
    + !   3000 stat  (in libsystem_kernel.dylib) + 8  [0x18941fc34]
";

    #[test]
    fn a_beat_out_four_seconds_is_a_stall_once_and_its_return_says_how_long() {
        let mut beats = Beats::default();
        assert_eq!(beats.tick(0, || unreachable!()), Tick::Post);
        assert_eq!(beats.tick(1_000, || unreachable!()), Tick::Wait);
        assert_eq!(beats.tick(4_000, || "hang-a".into()), Tick::Stalled("hang-a".into()));
        // Still stuck: one sample is enough.
        assert_eq!(beats.tick(5_000, || unreachable!()), Tick::Wait);
        assert_eq!(beats.arrived(6_500), Some(("hang-a".to_owned(), 6.5)));
        assert_eq!(beats.tick(7_000, || unreachable!()), Tick::Post);
        // A beat that comes straight back ends nothing.
        assert_eq!(beats.arrived(7_001), None);
    }

    #[test]
    fn a_record_reads_as_the_swift_app_writes_it() {
        let swift = r#"{"at":"2026-10-30T09:15:02Z","build":"abc1234","recoveredAfter":33.4,"context":{"mainPriority":4,"loadAverage":61.5,"idleSeconds":812.2}}"#;
        let record: Record = serde_json::from_str(swift).unwrap();
        assert_eq!(record.recovered_after, Some(33.4));
        assert_eq!(record.context.as_ref().and_then(|c| c.main_priority), Some(4));
        // Nothing it hasn't got is written as null.
        let fresh = Record { at: "2026-10-30T09:15:02Z".into(), ..Default::default() };
        assert_eq!(serde_json::to_string(&fresh).unwrap(), r#"{"at":"2026-10-30T09:15:02Z"}"#);
    }

    #[test]
    fn extras_are_numbers_and_names_only() {
        let record = Record { at: "x".into(), build: Some("abc1234".into()), recovered_after: Some(20.7), reported: None, context: Some(Context { main_priority: Some(46), load_average: Some(2.5), idle_seconds: Some(3.9) }) };
        let extras = record.extras(&["a (in threadbear-gpui)".into()]);
        assert_eq!(extras, json!({ "frames": ["a (in threadbear-gpui)"], "seconds": 20, "build": "abc1234", "mainPriority": 46, "loadAverage": 2.5, "idleSeconds": 3 }));
        let never = Record { at: "x".into(), ..Default::default() };
        assert_eq!(never.extras(&[])["seconds"], json!("never came back"));
    }

    #[test]
    fn only_settled_long_busy_stalls_are_sent_and_each_once() {
        let dir = folder("past");
        let now = crate::inbox::parse_ms("2026-11-11T10:00:00Z").unwrap();
        let old = "2026-11-11T09:00:00Z".to_owned();
        let stalls = [
            ("hang-1", Record { at: old.clone(), recovered_after: Some(20.), ..Default::default() }, BUSY),
            // Short: kept, not sent.
            ("hang-2", Record { at: old.clone(), recovered_after: Some(5.), ..Default::default() }, BUSY),
            // Never came back: a force-quit.
            ("hang-3", Record { at: old.clone(), ..Default::default() }, BUSY),
            // Sampled after it was over.
            ("hang-4", Record { at: old.clone(), recovered_after: Some(16.), ..Default::default() }, "Call graph:\n    5 Thread_1   DispatchQueue_1: com.apple.main-thread  (serial)\n    + 5 _DPSNextEvent  (in AppKit)\n    + ! 5 __CFRunLoopServiceMachPort  (in CoreFoundation)\n"),
            // Still settling: the next launch's.
            ("hang-5", Record { at: "2026-11-11T09:59:50Z".into(), ..Default::default() }, BUSY),
        ];
        for (name, record, sample) in &stalls {
            write(&dir, name, record).unwrap();
            std::fs::write(dir.join(format!("{name}.sample")), sample).unwrap();
        }
        let past = take_past(&dir, now);
        let sent: Vec<&str> = past.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(sent, ["hang-1", "hang-3"]);
        assert_eq!(stuck_in(&past[0].frames), "threadbear_gpui::sidebar::SidebarView::rows (in threadbear-gpui)");
        assert!(take_past(&dir, now).is_empty(), "each is sent once");
        assert_eq!(read(&dir, "hang-2").unwrap().reported, Some(true));
        // Settled by a later launch.
        assert_eq!(take_past(&dir, now + 60_000).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_newest_ten_are_kept() {
        let dir = folder("prune");
        for n in 0..12 {
            let name = format!("hang-2026-11-{n:02}");
            write(&dir, &name, &Record { at: "x".into(), ..Default::default() }).unwrap();
            std::fs::write(dir.join(format!("{name}.sample")), "").unwrap();
        }
        // Not one of the watch's: left alone.
        std::fs::write(dir.join("notes.json"), "{}").unwrap();
        prune(&dir);
        let left = names(&dir);
        assert_eq!(left.len(), KEEP);
        assert_eq!(left[0], "hang-2026-11-02");
        assert!(!dir.join("hang-2026-11-00.sample").exists());
        assert!(dir.join("notes.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
