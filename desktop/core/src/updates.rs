// What the Updates panel and its activity bar icon say, a port of
// UpdatesText.swift and `CliText` (AccountUsageText.swift): the runner's own
// update (docs/protocol.md, "Runner updates"), Mike's releases on a fork
// ("Mike's releases"), and the agent CLIs' versions (`clis.list`,
// `CliStatus`). Plain sentences, one per thing going on; the view adds the
// buttons.
//
// The app's own update is AppUpdater in the Swift app and app_update.rs in
// this one, each building main and swapping it in (`AppSide`).
use crate::inbox::parse_ms;
use crate::slash::cli_name;
use chrono::{DateTime, Duration, TimeZone};
use serde_json::Value;

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_owned)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

/// `RunnerUpdates`: the runner following main, old releases still finishing
/// turns, a LaunchAgent only install-runner.sh brings up to date, and Mike's
/// releases. From hello's `updates`, then `runner.updates`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunnerUpdates {
    /// off, idle, checking, building, waiting or failed.
    pub state: String,
    pub commit: Option<String>,
    pub subject: Option<String>,
    pub reason: Option<String>,
    pub old_runners: Vec<OldRunner>,
    pub launcher_changed: bool,
    /// Which way the LaunchAgent is behind main, in a sentence (runners from
    /// before 29 Oct 2026 don't send it).
    pub launcher_reason: Option<String>,
    /// Left out by runners before 30 Oct 2026, and while it's off.
    pub upstream: Option<Upstream>,
    /// This machine can't follow GitHub's main properly. None while it's fine.
    pub follow_problem: Option<String>,
    /// The other runners saying that of themselves (the main Mac's runner).
    pub behind_runners: Vec<BehindRunner>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OldRunner {
    pub version: String,
    pub threads: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BehindRunner {
    pub name: String,
    pub problem: String,
}

/// Mike's releases, for a fork (protocol "Mike's releases").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Upstream {
    /// off, idle, checking, available or failed.
    pub state: String,
    pub reason: Option<String>,
    /// The repo it looks at, where a bring-in thread starts.
    pub checkout: Option<String>,
    pub release: Option<Release>,
    /// Mike's own runner: the last release tag it made.
    pub published_tag: Option<String>,
    /// Mike's own runner: why today's release is waiting on GitHub's checks.
    pub held_why: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    /// "release/2026-10-30".
    pub tag: String,
    /// The commit the verified tag names: what Bring Them In merges, rather
    /// than a ref name a later fetch could move.
    pub commit: Option<String>,
    pub changes: i64,
    pub subjects: Vec<String>,
    /// Each verified release main doesn't have yet, newest first.
    pub notes: Vec<Notes>,
}

/// One verified release's notes, or its commit subjects as a fallback.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notes {
    pub tag: String,
    pub lines: Vec<String>,
}

impl RunnerUpdates {
    /// Read leniently, as the Swift decoder does; None when there's nothing
    /// there (an older runner).
    pub fn decode(v: &Value) -> Option<RunnerUpdates> {
        if !v.is_object() {
            return None;
        }
        Some(RunnerUpdates {
            state: v["state"].as_str().unwrap_or("idle").to_owned(),
            commit: text(v, "commit"),
            subject: text(v, "subject"),
            reason: text(v, "reason"),
            old_runners: v["oldRunners"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|o| o.is_object())
                .map(|o| OldRunner { version: o["version"].as_str().unwrap_or("?").to_owned(), threads: strings(&o["threads"]) })
                .collect(),
            launcher_changed: v["launcherChanged"].as_bool().unwrap_or(false),
            launcher_reason: text(v, "launcherReason"),
            upstream: Upstream::decode(&v["upstream"]),
            follow_problem: text(v, "followProblem"),
            behind_runners: v["behindRunners"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| Some(BehindRunner { name: b["name"].as_str()?.to_owned(), problem: b["problem"].as_str()?.to_owned() }))
                .collect(),
        })
    }

    /// Something's on its way: a release being built or about to start, an
    /// old runner still finishing turns, or a LaunchAgent to reinstall.
    pub fn is_pending(&self) -> bool {
        self.state == "building" || self.state == "waiting" || !self.old_runners.is_empty() || self.launcher_changed
    }

    pub fn failed(&self) -> bool {
        self.state == "failed"
    }
}

impl Upstream {
    fn decode(v: &Value) -> Option<Upstream> {
        if !v.is_object() {
            return None;
        }
        Some(Upstream {
            state: v["state"].as_str().unwrap_or("off").to_owned(),
            reason: text(v, "reason"),
            checkout: text(v, "checkout"),
            release: Release::decode(&v["release"]),
            published_tag: v["published"]["tag"].as_str().map(str::to_owned),
            held_why: v["held"]["why"].as_str().map(str::to_owned),
        })
    }

    /// A newer release than main has, to bring in.
    pub fn available(&self) -> bool {
        self.state == "available" && self.release.as_ref().is_some_and(|r| r.changes > 0)
    }
}

