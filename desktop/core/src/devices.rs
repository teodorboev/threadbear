// Settings › Phone & browser's words and rules, a port of ThreadbearCore's
// Devices.swift (Device, DeviceList, DeviceText), DevicesModel.swift's
// DefaultBrowser.shortName, RunnerStatus's `localOnly` and `pushOff`, and
// DevicesSettings.swift's PairingQR: the phones and browsers paired with the
// mirror as the runner lists them (docs/protocol.md, "Devices"), what each row
// says, and the QR code the iPhone app scans. A token is only ever in the QR
// code on screen or the link the browser opens: nothing here keeps one.
use crate::inbox::parse_ms;
use chrono::{DateTime, Datelike, TimeZone};
use serde_json::Value;

/// What a token was issued for (`Device.Kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Phone,
    Browser,
    /// A kind this app doesn't know, from a newer runner.
    Other,
}

impl Kind {
    fn decode(v: &Value) -> Kind {
        match v.as_str() {
            Some("phone") => Kind::Phone,
            Some("browser") => Kind::Browser,
            _ => Kind::Other,
        }
    }

    /// Its name in the protocol, as the Swift dump writes it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Phone => "phone",
            Kind::Browser => "browser",
            Kind::Other => "other",
        }
    }

    /// The row's icon (`DeviceRow.symbol`).
    pub fn symbol(self) -> &'static str {
        match self {
            Kind::Phone => "iphone",
            Kind::Browser => "globe",
            Kind::Other => "questionmark.circle",
        }
    }
}

/// One paired phone or browser (`Device`), read forgivingly: a field an older
/// runner leaves out takes its default.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The token's id, for `devices.revoke`.
    pub id: String,
    pub kind: Kind,
    /// "iPhone", "Chrome": what was paired, as the app named it.
    pub name: String,
    pub created_ms: Option<i64>,
    pub expires_ms: Option<i64>,
    pub expired: bool,
}

impl Device {
    pub fn decode(v: &Value) -> Option<Device> {
        let id = v["id"].as_str().filter(|s| !s.is_empty())?.to_owned();
        Some(Device {
            id,
            kind: Kind::decode(&v["kind"]),
            name: v["name"].as_str().unwrap_or("").to_owned(),
            created_ms: v["createdAt"].as_str().and_then(parse_ms),
            expires_ms: v["expiresAt"].as_str().and_then(parse_ms),
            expired: v["expired"].as_bool().unwrap_or(false),
        })
    }

    /// The row's name: what it was paired as, else what it is.
    pub fn title(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        if self.kind == Kind::Browser { "Browser" } else { "Phone" }.to_owned()
    }

    /// Run out, as the backend says or as its date does (`DeviceText.detail`).
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expired || self.expires_ms.is_some_and(|at| at < now_ms)
    }

    /// The line under its name: when it was paired and how long its token
    /// lasts, or that it's run out. "Paired 21 Oct, until 19 Jan 2027" or
    /// "Expired 29 Sep: pair it again" (`DeviceText.detail`).
    pub fn detail<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> String {
        if self.is_expired(now.timestamp_millis()) {
            return match self.expires_ms {
                Some(at) => format!("Expired {}: pair it again", day(at, now)),
                None => "Expired: pair it again".to_owned(),
            };
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(at) = self.created_ms {
            parts.push(format!("Paired {}", day(at, now)));
        }
        if let Some(at) = self.expires_ms {
            parts.push(format!("{} {}", if parts.is_empty() { "Until" } else { "until" }, day(at, now)));
        }
        parts.join(", ")
    }

    /// Revoke's tooltip.
    pub fn revoke_tip(&self) -> &'static str {
        if self.kind == Kind::Browser {
            "Sign this browser out. It shows the sign-in page at its next load."
        } else {
            "Unpair this phone. It shows the pairing screen at its next request."
        }
    }

    /// The device in the dump, as the Swift `settings` command writes it:
    /// `browser "Chrome": Paired 22 Oct, until 20 Jan 2027`.
    pub fn line<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> String {
        format!("{} \"{}\": {}", self.kind.name(), self.name, self.detail(now))
    }
}

/// "28 Oct", with the year once it isn't this one (`DeviceText.day`), in the
/// time zone of `now`. en_GB's short months, as the Swift formatter's are.
fn day<Tz: TimeZone>(at_ms: i64, now: &DateTime<Tz>) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let Some(date) = now.timezone().timestamp_millis_opt(at_ms).single() else { return String::new() };
    let short = format!("{} {}", date.day(), MONTHS[date.month0() as usize]);
    if date.year() == now.year() {
        short
    } else {
        format!("{short} {}", date.year())
    }
}

