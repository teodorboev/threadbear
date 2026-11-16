// What the nightly parity run (scripts/parity/nightly.sh) found the GPUI app
// leaving out of another runner's thread on 14 Nov 2026, with the events it
// got through Convex: the scenarios "a message saved after Stop before the
// model started" and "a turn that failed" (runner/test/helpers/scenario-list.ts).

mod support;

use serde_json::json;
use support::*;
use threadbear_core::reducer::*;

const SAVED: &str = "Your stopped message and pictures are saved for your next message.";

fn words(model: &TranscriptModel) -> Vec<String> {
    model.rows().iter().map(RowDebug::text).collect()
}

/// Mike stopped a turn before the model started: the runner keeps his
/// message (and its pictures) for the next send and says so in a note. The
/// note is what tells him his words weren't lost, so it stays on show, as
/// a login note does, rather than folding away as work. Another Mac gets it
/// with `login: true` (relay.ts), this one with `savedMessage: true`; the
/// words are the same.
#[test]
fn the_note_a_stopped_message_was_saved_stays_on_show() {
    for flag in ["savedMessage", "login"] {
        let events = vec![
            event_turn(1, "user.message", json!({"text": "Start", "images": []}), Some("t1")),
            event_turn(2, "assistant.text", json!({"itemId": "m1", "text": "Ready."}), Some("t1")),
            done_turn(3, "t1"),
            event_turn(4, "user.message", json!({"text": "Please fix the spot in this picture.", "images": []}), Some("t2")),
            event_turn(5, "system.note", json!({"text": SAVED, flag: true}), Some("t2")),
            done_turn(6, "t2"),
            event_turn(7, "user.message", json!({"text": "Continuing on another model.", "images": []}), Some("t3")),
            event_turn(8, "assistant.text", json!({"itemId": "m3", "text": "I can fix the spot."}), Some("t3")),
            done_turn(9, "t3"),
        ];
        let model = from_events(events);
        assert!(ids(&model).contains(&"n:5".to_owned()), "the note is a row of its own ({flag}): {:?}", ids(&model));
        assert!(words(&model).iter().any(|w| w == SAVED), "{:?}", words(&model));
    }
}

/// Another runner's failed turn: the app draws "Failed after 0 ms: The last
/// turn failed on mac-mini. ..." on its footer, so the row's text (what the
/// scripted app's dump and the parity run read) says the error too.
#[test]
fn a_failed_turns_footer_says_its_error() {
    let error = "The last turn failed on mac-mini. Open it there to see why, or send a message to try again.";
    let model = from_events(vec![
        event_turn(1, "user.message", json!({"text": "Start", "images": []}), Some("t1")),
        event_turn(2, "turn.completed", json!({"turnId": "t1", "outcome": "failed", "error": error, "durationMs": 0}), Some("t1")),
    ]);
    assert!(words(&model).iter().any(|w| w.contains(error)), "{:?}", words(&model));
}
