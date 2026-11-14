// A thread whose login ran out: what the transcript keeps on show. The GPUI
// app's own (the Swift app was frozen before this, so there's no Swift test
// to compare with): Mike, 13 Nov 2026, with a picture of a red "You've hit
// your 5-hour limit" under the same words in plain text, and then a reply:
// "I keep hitting this but then it carries on. Is that because it auto
// picked another provider? If so, it should tell me." The runner had said
// so, in two notes that folded into the line of work.

mod support;

use serde_json::json;
use support::*;
use threadbear_core::reducer::*;

const LIMIT: &str = "You've hit your 5-hour limit · resets 11:30am (Australia/Perth)";
const MOVED: &str = "Moved to mike@example.org: mike@example.net hit its 5-hour limit, back at 11:30 am.";
const MOVED_AGAIN: &str = "Moved to mike@example.com: mike@example.org hit its weekly limit, back at Thu 1 pm.";
const WAITING: &str = "Every login in Claude is out of room. Carrying on at 11:30 am when mike@example.net is back.";

fn mike(seq: i64, words: &str, turn: &str) -> Event {
    event_turn(seq, "user.message", json!({"text": words, "images": []}), Some(turn))
}

fn runner(seq: i64, words: &str, turn: &str) -> Event {
    event_turn(seq, "user.message", json!({"text": words, "images": [], "origin": "runner"}), Some(turn))
}

fn said(seq: i64, item: &str, words: &str, turn: &str) -> Event {
    event_turn(seq, "assistant.text", json!({"itemId": item, "text": words}), Some(turn))
}

fn note(seq: i64, words: &str) -> Event {
    event_no_turn(seq, "system.note", json!({"text": words}))
}

/// The three events a turn stopped at its limit ends with, as the Claude
/// CLI and the runner write them: the CLI's own message, the runner's error
/// with the same words, and the failed turn.
fn ran_dry(seq: i64, turn: &str) -> Vec<Event> {
    vec![
        said(seq, &format!("limit-{turn}"), LIMIT, turn),
        event_turn(seq + 1, "error", json!({"message": LIMIT}), Some(turn)),
        event_turn(seq + 2, "turn.completed", json!({"turnId": turn, "outcome": "failed", "durationMs": 600, "error": LIMIT, "errorKind": "usageLimit"}), Some(turn)),
    ]
}

/// A turn of work that runs dry at `seq`, after Mike's message.
fn work_then_dry() -> Vec<Event> {
    let mut events = vec![mike(1, "Land the fix", "t1"), call_titled(2, "c1", "tb land"), result(3, "c1")];
    events.extend(ran_dry(4, "t1"));
    events
}

fn words(model: &TranscriptModel) -> Vec<String> {
    model.rows().iter().map(RowDebug::text).collect()
}

/// As it happened in Mike's thread: the login ran out, the thread moved
/// twice (the first login it tried turned out to be out for the week) and
/// carried on. Both moves are on show, and the limit is said once, by them.
#[test]
fn a_thread_that_moved_to_another_login_says_so() {
    let mut events = work_then_dry();
    events.extend([
        note(7, MOVED),
        note(8, MOVED_AGAIN),
        runner(9, "Your last login hit its usage limit partway through that turn, so this thread has moved to another one. Carry on from where you left off.", "t2"),
        call_titled(10, "c2", "git push").with_turn("t2"),
        result(11, "c2").with_turn("t2"),
        said(12, "m2", "Landed on main.", "t2"),
        done_turn(13, "t2"),
    ]);
    let model = from_events(events);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "n:7", "n:8", "a:m2", "f:t2"]);
    let Some(RowContent::Group(group)) = model.row("g:t:c1").map(|r| &r.content) else { panic!("a group") };
    assert_eq!(group.member_ids, ["t:c1", "u:9", "t:c2"], "the work and the runner's message, with neither copy of the limit");
    assert!(!words(&model).iter().any(|w| w.contains("You've hit your")), "the notes say which login ran out: {:?}", words(&model));
}

/// The same thread a second after the limit, while the turn that carries on
/// is still running: the moves are already on show, with no red box.
#[test]
fn the_move_shows_while_the_thread_carries_on() {
    let mut events = work_then_dry();
    events.extend([
        note(7, MOVED),
        event_turn(8, "turn.started", json!({}), Some("t2")),
        runner(9, "Your last login hit its usage limit partway through that turn.", "t2"),
        call_titled(10, "c2", "git push").with_turn("t2"),
    ]);
    let model = from_events(events);
    let rows = ids(&model);
    assert!(rows.iter().any(|r| r == "n:7"), "{rows:?}");
    assert!(!model.rows().iter().any(|r| r.kind() == RowKind::Error), "{rows:?}");
    assert!(!rows.iter().any(|r| r == "a:limit-t1"), "{rows:?}");
}

/// The events arriving one at a time, as they do in an open thread: the red
/// box is there for the instant before the runner says what it did, then
/// the note takes its place.
#[test]
fn the_note_takes_the_errors_place_as_it_arrives() {
    let mut model = from_events(work_then_dry());
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "e:5", "f:t1"], "nothing says what happens next yet, so the error stays; its echo is gone");
    model.apply(vec![note(7, MOVED)]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "f:t1", "n:7"]);
}