impl Release {
    fn decode(v: &Value) -> Option<Release> {
        if !v.is_object() {
            return None;
        }
        Some(Release {
            tag: v["tag"].as_str().unwrap_or("?").to_owned(),
            commit: text(v, "commit"),
            changes: v["changes"].as_i64().or_else(|| v["changes"].as_f64().filter(|n| n.fract() == 0.).map(|n| n as i64)).unwrap_or(0),
            subjects: strings(&v["subjects"]),
            notes: v["notes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| Some(Notes { tag: n["tag"].as_str()?.to_owned(), lines: n["lines"].as_array().map(|_| strings(&n["lines"]))? }))
                .filter(|n| !n.lines.is_empty())
                .collect(),
        })
    }
}

/// The app's side, as the Swift app's AppUpdater knows it: a build of main
/// staged, building, or failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppSide {
    /// Short commit of the running build.
    pub running: Option<String>,
    /// Short commit waiting to be swapped in.
    pub staged: Option<String>,
    pub building: bool,
    pub failed: bool,
    /// The GPUI app's build found no Mac to build on, and tries again at its
    /// next look (the Swift app builds where it is, so never).
    pub waiting: bool,
    /// Why the installed GPUI app can't follow main (one installed before it
    /// could, say). The Swift app has no such case.
    pub not_following: Option<String>,
}

impl AppSide {
    pub fn is_pending(&self) -> bool {
        self.staged.is_some() || self.building || self.waiting
    }
}

fn runner_pending(runner: Option<&RunnerUpdates>) -> bool {
    runner.is_some_and(RunnerUpdates::is_pending)
}

fn runner_failed(runner: Option<&RunnerUpdates>) -> bool {
    runner.is_some_and(RunnerUpdates::failed)
}

/// Something on its way, or something that went wrong, for the app or the
/// runner: the icon's grey dot. The icon itself is always there since the
/// CLIs' versions moved into its panel (28 Oct).
pub fn shown(app: &AppSide, runner: Option<&RunnerUpdates>) -> bool {
    app.is_pending() || app.failed || app.not_following.is_some() || runner_pending(runner) || runner_failed(runner) || from_mike(runner) || not_following(runner)
}

/// A release of Mike's this copy's main doesn't have yet.
pub fn from_mike(runner: Option<&RunnerUpdates>) -> bool {
    runner.and_then(|r| r.upstream.as_ref()).is_some_and(Upstream::available)
}

/// A CLI with a new version, held back, updating, or whose update failed:
/// also a grey dot, and a line in the tooltip.
pub fn cli_pending(cli: &CliStatus) -> bool {
    matches!(cli.status.as_str(), "behind" | "held" | "missing") || cli.update.as_ref().is_some_and(|u| matches!(u.state.as_str(), "waiting" | "running" | "failed"))
}

/// The tooltip's line for a Threadbear Runner.app (or launch.mjs) that only
/// install-runner.sh brings up to date.
pub const REINSTALL_LINE: &str = "The runner needs reinstalling, which only you can do";

/// The icon's tooltip: "Updates", then a line for each thing going on, or
/// that everything is up to date. The panel has the rest.
pub fn icon_tooltip(app: &AppSide, runner: Option<&RunnerUpdates>, clis: &[CliStatus]) -> String {
    let mut lines = vec!["Updates".to_owned()];
    // A runner app to reinstall gets its own line, as it's the one that's
    // Mike's to do (the accent dot); left in, it read as "A runner update",
    // which looks like one on its way by itself (12 Nov, Mike: "I thought
    // blue dots reserved for something that needs me to action on it").
    let moving = runner.map(|r| RunnerUpdates { launcher_changed: false, ..r.clone() });
    let moving = moving.as_ref();
    if app.is_pending() || app.failed || runner_pending(moving) || runner_failed(moving) {
        lines.push(tooltip(app, moving));
    }
    if runner.is_some_and(|r| r.launcher_changed) {
        lines.push(REINSTALL_LINE.to_owned());
    }
    if let Some(line) = upstream_tooltip(runner.and_then(|r| r.upstream.as_ref())) {
        lines.push(line);
    }
    if app.not_following.is_some() {
        lines.push("The app isn't following main".to_owned());
    }
    if not_following(runner) {
        lines.push("A runner isn't following GitHub's main".to_owned());
    }
    for cli in clis.iter().filter(|c| cli_pending(c)) {
        lines.push(banner_text(cli).unwrap_or_else(|| status_line(cli)));
    }
    if lines.len() == 1 {
        lines.push("Everything is up to date".to_owned());
    }
    lines.join("\n")
}

/// Whether it wants Mike (the accent dot): a launch.mjs only he can install,
/// a release of his to bring in (they only come in when he says), a
/// machine that isn't following GitHub's main, or an installed GPUI app that
/// can't follow main until it's installed again. Anything else sorts itself
/// out, a failed build included, so it gets the grey dot (27 Oct, Mike: "the
/// blue dot usually indicates there's something for me to action").
pub fn needs_you(app: &AppSide, runner: Option<&RunnerUpdates>) -> bool {
    app.not_following.is_some() || runner.is_some_and(|r| r.launcher_changed) || from_mike(runner) || not_following(runner)
}

/// A machine that isn't following GitHub's main: nothing fixes that by
/// itself, and bruce sat three days behind unnoticed (9 Nov).
fn not_following(runner: Option<&RunnerUpdates>) -> bool {
    runner.is_some_and(|u| u.follow_problem.is_some() || !u.behind_runners.is_empty())
}