/// `devices.list` and `devices.revoke`: the devices, the website's root while
/// sync is on, or why nothing can pair while it's off (`DeviceList`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceList {
    pub devices: Vec<Device>,
    pub site: Option<String>,
    pub reason: Option<String>,
}

impl DeviceList {
    pub fn decode(v: &Value) -> DeviceList {
        let text = |key: &str| v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        DeviceList { devices: v["devices"].as_array().into_iter().flatten().filter_map(Device::decode).collect(), site: text("site"), reason: text("reason") }
    }
}

// ---------------------------------------------------------------- runner.status

/// The backend is on this Mac only (`setup --self-hosted --local`): its
/// `sync.url` is on loopback, so no phone can pair with it and the website
/// only opens here (`RunnerStatus.localOnly`).
pub fn local_only(status: &Value) -> bool {
    status["sync"]["url"].as_str().is_some_and(is_loopback)
}

/// The backend can't send phone notifications: a self-hosted one, which the
/// push relay doesn't reach (`sync.pushOff`).
pub fn push_off(status: &Value) -> bool {
    status["sync"]["pushOff"].as_bool().unwrap_or(false)
}

/// The same hosts the runner counts as this Mac (runner/src/secure-url.ts).
fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.split_once("://").map(|(_, rest)| rest) else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    // "[::1]:3210" keeps its brackets; "127.0.0.1:3210" loses its port.
    let host = if authority.starts_with('[') { authority.split(']').next().map(|h| format!("{h}]")).unwrap_or_default() } else { authority.split(':').next().unwrap_or("").to_owned() };
    let host = host.to_lowercase();
    if host == "localhost" || host == "::1" || host == "[::1]" {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts[0] == "127" && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

// ---------------------------------------------------------------- the page's words

/// Why nothing can be shown yet, in place of the sections (`DevicesSettings`'
/// first branches), or None when the sections can show.
pub fn blocked(unsupported: bool, connected: bool, list: &DeviceList) -> Option<Vec<String>> {
    if unsupported {
        return Some(vec!["This runner is too old to pair phones. It gets it when it next updates.".to_owned()]);
    }
    if !connected {
        return Some(vec!["Not connected to the runner.".to_owned()]);
    }
    list.reason.as_ref().map(|reason| {
        vec![
            format!("Your threads aren't mirrored to Convex yet ({reason}), so there's nothing for a phone or browser to read."),
            "Ask your agent to set it up, or run ./threadbear setup in the repo, then come back here.".to_owned(),
        ]
    })
}

/// The first section's heading.
pub fn add_title(local_only: bool) -> &'static str {
    if local_only { "Add a browser" } else { "Add a phone or browser" }
}

/// What it is, behind its ⓘ.
pub fn add_about(local_only: bool, site: Option<&str>) -> String {
    if local_only {
        format!(
            "Your backend is on this Mac only, so the website opens in a browser on this Mac, at {}. Each browser keeps a key of its own.\n\nThe iPhone app needs a backend it can reach: Convex cloud, or a self-hosted one on your tailnet or at an address of your own (docs/self-hosting.md).",
            site.unwrap_or("its local address")
        )
    } else {
        format!(
            "The Threadbear iPhone app shows your inbox, lets you reply, approve and start threads, and gets a push when a thread needs you. It reads the Convex mirror, so it works while the Mac is asleep.\n\nThe website is your threads on the web, at {}: a link you can paste anywhere. Each browser keeps a key of its own; sign another one in from here too.",
            site.unwrap_or("your deployment's .convex.site address")
        )
    }
}

pub const ADD_ASK: &str = "put the iPhone app on your phone, or change how the mirror is set up.";
pub const LOCAL_ONLY_IPHONE: &str = "Can't pair: your backend is on this Mac only";
pub const LOCAL_ONLY_NOTE: &str = "A phone can't reach it. To use the iPhone app, run ./threadbear setup --self-hosted --tailnet (with Tailscale), or with --url and --site-url for an address of your own. Your threads stay.";
pub const PUSH_OFF_NOTE: &str = "Your backend is self-hosted, so the iPhone app gets no notifications. It shows what needs you when you open it.";
pub const PHONE_BUTTON_TIP: &str = "A code to scan with the Threadbear iPhone app. It pairs the phone with your threads for 90 days.";
pub const PAIRED_ABOUT: &str = "Each phone and browser holds a key of its own, good for 90 days. Revoke signs that one out at its next request; pair it again for a new key.";
pub const PAIRED_ASK: &str = "explain what a paired phone or browser can see and do.";

