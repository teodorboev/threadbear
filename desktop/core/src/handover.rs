//! Restarting onto a new build without the window going away, as the Swift
//! app's Handover.swift does (Mike, 29 Oct: "restart with a very, very
//! minimal flicker, if none at all"). The pure parts: the incoming app's
//! arguments, when the window can be handed over, when the incoming app is
//! ready to be seen, and the messages the two apps pass.
//!
//! The running app (the outgoing one) starts the new build as a second copy
//! with `--handover-from <its pid> --handover-below <its window number>
//! --handover-frame <its window's frame>`, and carries on as it was. The
//! incoming copy keeps its window invisible while it connects, loads the
//! threads and opens the thread that was open, in the same frame. Once that's
//! drawn it puts its window just under the outgoing one's, makes it visible
//! there, and says "ready". The outgoing app hands over being the active app
//! if it was, says "go" with what changed meanwhile (the list, the thread),
//! waits for "done", then hides its window and quits. So one of the two
//! windows is always showing, in the same place, with the same thing in it.
//!
//! Swift passes the messages as distributed notifications; this app writes
//! them as small files in a folder named for the outgoing pid
//! (`channel_dir`), which both poll every 50 ms. Two Threadbears (Mike's and
//! a dev app) never mix them up, and it needs nothing from the platform.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FROM_FLAG: &str = "--handover-from";
pub const BELOW_FLAG: &str = "--handover-below";
pub const FRAME_FLAG: &str = "--handover-frame";

/// How long the outgoing app waits for "ready" before quitting anyway (a
/// launch, a connect and a thread's first page take a second or two).
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the incoming app waits to be loaded before showing itself anyway
/// (a runner that's down never loads anything).
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(15);
/// After everything's loaded: a few frames for the transcript to lay out
/// and draw before it's shown.
pub const SETTLE_TIME: Duration = Duration::from_millis(250);
/// How long the outgoing app waits for "done" before hiding its window.
pub const DONE_TIMEOUT: Duration = Duration::from_secs(1);
/// How often each side looks for the other's message.
pub const POLL: Duration = Duration::from_millis(50);

/// A window's place on screen, in AppKit's own coordinates (bottom-left
/// origin), as the outgoing app's NSWindow has it: the incoming app's goes
/// exactly there, without converting through GPUI's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Frame {
    pub fn encode(&self) -> String {
        format!("{},{},{},{}", self.x, self.y, self.width, self.height)
    }

    pub fn decode(text: &str) -> Option<Frame> {
        let parts: Vec<f64> = text.split(',').map(|p| p.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
        let [x, y, width, height] = parts[..] else { return None };
        (width > 0. && height > 0. && [x, y, width, height].iter().all(|v| v.is_finite())).then_some(Frame { x, y, width, height })
    }
}

/// What the incoming app was started with, read from its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    pub from: u32,
    pub below: Option<i64>,
    pub frame: Option<Frame>,
}

fn after<'a>(flag: &str, arguments: &'a [String]) -> Option<&'a str> {
    let at = arguments.iter().position(|a| a == flag)?;
    arguments.get(at + 1).map(String::as_str)
}

/// The handover this launch is the incoming side of, if any.
pub fn incoming(arguments: &[String]) -> Option<Incoming> {
    let from = after(FROM_FLAG, arguments)?.parse::<u32>().ok().filter(|p| *p > 0)?;
    Some(Incoming { from, below: after(BELOW_FLAG, arguments).and_then(|n| n.parse().ok()), frame: after(FRAME_FLAG, arguments).and_then(Frame::decode) })
}

/// The incoming app's arguments: the outgoing app's own, without an earlier
/// handover's, plus this one's.
pub fn arguments(current: &[String], outgoing_pid: u32, window_number: i64, frame: Frame) -> Vec<String> {
    let mut kept = Vec::new();
    let mut skip = false;
    for argument in current {
        if skip {
            skip = false;
            continue;
        }
        if [FROM_FLAG, BELOW_FLAG, FRAME_FLAG].contains(&argument.as_str()) {
            skip = true;
            continue;
        }
        kept.push(argument.clone());
    }
    kept.extend([FROM_FLAG.into(), outgoing_pid.to_string(), BELOW_FLAG.into(), window_number.to_string(), FRAME_FLAG.into(), frame.encode()]);
    kept
}

/// Only a window that's plainly on screen is handed over: a minimised or
/// full-screen one, or a hidden app, restarts as before (the new window
/// would show where the old one wasn't).
pub fn can_hand_over(visible: bool, miniaturized: bool, full_screen: bool, app_hidden: bool) -> bool {
    visible && !miniaturized && !full_screen && !app_hidden
}

/// The incoming app is ready to be seen: the threads are in, the thread that
/// was open has its first page, or it's waited long enough.
pub fn loaded(threads_loaded: bool, connected: bool, selection_loaded: bool, waited: Duration) -> bool {
    (threads_loaded && connected && selection_loaded) || waited >= LOAD_TIMEOUT
}

/// The folder the two apps talk through, named for the outgoing pid.
pub fn channel_dir(temp: &Path, outgoing_pid: u32) -> PathBuf {
    temp.join(format!("threadbear-handover-{outgoing_pid}"))
}