/// One line for each machine that isn't following GitHub's main.
pub fn follow_lines(u: &RunnerUpdates) -> Vec<String> {
    let mut lines: Vec<String> = u.follow_problem.iter().map(|p| format!("This runner isn't following GitHub's main properly: {p}")).collect();
    lines.extend(u.behind_runners.iter().map(|b| format!("{} isn't following GitHub's main properly: {}", b.name, b.problem)));
    lines
}

fn changes_words(n: i64, suffix: &str) -> String {
    if n == 1 {
        format!("1 new change{suffix}")
    } else {
        format!("{n} new changes{suffix}")
    }
}

/// "12 new changes from Mike", for the icon's tooltip.
pub fn upstream_tooltip(u: Option<&Upstream>) -> Option<String> {
    let u = u.filter(|u| u.available())?;
    Some(changes_words(u.release.as_ref()?.changes, " from Mike"))
}

/// The panel's From Mike section: what's new, that it's up to date, or why
/// it can't tell. None when there's nothing to say.
pub fn upstream_line(u: Option<&Upstream>) -> Option<String> {
    let u = u?;
    // Held comes first: it's the one that wants something doing (a red
    // build on main), and the last tag is still in tb status.
    if let (Some(why), "off") = (&u.held_why, u.state.as_str()) {
        return Some(format!("Your next release for everyone else is waiting: {why}."));
    }
    if let (Some(tag), "off") = (&u.published_tag, u.state.as_str()) {
        return Some(format!("Your last release for everyone else is {tag}."));
    }
    match u.state.as_str() {
        "available" => {
            let r = u.release.as_ref()?;
            Some(format!("Mike's release {} has {}.", release_date(&r.tag), changes_words(r.changes, "")))
        }
        "idle" => Some(u.release.as_ref().map_or_else(|| "Mike hasn't made a release yet.".to_owned(), |r| format!("You're up to date with Mike's release {}.", release_date(&r.tag)))),
        "checking" => Some("Checking for Mike's releases.".to_owned()),
        "failed" => Some(format!("Couldn't check for Mike's releases{}", u.reason.as_ref().map_or_else(|| ".".to_owned(), |r| format!(": {r}")))),
        _ => None,
    }
}

/// What the From Mike section lists under a release: the plain-words notes
/// Mike's runner wrote, a heading per release when there's more than one, or
/// (from older tags) the first commit subjects.
pub fn release_lines(r: &Release) -> Vec<(Option<String>, Vec<String>)> {
    let first_subjects = || r.subjects.iter().take(5).cloned().collect::<Vec<_>>();
    if r.notes.is_empty() {
        return vec![(None, first_subjects())];
    }
    if r.notes.len() == 1 && r.notes[0].tag == r.tag {
        return vec![(None, r.notes[0].lines.clone())];
    }
    let mut groups: Vec<(Option<String>, Vec<String>)> = r
        .notes
        .iter()
        .map(|note| {
            let date = release_date(&note.tag);
            (Some(date.strip_prefix("of ").map_or(date.clone(), str::to_owned)), note.lines.clone())
        })
        .collect();
    // An older runner may supply notes only for older tags. Keep those
    // dated, and show the subjects so the newest changes remain visible.
    if !r.notes.iter().any(|n| n.tag == r.tag) && !r.subjects.is_empty() {
        groups.push((Some("Every change".to_owned()), first_subjects()));
    }
    groups
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "release/2026-10-30" as "of 30 Oct".
pub fn release_date(tag: &str) -> String {
    let date = tag.strip_prefix("release/").unwrap_or(tag);
    let parts: Vec<i64> = date.split('-').filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [_, month @ 1..=12, day] => format!("of {day} {}", MONTHS[(*month - 1) as usize]),
        _ => tag.to_owned(),
    }
}

/// The commit the runner checked the release's signature for, so the thread
/// merges that and not a ref a later fetch could have moved.
fn commit_line(r: &Release, merge: bool) -> String {
    let Some(c) = r.commit.as_deref().filter(|c| (c.len() == 40 || c.len() == 64) && c.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) else { return String::new() };
    if merge {
        format!(" Merge its commit {c}, the one its signature was checked for.")
    } else {
        format!(" Its commit is {c}, the one its signature was checked for.")
    }
}

fn plural(n: i64) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The first message of the thread Bring Them In starts.
pub fn bring_in_prompt(r: &Release) -> String {
    format!(
        "Bring in Mike's release {} ({} new change{}) to this copy of Threadbear.{} Follow docs/bringing-in-updates.md, and ask me before it lands.",
        r.tag,
        r.changes,
        plural(r.changes),
        commit_line(r, true)
    )
}

/// Let's Discuss: the same release, talked through before anything changes
/// (Mike, 4 Nov 2026), for someone who wants to know what it does to their
/// own changes first.
pub fn discuss_prompt(r: &Release) -> String {
    format!(
        "Mike's release {} has {} new change{} for this copy of Threadbear. Before bringing any of it in, talk me through it: what's in it that I'd notice, anything that touches or clashes with the changes I've made in this copy, and anything risky. Section 1 of docs/bringing-in-updates.md says how to see both sides.{} Don't change anything yet; once we've talked it over, I'll say whether to bring it in, and then follow that doc.",
        r.tag,
        r.changes,
        plural(r.changes),
        commit_line(r, false)
    )
}

