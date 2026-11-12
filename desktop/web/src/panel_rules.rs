// What a web panel's page may do, with nothing of Chromium's in it, so it's
// the same on every system and tested anywhere: which sites count as the
// panel's own (WebPanelPool.sameSite and leavesPanel in the Swift app), and
// what a page tells the app. mac/panel.rs acts on them.

/// What a panel's page tells the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A link, form or new window to another site, for the app to open.
    Leave(String),
    /// Loading started or stopped, going back became possible or not, or the
    /// title or address changed.
    Changed,
}

/// Whose page it is, which decides where its links may go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A patch's url panel: its own site stays, other sites leave.
    Patch,
    /// The Convex embedded dashboard: only its own site stays, and the page
    /// answers its credentials request (docs/protocol.md, "The Convex panel")
    /// with this script, run in it before its own.
    Convex { credentials_script: String },
}

/// `https://a.b:8443/x` → `https://a.b:8443`: what counts as the same site
/// (scheme, host and port, with the default ports). None for anything that
/// isn't an http or https address.
pub fn site(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority).to_ascii_lowercase();
    let default = if scheme == "https" { "443" } else { "80" };
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        // An IPv6 address, its colons inside the brackets.
        let (inside, after) = rest.split_once(']')?;
        (format!("[{inside}]"), after.strip_prefix(':').unwrap_or(""))
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host.to_owned(), port),
            None => (authority.clone(), ""),
        }
    };
    let port = if port.is_empty() { default } else { port };
    (!host.is_empty()).then(|| format!("{scheme}://{host}:{port}"))
}

/// Whether a navigation to `url` from a page whose site is `home` leaves the
/// panel: a web page on another site (for the Convex dashboard, any but its
/// own). Not a web page (about:blank, data:): it stays, and Chromium's own
/// rules keep it in its frame.
pub fn leaves(url: &str, home: &str) -> bool {
    site(url).is_some_and(|target| target != home)
}

/// What a navigation of a panel's main frame does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browse {
    /// It loads in the panel.
    Stay,
    /// It opens in the browser, and the panel keeps its page.
    Leave,
    /// It's cancelled, and nothing opens anywhere.
    Drop,
}

/// A navigation of the panel's main frame to `url`, from a page on `home`
/// (its site), where `own` is the address the app loaded in the panel.
/// `gesture` is a person's click or key behind it, `redirect` a server's
/// redirect.
///
/// - Only a web address loads as the panel's page, or its own (a `data:`
///   page a patch named) or a blank one: a file dropped on the page, or
///   anything else Chromium would load by itself, is cancelled (Codex
///   review, 11 Nov). This is the panel's own frame; an iframe's are
///   Chromium's to rule on, and it keeps files out of them.
/// - On its own site it stays.
/// - Another site, clicked: the browser, as WebPanelPool.leavesPanel has it.
/// - Another site, not clicked: in a patch's panel it stays, so a sign-in
///   that goes through another site and back still works, as in the Swift
///   app. In the Convex dashboard it's cancelled: the dashboard's own
///   scripts and redirects never reach the browser, or a compromised script
///   could open a browser tab a second (Codex review, 11 Nov).
pub fn browse(url: &str, home: &str, own: &str, convex: bool, gesture: bool, redirect: bool) -> Browse {
    if site(url).is_none() {
        return if url == own || url == "about:blank" { Browse::Stay } else { Browse::Drop };
    }
    if !leaves(url, home) {
        return Browse::Stay;
    }
    match (gesture && !redirect, convex) {
        (true, _) => Browse::Leave,
        (false, true) => Browse::Drop,
        (false, false) => Browse::Stay,
    }
}

/// What a page's request for a new window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    /// It opens in the browser.
    Leave,
    /// It loads in the panel itself: there's no other window to give it.
    LoadHere,
    /// Nothing happens.
    Drop,
}

/// A new window for `url`, asked for by a page on `home`. Only a person's
/// click opens one: a page's own `window.open` (from a timer, say) is
/// dropped. Only a web address goes anywhere: a file or `data:` page asked
/// for in a new window doesn't load in the panel (Codex review, 11 Nov).
pub fn popup(url: &str, home: &str, convex: bool, gesture: bool) -> Popup {
    if !gesture || site(url).is_none() {
        Popup::Drop
    } else if convex || leaves(url, home) {
        Popup::Leave
    } else {
        Popup::LoadHere
    }
}

/// How often a panel may send a page to the browser: once a second at most,
/// however fast its clicks come (each click is a gesture, so a page could
/// turn every one into a new window).
pub const LEAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// Lets a panel send a page to the browser at most once every
/// `LEAVE_EVERY`, and one for each click or key: after one goes out, the
/// next needs a new click or key in the page (Codex review, 12 Nov).
/// Chromium leaves a click with the page after a new window goes out, as it
/// uses a click up for a window only when it makes one and a panel makes
/// none, so one click could otherwise send a window, another and a link a
/// second apart. Waiting for the click's activation to run out proves
/// nothing either: a page can hold a link back, still carrying the click,
/// for as long as it likes. A press from VoiceOver isn't counted (AppKit
/// never sees it), so after one page goes out, the next needs a click or a
/// key in the page.
#[derive(Debug, Default)]
pub struct LeaveGate {
    last: Option<std::time::Instant>,
    /// The person's clicks and keys in the page when the last went out.
    inputs_then: u64,
}

