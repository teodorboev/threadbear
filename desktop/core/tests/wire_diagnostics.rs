// Its own integration-test process: no other test can consume the global
// once-per-field diagnostics before the privacy assertions exercise them.
use serde_json::json;
use threadbear_core::{inbox::{ThreadList, ThreadSummary}, logins::RunnerLogins};

#[test]
fn decode_diagnostics_report_real_paths_without_values_or_map_keys() {
    static MESSAGES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    threadbear_core::wire::set_logger(|message| MESSAGES.lock().unwrap().push(message.to_owned()));
    let expect_message = |kind: &str, decode: fn()| {
        MESSAGES.lock().unwrap().clear();
        decode();
        let messages = MESSAGES.lock().unwrap();
        assert!(messages.iter().any(|m| m.contains(kind)), "{kind} path must actually report: {messages:?}");
        assert!(messages.iter().all(|m| !m.contains("PRIVATE SENTINEL")));
    };
    expect_message("dropped invalid field (core::option::Option<i64>)", || {
        let _: ThreadSummary = serde_json::from_value(json!({"id":"t", "contextTokens":"PRIVATE SENTINEL"})).unwrap();
    });
    expect_message("thread list entry", || {
        let _ = ThreadList::decode(&json!({"threads":[{"id":{"PRIVATE SENTINEL":42}}]}));
    });
    expect_message("map entry", || {
        let _ = RunnerLogins::from_hello(&json!({"defaults":{"models":{"PRIVATE SENTINEL":false}}}));
    });
    expect_message("event payload", || {
        let event = threadbear_core::reducer::Event::decode(&json!({"seq":"17", "type":"plan.proposed", "text":"PRIVATE SENTINEL", "plan":42})).unwrap();
        assert_eq!(event.seq, 17);
        assert_eq!(event.payload, threadbear_core::reducer::EventPayload::Unknown);
    });
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(deserialize_with = "threadbear_core::wire::field")]
        _number: Option<i128>,
        #[serde(deserialize_with = "threadbear_core::wire::field")]
        _other: Option<i128>,
    }
    MESSAGES.lock().unwrap().clear();
    for _ in 0..20 {
        let _: Probe = serde_json::from_value(json!({"_number":"PRIVATE SENTINEL", "_other":"PRIVATE SENTINEL"})).unwrap();
    }
    let messages = MESSAGES.lock().unwrap();
    assert_eq!(messages.len(), 2, "each field reports separately, repeated decoding logs only once");
    assert!(messages.iter().all(|m| m.contains("i128") && m.contains("wire_diagnostics.rs:") && !m.contains("PRIVATE SENTINEL")));
}