/// The thread's title: "Bring in Mike's release of 6 Nov", or "Discuss ...".
pub fn bring_in_title(r: &Release, discuss: bool) -> String {
    format!("{} Mike's release {}", if discuss { "Discuss" } else { "Bring in" }, release_date(&r.tag))
}

/// The icon's tooltip line about the app and the runner. A failure is said,
/// as it shares the grey dot with an update on its way.
pub fn tooltip(app: &AppSide, runner: Option<&RunnerUpdates>) -> String {
    let app_part = app.is_pending() || app.failed;
    let runner_part = runner_pending(runner) || runner_failed(runner);
    let app_failed = app.failed && !app.is_pending();
    let runner_failed_only = runner_failed(runner) && !runner_pending(runner);
    if app_failed && runner_failed_only {
        return "The app's and the runner's updates failed".to_owned();
    }
    if app_failed && !runner_part {
        return "The app's update failed".to_owned();
    }
    if runner_failed_only && !app_part {
        return "The runner's update failed".to_owned();
    }
    match (app_part, runner_part) {
        (true, true) => "Updates for the app and the runner",
        (true, false) => "An app update",
        (false, true) => "A runner update",
        _ => "Updates",
    }
    .to_owned()
}

pub fn app_line(a: &AppSide) -> String {
    if let Some(staged) = &a.staged {
        return format!("The app's next version ({staged}) is ready. It switches in when you're not using the app, or now.");
    }
    if a.building {
        return "Building the app from main.".to_owned();
    }
    if a.waiting {
        return "The app has a new version on main, waiting for a Mac to build it on.".to_owned();
    }
    if a.failed {
        return "The app's last build from main failed. The log says why.".to_owned();
    }
    if let Some(why) = &a.not_following {
        return format!("The app isn't following main: {why}.");
    }
    a.running.as_ref().map_or_else(|| "The app is up to date.".to_owned(), |r| format!("The app is up to date ({r})."))
}

/// The Updates icon's right-click popover, in a line (`popoverLine`).
pub fn popover_line(a: &AppSide) -> String {
    if a.staged.is_some() {
        return "A new version of the app is ready.".to_owned();
    }
    if a.building {
        return "Building the app's next version.".to_owned();
    }
    if a.failed {
        return "The app's last build failed.".to_owned();
    }
    app_line(a)
}

/// The runner's own build: building, built and starting, or failed.
pub fn build_line(u: &RunnerUpdates) -> Option<String> {
    let at = u.commit.as_ref().map_or_else(String::new, |c| format!(" at {c}"));
    let what = u.subject.as_ref().map_or_else(String::new, |s| format!(": {s}"));
    match u.state.as_str() {
        "building" => Some(format!("Building the runner from main{at}{what}.")),
        "waiting" => Some(format!("The runner from main{at} is built and starting.")),
        "failed" => Some(format!("The runner couldn't update to main{at}{}", u.reason.as_ref().map_or_else(|| ".".to_owned(), |r| format!(": {r}")))),
        _ => None,
    }
}

/// Threads still finishing on an old runner; the view links each one under it.
pub fn old_runners_line(u: &RunnerUpdates) -> Option<String> {
    let threads: usize = u.old_runners.iter().map(|o| o.threads.len()).sum();
    match threads {
        0 => None,
        1 => Some("1 thread is finishing its turn on the previous version. It moves to the new one when the turn ends.".to_owned()),
        n => Some(format!("{n} threads are finishing their turns on the previous version. Each moves to the new one when its turn ends.")),
    }
}

/// A LaunchAgent only install-runner.sh brings up to date (launch.mjs, the
/// plist, Threadbear Runner.app); the view shows the command under it. An
/// older runner sends no reason, and then it was launch.mjs.
pub fn launcher_line(u: &RunnerUpdates) -> Option<String> {
    if !u.launcher_changed {
        return None;
    }
    let why = u.launcher_reason.as_deref().unwrap_or("launch.mjs has changed on main.");
    Some(format!("{why} It only updates when you run this, once nothing is busy:"))
}

/// Every runner line in order, or the one that says there's nothing.
pub fn runner_lines(u: Option<&RunnerUpdates>) -> Vec<String> {
    let Some(u) = u else { return vec!["This runner doesn't say how its updates are going.".to_owned()] };
    let mut out: Vec<String> = [build_line(u), old_runners_line(u), launcher_line(u)].into_iter().flatten().collect();
    out.extend(follow_lines(u));
    if out.is_empty() {
        out.push("The runner is up to date.".to_owned());
    }
    out
}

/// The command for a changed launch.mjs, from the checkout with `~` for the
/// home folder. A runner that doesn't say where its checkout is: the README's
/// `gh repo fork --clone` puts it in ~/threadbear from the home folder.
pub fn install_command(checkout: Option<&str>, home: &str) -> String {
    let root = match checkout {
        Some(path) if !home.is_empty() && (path == home || path.starts_with(&format!("{}/", home.trim_end_matches('/')))) => format!("~{}", &path[home.trim_end_matches('/').len()..]),
        Some(path) => path.to_owned(),
        None => "~/threadbear".to_owned(),
    };
    format!("{root}/runner/scripts/install-runner.sh")
}

// ---- The CLIs (`clis.list`, `CliText`)

