// Mike's Connectors gateway, as the runner last heard from its /health
// (docs/protocol.md, "Connectors"): `connectors.status` and
// `connectors.updated`, and what the Connectors panel and its activity bar
// icon say about it, a port of Connectors.swift and ConnectorsText.swift.
// The accent dot only when a service is signed out, as only Mike can sign it
// back in; a grey one while the gateway can't be reached or a service is
// connecting or in error, which usually sorts itself out (as Updates' dots
// go, 27 Oct). The app never calls the gateway itself: it only opens its
// pages in the browser.
use crate::inbox::parse_ms;
use serde_json::Value;

/// `ConnectorsStatus`, read leniently, as the gateway's answer is still
/// changing: an unknown state is shown as it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectorsStatus {
    /// ok, unreachable, off or checking.
    pub state: String,
    /// The gateway's base, https://host:port.
    pub url: Option<String>,
    pub checked_ms: Option<i64>,
    pub error: Option<String>,
    pub version: Option<String>,
    pub services: Vec<ConnectorService>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectorService {
    pub id: String,
    pub title: String,
    /// connected, signed_out, connecting, error, disabled or unknown (or a
    /// newer word).
    pub state: String,
    pub message: Option<String>,
    /// Where Mike signs it in; set on anything not connected or disabled.
    pub connect_url: Option<String>,
}

impl ConnectorsStatus {
    /// `{ connectors }`' value.
    pub fn decode(v: &Value) -> ConnectorsStatus {
        let text = |key: &str| v[key].as_str().map(str::to_owned);
        ConnectorsStatus {
            state: v["state"].as_str().unwrap_or("checking").to_owned(),
            url: text("url"),
            checked_ms: v["checkedAt"].as_str().and_then(parse_ms),
            error: text("error"),
            version: text("version"),
            services: v["services"].as_array().into_iter().flatten().filter_map(ConnectorService::decode).collect(),
        }
    }
}

impl ConnectorService {
    fn decode(v: &Value) -> Option<ConnectorService> {
        let id = v["id"].as_str()?.to_owned();
        Some(ConnectorService {
            title: v["title"].as_str().map_or_else(|| id.clone(), str::to_owned),
            state: v["state"].as_str().unwrap_or("unknown").to_owned(),
            message: v["message"].as_str().map(str::to_owned),
            connect_url: v["connectUrl"].as_str().map(str::to_owned),
            id,
        })
    }
}

/// The services that need Mike to sign in again.
pub fn signed_out(s: Option<&ConnectorsStatus>) -> Vec<&ConnectorService> {
    s.filter(|s| s.state == "ok").map_or_else(Vec::new, |s| s.services.iter().filter(|x| x.state == "signed_out").collect())
}

/// The accent dot.
pub fn needs_you(s: Option<&ConnectorsStatus>) -> bool {
    !signed_out(s).is_empty()
}

/// The grey dot: something's off that isn't Mike's to fix yet.
pub fn quiet(s: Option<&ConnectorsStatus>) -> bool {
    let Some(s) = s else { return false };
    match s.state.as_str() {
        "unreachable" => true,
        "ok" => s.services.iter().any(|x| x.state == "error" || x.state == "connecting"),
        _ => false,
    }
}

/// The icon's tooltip: "Connectors", then what needs a look, or that
/// everything's signed in.
pub fn tooltip(s: Option<&ConnectorsStatus>) -> String {
    let mut lines = vec!["Connectors".to_owned()];
    match s.map(|s| s.state.as_str()) {
        Some("ok") => {
            let all = &s.expect("ok has a status").services;
            let out: Vec<&str> = signed_out(s).iter().map(|x| x.title.as_str()).collect();
            if !out.is_empty() {
                lines.push(format!("{}{}", list(&out), if out.len() == 1 { " needs signing in" } else { " need signing in" }));
            }
            let broken: Vec<&str> = all.iter().filter(|x| x.state == "error").map(|x| x.title.as_str()).collect();
            if !broken.is_empty() {
                lines.push(format!("{}{}", list(&broken), if broken.len() == 1 { " has a problem" } else { " have problems" }));
            }
            if out.is_empty() && broken.is_empty() {
                lines.push(if all.is_empty() { "No services yet" } else { "Everything is signed in" }.to_owned());
            }
        }
        Some("unreachable") => lines.push("Can't reach the gateway".to_owned()),
        Some("checking") => lines.push("Checking".to_owned()),
        _ => {}
    }
    lines.join("\n")
}