/// Open signed in's tooltip, naming the browser the link goes to.
pub fn website_tip(browser: Option<&str>) -> String {
    format!("Opens your Threadbear website in {}, signed in for 90 days", browser.unwrap_or("your browser"))
}

/// What the Paired list says with nothing in it.
pub fn empty_line(loaded: bool) -> &'static str {
    if loaded { "Nothing paired yet" } else { "Loading…" }
}

/// "Google Chrome" is Chrome to everyone; the rest keep their names
/// (`DefaultBrowser.shortName`). What the browser's token is called.
pub fn browser_name(raw: &str) -> String {
    let trimmed = raw.trim();
    let name = trimmed.strip_prefix("Google ").unwrap_or(trimmed);
    if name.is_empty() { "Browser".to_owned() } else { name.to_owned() }
}

// ---------------------------------------------------------------- the pairing code

/// A QR code's squares, row by row, with the one-square margin CoreImage's
/// generator puts round it (`PairingQR`, as ios/scripts/pair-qr.swift draws
/// it): at the Swift sheet's sizes the two come out the same.
#[derive(Clone, Debug, PartialEq)]
pub struct Qr {
    /// Squares a side, margin included.
    pub size: usize,
    dark: Vec<bool>,
}

impl Qr {
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.dark[y * self.size + x]
    }

    /// Each row's dark squares as runs (start, length), so a row draws as a
    /// few rectangles rather than a square each.
    pub fn runs(&self, y: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut x = 0;
        while x < self.size {
            if self.is_dark(x, y) {
                let start = x;
                while x < self.size && self.is_dark(x, y) {
                    x += 1;
                }
                out.push((start, x - start));
            } else {
                x += 1;
            }
        }
        out
    }

    /// How many pixels a square takes so the code fits `side` (a whole
    /// number, at least 1, so the squares stay crisp): CoreImage's output
    /// scaled with no smoothing, as the Swift sheet does it.
    pub fn scale(&self, side: f32) -> f32 {
        (side / self.size as f32).floor().max(1.)
    }
}

