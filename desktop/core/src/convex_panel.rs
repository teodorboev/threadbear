// The Convex panel's data and rules, with nothing drawn (docs/protocol.md,
// "The Convex panel"): what the runner answers, the one sign-in page the app
// will open, and the script that hands Convex's embedded dashboard its deploy
// key. A port of the Swift app's ConvexDeployment, ConvexDeploymentChoice,
// ConvexLogin, ConvexPanelKey, LinkPolicy.convexSignIn and
// WebPanelPool.convexCredentialsScript.
use serde::Deserialize;
use serde_json::json;

/// Convex's embedded dashboard: it takes a deployment and a deploy key
/// instead of a Convex login (docs.convex.dev/platform-apis/embedded-dashboard).
pub const EMBEDDED: &str = "https://dashboard-embedded.convex.dev/";

/// The only host whose sign-in page the app opens (`LinkPolicy.convexSignInHost`).
pub const SIGN_IN_HOST: &str = "auth.convex.dev";

/// `panels.convexKey` when the runner's Convex CLI isn't signed in, or its
/// login expired (`RPCError.convexSignedOut`): Sign in to Convex.
pub const SIGNED_OUT: i64 = -32040;

/// When its login can't reach the deployment (`RPCError.convexNoAccess`):
/// Sign in as someone else.
pub const NO_ACCESS: i64 = -32041;

/// A Convex cloud deployment a panel shows (a panel's `convex`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Deployment {
    /// Its name, "happy-otter-123".
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub deployment: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub deployment_url: String,
    /// The Convex team and project, when `convex dev` noted them in .env.local.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub team: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub project: Option<String>,
}

impl Deployment {
    /// The project's name's tip, as the Swift panel's header says it.
    pub fn project_tip(&self) -> Option<String> {
        let project = self.project.as_ref()?;
        Some(match &self.team {
            Some(team) => format!("Convex project {project}, team {team}"),
            None => format!("Convex project {project}"),
        })
    }
}

/// One of the deployments the panel's menu offers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Choice {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub deployment: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub deployment_url: String,
    /// "Development", "Production", or "Configured" for the panel's own.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub label: String,
}

/// `panels.convexDeployments`' answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Choices {
    #[serde(default, deserialize_with = "crate::wire::list")]
    pub deployments: Vec<Choice>,
    /// Why some aren't there, in the runner's words.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub note: Option<String>,
}

/// What the menu lists: the runner's choices, with the configured deployment
/// first as "Configured" when the runner didn't list it (an older runner, or
/// its lookup failed), so the menu always has the panel's own.
pub fn with_configured(mut listed: Vec<Choice>, configured: &Deployment) -> Vec<Choice> {
    if !listed.iter().any(|c| c.deployment == configured.deployment) {
        listed.insert(0, Choice { deployment: configured.deployment.clone(), deployment_url: configured.deployment_url.clone(), label: "Configured".into() });
    }
    listed
}

/// The menu's label for the deployment shown.
pub fn label<'a>(choices: &'a [Choice], selected: &str) -> &'a str {
    choices.iter().find(|c| c.deployment == selected).map_or("Configured", |c| c.label.as_str())
}

/// `panels.convexKey`'s answer. The key is admin of its deployment, so it's
/// left out of `Debug`, in case one ever reaches a log line.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub deployment: String,
    pub deployment_url: String,
    pub admin_key: String,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({})", self.deployment)
    }
}

/// `panels.convexLogin`'s answer: a sign-in on the runner's Convex CLI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Login {
    /// idle, waiting, done or failed.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub state: String,
    /// The page to approve it on.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub url: Option<String>,
    /// What that page shows.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub code: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub message: Option<String>,
}

impl Login {
    /// The page to open: only ever Convex's own device sign-in. The runner
    /// may be another Mac's, and its answer is opened on this one, so a
    /// `file:` URL or another site from it must never reach the browser or
    /// another app (Daybreak, 7 Nov).
    pub fn page(&self) -> Option<String> {
        sign_in_page(self.url.as_deref()?)
    }

    /// Why the sign-in stops: a page that isn't Convex's, in the first answer
    /// or any later one. None when there's no page or it's fine.
    pub fn refused_page(&self) -> Option<String> {
        if self.url.is_none() || self.page().is_some() {
            return None;
        }
        Some(format!("The runner's sign-in page isn't Convex's ({SIGN_IN_HOST}), so Threadbear didn't open it. Run npx convex login on that Mac instead."))
    }
}

/// `url` if it's Convex's device sign-in page: https, its host exactly, no
/// other port, no user name or password, a `/device` path (a code may follow
/// in segments of letters, digits, `_` and `-`), and a web address the open
/// gate takes. `LinkPolicy.convexSignIn`.
pub fn sign_in_page(url: &str) -> Option<String> {
    if url.len() > 2_048 || !url.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let rest = url.strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, after) = rest.split_at(end);
    if authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if host != SIGN_IN_HOST || port.is_some_and(|p| p != "443") {
        return None;
    }
    let path = &after[..after.find(['?', '#']).unwrap_or(after.len())];
    let mut segments = path.strip_prefix("/device")?.split('/');
    // "/device", "/device/", "/device/AB-12", "/device/AB-12/".
    let first = segments.next()?;
    if !first.is_empty() {
        return None;
    }
    let tail: Vec<&str> = segments.collect();
    let names = match tail.last() {
        Some(&"") => &tail[..tail.len() - 1],
        _ => &tail[..],
    };
    if !names.iter().all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')) {
        return None;
    }
    match crate::open_policy::link_kind(url) {
        crate::open_policy::LinkKind::Web(web) => Some(web),
        _ => None,
    }
}