/// The panel's line when there are no services to list.
pub fn empty_line(s: Option<&ConnectorsStatus>) -> Option<String> {
    let Some(s) = s else { return Some("Checking the gateway.".to_owned()) };
    match s.state.as_str() {
        "off" => Some("No Connectors gateway is set up for this runner.".to_owned()),
        "checking" => Some("Checking the gateway.".to_owned()),
        "unreachable" => {
            let host = host(Some(s)).unwrap_or_else(|| "the gateway".to_owned());
            Some(format!("Can't reach {host}{}", s.error.as_ref().map_or_else(|| ".".to_owned(), |e| format!(": {e}."))))
        }
        _ => s.services.is_empty().then(|| "The gateway has no services set up.".to_owned()),
    }
}

/// The gateway's machine, as its owner calls it: "bruce" for
/// bruce.example.ts.net.
pub fn host(s: Option<&ConnectorsStatus>) -> Option<String> {
    let (_, authority) = web_parts(s?.url.as_deref()?)?;
    let host = host_of(authority);
    if host.is_empty() {
        return None;
    }
    Some(host.split('.').next().unwrap_or(host).to_owned())
}

/// The gateway's admin page, where every service can be connected, signed
/// out or retried, for the panel's manage button.
pub fn manage_url(s: Option<&ConnectorsStatus>) -> Option<String> {
    admin_url(s, "/admin")
}

/// One service's page on the gateway (its Connect, Disconnect or Try again),
/// whatever its state. The runner only sends a connect URL for a service that
/// isn't signed in, so a signed-in one gets the gateway's page for it, as the
/// runner's own fallback does.
pub fn service_url(s: Option<&ConnectorsStatus>, service: &ConnectorService) -> Option<String> {
    if let Some(raw) = service.connect_url.as_deref().filter(|raw| web_parts(raw).is_some()) {
        return Some(raw.to_owned());
    }
    admin_url(s, &format!("/admin/connect/{}", path_segment(&service.id)))
}

fn admin_url(s: Option<&ConnectorsStatus>, path: &str) -> Option<String> {
    let s = s.filter(|s| s.state != "off")?;
    let (scheme, authority) = web_parts(s.url.as_deref()?)?;
    Some(format!("{scheme}://{authority}{path}"))
}

/// A web address's scheme and the part before its path
/// ("bruce.example.ts.net:8443"), or None for anything but http(s), or
/// something that doesn't read as an address at all.
fn web_parts(raw: &str) -> Option<(&str, &str)> {
    if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (scheme, rest) = raw.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    (!authority.is_empty()).then_some((scheme, authority))
}

/// The host in an authority, without a name and password or the port.
fn host_of(authority: &str) -> &str {
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match host.strip_prefix('[') {
        Some(inner) => inner.split(']').next().unwrap_or(inner),
        None => host.split(':').next().unwrap_or(host),
    }
}

/// An id as one piece of a path: everything but the characters a path keeps
/// as they are (`urlPathAllowed`, less "/") is percent-encoded.
fn path_segment(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b"!$&'()*+,-.:;=@_~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A service's state in a word or two.
pub fn state_line(service: &ConnectorService) -> String {
    match service.state.as_str() {
        "connected" => "Signed in".to_owned(),
        "signed_out" => "Signed out".to_owned(),
        "connecting" => "Connecting".to_owned(),
        "error" => "Not working".to_owned(),
        "disabled" => "Turned off".to_owned(),
        other => {
            let words = other.replace('_', " ");
            let mut chars = words.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        }
    }
}

/// The button beside a service, or None when there's nothing to press.
pub fn button_title(service: &ConnectorService) -> Option<&'static str> {
    service.connect_url.as_deref().filter(|u| !u.is_empty())?;
    match service.state.as_str() {
        "connected" | "disabled" => None,
        "signed_out" => Some("Connect"),
        _ => Some("Reconnect"),
    }
}

/// The ⓘ's words: where the services come from, and the claude.ai ones this
/// panel doesn't cover.
pub fn about(s: Option<&ConnectorsStatus>) -> [String; 3] {
    let on = host(s).map_or_else(String::new, |h| format!(" on {h}"));
    [
        format!("These are the services your agents get from the Connectors gateway{on}. Connect opens a service's sign-in page in your browser. The arrows open the gateway's own pages, to sign a service out or reconnect it."),
        "The runner checks every few minutes and when this panel opens. When an agent runs into a service that needs signing in, it asks you with a link to the page.".to_owned(),
        "claude.ai's own connectors are signed in on claude.ai, for each login.".to_owned(),
    ]
}

