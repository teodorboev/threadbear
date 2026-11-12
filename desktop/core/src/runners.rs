// The Runners panel's words and numbers, a port of RunnersText.swift and
// RunnerStats.swift: which machines are up and for how long, the build each
// one runs, its CPU, memory and disk (`runners.stats`), and the order Mike
// keeps them in (RemoteRunners.swift's `orderedRunners`, ReorderDrag's
// `moving` and `merged`). Mike asked on 5 Nov for the website's runner view
// on the Mac: which machines are up, what they're on and what they're
// running, without going to Settings.
use crate::inbox::{parse_ms, ThreadSummary};
use crate::remote::RunnerInfo;
use serde_json::Value;

/// `RunnersText.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Online,
    /// An old release finishing its threads in a handover.
    Updating,
    Offline,
}

pub fn state(r: &RunnerInfo) -> State {
    if !r.online {
        State::Offline
    } else if r.draining {
        State::Updating
    } else {
        State::Online
    }
}

/// The line under a runner's name: "Online, up 3h", "Updating",
/// "Offline, last seen 2d ago".
pub fn state_line(r: &RunnerInfo, now_ms: i64) -> String {
    match state(r) {
        State::Online => r.started_ms.map_or_else(|| "Online".to_owned(), |at| format!("Online, up {}", span(at, now_ms))),
        State::Updating => "Updating".to_owned(),
        State::Offline => r.last_seen_ms.map_or_else(|| "Offline".to_owned(), |at| format!("Offline, last seen {} ago", span(at, now_ms))),
    }
}

/// The short commit of the release it runs ("c4d90ca" from
/// "20261105T064853Z-c4d90ca"), or its version for one running from a
/// checkout ("0.1.0+source"). None from a runner that doesn't say.
pub fn build(r: &RunnerInfo) -> Option<String> {
    if let Some(release) = r.release.as_deref().filter(|s| !s.is_empty()) {
        // Swift's `split` leaves out empty pieces, so "abc-" is still "abc".
        return Some(release.split('-').filter(|s| !s.is_empty()).last().unwrap_or(release).to_owned());
    }
    r.version.clone()
}

/// The icon's tooltip: "Runners", then how many are up and which aren't.
pub fn tooltip(runners: &[RunnerInfo]) -> String {
    let mut lines = vec!["Runners".to_owned()];
    let names = |want: State| runners.iter().filter(|r| state(r) == want).map(|r| r.name.as_str()).collect::<Vec<_>>();
    let (down, updating) = (names(State::Offline), names(State::Updating));
    if down.is_empty() {
        lines.push(if runners.len() == 2 { "Both online".to_owned() } else { format!("All {} online", runners.len()) });
    } else {
        lines.push(format!("{} of {} online", runners.len() - down.len(), runners.len()));
        lines.push(format!("{} offline", list(&down)));
    }
    if !updating.is_empty() {
        lines.push(format!("{}{}", list(&updating), if updating.len() == 1 { " is updating" } else { " are updating" }));
    }
    lines.join("\n")
}

/// The panel's first line: "All online", or "1 of 2 online" (an updating
/// one counts as up).
pub fn summary(runners: &[RunnerInfo]) -> String {
    let up = runners.iter().filter(|r| state(r) != State::Offline).count();
    if up == runners.len() {
        "All online".to_owned()
    } else {
        format!("{up} of {} online", runners.len())
    }
}

/// The threads running on a runner: its own sidebar rows (`runner_id` None
/// for this Mac's) in the Running section's order.
pub fn running<'a>(r: &RunnerInfo, running: &'a [ThreadSummary]) -> Vec<&'a ThreadSummary> {
    running
        .iter()
        .filter(|t| match t.runner_id.as_deref() {
            None => r.is_self,
            Some(id) => id == r.id,
        })
        .collect()
}

/// What a runner's details say before there are figures to show.
pub fn empty_line(r: &RunnerInfo, asked: bool) -> &'static str {
    if state(r) == State::Offline {
        "Offline, so it isn't sending figures."
    } else if asked {
        "Waiting for its figures. An older runner doesn't send them."
    } else {
        "Asking for its figures…"
    }
}

