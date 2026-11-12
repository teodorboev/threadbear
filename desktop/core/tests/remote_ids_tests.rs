// Thread ids from outside this runner (Daybreak findings 87 and 102, 11 Nov
// 2026): another runner's ids come through the backend, which took any
// string a runner wrote, so a compromised extra Mac could name a thread
// `remote$(…)` or `deadbeef) ![x](/tmp/x.png`. Such a row never reaches the
// sidebar or the chips' index, and Copy debug info quotes the id anyway.
#![allow(non_snake_case)]

mod support;

use serde_json::json;
use support::*;
use threadbear_core::chips::ThreadIndex;
use threadbear_core::reducer::row_debug::{info, shell_word, EventsSource};
use threadbear_core::remote::decode_remote_threads;
use threadbear_core::thread_link::is_thread_id;

const SHELL_ID: &str = "remote$(printf TB_ID_EXECUTED >&2)";
const MARKDOWN_ID: &str = "deadbeef) ![local](/tmp/private.png";

fn remote_row(id: &str) -> serde_json::Value {
    json!({ "runnerId": "bruce", "threadId": id, "title": "t", "createdAt": "2026-11-11T00:00:00Z" })
}

#[test]
fn testOnlyThreadIdShapesAreThreadIds() {
    for good in ["0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e", "demo_upload", "a", &"a".repeat(64)] {
        assert!(is_thread_id(good), "{good}");
    }
    for bad in ["", SHELL_ID, MARKDOWN_ID, "a b", "a'b", "a\"b", "../x", "a.b", "a;b", "a\nb", "é", &"a".repeat(65)] {
        assert!(!is_thread_id(bad), "{bad:?}");
    }
}

#[test]
fn testAnotherRunnersRowWithAMalformedIdIsLeftOut() {
    let good = "0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e";
    let list = json!({ "runners": [], "threads": [remote_row(SHELL_ID), remote_row(MARKDOWN_ID), remote_row("decafbad) # heading"), remote_row(good)] });
    let ids: Vec<String> = decode_remote_threads(&list).threads.into_iter().map(|t| t.id).collect();
    assert_eq!(ids, [good]);
}

#[test]
fn testAMalformedIdIsNoChipEvenByItsPrefix() {
    let index = ThreadIndex::new([MARKDOWN_ID, "0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e"]);
    assert_eq!(index.resolve("deadbeef"), None, "its hex prefix names nothing");
    assert_eq!(index.resolve(MARKDOWN_ID), None);
    assert_eq!(index.resolve("0cf430ac"), Some("0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e"));
}

#[test]
fn testShellWordQuotesAnythingButSafeCharacters() {
    assert_eq!(shell_word("0cf430ac-f429"), "0cf430ac-f429");
    assert_eq!(shell_word(SHELL_ID), "'remote$(printf TB_ID_EXECUTED >&2)'");
    assert_eq!(shell_word("it's"), "'it'\\''s'");
    assert_eq!(shell_word(""), "''");
}

/// The line Copy debug info gives, run by a real shell against a `tb`
/// that prints its arguments: the id arrives as one argument, and nothing
/// in it runs (Daybreak's own reproduction of 87).
/// This executes a POSIX shell function through /bin/sh; the quoting tests
/// above also run on Windows, which has no /bin/sh.
#[cfg(unix)]
#[test]
fn testTheDebugLinesIdRunsAsOneArgumentNotShellSource() {
    let model = from_events(vec![user(1, "Fix it"), event(2, "turn.started", json!({"turnId": "t1"})), text(9, "m1", "Fixed."), done(10)]);
    let row = model.item("a:m1").expect("the reply").clone();
    for id in [SHELL_ID, "x'; printf TB_ID_EXECUTED >&2; '", "x`printf TB_ID_EXECUTED >&2`", "x\nprintf TB_ID_EXECUTED >&2"] {
        let text = info(id, &row, &[9], "1.0", &EventsSource::Tb("tb".into()));
        // The last line, to the end: an id with a newline in it keeps it
        // inside its quotes.
        let line = text.split_once("\nsee: ").expect("a see line").1.trim_end_matches('\n');
        let out = std::process::Command::new("/bin/sh").arg("-c").arg(format!("tb() {{ for a in \"$@\"; do printf '<%s>' \"$a\"; done; }}; {line}")).output().expect("sh");
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(!stderr.contains("TB_ID_EXECUTED"), "the id ran: {line}");
        assert_eq!(stdout, format!("<events><{id}><--from><9><--to><9>"), "{line}");
    }
}