/// "Gmail", "Gmail and Notion", "Gmail, Notion and Sentry".
fn list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What the mock runner's gateway says (app/Tools/mock-runner/server.mjs).
    fn mock() -> ConnectorsStatus {
        ConnectorsStatus::decode(&json!({ "state": "ok", "url": "https://bruce.example.ts.net:8443", "version": "0.2.0", "checkedAt": "2026-11-11T01:00:00.000Z", "services": [
            { "id": "notion", "title": "Notion", "state": "connected" },
            { "id": "sentry", "title": "Sentry", "state": "signed_out", "message": "The sign-in expired.", "connectUrl": "https://bruce.example.ts.net:8443/admin/connect/sentry" },
            { "id": "cloudflare", "state": "connected" },
            { "title": "No id" },
        ]}))
    }

    #[test]
    fn a_signed_out_service_wants_mike() {
        let s = mock();
        assert_eq!(s.services.len(), 3);
        assert_eq!(s.services[2].title, "cloudflare", "no title is its id");
        assert!(needs_you(Some(&s)));
        assert!(!quiet(Some(&s)));
        assert_eq!(tooltip(Some(&s)), "Connectors\nSentry needs signing in");
        assert_eq!(empty_line(Some(&s)), None);
        assert_eq!(state_line(&s.services[1]), "Signed out");
        assert_eq!(button_title(&s.services[1]), Some("Connect"));
        assert_eq!(button_title(&s.services[0]), None);
        assert_eq!(host(Some(&s)).as_deref(), Some("bruce"));
        assert_eq!(manage_url(Some(&s)).as_deref(), Some("https://bruce.example.ts.net:8443/admin"));
        assert_eq!(service_url(Some(&s), &s.services[1]).as_deref(), Some("https://bruce.example.ts.net:8443/admin/connect/sentry"));
        assert_eq!(service_url(Some(&s), &s.services[0]).as_deref(), Some("https://bruce.example.ts.net:8443/admin/connect/notion"), "a signed-in one gets its page on the gateway");
        assert!(about(Some(&s))[0].contains("Connectors gateway on bruce."));
    }

    #[test]
    fn trouble_is_the_grey_dot() {
        let mut s = mock();
        s.services[1].state = "error".into();
        assert!(!needs_you(Some(&s)));
        assert!(quiet(Some(&s)));
        assert_eq!(button_title(&s.services[1]), Some("Reconnect"));
        assert_eq!(tooltip(Some(&s)), "Connectors\nSentry has a problem");
        let down = ConnectorsStatus::decode(&json!({ "state": "unreachable", "url": "https://bruce.example.ts.net:8443", "error": "timed out", "services": [] }));
        assert!(quiet(Some(&down)));
        assert_eq!(tooltip(Some(&down)), "Connectors\nCan't reach the gateway");
        assert_eq!(empty_line(Some(&down)).as_deref(), Some("Can't reach bruce: timed out."));
        let fine = ConnectorsStatus { services: vec![ConnectorService { id: "n".into(), title: "Notion".into(), state: "connected".into(), ..Default::default() }], ..mock() };
        assert_eq!(tooltip(Some(&fine)), "Connectors\nEverything is signed in");
        assert!(!needs_you(Some(&fine)) && !quiet(Some(&fine)));
        let off = ConnectorsStatus::decode(&json!({ "state": "off" }));
        assert_eq!(empty_line(Some(&off)).as_deref(), Some("No Connectors gateway is set up for this runner."));
        assert_eq!(manage_url(Some(&off)), None);
        assert_eq!(empty_line(None).as_deref(), Some("Checking the gateway."));
        assert_eq!(state_line(&ConnectorService { state: "needs_more_love".into(), ..Default::default() }), "Needs more love");
    }

    #[test]
    fn only_web_addresses_are_opened() {
        let mut s = mock();
        s.url = Some("file:///etc/passwd".into());
        assert_eq!(manage_url(Some(&s)), None);
        assert_eq!(host(Some(&s)), None);
        s.url = Some("http://localhost:7000/some/path?x#y".into());
        assert_eq!(manage_url(Some(&s)).as_deref(), Some("http://localhost:7000/admin"));
        let odd = ConnectorService { id: "a/b c".into(), connect_url: Some("javascript:alert(1)".into()), ..Default::default() };
        assert_eq!(service_url(Some(&s), &odd).as_deref(), Some("http://localhost:7000/admin/connect/a%2Fb%20c"));
        s.url = Some("https://[::1]:8443".into());
        assert_eq!(host(Some(&s)).as_deref(), Some("::1"));
    }
}