/// The ⓘ beside the panel's first line: where it comes from, and where
/// runners are set up. `tb pair` is drawn as code, as the Swift popover's
/// Markdown has it.
pub const ABOUT: [&str; 3] = [
    "The machines your threads run on, as your backend last heard from each. A runner that hasn't checked in for 3 minutes shows as offline. Updating means an old release is finishing its threads before handing over.",
    "The code on the right is the commit each one runs. Click a running thread to open it. A runner that isn't paired with this Mac lists its threads, but they open only once you run `tb pair` there.",
    "The star is where new threads start, which you can change in Settings > Runners. A runner is added or renamed on its own machine, so ask an agent to do it.",
];

// ---- A runner's figures (runners.stats)

/// One runner's machine figures (`RunnerMachineStats`): what its last
/// report said, every 10 s while anyone has a runner's details open.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineStats {
    pub runner_id: String,
    /// When it reported them, if it said in a form that reads.
    pub at_ms: Option<i64>,
    pub cpu_percent: f64,
    pub cores: u64,
    /// Bytes, as Activity Monitor's Memory Used counts them on a Mac.
    pub mem_used: f64,
    pub mem_total: f64,
    /// Bytes free and in all on the disk the runner's data is on.
    pub disk_free: f64,
    pub disk_total: f64,
    pub load1: Option<f64>,
    /// How long the machine (not the runner) has been up.
    pub uptime_sec: f64,
    /// Its CPU's name ("Apple M5").
    pub machine: Option<String>,
    /// "macOS 27.2", "Ubuntu 24.04.1 LTS".
    pub os: Option<String>,
}

/// The most each figure can really be (`RunnerMachineStats.Limits`). Another
/// runner writes its own row, and its `cores: 1e20` once stopped the Swift
/// app (Daybreak, 7 Nov 2026), so a row past these is left out.
pub mod limits {
    pub const CORES: f64 = 1_000_000.;
    /// An exabyte, of memory or disk.
    pub const BYTES: f64 = 1e18;
    pub const LOAD: f64 = 1_000_000.;
    /// A thousand years.
    pub const UPTIME_SEC: f64 = 3.2e10;
}

impl MachineStats {
    /// One row of `runners.stats`, read as the Swift decoder does: a figure
    /// that isn't a number counts as 0, and one that's negative, not finite or
    /// past its limit leaves the whole row out (None).
    pub fn decode(v: &Value) -> Option<MachineStats> {
        let runner_id = v["runnerId"].as_str()?.to_owned();
        let number = |key: &str, most: f64| -> Option<f64> {
            match v.get(key).and_then(Value::as_f64) {
                None => Some(0.),
                Some(n) if n.is_finite() && (0. ..=most).contains(&n) => Some(n),
                Some(_) => None,
            }
        };
        let text = |key: &str| v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let cores = number("cores", limits::CORES)?;
        if cores.fract() != 0. {
            return None;
        }
        let load1 = match v.get("load1").and_then(Value::as_f64) {
            Some(_) => Some(number("load1", limits::LOAD)?),
            None => None,
        };
        Some(MachineStats {
            runner_id,
            at_ms: v["at"].as_str().and_then(parse_ms),
            cpu_percent: number("cpuPercent", 100.)?,
            cores: cores as u64,
            mem_used: number("memUsed", limits::BYTES)?,
            mem_total: number("memTotal", limits::BYTES)?,
            disk_free: number("diskFree", limits::BYTES)?,
            disk_total: number("diskTotal", limits::BYTES)?,
            load1,
            uptime_sec: number("uptimeSec", limits::UPTIME_SEC)?,
            machine: text("machine"),
            os: text("os"),
        })
    }
}

/// `runners.stats`' answer, one row a runner (the newest, should a runner
/// appear twice), leaving out rows that don't read.
pub fn decode_stats(value: &Value) -> Vec<MachineStats> {
    let mut out: Vec<MachineStats> = Vec::new();
    for s in value["stats"].as_array().into_iter().flatten().filter_map(MachineStats::decode) {
        match out.iter_mut().find(|o| o.runner_id == s.runner_id) {
            Some(old) if s.at_ms > old.at_ms => *old = s,
            Some(_) => {}
            None => out.push(s),
        }
    }
    out
}