/// The selected deployment's full dashboard, for Open in browser. None for a
/// name that isn't one (the runner sends it).
pub fn browser_url(deployment: &str) -> Option<String> {
    let fine = !deployment.is_empty() && deployment.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    fine.then(|| format!("https://dashboard.convex.dev/d/{deployment}"))
}

/// Answers the embedded dashboard's `dashboard-credentials-request` with the
/// deploy key, only on `origin` (the dashboard's, `EMBEDDED` less its slash)
/// and only to a message from the page itself. Run in the page before its own
/// scripts, as the Swift panel's WKUserScript at document start.
pub fn credentials_script(key: &Key, origin: &str) -> String {
    let credentials = json!({
        "type": "dashboard-credentials",
        "adminKey": key.admin_key,
        "deploymentUrl": key.deployment_url,
        "deploymentName": key.deployment,
    });
    let origin = serde_json::to_string(origin).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(() => {{\n  if (location.origin !== {origin}) return;\n  const credentials = {credentials};\n  window.addEventListener('message', (e) => {{\n    if (e.source === window && e.data && e.data.type === 'dashboard-credentials-request') window.postMessage(credentials, location.origin);\n  }});\n}})();"
    )
}

/// An origin from a page's address: its scheme, host and port as written,
/// without the path (`https://dashboard-embedded.convex.dev`).
pub fn origin_of(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => format!("{scheme}://{}", &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())]),
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_only_convexs_own_sign_in_page() {
        for good in ["https://auth.convex.dev/device", "https://auth.convex.dev/device/", "https://auth.convex.dev/device/ABCD-1234", "https://auth.convex.dev:443/device?user_code=AB12", "https://auth.convex.dev/device#x"] {
            assert_eq!(sign_in_page(good).as_deref(), Some(good), "{good}");
        }
        for bad in [
            "http://auth.convex.dev/device",
            "https://auth.convex.dev.evil.example/device",
            "https://evil.example/device",
            "https://user@auth.convex.dev/device",
            "https://user:pw@auth.convex.dev/device",
            "https://auth.convex.dev:8443/device",
            "https://auth.convex.dev/other",
            "https://auth.convex.dev/devices",
            "https://auth.convex.dev/device/../x",
            "https://auth.convex.dev/device//x",
            "https://auth.convex.dev/device/a b",
            "file:///Users/mike/.ssh/id_rsa",
            "javascript:alert(1)",
            "https://AUTH.convex.dev/device",
        ] {
            assert_eq!(sign_in_page(bad), None, "{bad}");
        }
        assert_eq!(sign_in_page(&format!("https://auth.convex.dev/device/{}", "a".repeat(2_100))), None);
    }

    #[test]
    fn a_login_with_another_page_stops_saying_so() {
        let fine = Login { state: "waiting".into(), url: Some("https://auth.convex.dev/device".into()), code: Some("AB-12".into()), message: None };
        assert_eq!(fine.page().as_deref(), Some("https://auth.convex.dev/device"));
        assert_eq!(fine.refused_page(), None);
        let other = Login { url: Some("https://example.com/device".into()), ..fine.clone() };
        assert_eq!(other.page(), None);
        assert!(other.refused_page().unwrap().contains("isn't Convex's (auth.convex.dev)"));
        let none = Login { url: None, ..fine };
        assert_eq!(none.refused_page(), None);
    }

    #[test]
    fn the_menu_always_has_the_configured_deployment() {
        let configured = Deployment { deployment: "happy-otter-123".into(), deployment_url: "https://happy-otter-123.convex.cloud".into(), ..Default::default() };
        let listed = vec![Choice { deployment: "busy-bear-456".into(), deployment_url: "https://busy-bear-456.convex.cloud".into(), label: "Production".into() }];
        let choices = with_configured(listed.clone(), &configured);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "Configured");
        assert_eq!(label(&choices, "busy-bear-456"), "Production");
        assert_eq!(label(&choices, "gone-away-1"), "Configured");
        let both = with_configured(vec![Choice { deployment: "happy-otter-123".into(), deployment_url: String::new(), label: "Development".into() }], &configured);
        assert_eq!(both.len(), 1);
        assert_eq!(label(&both, "happy-otter-123"), "Development");
    }

    #[test]
    fn the_key_stays_out_of_debug_and_in_the_script() {
        let key = Key { deployment: "happy-otter-123".into(), deployment_url: "https://happy-otter-123.convex.cloud".into(), admin_key: "prod:secret|abc".into() };
        assert_eq!(format!("{key:?}"), "Key(happy-otter-123)");
        let script = credentials_script(&key, &origin_of(EMBEDDED));
        assert!(script.contains("if (location.origin !== \"https://dashboard-embedded.convex.dev\") return;"), "{script}");
        assert!(script.contains("\"adminKey\":\"prod:secret|abc\""), "{script}");
        assert!(script.contains("e.source === window"), "{script}");
        // A quote in what the runner sent can't end the string early.
        let odd = Key { admin_key: "a'b\"c</script>".into(), ..key };
        assert!(credentials_script(&odd, "https://x").contains(r#""adminKey":"a'b\"c</script>""#));
    }

    #[test]
    fn the_browser_gets_only_a_deployments_dashboard() {
        assert_eq!(browser_url("happy-otter-123").as_deref(), Some("https://dashboard.convex.dev/d/happy-otter-123"));
        assert_eq!(browser_url("../../evil"), None);
        assert_eq!(browser_url(""), None);
        assert_eq!(origin_of("http://127.0.0.1:4000/dash?x=1"), "http://127.0.0.1:4000");
        let deployment = Deployment { project: Some("walkies".into()), team: Some("mike".into()), ..Default::default() };
        assert_eq!(deployment.project_tip().as_deref(), Some("Convex project walkies, team mike"));
    }
}