/// Nothing in the pool had room: the thread waits, and the note says until
/// when. The box over the message field has the buttons.
#[test]
fn a_thread_waiting_for_room_says_when_it_carries_on() {
    let mut events = work_then_dry();
    events.push(note(7, WAITING));
    let model = from_events(events);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "f:t1", "n:7"]);
    assert_eq!(words(&model).last().map(String::as_str), Some(WAITING));
}

/// Told not to carry on by itself, the runner leaves no note: the error is
/// all there is to say why the thread stopped, so it stays, said once.
#[test]
fn a_limit_nothing_was_done_about_keeps_its_error() {
    let mut events = work_then_dry();
    // Not a login's note: it doesn't stand in for the error.
    events.push(note(7, "Put in Threadbear by the librarian."));
    events.extend([mike(8, "Carry on", "t2"), said(9, "m2", "Done.", "t2"), done_turn(10, "t2")]);
    let model = from_events(events);
    let rows = ids(&model);
    assert!(rows.iter().any(|r| r == "e:5"), "{rows:?}");
    assert!(!rows.iter().any(|r| r == "a:limit-t1"), "the same words twice: {rows:?}");
    assert!(!rows.iter().any(|r| r == "n:7"), "an ordinary note still folds: {rows:?}");
}

/// Only the error's own words over again go. A failed turn that said
/// something else first keeps it as its reply.
#[test]
fn a_failed_turns_other_words_stay() {
    let model = from_events(vec![
        mike(1, "Deploy it", "t1"),
        said(2, "m1", "Deploying now.", "t1"),
        event_turn(3, "error", json!({"message": "The CLI stopped."}), Some("t1")),
        event_turn(4, "turn.completed", json!({"turnId": "t1", "outcome": "failed", "durationMs": 600, "error": "The CLI stopped."}), Some("t1")),
    ]);
    assert_eq!(ids(&model), ["u:1", "a:m1", "e:3", "f:t1"]);
}

/// The turn ended its work with end_turn and a summary, and its login ran
/// out before it wrote its reply: the summary is the reply, not lost under
/// the limit's words (from Codex's review of this).
#[test]
fn a_summary_written_before_the_limit_is_still_the_reply() {
    let mut events = vec![
        mike(1, "Patch it", "t1"),
        call_titled(2, "c1", "make"),
        result(3, "c1"),
        call_with(4, "e1", "mcp__threadbear__end_turn", "command", "ls", json!({"needs": "nothing"})),
        result_text(5, "e1", "Noted: this thread is done."),
        event_turn(6, "assistant.text", json!({"itemId": "e1.summary", "text": "- Patched.", "summary": true}), Some("t1")),
    ];
    events.extend(ran_dry(7, "t1"));
    events.push(note(10, MOVED));
    let model = from_events(events);
    let rows = ids(&model);
    assert!(rows.iter().any(|r| r == "a:e1.summary"), "{rows:?}");
    assert!(rows.iter().any(|r| r == "n:10"), "{rows:?}");
    assert!(!rows.iter().any(|r| r == "a:limit-t1" || r == "e:8"), "{rows:?}");
}

/// A message that was streaming as the app reconnected never gets its turn
/// (the live copy has none), so its error is found by place.
#[test]
fn an_echo_with_no_turn_goes_too() {
    let mut events = work_then_dry();
    events[3].turn_id = None;
    events.push(note(7, MOVED));
    let model = from_events(events);
    let rows = ids(&model);
    assert!(!rows.iter().any(|r| r == "a:limit-t1" || r == "e:5"), "{rows:?}");
}

/// Mike's No on the limit box takes the wait back: that stays on show too,
/// or the line above it would go on promising a time.
#[test]
fn turning_carry_on_off_shows_under_the_wait() {
    let mut events = work_then_dry();
    events.extend([note(7, WAITING), note(8, "Won't carry on by itself. Send a message when you want it to.")]);
    let model = from_events(events);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "f:t1", "n:7", "n:8"]);
}

#[test]
fn which_notes_are_a_logins() {
    for text in [MOVED, MOVED_AGAIN, WAITING, "Won't carry on by itself. Send a message when you want it to.", "Moved to Codex: mike@example.net isn't in Claude.", "Still no room in Claude. Carrying on at 5:00 am.", "mike@example.net hit its usage limit. Carrying on at 6:10 pm when mike@example.net is back."] {
        assert!(RowContent::Note(text.to_owned()).is_login_note(), "{text}");
    }
    // A move to another folder is one Mike (or the agent) asked for.
    for text in [
        "Moved to /Users/mike/dev/app. The session carries on there.",
        "Moved to /Users/mike/dev/app on mike@example.org. The session carries on there, with the whole conversation.",
        "Resumed the session on mike@example.org, with the whole conversation.",
        "Put in Threadbear by the librarian.",
        "Woke up after hibernating for 2 days.",
    ] {
        assert!(!RowContent::Note(text.to_owned()).is_login_note(), "{text}");
    }
}