/// One bar in a runner's details: how full, its words, and whether it's
/// worth noticing (the runner's own pressure rules, runner-stats.ts).
#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    pub title: &'static str,
    pub fraction: f64,
    pub text: String,
    pub high: bool,
}

const GIB: f64 = 1024. * 1024. * 1024.;

/// CPU, then memory and disk when the runner said how much there is.
/// Orange only past the runner's own warning lines: 90% memory, under 10 GB
/// of disk, and 90% CPU.
pub fn gauges(s: &MachineStats) -> Vec<Gauge> {
    let mut out = vec![Gauge { title: "CPU", fraction: (s.cpu_percent / 100.).clamp(0., 1.), text: format!("{}%", s.cpu_percent.round() as i64), high: s.cpu_percent >= 90. }];
    if s.mem_total > 0. {
        let used = s.mem_used / s.mem_total;
        out.push(Gauge { title: "Memory", fraction: used.min(1.), text: format!("{} of {}", number(s.mem_used), gb(s.mem_total)), high: used >= 0.9 });
    }
    if s.disk_total > 0. {
        out.push(Gauge { title: "Disk", fraction: (1. - s.disk_free / s.disk_total).clamp(0., 1.), text: format!("{} free", gb(s.disk_free)), high: s.disk_free < 10. * GIB });
    }
    out
}

/// The lines under the bars: what it is, and how busy and long up. "Apple
/// M5, macOS 27.2" and "Machine up 4d, load 2.1 on 10 cores" (the machine,
/// as the state line above says how long the runner has been up).
pub fn machine_lines(s: &MachineStats) -> Vec<String> {
    let mut lines = Vec::new();
    let what: Vec<&str> = [s.machine.as_deref(), s.os.as_deref()].into_iter().flatten().collect();
    if !what.is_empty() {
        lines.push(what.join(", "));
    }
    let mut up = format!("Machine up {}", span_seconds(s.uptime_sec));
    if let Some(load) = s.load1 {
        up.push_str(&format!(", load {load:.1}"));
        if s.cores > 0 {
            up.push_str(&format!(" on {} cores", s.cores));
        }
    }
    lines.push(up);
    lines
}

/// Said when the last report is old enough to doubt: it's sent every 10 s
/// while anyone's looking.
pub fn stale_line(s: &MachineStats, now_ms: i64) -> Option<String> {
    let at = s.at_ms?;
    (now_ms - at > 60_000).then(|| format!("From {} ago", span(at, now_ms)))
}

/// "9.1 GB", "24 GB", "512 GB", "1.8 TB", in the binary units Activity
/// Monitor uses.
pub fn gb(bytes: f64) -> String {
    if bytes / GIB >= 1000. {
        format!("{} TB", number(bytes / 1024.))
    } else {
        format!("{} GB", number(bytes))
    }
}

/// The same without the unit, for "7.4 of 8 GB": a tenth under 100, and no
/// ".0".
pub fn number(bytes: f64) -> String {
    let g = bytes / GIB;
    if g >= 100. {
        return format!("{}", g.round() as i64);
    }
    let tenth = (g * 10.).round() / 10.;
    if tenth == tenth.round() {
        format!("{}", tenth as i64)
    } else {
        format!("{tenth:.1}")
    }
}

/// "12s", "4m", "3h", "2d", as the website says them.
pub fn span(from_ms: i64, to_ms: i64) -> String {
    span_whole(((to_ms - from_ms) as f64 / 1000.).round() as i64)
}

pub fn span_seconds(seconds: f64) -> String {
    span_whole(seconds.round() as i64)
}

