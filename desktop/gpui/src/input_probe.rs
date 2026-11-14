// How long a hover or a press takes to show: the moment the app handles the
// event, where the pointer was, and when the frame after it was handed to the
// screen. features/tools/hover-perf.ts posts real pointer moves and presses,
// notes when it posted each one, and joins the two, so the number it gives is
// from the event leaving the window server to the frame that shows it being
// submitted (all but the last vsync to the glass).
//
// Frames come from GPUI's own foreground journal (its profiler), which sees
// every presented frame however much of it was drawn from the last one: a
// marker painted by the app misses a frame that only redraws the sidebar, as
// the views around it are replayed rather than painted (Codex's review).
// Presses come from the journal's input dispatches too.
//
// It only records between the automation's `input-probe` with `start` and
// the next one; outside those, `input` reads a flag and returns.
use gpui_kit::*;
use std::cell::{Cell, RefCell};

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    /// (what, window x, window y, handled at) for each hover since `start`.
    static INPUTS: RefCell<Vec<(&'static str, f32, f32, u64)>> = const { RefCell::new(Vec::new()) };
}

/// The same clock the pointer tool reads (`clock_gettime_nsec_np` with
/// CLOCK_UPTIME_RAW): what macOS stamps events with, and what `Instant` reads.
fn now_ns() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // Not in the libc crate. CLOCK_UPTIME_RAW is 8 in <time.h>.
        unsafe extern "C" {
            fn clock_gettime_nsec_np(clock: u32) -> u64;
        }
        unsafe { clock_gettime_nsec_np(8) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

/// A hover that changes what's drawn, like a row's highlight coming on.
pub(crate) fn input(what: &'static str, window: &Window) {
    if !ON.with(Cell::get) {
        return;
    }
    let at = window.mouse_position();
    INPUTS.with(|i| i.borrow_mut().push((what, f32::from(at.x), f32::from(at.y), now_ns())));
}

/// CPU time this thread (the main one, where the automation's commands run)
/// has used: unlike the time a frame takes, other busy processes hardly move
/// it, so two builds compare on a loaded machine.
#[cfg(all(feature = "automation", feature = "frame-stats"))]
fn thread_cpu_ns() -> i64 {
    crate::frame_stats::thread_cpu_nanos() as i64
}

#[cfg(all(feature = "automation", feature = "frame-stats"))]
mod journal {
    use super::*;
    use gpui_kit::profiler::journal::{ForegroundEvent, ForegroundJournalCollector, ForegroundJournalEntry, IntervalBoundary};
    use serde_json::{json, Value};
    use std::time::Instant;

    thread_local! {
        /// The journal's reader since `start`, and an `Instant` with the same
        /// moment on the pointer tool's clock, to turn one into the other.
        static READER: RefCell<Option<(ForegroundJournalCollector, Instant, u64, i64)>> = const { RefCell::new(None) };
    }

    pub(crate) fn report(start: bool, cx: &App) -> Value {
        if start {
            INPUTS.with(|i| i.borrow_mut().clear());
            READER.with(|r| *r.borrow_mut() = Some((cx.foreground_journal().collector(), Instant::now(), now_ns(), thread_cpu_ns())));
            ON.with(|o| o.set(true));
            return json!("recording");
        }
        ON.with(|o| o.set(false));
        let cpu_ns = thread_cpu_ns();
        let Some((mut reader, base, base_ns, base_cpu)) = READER.with(|r| r.borrow_mut().take()) else { return json!({ "error": "not started" }) };
        let ns = |at: Instant| base_ns as i64 + if at >= base { at.duration_since(base).as_nanos() as i64 } else { -(base.duration_since(at).as_nanos() as i64) };
        let drained = reader.collect_unseen();
        let (mut presented, mut presses) = (Vec::<i64>::new(), Vec::<(i64, i64)>::new());
        for entry in &drained.entries {
            match entry {
                ForegroundJournalEntry::Boundary(IntervalBoundary::Presented(p)) => presented.push(ns(p.presentation.present_end)),
                ForegroundJournalEntry::Event(ForegroundEvent::Input(i)) if i.kind == "mouse_down" => presses.push((ns(i.start), ns(i.end))),
                _ => {}
            }
        }
        presented.sort_unstable();
        // The first frame handed over after the handler ran is the one that
        // shows what it changed: handlers and drawing share the main thread.
        let after = |t: i64| presented.iter().find(|p| **p >= t).copied();
        let hovers: Vec<Value> = INPUTS
            .with(|i| std::mem::take(&mut *i.borrow_mut()))
            .into_iter()
            .map(|(what, x, y, at)| json!({ "what": what, "x": x, "y": y, "handledNs": at, "presentedNs": after(at as i64) }))
            .collect();
        let presses: Vec<Value> = presses.into_iter().map(|(start, end)| json!({ "dispatchedNs": start, "presentedNs": after(end) })).collect();
        json!({ "hovers": hovers, "presses": presses, "frames": presented.len(), "lost": drained.lost, "mainCpuMs": (cpu_ns - base_cpu) as f64 / 1e6 })
    }
}

/// `input-probe`: with `start`, records from now; without, hands back what it
/// recorded and stops.
#[cfg(feature = "automation")]
pub(crate) fn report(start: bool, cx: &App) -> serde_json::Value {
    #[cfg(feature = "frame-stats")]
    {
        journal::report(start, cx)
    }
    #[cfg(not(feature = "frame-stats"))]
    {
        let _ = (start, cx);
        serde_json::json!({ "error": "this build can't time frames: build it with --features frame-stats" })
    }
}