/// One agent CLI (`CliStatus`): its version, whether there's a newer one, and
/// the update it's doing or did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CliStatus {
    pub provider: String,
    pub path: String,
    /// "threadbear" (Codex's own copies), "cli" (Claude's installer) or
    /// "none" (a path set by hand, never updated).
    pub managed_by: String,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    /// current, behind, held, unknown or missing.
    pub status: String,
    /// auto, notify or off.
    pub policy: String,
    /// Codex: the kept version Go back returns to.
    pub previous_version: Option<String>,
    /// An agent with no version feed: Update is offered at any time.
    pub can_update_now: bool,
    pub checked_ms: Option<i64>,
    pub hold: Option<Hold>,
    pub update: Option<CliUpdate>,
}

/// Why an update wasn't installed by itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hold {
    pub version: String,
    pub reason: String,
    pub missing: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CliUpdate {
    /// waiting, running, succeeded, unchanged or failed.
    pub state: String,
    pub target_version: Option<String>,
    pub waiting_for: Vec<String>,
    pub finished_ms: Option<i64>,
    pub message: Option<String>,
}

impl CliStatus {
    pub fn decode(v: &Value) -> Option<CliStatus> {
        let provider = v["provider"].as_str()?.to_owned();
        Some(CliStatus {
            provider,
            path: v["path"].as_str().unwrap_or("").to_owned(),
            managed_by: v["managedBy"].as_str().unwrap_or("cli").to_owned(),
            installed_version: text(v, "installedVersion"),
            latest_version: text(v, "latestVersion"),
            status: v["status"].as_str().unwrap_or("unknown").to_owned(),
            policy: v["policy"].as_str().unwrap_or("auto").to_owned(),
            previous_version: text(v, "previousVersion"),
            can_update_now: v["canUpdateNow"].as_bool().unwrap_or(false),
            checked_ms: v["checkedAt"].as_str().and_then(parse_ms),
            hold: v["hold"].is_object().then(|| Hold { version: v["hold"]["version"].as_str().unwrap_or("").to_owned(), reason: v["hold"]["reason"].as_str().unwrap_or("").to_owned(), missing: strings(&v["hold"]["missing"]) }),
            update: v["update"].is_object().then(|| {
                let u = &v["update"];
                CliUpdate {
                    state: u["state"].as_str().unwrap_or("unknown").to_owned(),
                    target_version: text(u, "targetVersion"),
                    waiting_for: strings(&u["waitingFor"]),
                    finished_ms: u["finishedAt"].as_str().and_then(parse_ms),
                    message: text(u, "message"),
                }
            }),
        })
    }
}

/// `{ clis: CliStatus[] }`, as `clis.list`, `clis.updated` and the buttons'
/// answers have it, or hello's `clis`.
pub fn decode_clis(list: &Value) -> Vec<CliStatus> {
    list.as_array().into_iter().flatten().filter_map(CliStatus::decode).collect()
}

/// "Claude Code 2.1.311, up to date", "Claude Code 2.1.311, 2.1.312
/// available", "Codex 0.165.0 is held back because the app-server protocol
/// changed".
pub fn status_line(cli: &CliStatus) -> String {
    let name = cli_name(&cli.provider);
    let installed = cli.installed_version.as_ref().map_or_else(|| name.clone(), |v| format!("{name} {v}"));
    match cli.status.as_str() {
        "current" => format!("{installed}, up to date"),
        "behind" => format!("{installed}{}", cli.latest_version.as_ref().map_or_else(|| ", an update is available".to_owned(), |v| format!(", {v} available"))),
        "held" => {
            let version = cli.hold.as_ref().map(|h| h.version.clone()).or_else(|| cli.latest_version.clone()).unwrap_or_else(|| "The new version".to_owned());
            let reason = cli.hold.as_ref().map_or("", |h| h.reason.as_str());
            format!("{name} {version} is held back{}", if reason.is_empty() { String::new() } else { format!(" because {reason}") })
        }
        "missing" => format!("{name} isn't installed"),
        _ => installed,
    }
}

/// Who keeps it up to date, when that isn't the usual: a path set by hand is
/// never updated.
pub fn managed_line(cli: &CliStatus) -> Option<&'static str> {
    (cli.managed_by == "none").then_some("Set by hand in runner.json, so Threadbear leaves it alone")
}

/// The update button's title, or None when there's nothing to install.
pub fn update_title(cli: &CliStatus) -> Option<&'static str> {
    if is_updating(cli) {
        return None;
    }
    if cli.can_update_now {
        return Some("Update");
    }
    match cli.status.as_str() {
        "behind" => Some("Update"),
        "held" => Some("Update anyway"),
        _ => None,
    }
}

/// "Go back to 0.163.0", for Codex with a kept version. Not when the kept
/// version is the one held (Mike went back from it): the hold's "Update
/// anyway" already reinstalls it.
pub fn go_back_title(cli: &CliStatus) -> Option<String> {
    let previous = cli.previous_version.as_deref().filter(|p| !p.is_empty())?;
    if Some(previous) == cli.installed_version.as_deref() || is_updating(cli) || cli.hold.as_ref().is_some_and(|h| h.version == previous) {
        return None;
    }
    Some(format!("Go back to {previous}"))
}

pub fn is_updating(cli: &CliStatus) -> bool {
    cli.update.as_ref().is_some_and(|u| u.state == "running" || u.state == "waiting")
}