pub const READY_FILE: &str = "ready.json";
pub const GO_FILE: &str = "go.json";
pub const DONE_FILE: &str = "done";

/// "ready": the incoming app's pid, so the outgoing one can hand it the
/// keys.
pub fn ready_message(incoming_pid: u32) -> Value {
    json!({ "pid": incoming_pid })
}

pub fn ready_pid(message: &Value) -> Option<u32> {
    message["pid"].as_u64().and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0)
}

/// "go": whether the outgoing app was in front, and what Mike did in its
/// window while the incoming one loaded (the list showing and the thread
/// open), and each open thread's place. Drafts and settings go through
/// their own files, written just before.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Go {
    pub active: bool,
    pub mode: Option<String>,
    pub selection: Option<String>,
    /// Each open thread's place as it went (gpui's `rows::live_scroll_json`).
    pub scroll: Value,
}

impl Go {
    pub fn to_json(&self) -> Value {
        json!({ "active": self.active, "mode": self.mode, "selection": self.selection, "scroll": self.scroll })
    }

    pub fn from_json(value: &Value) -> Go {
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        Go { active: value["active"].as_bool().unwrap_or(false), mode: text("mode"), selection: text("selection"), scroll: value["scroll"].clone() }
    }
}

/// Written whole and renamed into place, so the other side never reads half.
pub fn write_message(dir: &Path, name: &str, message: &Value) -> std::io::Result<()> {
    let data = serde_json::to_vec(message).map_err(std::io::Error::other)?;
    crate::private_file::write(&dir.join(name), &data)
}

pub fn read_message(dir: &Path, name: &str) -> Option<Value> {
    let data = std::fs::read(dir.join(name)).ok()?;
    serde_json::from_slice(&data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_incoming_app_gets_the_outgoing_ones_arguments_without_an_earlier_handovers() {
        let frame = Frame { x: 10., y: 20.5, width: 1200., height: 800. };
        let current = strings(&["--data-dir", "/tmp/x", FROM_FLAG, "41", BELOW_FLAG, "7", FRAME_FLAG, "0,0,1,1"]);
        let out = arguments(&current, 99, 12, frame);
        assert_eq!(out, strings(&["--data-dir", "/tmp/x", FROM_FLAG, "99", BELOW_FLAG, "12", FRAME_FLAG, "10,20.5,1200,800"]));
        assert_eq!(incoming(&out), Some(Incoming { from: 99, below: Some(12), frame: Some(frame) }));
    }

    #[test]
    fn a_launch_without_the_flag_is_no_handover() {
        assert_eq!(incoming(&strings(&["--data-dir", "/tmp/x"])), None);
        assert_eq!(incoming(&strings(&[FROM_FLAG])), None);
        assert_eq!(incoming(&strings(&[FROM_FLAG, "0"])), None);
        assert_eq!(incoming(&strings(&[FROM_FLAG, "12", FRAME_FLAG, "nonsense"])), Some(Incoming { from: 12, below: None, frame: None }));
    }

    #[test]
    fn a_frame_with_no_size_is_no_frame() {
        assert_eq!(Frame::decode("1,2,0,5"), None);
        assert_eq!(Frame::decode("1,2,3"), None);
        assert_eq!(Frame::decode("1,2,3,4,5"), None);
        assert_eq!(Frame::decode("-40,2,300,400"), Some(Frame { x: -40., y: 2., width: 300., height: 400. }));
    }

    #[test]
    fn only_a_window_plainly_on_screen_is_handed_over() {
        assert!(can_hand_over(true, false, false, false));
        assert!(!can_hand_over(false, false, false, false));
        assert!(!can_hand_over(true, true, false, false));
        assert!(!can_hand_over(true, false, true, false));
        assert!(!can_hand_over(true, false, false, true));
    }

    #[test]
    fn the_incoming_app_shows_once_everything_is_in_or_it_has_waited_long_enough() {
        assert!(loaded(true, true, true, Duration::ZERO));
        assert!(!loaded(true, true, false, Duration::from_secs(3)));
        assert!(!loaded(false, true, true, Duration::from_secs(3)));
        assert!(!loaded(true, false, true, Duration::from_secs(3)));
        assert!(loaded(false, false, false, LOAD_TIMEOUT));
    }

    #[test]
    fn messages_go_round_through_the_channel() {
        let temp = std::env::temp_dir().join(format!("tb-handover-{}", std::process::id()));
        let dir = channel_dir(&temp, 4242);
        assert!(dir.ends_with("threadbear-handover-4242"));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_message(&dir, READY_FILE), None);
        write_message(&dir, READY_FILE, &ready_message(77)).unwrap();
        assert_eq!(read_message(&dir, READY_FILE).as_ref().and_then(ready_pid), Some(77));
        let go = Go { active: true, mode: Some("scheduled".into()), selection: Some("t1".into()), scroll: json!({ "t1": { "pinned": true } }) };
        write_message(&dir, GO_FILE, &go.to_json()).unwrap();
        assert_eq!(Go::from_json(&read_message(&dir, GO_FILE).unwrap()), go);
        assert_eq!(Go::from_json(&json!({})), Go::default());
        let _ = std::fs::remove_dir_all(&temp);
    }
}