/// The pairing link as a QR code at correction level M, the smallest version
/// that holds it in byte mode, as CIQRCodeGenerator makes it. None if it's too
/// long for any QR code.
pub fn pairing_qr(text: &str) -> Option<Qr> {
    use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
    let segments = [QrSegment::make_bytes(text.as_bytes())];
    // No boosting the correction level: CoreImage keeps the one it's given.
    let code = QrCode::encode_segments_advanced(&segments, QrCodeEcc::Medium, Version::MIN, Version::MAX, None, false).ok()?;
    let inner = code.size() as usize;
    let size = inner + 2;
    let mut dark = vec![false; size * size];
    for y in 0..inner {
        for x in 0..inner {
            dark[(y + 1) * size + x + 1] = code.get_module(x as i32, y as i32);
        }
    }
    Some(Qr { size, dark })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
    use serde_json::json;

    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2026, 11, 11, 12, 0, 0).unwrap()
    }

    fn mock_list() -> DeviceList {
        DeviceList::decode(&json!({
            "devices": [
                { "id": "tok1", "kind": "phone", "name": "iPhone", "createdAt": "2026-10-21T09:12:00.000Z", "expiresAt": "2027-01-19T09:12:00.000Z", "expired": false },
                { "id": "tok2", "kind": "browser", "name": "Chrome", "createdAt": "2026-10-22T14:30:00.000Z", "expiresAt": "2027-01-20T14:30:00.000Z" },
                { "id": "tok3", "kind": "browser", "name": "Safari", "createdAt": "2026-07-01T08:00:00.000Z", "expiresAt": "2026-09-29T08:00:00.000Z", "expired": true },
                { "kind": "phone", "name": "No id, so it can't be revoked" },
                { "id": "tok4", "kind": "watch" },
            ],
            "site": "https://demo-bear-123.convex.site",
            "reason": "",
        }))
    }

    #[test]
    fn reads_the_list_as_the_swift_app_does() {
        let list = mock_list();
        assert_eq!(list.devices.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["tok1", "tok2", "tok3", "tok4"], "a row with no id is left out");
        assert_eq!(list.site.as_deref(), Some("https://demo-bear-123.convex.site"));
        assert_eq!(list.reason, None, "an empty reason is none");
        let other = &list.devices[3];
        assert_eq!(other.kind, Kind::Other);
        assert_eq!(other.title(), "Phone", "a nameless device is called what it is");
        assert_eq!(other.kind.symbol(), "questionmark.circle");
        assert_eq!(DeviceList::decode(&json!({})), DeviceList::default());
    }

    #[test]
    fn each_row_says_when_it_was_paired_and_until_when() {
        let list = mock_list();
        let lines: Vec<String> = list.devices.iter().take(3).map(|d| d.line(&now())).collect();
        // The same lines the Swift app's `settings` command gave for the mock.
        assert_eq!(lines, [r#"phone "iPhone": Paired 21 Oct, until 19 Jan 2027"#, r#"browser "Chrome": Paired 22 Oct, until 20 Jan 2027"#, r#"browser "Safari": Expired 29 Sep: pair it again"#]);
        // Out of date though the backend hasn't said so yet.
        let late = Device { expired: false, ..list.devices[1].clone() };
        let feb = FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2027, 2, 1, 0, 0, 0).unwrap();
        assert_eq!(late.detail(&feb), "Expired 20 Jan: pair it again");
        let bare = Device { created_ms: None, ..list.devices[0].clone() };
        assert_eq!(bare.detail(&now()), "Until 19 Jan 2027");
        let undated = Device { created_ms: None, expires_ms: None, expired: true, ..list.devices[0].clone() };
        assert_eq!(undated.detail(&now()), "Expired: pair it again");
        assert_eq!(list.devices[1].revoke_tip(), "Sign this browser out. It shows the sign-in page at its next load.");
    }

    #[test]
    fn says_why_nothing_can_pair() {
        let list = DeviceList::default();
        assert_eq!(blocked(true, true, &list).unwrap()[0], "This runner is too old to pair phones. It gets it when it next updates.");
        assert_eq!(blocked(false, false, &list).unwrap()[0], "Not connected to the runner.");
        assert_eq!(blocked(false, true, &list), None);
        let off = DeviceList { reason: Some("sync is off".into()), ..Default::default() };
        assert_eq!(blocked(false, true, &off).unwrap()[0], "Your threads aren't mirrored to Convex yet (sync is off), so there's nothing for a phone or browser to read.");
    }

    #[test]
    fn a_backend_on_this_mac_only_is_a_loopback_one() {
        assert!(local_only(&json!({ "sync": { "url": "http://127.0.0.1:13210" } })));
        assert!(local_only(&json!({ "sync": { "url": "http://localhost:3210/" } })));
        assert!(local_only(&json!({ "sync": { "url": "http://[::1]:3210" } })));
        assert!(!local_only(&json!({ "sync": { "url": "https://demo-bear-123.convex.cloud" } })));
        assert!(!local_only(&json!({ "sync": { "url": "http://127.0.0.1.example.com" } })));
        assert!(!local_only(&json!({})));
        assert!(push_off(&json!({ "sync": { "pushOff": true } })));
        assert!(!push_off(&json!({ "sync": {} })));
        assert_eq!(add_title(true), "Add a browser");
        assert!(add_about(false, None).contains("your deployment's .convex.site address"));
    }

    #[test]
    fn a_browser_is_called_what_people_call_it() {
        assert_eq!(browser_name("Google Chrome"), "Chrome");
        assert_eq!(browser_name("Safari"), "Safari");
        assert_eq!(browser_name("  "), "Browser");
        assert_eq!(website_tip(Some("Chrome")), "Opens your Threadbear website in Chrome, signed in for 90 days");
        assert_eq!(website_tip(None), "Opens your Threadbear website in your browser, signed in for 90 days");
    }

    #[test]
    fn the_pairing_code_is_corecimages_size() {
        // The mock's 80-byte link is version 5 at level M (37 squares), with a
        // square of margin round it: 39 a side, 5 pixels each in the sheet's 220.
        let link = "threadbear://pair?url=https%3A%2F%2Fdemo-bear-123.convex.cloud&token=tb_demo0004";
        let qr = pairing_qr(link).unwrap();
        assert_eq!(qr.size, 39);
        assert_eq!(qr.scale(220.), 5.);
        // The margin is light, and the finder pattern's corner is dark.
        assert!((0..qr.size).all(|i| !qr.is_dark(i, 0) && !qr.is_dark(0, i)));
        assert!(qr.is_dark(1, 1) && qr.is_dark(7, 1) && !qr.is_dark(8, 1));
        assert_eq!(qr.runs(1).first(), Some(&(1, 7)), "the finder's top edge is one run");
        // A real link, with the runner's key, takes a bigger code but still fits.
        let real = format!("{link}&key=runner_3f6c2a8e-9b1d-4c55-a2b0-7e1f0d9c4b2a:{}", "A".repeat(44));
        let big = pairing_qr(&real).unwrap();
        assert!(big.size > qr.size && big.scale(220.) >= 3.);
        assert!(pairing_qr(&"x".repeat(5000)).is_none(), "too long for any code");
    }
}