fn span_whole(seconds: i64) -> String {
    let s = seconds.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// "bruce", "bruce and m5", "bruce, m5 and cloud".
fn list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

// ---- The order (RemoteRunners.swift)

/// `orderedRunners`: `runners.list` in Mike's order (the phone's and Settings'
/// too), then any he hasn't placed, this Mac first and the rest as the runner
/// has them.
pub fn ordered(runners: &[RunnerInfo], order: Option<&[String]>) -> Vec<RunnerInfo> {
    let order = order.unwrap_or(&[]);
    let rank = |r: &RunnerInfo| order.iter().position(|id| *id == r.id).unwrap_or(order.len() + usize::from(!r.is_self));
    let mut placed: Vec<(usize, &RunnerInfo)> = runners.iter().enumerate().collect();
    placed.sort_by_key(|(i, r)| (rank(r), *i));
    placed.into_iter().map(|(_, r)| r.clone()).collect()
}

/// `orderedRunnerIds`: every runner as Settings › Runners lists them, this
/// Mac's and the backend's others (`remoteRunners`), in Mike's order, then
/// any he hasn't placed.
pub fn ordered_ids(self_id: Option<&str>, others: &[String], order: Option<&[String]>) -> Vec<String> {
    let order = order.unwrap_or(&[]);
    let all: Vec<String> = self_id.map(str::to_owned).into_iter().chain(others.iter().cloned()).collect();
    let rank = |id: &str| order.iter().position(|o| o == id).unwrap_or(order.len());
    let mut placed: Vec<(usize, &String)> = all.iter().enumerate().collect();
    placed.sort_by_key(|(i, id)| (rank(id), *i));
    placed.into_iter().map(|(_, id)| id.clone()).collect()
}

/// `ReorderDrag.moving`: the row at `from` taken out and put back at `to`.
pub fn moving(ids: &[String], from: usize, to: usize) -> Vec<String> {
    if from >= ids.len() || to >= ids.len() || from == to {
        return ids.to_vec();
    }
    let mut next = ids.to_vec();
    let id = next.remove(from);
    next.insert(to, id);
    next
}

/// `ReorderDrag.merged`: a list that's only part of `order`, rearranged.
/// Those ids take the places they had between them, in their new order, and
/// the rest stay put.
pub fn merged(part: &[String], order: &[String]) -> Vec<String> {
    let mut queue = part.iter();
    order.iter().map(|id| if part.contains(id) { queue.next().cloned().unwrap_or_else(|| id.clone()) } else { id.clone() }).collect()
}

/// `setRunnerOrder(part:)`: the panel lists `runners.list`'s runners, which
/// may be only some of the order; they swap places among themselves and the
/// rest of the order stays.
pub fn order_after_drop(part: &[String], self_id: Option<&str>, others: &[String], order: Option<&[String]>) -> Vec<String> {
    let mut full = ordered_ids(self_id, others, order);
    for id in part {
        if !full.contains(id) {
            full.push(id.clone());
        }
    }
    merged(part, &full)
}

// ---- Settings › Runners (RunnersSettings.swift)

/// A runner's row in Settings › Runners (`RunnerRow`): its icon, name, how it
/// is, and the star where new threads start.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsRow {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    /// "This Mac", "Online", "Offline", or with the direct link on and no
    /// token copied in, "Not paired with this Mac".
    pub status: String,
    pub is_default: bool,
    pub is_self: bool,
}

/// The rows: this Mac and the backend's other runners (`remoteRunners`). In
/// Mike's order once it can be changed (`orderedRunnerIds`), else this Mac
/// first and the rest as the backend has them. `default` is this Mac's own
/// choice, None for itself.
pub fn settings_rows(this_mac: Option<&RunnerInfo>, others: &[crate::remote::RemoteRunner], order: Option<&[String]>, default: Option<&str>, orderable: bool, direct_link: bool) -> Vec<SettingsRow> {
    let own = SettingsRow {
        id: this_mac.map(|r| r.id.clone()).unwrap_or_default(),
        // Its own name, as other machines see it ("m5").
        name: this_mac.map_or_else(|| "This Mac".to_owned(), |r| r.name.clone()),
        icon: this_mac.and_then(|r| r.icon.clone()),
        status: "This Mac".to_owned(),
        is_default: default.is_none(),
        is_self: true,
    };
    let row = |r: &crate::remote::RemoteRunner| SettingsRow {
        id: r.id.clone(),
        name: r.name.clone(),
        icon: r.icon.clone(),
        status: if direct_link && r.token_file.is_none() {
            "Not paired with this Mac".to_owned()
        } else if r.online {
            "Online".to_owned()
        } else {
            "Offline".to_owned()
        },
        is_default: default == Some(r.id.as_str()),
        is_self: false,
    };
    if !orderable {
        return std::iter::once(own).chain(others.iter().map(row)).collect();
    }
    let ids: Vec<String> = others.iter().map(|r| r.id.clone()).collect();
    ordered_ids(this_mac.map(|r| r.id.as_str()), &ids, order)
        .into_iter()
        .map(|id| others.iter().find(|r| r.id == id).map_or_else(|| own.clone(), row))
        .collect()
}