/// While an update runs or after one failed: "Updating Claude Code to
/// 2.1.312", "Installing Codex 0.165.0".
pub fn banner_text(cli: &CliStatus) -> Option<String> {
    let update = cli.update.as_ref()?;
    let name = cli_name(&cli.provider);
    let target = update.target_version.clone().or_else(|| cli.latest_version.clone());
    let to = target.as_ref().map_or_else(String::new, |t| format!(" to {t}"));
    match update.state.as_str() {
        "running" if cli.provider == "codex" => Some(format!("Installing Codex{}", target.as_ref().map_or_else(String::new, |t| format!(" {t}")))),
        "running" => Some(format!("Updating {name}{to}")),
        "waiting" => Some(format!("Waiting to update {name}{to}")),
        "failed" => {
            let mut text = format!("Couldn't update {name}{to}");
            if let Some(message) = update.message.as_deref().filter(|m| !m.is_empty()) {
                text.push_str(": ");
                text.push_str(message);
            }
            Some(text)
        }
        _ => None,
    }
}

pub fn policy_title(policy: &str) -> String {
    match policy {
        "auto" => "Automatic".to_owned(),
        "notify" => "Ask me".to_owned(),
        "off" => "Off".to_owned(),
        other => other.to_owned(),
    }
}

pub const POLICIES: [&str; 3] = ["auto", "notify", "off"];

pub fn policy_symbol(policy: &str) -> &'static str {
    match policy {
        "auto" => "arrow.triangle.2.circlepath",
        "notify" => "bell",
        _ => "nosign",
    }
}

/// "Checked at 9:40 am", "Checked Mon 9:40 am".
pub fn checked_line<Tz: TimeZone>(clis: &[CliStatus], now: &DateTime<Tz>) -> Option<String> {
    let at_ms = clis.iter().filter_map(|c| c.checked_ms).max()?;
    let checked = now.timezone().timestamp_millis_opt(at_ms).single()?;
    let clock = crate::new_thread::clock(&checked);
    if checked.date_naive() == now.date_naive() {
        return Some(format!("Checked at {clock}"));
    }
    let day = if checked.date_naive() == (now.clone() + Duration::days(1)).date_naive() {
        "tomorrow".to_owned()
    } else if checked.date_naive() == (now.clone() - Duration::days(1)).date_naive() {
        "yesterday".to_owned()
    } else {
        use chrono::Datelike as _;
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][checked.weekday().num_days_from_sunday() as usize].to_owned()
    };
    Some(format!("Checked {day} {clock}"))
}

