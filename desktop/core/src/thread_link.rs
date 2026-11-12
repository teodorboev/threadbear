// `threadbear://thread/<id>`: a link that brings a thread up in the app, a port
// of ThreadbearCore's ThreadLink.swift. BrowserBear's Open thread pill on an
// agent's Chrome tab opens one (`open threadbear://thread/<id>`), so the thread
// shows in the app rather than on the website. It comes from outside the app,
// so all it can ever do is name a thread to show: anything else under the
// scheme (the phone's `threadbear://pair`, `quickask`) is no thread link here.

pub const SCHEME: &str = "threadbear";

/// The link for a thread.
pub fn url(thread_id: &str) -> String {
    format!("{SCHEME}://thread/{thread_id}")
}

/// The thread a link points at, or None for any other link
/// (`ThreadLink.threadId(from:)`). The scheme and the host are matched
/// whatever their case; the id is the path's first part.
pub fn thread_id(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once(':')?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let rest = rest.strip_prefix("//")?;
    // Cut the query and fragment off first, as a URL parser does.
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    // A user before the host, or a port after it, are still that host.
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if !host.eq_ignore_ascii_case("thread") {
        return None;
    }
    let id = crate::open_policy::percent_decode(path.trim_start_matches('/').split('/').next().unwrap_or(""));
    // Thread ids are UUIDs and the like: nothing that could be a path.
    if !is_thread_id(&id) {
        return None;
    }
    Some(id)
}

/// Whether `id` is shaped like a thread id: 1 to 64 letters, digits, `-`
/// and `_` (a runner's are UUIDs; the mock's and fixtures' are words). The
/// same rule as the backend's `isThreadId` (convex/runnerAccess.ts) and the
/// runner's (runner/src/thread-id.ts). Another runner's ids come through
/// the backend, and a compromised extra Mac can write any string there, so
/// an id from outside this runner is checked with this before it's kept:
/// one with a quote, a space, a `)` or a `$(` would otherwise end up in a
/// shell line (Copy debug info) or markdown (a thread chip).
pub fn is_thread_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// The last thread among links the system handed over at once, as the Swift
/// app's `application(_:open:)` takes the last of them.
pub fn last_thread<'a>(urls: impl IntoIterator<Item = &'a str>) -> Option<String> {
    urls.into_iter().filter_map(thread_id).last()
}

/// Why a reply's `thread:` link opened nothing, matching `matches` of the
/// threads in the sidebar (none, or more than one), as the Swift app's
/// error bar says it (`TranscriptController.openThreadLink`).
pub fn unopened_link_message(matches: usize) -> String {
    if matches == 0 {
        "That thread isn't in your sidebar, so it can't open here. It was deleted, or it's archived on another machine, and those aren't listed on this Mac.".to_owned()
    } else {
        format!("That link matches {matches} threads, so none was opened.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_that_opens_nothing_says_why() {
        assert!(unopened_link_message(0).starts_with("That thread isn't in your sidebar"));
        assert_eq!(unopened_link_message(3), "That link matches 3 threads, so none was opened.");
    }

    #[test]
    fn a_thread_link_names_its_thread() {
        let id = "0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e";
        assert_eq!(thread_id(&url(id)).as_deref(), Some(id));
        assert_eq!(thread_id("THREADBEAR://Thread/abc_1").as_deref(), Some("abc_1"), "scheme and host in any case");
        assert_eq!(thread_id("threadbear://thread/abc/more").as_deref(), Some("abc"), "the path's first part");
        assert_eq!(thread_id("threadbear://thread/abc?from=pill#top").as_deref(), Some("abc"));
        assert_eq!(thread_id("threadbear://thread/abc/").as_deref(), Some("abc"));
    }

    #[test]
    fn other_links_name_no_thread() {
        for link in [
            "threadbear://pair?url=https%3A%2F%2Fx.convex.cloud&token=tb_1",
            "threadbear://quickask?url=x",
            "threadbear://thread",
            "threadbear://thread/",
            "threadbear:thread/abc",
            "https://thread/abc",
            "thread/abc",
            "",
        ] {
            assert_eq!(thread_id(link), None, "{link}");
        }
    }

    #[test]
    fn nothing_that_could_be_a_path_or_more_gets_through() {
        for link in ["threadbear://thread/..", "threadbear://thread/a%2Fb", "threadbear://thread/a%20b", "threadbear://thread/a.b", "threadbear://thread/%2e%2e", "threadbear://thread/a;rm"] {
            assert_eq!(thread_id(link), None, "{link}");
        }
    }

    #[test]
    fn the_last_thread_link_of_several_wins() {
        let links = ["threadbear://thread/one", "https://example.com", "threadbear://thread/two", "threadbear://pair?x"];
        assert_eq!(last_thread(links).as_deref(), Some("two"));
        assert_eq!(last_thread(["https://example.com"]), None);
    }
}
