// A thread caught up through `threads.subscribe` (a reconnect, or a thread the
// GPUI app kept loaded coming back into view, pane_cache.rs): deltas pushed
// while the subscribe was in flight are held and come after the reply's
// events, so one for a reply that's since been finished mustn't add its words
// again or set it streaming (Codex's review of the kept threads, 12 Nov).
mod support;

use serde_json::json;
use support::*;

#[test]
fn relay_subscription_reports_its_own_sparse_events_including_unflushed_results() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![user(25, "First"), call(41, "tool"), text(73, "reply", "Done")], &[]);
    let mut feed = ThreadFeed::new(model);
    assert_eq!(feed.relay_seen(), None);
    feed.set_sparse(true);
    feed.receive(result(42, "tool"));
    assert_eq!(feed.relay_seen(), Some(vec![[25, 25], [41, 42], [73, 73]]));
    feed.flush();
    assert_eq!(feed.relay_seen(), Some(vec![[25, 25], [41, 42], [73, 73]]));
}
use threadbear_core::reducer::*;

#[test]
fn a_held_delta_after_the_reply_finished_changes_nothing() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "Hi")], &[]);
    let mut feed = ThreadFeed::new(model);
    feed.begin_subscribe();
    // "Hello" streamed before the subscribe; "!" pushed while it was in flight.
    let _ = feed.receive(delta(0, "m1", "!"));
    let in_progress = [InProgress { item_id: "m1".into(), type_: "assistant.text".into(), text: "Hello".into() }];
    let _ = feed.finish_subscribe(vec![text(3, "m1", "Hello!"), done(4)], &in_progress);
    let _ = feed.flush();
    let RowContent::Assistant(item) = &feed.model().item("a:m1").expect("the reply").content else { panic!("an assistant row") };
    assert_eq!(item.text, "Hello!");
    assert!(!item.is_streaming, "finished");
}

#[test]
fn a_delta_while_it_streams_still_adds_on() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "Hi")], &[]);
    let _ = model.apply(vec![delta(0, "m1", "Hel")]);
    let _ = model.apply(vec![delta(0, "m1", "lo")]);
    let RowContent::Assistant(item) = &model.item("a:m1").expect("the reply").content else { panic!("an assistant row") };
    assert_eq!(item.text, "Hello");
    assert!(item.is_streaming);
}

// Claude's narration streams as reasoning and lands as assistant text with the
// same item: a reasoning delta held past it doesn't bring a thinking row back.
#[test]
fn a_held_reasoning_delta_after_narration_landed_adds_no_row() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "Hi"), text(3, "n1", "Looking now."), done(4)], &[]);
    let before = ids(&model);
    let _ = model.apply(vec![reasoning_delta(0, "n1", " more")]);
    assert_eq!(ids(&model), before);
}