/// The order can be changed once the backend has one (API 58) and this Mac's
/// own runner is known: without it a move would save an order that leaves
/// this Mac out (`canOrderRunners`).
pub fn can_order(order_known: bool, others: &[crate::remote::RemoteRunner], all: &[RunnerInfo]) -> bool {
    order_known && !others.is_empty() && all.iter().any(|r| r.is_self)
}

/// The default runner's picker: "This Mac", then each other runner by name.
/// What it says now: the default's name, or "This Mac".
pub fn default_title(default: Option<&str>, others: &[crate::remote::RemoteRunner]) -> String {
    match default {
        None => "This Mac".to_owned(),
        Some(id) => others.iter().find(|r| r.id == id).map_or_else(|| id.to_owned(), |r| r.name.clone()),
    }
}

/// Behind the sections' ⓘs, as the Swift view has them.
pub const DEFAULT_ABOUT: &str = "Where a new thread runs unless you pick another machine on the composer. This is this Mac's own choice.";
pub const DEFAULT_ASK: &str = "change it, or to set up another machine to run threads on.";
pub const DEFAULT_TIP: &str = "Where a new thread runs unless you pick another machine on the composer";
pub const RUNNERS_ASK: &str = "add one, rename one, pair it with this Mac or change how it runs. It knows how, and a Linux server's steps are in INSTALL.md.";
pub const OTHERS_ABOUT: &str = "Threads on other machines open through this Mac's runner and Convex, with no network link between the machines. Tool output stays on the machine that ran it for now. Connecting directly reaches the other machine over Tailscale instead, and needs it paired with this Mac.";
pub const OTHERS_ASK: &str = "explain how other machines' threads reach this Mac, or pair one for a direct connection.";

pub fn runners_about(orderable: bool) -> &'static str {
    if orderable {
        "The machines your threads can run on, in your order. New threads from your phone start on the first one that's online. Drag a runner to change the order. Each runner is set up on its own machine."
    } else {
        "The machines your threads can run on. Each runner is set up on its own machine."
    }
}