/// The CLI's line as both apps' dumps give it (AppDump.usageLines): its
/// status, the policy, the buttons it has, and what an update is doing.
pub fn dump_line(cli: &CliStatus) -> String {
    let mut line = format!("{} ({})", status_line(cli), policy_title(&cli.policy).to_lowercase());
    if let Some(button) = update_title(cli) {
        line.push_str(&format!(" [{button}]"));
    }
    if let Some(back) = go_back_title(cli) {
        line.push_str(&format!(" [{back}]"));
    }
    if let (true, Some(text)) = (is_updating(cli), banner_text(cli)) {
        line.push_str(": ");
        line.push_str(&text);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Local};
    use serde_json::json;

    /// The mock runner's two CLIs (app/Tools/mock-runner/server.mjs).
    fn mock_clis() -> Vec<CliStatus> {
        decode_clis(&json!([
            { "provider": "claude", "path": "/x/claude", "managedBy": "cli", "installedVersion": "2.1.311", "latestVersion": "2.1.312", "status": "behind", "policy": "auto", "inUse": [], "checkedAt": "2026-11-11T01:00:00.000Z" },
            { "provider": "codex", "path": "/x/codex", "managedBy": "threadbear", "installedVersion": "0.164.0", "latestVersion": "0.165.0", "status": "held", "policy": "auto", "inUse": [], "previousVersion": "0.163.0",
              "checkedAt": "2026-11-11T01:00:00.000Z", "hold": { "version": "0.165.0", "reason": "the app-server protocol changed", "missing": ["thread/backgroundTerminals/list"] } },
            { "path": "no provider" },
        ]))
    }

    #[test]
    fn the_clis_say_what_they_have_and_what_they_offer() {
        let clis = mock_clis();
        assert_eq!(clis.len(), 2);
        assert_eq!(status_line(&clis[0]), "Claude Code 2.1.311, 2.1.312 available");
        assert_eq!(update_title(&clis[0]), Some("Update"));
        assert_eq!(go_back_title(&clis[0]), None);
        assert_eq!(status_line(&clis[1]), "Codex 0.165.0 is held back because the app-server protocol changed");
        assert_eq!(update_title(&clis[1]), Some("Update anyway"));
        assert_eq!(go_back_title(&clis[1]).as_deref(), Some("Go back to 0.163.0"));
        assert_eq!(clis[1].hold.as_ref().unwrap().missing, ["thread/backgroundTerminals/list"]);
        assert_eq!(dump_line(&clis[0]), "Claude Code 2.1.311, 2.1.312 available (automatic) [Update]");
        assert_eq!(dump_line(&clis[1]), "Codex 0.165.0 is held back because the app-server protocol changed (automatic) [Update anyway] [Go back to 0.163.0]");
        assert!(clis.iter().all(cli_pending));
        let mut running = clis[0].clone();
        running.update = Some(CliUpdate { state: "running".into(), target_version: Some("2.1.312".into()), ..Default::default() });
        assert_eq!(update_title(&running), None, "nothing to press while it updates");
        assert_eq!(dump_line(&running), "Claude Code 2.1.311, 2.1.312 available (automatic): Updating Claude Code to 2.1.312");
        let mut codex = clis[1].clone();
        codex.update = Some(CliUpdate { state: "running".into(), target_version: Some("0.165.0".into()), ..Default::default() });
        assert_eq!(banner_text(&codex).as_deref(), Some("Installing Codex 0.165.0"));
        codex.update = Some(CliUpdate { state: "failed".into(), target_version: Some("0.165.0".into()), message: Some("0.165.0 is held back".into()), ..Default::default() });
        assert_eq!(banner_text(&codex).as_deref(), Some("Couldn't update Codex to 0.165.0: 0.165.0 is held back"));
        let current = CliStatus { status: "current".into(), ..clis[0].clone() };
        assert_eq!(status_line(&current), "Claude Code 2.1.311, up to date");
        assert!(!cli_pending(&current));
        assert_eq!(managed_line(&CliStatus { managed_by: "none".into(), ..current }), Some("Set by hand in runner.json, so Threadbear leaves it alone"));
    }

    #[test]
    fn the_popover_says_the_app_update_in_a_line() {
        let side = |staged: Option<&str>, building: bool, failed: bool| AppSide { running: Some("abc1234".into()), staged: staged.map(str::to_owned), building, failed, ..Default::default() };
        assert_eq!(popover_line(&side(Some("def5678"), true, false)), "A new version of the app is ready.");
        assert_eq!(popover_line(&side(None, true, false)), "Building the app's next version.");
        assert_eq!(popover_line(&side(None, false, true)), "The app's last build failed.");
        assert_eq!(popover_line(&side(None, false, false)), "The app is up to date (abc1234).");
    }

    #[test]
    fn checked_says_the_time_or_the_day() {
        let zone = FixedOffset::east_opt(8 * 3600).unwrap();
        let clis = mock_clis();
        let now = zone.timestamp_millis_opt(parse_ms("2026-11-11T01:20:00.000Z").unwrap()).unwrap();
        assert_eq!(checked_line(&clis, &now).as_deref(), Some("Checked at 9 am"));
        let later = zone.timestamp_millis_opt(parse_ms("2026-11-12T01:20:00.000Z").unwrap()).unwrap();
        assert_eq!(checked_line(&clis, &later).as_deref(), Some("Checked yesterday 9 am"));
        let week = zone.timestamp_millis_opt(parse_ms("2026-11-14T01:20:00.000Z").unwrap()).unwrap();
        assert_eq!(checked_line(&clis, &week).as_deref(), Some("Checked Wed 9 am"));
        assert_eq!(checked_line(&[], &Local::now()), None);
    }

    #[test]
    fn the_icon_says_whats_going_on() {
        let app = AppSide::default();
        let clis = mock_clis();
        let idle = RunnerUpdates::decode(&json!({ "state": "idle", "oldRunners": [] })).unwrap();
        assert!(!shown(&app, Some(&idle)));
        assert!(!needs_you(&app, Some(&idle)));
        assert_eq!(icon_tooltip(&app, Some(&idle), &[]), "Updates\nEverything is up to date");
        assert_eq!(icon_tooltip(&app, Some(&idle), &clis), "Updates\nClaude Code 2.1.311, 2.1.312 available\nCodex 0.165.0 is held back because the app-server protocol changed");
        let building = RunnerUpdates::decode(&json!({ "state": "building", "commit": "abc1234", "subject": "Fix the thing", "oldRunners": [] })).unwrap();
        assert!(shown(&app, Some(&building)));
        assert!(!needs_you(&app, Some(&building)), "it sorts itself out");
        assert_eq!(icon_tooltip(&app, Some(&building), &[]), "Updates\nA runner update");
        assert_eq!(runner_lines(Some(&building)), ["Building the runner from main at abc1234: Fix the thing."]);
        let failed = RunnerUpdates::decode(&json!({ "state": "failed", "commit": "abc1234", "reason": "npm ci failed" })).unwrap();
        assert_eq!(tooltip(&app, Some(&failed)), "The runner's update failed");
        assert_eq!(runner_lines(Some(&failed)), ["The runner couldn't update to main at abc1234: npm ci failed"]);
        let launcher = RunnerUpdates::decode(&json!({ "state": "idle", "launcherChanged": true })).unwrap();
        assert!(needs_you(&app, Some(&launcher)));
        assert_eq!(icon_tooltip(&app, Some(&launcher), &[]), "Updates\nThe runner needs reinstalling, which only you can do");
        let both = RunnerUpdates::decode(&json!({ "state": "building", "commit": "abc1234", "launcherChanged": true })).unwrap();
        assert_eq!(icon_tooltip(&app, Some(&both), &[]), "Updates\nA runner update\nThe runner needs reinstalling, which only you can do");
        assert_eq!(runner_lines(Some(&launcher)), ["launch.mjs has changed on main. It only updates when you run this, once nothing is busy:"]);
        let behind = RunnerUpdates::decode(&json!({ "state": "idle", "behindRunners": [{ "name": "bruce", "problem": "3 days behind" }] })).unwrap();
        assert!(needs_you(&app, Some(&behind)));
        assert_eq!(icon_tooltip(&app, Some(&behind), &[]), "Updates\nA runner isn't following GitHub's main");
        assert_eq!(runner_lines(Some(&behind)), ["bruce isn't following GitHub's main properly: 3 days behind"]);
        let draining = RunnerUpdates::decode(&json!({ "state": "idle", "oldRunners": [{ "version": "0.1.0", "threads": ["t1", "t2"] }] })).unwrap();
        assert_eq!(runner_lines(Some(&draining)), ["2 threads are finishing their turns on the previous version. Each moves to the new one when its turn ends."]);
        assert_eq!(runner_lines(Some(&idle)), ["The runner is up to date."]);
        assert_eq!(runner_lines(None), ["This runner doesn't say how its updates are going."]);
        assert_eq!(RunnerUpdates::decode(&Value::Null), None);
        assert_eq!(app_line(&app), "The app is up to date.");
    }

    // The installed GPUI app can't follow main until it's installed again:
    // only Mike can do that, so the accent dot.
    #[test]
    fn an_app_that_cant_follow_main_says_why_and_needs_you() {
        let app = AppSide { running: Some("abc1234".into()), not_following: Some("it was installed before it could".into()), ..Default::default() };
        assert_eq!(app_line(&app), "The app isn't following main: it was installed before it could.");
        assert!(shown(&app, None) && needs_you(&app, None));
        assert_eq!(icon_tooltip(&app, None, &[]), "Updates\nThe app isn't following main");
        // Waiting for a Mac is on its way, so grey.
        let waiting = AppSide { running: Some("abc1234".into()), waiting: true, ..Default::default() };
        assert!(shown(&waiting, None) && !needs_you(&waiting, None));
        assert_eq!(app_line(&waiting), "The app has a new version on main, waiting for a Mac to build it on.");
    }

    #[test]
    fn mikes_releases_say_whats_new_and_start_a_thread() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let u = RunnerUpdates::decode(&json!({ "state": "idle", "upstream": { "state": "available", "checkout": "/Users/me/threadbear",
            "release": { "tag": "release/2026-11-10", "commit": commit, "changes": 3, "subjects": ["a", "b", "c"], "notes": [{ "tag": "release/2026-11-10", "lines": ["Runners in the app"] }, { "tag": "bad" }] } } })).unwrap();
        assert!(from_mike(Some(&u)));
        assert!(needs_you(&AppSide::default(), Some(&u)));
        let upstream = u.upstream.as_ref().unwrap();
        assert_eq!(upstream_line(Some(upstream)).as_deref(), Some("Mike's release of 10 Nov has 3 new changes."));
        assert_eq!(upstream_tooltip(Some(upstream)).as_deref(), Some("3 new changes from Mike"));
        let release = upstream.release.as_ref().unwrap();
        assert_eq!(release_lines(release), [(None, vec!["Runners in the app".to_string()])]);
        assert_eq!(bring_in_title(release, false), "Bring in Mike's release of 10 Nov");
        assert_eq!(bring_in_title(release, true), "Discuss Mike's release of 10 Nov");
        assert!(bring_in_prompt(release).contains(&format!("(3 new changes) to this copy of Threadbear. Merge its commit {commit}, the one its signature was checked for. Follow")));
        assert!(discuss_prompt(release).contains(&format!("Its commit is {commit}, the one")));
        let unsigned = Release { commit: Some("main".into()), ..release.clone() };
        assert!(bring_in_prompt(&unsigned).contains("Threadbear. Follow"), "a commit that isn't one is left out");
        // Notes for older tags only: dated, and the subjects after.
        let older = Release { notes: vec![Notes { tag: "release/2026-11-08".into(), lines: vec!["x".into()] }], ..release.clone() };
        assert_eq!(release_lines(&older), [(Some("8 Nov".to_string()), vec!["x".to_string()]), (Some("Every change".to_string()), vec!["a".to_string(), "b".to_string(), "c".to_string()])]);
        let idle = Upstream { state: "idle".into(), release: Some(release.clone()), ..Default::default() };
        assert_eq!(upstream_line(Some(&idle)).as_deref(), Some("You're up to date with Mike's release of 10 Nov."));
        let mine = Upstream { state: "off".into(), published_tag: Some("release/2026-11-11".into()), ..Default::default() };
        assert_eq!(upstream_line(Some(&mine)).as_deref(), Some("Your last release for everyone else is release/2026-11-11."));
        let failed = Upstream { state: "failed".into(), reason: Some("no network".into()), ..Default::default() };
        assert_eq!(upstream_line(Some(&failed)).as_deref(), Some("Couldn't check for Mike's releases: no network"));
        assert_eq!(release_date("release/2026-13-01"), "release/2026-13-01");
    }

    #[test]
    fn the_install_command_starts_from_the_checkout() {
        assert_eq!(install_command(Some("/Users/me/dev/threadbear"), "/Users/me"), "~/dev/threadbear/runner/scripts/install-runner.sh");
        assert_eq!(install_command(Some("/opt/threadbear"), "/Users/me"), "/opt/threadbear/runner/scripts/install-runner.sh");
        assert_eq!(install_command(Some("/Users/me2/x"), "/Users/me"), "/Users/me2/x/runner/scripts/install-runner.sh");
        assert_eq!(install_command(None, "/Users/me"), "~/threadbear/runner/scripts/install-runner.sh");
    }
}