impl LeaveGate {
    /// Whether one may go now, `inputs` being the person's clicks and keys
    /// in the page so far; if so, the next has to wait.
    pub fn allow(&mut self, now: std::time::Instant, inputs: u64) -> bool {
        if self.last.is_some_and(|last| now.saturating_duration_since(last) < LEAVE_EVERY || inputs <= self.inputs_then) {
            return false;
        }
        self.last = Some(now);
        self.inputs_then = inputs;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_counts_as_the_same_site() {
        assert_eq!(site("https://Dash.Example.com/a?b").as_deref(), Some("https://dash.example.com:443"));
        assert_eq!(site("http://localhost:3000/x").as_deref(), Some("http://localhost:3000"));
        assert_eq!(site("http://localhost/x").as_deref(), Some("http://localhost:80"));
        assert_eq!(site("https://user@example.com/").as_deref(), Some("https://example.com:443"));
        assert_eq!(site("http://[::1]:8080/").as_deref(), Some("http://[::1]:8080"));
        assert_eq!(site("about:blank"), None);
        assert_eq!(site("file:///tmp/a.html"), None);
        assert_eq!(site("data:text/html,hi"), None);
        let home = site("http://localhost:3000/").unwrap();
        assert!(!leaves("http://localhost:3000/settings", &home), "same site stays");
        assert!(leaves("https://youtube.com/watch", &home), "another site leaves");
        assert!(leaves("http://localhost:3001/", &home), "another port is another site");
        assert!(leaves("https://localhost:3000/", &home), "another scheme is another site");
        assert!(!leaves("about:blank", &home));
        // A data: page has no site of its own, so every web link leaves it.
        assert!(leaves("https://example.com/", ""));
    }

    #[test]
    fn only_a_click_sends_a_page_to_the_browser() {
        let own = "http://localhost:3000/";
        let home = site(own).unwrap();
        let other = "https://attacker.example/1";
        // (url, convex, gesture, redirect) -> what happens
        let cases = [
            ("http://localhost:3000/settings", false, false, false, Browse::Stay),
            ("http://localhost:3000/settings", true, true, false, Browse::Stay),
            (other, false, true, false, Browse::Leave),
            (other, true, true, false, Browse::Leave),
            // A patch's page going through another site by itself (a sign-in).
            (other, false, false, false, Browse::Stay),
            (other, false, true, true, Browse::Stay),
            // The dashboard's own script or a redirect: nothing opens.
            (other, true, false, false, Browse::Drop),
            (other, true, true, true, Browse::Drop),
        ];
        for (url, convex, gesture, redirect, want) in cases {
            assert_eq!(browse(url, &home, own, convex, gesture, redirect), want, "{url} convex {convex} gesture {gesture} redirect {redirect}");
        }
    }

    #[test]
    fn only_a_web_page_loads_in_a_panel() {
        let home = site("http://localhost:3000/").unwrap();
        for url in ["file:///etc/hosts", "data:text/html,<title>x</title>", "blob:http://localhost:3000/abc", "chrome://version", "mailto:a@b.c"] {
            assert_eq!(browse(url, &home, "http://localhost:3000/", false, true, false), Browse::Drop, "{url}");
            assert_eq!(popup(url, &home, false, true), Popup::Drop, "{url} in a new window");
        }
        // The panel's own data: page (the mock runner's stand-ins), and the
        // blank start of the Convex view, do load.
        let own = "data:text/html,<title>stand-in</title>";
        assert_eq!(browse(own, "", own, false, false, false), Browse::Stay);
        assert_eq!(browse("about:blank", &home, "http://localhost:3000/", true, false, false), Browse::Stay);
    }

    #[test]
    fn a_new_window_needs_a_click() {
        let home = site("http://localhost:3000/").unwrap();
        assert_eq!(popup("https://attacker.example/", &home, false, false), Popup::Drop, "a script's window.open");
        assert_eq!(popup("http://localhost:3000/two", &home, false, false), Popup::Drop, "even on its own site");
        assert_eq!(popup("https://example.com/", &home, false, true), Popup::Leave);
        assert_eq!(popup("http://localhost:3000/two", &home, false, true), Popup::LoadHere);
        assert_eq!(popup("http://localhost:3000/two", &home, true, true), Popup::Leave, "the dashboard's all go out");
    }

    #[test]
    fn a_panel_sends_at_most_one_page_a_second() {
        let ms = std::time::Duration::from_millis;
        let start = std::time::Instant::now();
        let mut gate = LeaveGate::default();
        // A click each time, so only the second's wait holds them back.
        assert!(gate.allow(start, 1));
        assert!(!gate.allow(start + ms(300), 2));
        assert!(!gate.allow(start + ms(999), 3));
        assert!(gate.allow(start + LEAVE_EVERY, 4));
        assert!(!gate.allow(start + LEAVE_EVERY + ms(10), 5));
    }

    #[test]
    fn one_click_sends_one_page() {
        let ms = std::time::Duration::from_millis;
        let start = std::time::Instant::now();
        // With no new click or key, nothing more goes out on the same one,
        // however long the page waits.
        let mut gate = LeaveGate::default();
        assert!(gate.allow(start, 1));
        assert!(!gate.allow(start + ms(1200), 1));
        assert!(!gate.allow(start + ms(2400), 1));
        assert!(!gate.allow(start + ms(60_000), 1));
        // A new click or key a second later sends another.
        let mut gate = LeaveGate::default();
        assert!(gate.allow(start, 1));
        assert!(gate.allow(start + ms(1200), 2));
        assert!(!gate.allow(start + ms(2400), 2));
    }
}