/// A row in the dump: "m5 *: This Mac", "bruce: Online", with " *" after the
/// name where new threads start, as the Runners panel's lines have it.
pub fn settings_line(row: &SettingsRow) -> String {
    format!("{}{}: {}", row.name, if row.is_default { " *" } else { "" }, row.status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MIN: i64 = 60_000;

    fn runner(id: &str, name: &str) -> RunnerInfo {
        RunnerInfo { id: id.into(), name: name.into(), online: true, ..Default::default() }
    }

    #[test]
    fn the_state_line_says_how_long_its_been_up_or_gone() {
        let now = 1_000_000 * MIN;
        let mut r = runner("b", "bruce");
        assert_eq!(state_line(&r, now), "Online");
        r.started_ms = Some(now - 16 * MIN - 20_000);
        assert_eq!(state_line(&r, now), "Online, up 16m");
        r.started_ms = Some(now - 26 * 60 * MIN);
        assert_eq!(state_line(&r, now), "Online, up 1d");
        r.draining = true;
        assert_eq!(state(&r), State::Updating);
        assert_eq!(state_line(&r, now), "Updating");
        r.online = false;
        assert_eq!(state_line(&r, now), "Offline");
        r.last_seen_ms = Some(now - 49 * 60 * MIN);
        assert_eq!(state_line(&r, now), "Offline, last seen 2d ago");
        assert_eq!(span(now, now - MIN), "0s", "a time a little ahead is now");
    }

    #[test]
    fn the_build_is_the_releases_commit_or_the_version() {
        let mut r = runner("m", "m5");
        assert_eq!(build(&r), None);
        r.version = Some("0.1.0+source".into());
        assert_eq!(build(&r).as_deref(), Some("0.1.0+source"));
        r.release = Some("20261105T064853Z-c4d90ca".into());
        assert_eq!(build(&r).as_deref(), Some("c4d90ca"));
        r.release = Some("c4d90ca-".into());
        assert_eq!(build(&r).as_deref(), Some("c4d90ca"));
    }

    #[test]
    fn the_icon_says_whos_up() {
        let mut list = vec![runner("m", "m5"), runner("b", "bruce")];
        assert_eq!(tooltip(&list), "Runners\nBoth online");
        assert_eq!(summary(&list), "All online");
        list.push(runner("c", "cloud"));
        assert_eq!(tooltip(&list), "Runners\nAll 3 online");
        list[1].online = false;
        list[2].online = false;
        assert_eq!(tooltip(&list), "Runners\n1 of 3 online\nbruce and cloud offline");
        assert_eq!(summary(&list), "1 of 3 online");
        list[1].online = true;
        list[1].draining = true;
        assert_eq!(tooltip(&list), "Runners\n2 of 3 online\ncloud offline\nbruce is updating");
        assert_eq!(summary(&list), "2 of 3 online", "an updating runner is up");
    }

    #[test]
    fn each_runner_lists_its_own_running_threads() {
        let me = RunnerInfo { is_self: true, ..runner("m", "m5") };
        let bruce = runner("b", "bruce");
        let mine = ThreadSummary { id: "t1".into(), ..Default::default() };
        let theirs = ThreadSummary { id: "t2".into(), runner_id: Some("b".into()), ..Default::default() };
        let rows = vec![mine, theirs];
        assert_eq!(running(&me, &rows).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t1"]);
        assert_eq!(running(&bruce, &rows).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t2"]);
    }

    #[test]
    fn figures_read_as_the_swift_app_reads_them() {
        let gb = 1024f64.powi(3);
        let answer = json!({ "stats": [
            { "runnerId": "m", "at": "2026-11-11T01:00:00.000Z", "cpuPercent": 34.6, "cores": 10, "memUsed": 17.4 * gb, "memTotal": 24.0 * gb, "diskFree": 212.0 * gb, "diskTotal": 994.0 * gb, "load1": 3.42, "uptimeSec": 4.0 * 86400.0 + 3600.0, "machine": "Apple M5", "os": "macOS 27.2" },
            { "runnerId": "b", "at": "2026-11-11T01:00:00.000Z", "cpuPercent": 8, "cores": 8, "memUsed": 7.4 * gb, "memTotal": 8.0 * gb, "diskFree": 9.0 * gb, "diskTotal": 228.0 * gb, "uptimeSec": 19 * 86400 },
            { "runnerId": "x", "cores": 1e20 },
            { "runnerId": "y", "cores": 2.5 },
            { "runnerId": "z", "cpuPercent": -1 },
            { "runnerId": "b", "at": "2026-11-11T00:59:00.000Z", "cpuPercent": 99 },
            { "cpuPercent": 3 },
        ]});
        let stats = decode_stats(&answer);
        assert_eq!(stats.iter().map(|s| s.runner_id.as_str()).collect::<Vec<_>>(), ["m", "b"], "out of range rows go, and the older of two goes");
        let m = &stats[0];
        let g = gauges(m);
        assert_eq!(g.iter().map(|g| (g.title, g.text.as_str(), g.high)).collect::<Vec<_>>(), [("CPU", "35%", false), ("Memory", "17.4 of 24 GB", false), ("Disk", "212 GB free", false)]);
        assert_eq!(machine_lines(m), ["Apple M5, macOS 27.2", "Machine up 4d, load 3.4 on 10 cores"]);
        let b = &stats[1];
        assert_eq!(gauges(b).iter().map(|g| (g.title, g.text.as_str(), g.high)).collect::<Vec<_>>(), [("CPU", "8%", false), ("Memory", "7.4 of 8 GB", true), ("Disk", "9 GB free", true)]);
        assert_eq!(machine_lines(b), ["Machine up 19d"], "no load, no cores");
        let now = parse_ms("2026-11-11T01:00:30.000Z").unwrap();
        assert_eq!(stale_line(m, now), None);
        assert_eq!(stale_line(m, now + 5 * MIN).as_deref(), Some("From 5m ago"));
    }

    #[test]
    fn sizes_in_binary_units() {
        let gb_ = 1024f64.powi(3);
        assert_eq!(gb(9.1 * gb_), "9.1 GB");
        assert_eq!(gb(24. * gb_), "24 GB");
        assert_eq!(gb(512. * gb_), "512 GB");
        assert_eq!(gb(1.8 * 1024. * gb_), "1.8 TB");
        assert_eq!(number(99.96 * gb_), "100");
        assert_eq!(number(7.44 * gb_), "7.4");
        assert_eq!(span_seconds(59.4), "59s");
    }

    #[test]
    fn the_order_puts_mikes_first_then_this_mac() {
        let me = RunnerInfo { is_self: true, ..runner("m", "m5") };
        let list = vec![me, runner("b", "bruce"), runner("c", "cloud")];
        let names = |l: Vec<RunnerInfo>| l.into_iter().map(|r| r.name).collect::<Vec<_>>();
        assert_eq!(names(ordered(&list, None)), ["m5", "bruce", "cloud"]);
        let order = ["c".to_string(), "m".to_string()];
        assert_eq!(names(ordered(&list, Some(&order))), ["cloud", "m5", "bruce"]);
        let others = ["b".to_string(), "c".to_string(), "p".to_string()];
        assert_eq!(ordered_ids(Some("m"), &others, Some(&order)), ["c", "m", "b", "p"]);
    }

    #[test]
    fn settings_lists_this_mac_and_the_others_in_mikes_order() {
        use crate::remote::RemoteRunner;
        let me = RunnerInfo { is_self: true, icon: Some("laptopcomputer".into()), ..runner("m", "m5") };
        let bruce = RemoteRunner { id: "b".into(), name: "bruce".into(), icon: Some("macmini".into()), online: true, token_file: Some("/x/b.token".into()) };
        let cloud = RemoteRunner { id: "c".into(), name: "cloud".into(), online: false, ..Default::default() };
        let others = [bruce.clone(), cloud.clone()];
        let lines = |rows: Vec<SettingsRow>| rows.iter().map(settings_line).collect::<Vec<_>>();
        // Not orderable yet: this Mac first, the rest as the backend has them.
        assert_eq!(lines(settings_rows(Some(&me), &others, None, None, false, false)), ["m5 *: This Mac", "bruce: Online", "cloud: Offline"]);
        // In Mike's order, with the star on another default.
        let order = ["c".to_string(), "m".to_string(), "b".to_string()];
        let rows = settings_rows(Some(&me), &others, Some(&order), Some("b"), true, false);
        assert_eq!(lines(rows.clone()), ["cloud: Offline", "m5: This Mac", "bruce *: Online"]);
        assert_eq!(rows[1].icon.as_deref(), Some("laptopcomputer"));
        assert!(rows[1].is_self && !rows[0].is_self);
        // The direct link needs a token copied in.
        assert_eq!(lines(settings_rows(Some(&me), &others, None, None, false, true))[2], "cloud: Not paired with this Mac");
        // This Mac before runners.list has said its name.
        assert_eq!(settings_rows(None, &[], None, None, false, false)[0].name, "This Mac");
        assert!(can_order(true, &others, &[me.clone()]));
        assert!(!can_order(false, &others, &[me.clone()]), "no order from the backend yet");
        assert!(!can_order(true, &others, &[]), "this Mac's id isn't known yet");
        assert_eq!(default_title(None, &others), "This Mac");
        assert_eq!(default_title(Some("b"), &others), "bruce");
        assert!(runners_about(true).contains("Drag a runner"));
    }

    #[test]
    fn a_drop_moves_one_and_leaves_the_rest() {
        let ids: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(moving(&ids, 0, 2), ["b", "c", "a", "d"]);
        assert_eq!(moving(&ids, 3, 0), ["d", "a", "b", "c"]);
        assert_eq!(moving(&ids, 1, 9), ids, "nowhere to go");
        // The panel shows a, c (b and d aren't in runners.list): swapping
        // them leaves b and d where they were.
        let part = ["c".to_string(), "a".to_string()];
        assert_eq!(merged(&part, &ids), ["c", "b", "a", "d"]);
        let others = ["b".to_string(), "c".to_string()];
        assert_eq!(order_after_drop(&["c".to_string(), "m".to_string()], Some("m"), &others, None), ["c", "b", "m"]);
    }
}
