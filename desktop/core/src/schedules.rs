// The Scheduled page's rules, ported from the Swift app (ScheduleText.swift,
// ScheduledHome.swift's ScheduledStats): the list's headings, what a row says,
// the page's numbers, and the editor's Repeat choices. The model itself is
// `highlights::Schedule`, which the Highlights' Scheduled group shares.
pub use crate::home::HomeStat;
use crate::highlights::{schedule_relative, Schedule};
use crate::composer::{effort_title, model_title};
use crate::logins::{choice_for, effort_levels, implied_effort, picker_rows, resolved_model, Account, ModelChoice, PickerRow, RunnerLogins};
use crate::new_thread::{pool_logins, short_folder};
use chrono::{DateTime, Datelike, TimeZone, Timelike};

/// The list's headings, folded like the inbox's (Mike asked 30 Oct, with 31
/// schedules in one list): what's coming up, what's paused, and one-offs
/// that have run (`ScheduleSection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScheduleSection {
    Active,
    Paused,
    Done,
}

impl ScheduleSection {
    pub const ALL: [ScheduleSection; 3] = [ScheduleSection::Active, ScheduleSection::Paused, ScheduleSection::Done];

    pub fn title(self) -> &'static str {
        match self {
            ScheduleSection::Active => "Active",
            ScheduleSection::Paused => "Paused",
            ScheduleSection::Done => "Done",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ScheduleSection::Active => "active",
            ScheduleSection::Paused => "paused",
            ScheduleSection::Done => "done",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            ScheduleSection::Active => "clock",
            ScheduleSection::Paused => "pause",
            ScheduleSection::Done => "checkmark",
        }
    }

    /// A one-off that has run is done even if it was paused first: it won't
    /// run again either way.
    pub fn of(schedule: &Schedule) -> ScheduleSection {
        if schedule.is_finished() {
            ScheduleSection::Done
        } else if schedule.paused {
            ScheduleSection::Paused
        } else {
            ScheduleSection::Active
        }
    }

    /// The schedules under each heading, in the headings' order, keeping the
    /// runner's order within each; a heading with none isn't there.
    pub fn group(schedules: &[Schedule]) -> Vec<(ScheduleSection, Vec<Schedule>)> {
        ScheduleSection::ALL
            .into_iter()
            .map(|section| (section, schedules.iter().filter(|s| ScheduleSection::of(s) == section).cloned().collect::<Vec<_>>()))
            .filter(|(_, rows)| !rows.is_empty())
            .collect()
    }
}

/// Instead of a time on a row: why it won't run (`ScheduleRowView.state`).
pub fn row_state(schedule: &Schedule) -> Option<&'static str> {
    if schedule.paused {
        Some("Paused")
    } else if schedule.is_finished() {
        Some("Done")
    } else {
        None
    }
}

/// "in 12m", "in 3h", "in 2 days", or "now" for a row's next run.
pub fn row_next(schedule: &Schedule, now_ms: i64) -> Option<String> {
    if schedule.paused || schedule.is_finished() {
        return None;
    }
    schedule.next_ms().map(|at| schedule_relative(at, now_ms))
}

/// The last run, only when it didn't send, and whether it failed
/// (`ScheduleRowView.problem`).
pub fn row_problem(schedule: &Schedule) -> Option<(String, bool)> {
    let last = schedule.last_run.as_ref()?;
    let note = last.note.as_deref().map(|n| format!(": {n}")).unwrap_or_default();
    match last.outcome.as_str() {
        "failed" => Some((format!("Last run failed{note}"), true)),
        "skipped" => Some((format!("Skipped last run{note}"), false)),
        _ => None,
    }
}

fn stat(value: usize, label: &str, help: &str) -> HomeStat {
    HomeStat::new(value, label, help)
}

/// The page's numbers, from the schedules the app already has
/// (`ScheduledStats.tiles`).
pub fn stats(schedules: &[Schedule], now_ms: i64) -> Vec<HomeStat> {
    let active: Vec<&Schedule> = schedules.iter().filter(|s| s.is_active()).collect();
    let day_ahead = active.iter().filter(|s| s.next_ms().is_some_and(|at| at <= now_ms + 86_400_000)).count();
    let repeating = active.iter().filter(|s| !s.is_one_off()).count();
    let by_agents = active.iter().filter(|s| s.created_by.is_some()).count();
    let paused = schedules.iter().filter(|s| s.paused && !s.is_finished()).count();
    let runs = schedules.iter().map(|s| s.runs.max(0) as usize).sum::<usize>();
    vec![
        stat(active.len(), "On", "Schedules that will still run: not paused and not finished"),
        stat(day_ahead, "Due in a day", "On schedules due to run in the next 24 hours"),
        stat(repeating, "Repeating", "On schedules that run over and over, rather than once"),
        stat(by_agents, if by_agents == 1 { "Set by a thread" } else { "Set by threads" }, "On schedules an agent booked, usually to check back on its own thread"),
        stat(paused, "Paused", "Schedules waiting for you to resume them"),
        stat(runs, if runs == 1 { "Run so far" } else { "Runs so far" }, "Times the schedules in the list have sent their message"),
    ]
}

/// The active schedule due soonest (`ScheduledStats.next`).
pub fn next(schedules: &[Schedule]) -> Option<&Schedule> {
    schedules.iter().filter(|s| s.is_active()).filter_map(|s| s.next_ms().map(|at| (s, at))).min_by_key(|(_, at)| *at).map(|(s, _)| s)
}

/// "in 12 min", "at 2:00 pm", "Fri 9:00 am" (`ScheduledStats.when`).
pub fn when<Tz: TimeZone>(schedule: &Schedule, now: &DateTime<Tz>) -> String {
    let Some(at) = schedule.next_ms() else { return schedule.when_text.clone() };
    let seconds = (at - now.timestamp_millis()) / 1000;
    if seconds < 60 {
        return "any moment".into();
    }
    if seconds < 3600 {
        return format!("in {} min", seconds / 60);
    }
    let words = crate::sidebar::until_words(at, now);
    let same_day = now.timezone().timestamp_millis_opt(at).single().is_some_and(|d| d.date_naive() == now.date_naive());
    if same_day {
        format!("at {words}")
    } else {
        words
    }
}

/// Where a schedule sends, as its row and the editor say it
/// (`scheduleTarget`): its thread's title (`thread_title` is None when that
/// thread's gone), or "New thread in misc". `away` is another runner's: its
/// default folder isn't known here.
pub fn target_text(schedule: &Schedule, thread_title: Option<&str>, away: bool, default_cwd: &str) -> String {
    if schedule.thread_id.is_some() {
        return match thread_title {
            None => "A thread that's gone".into(),
            Some("") => "Untitled".into(),
            Some(title) => title.to_owned(),
        };
    }
    if away {
        return schedule.cwd.as_deref().map_or_else(|| "New thread".into(), |cwd| format!("New thread in {}", short_folder(cwd)));
    }
    format!("New thread in {}", short_folder(schedule.cwd.as_deref().unwrap_or(default_cwd)))
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "2:05 pm", with its minutes always (`formatted(date: .omitted, time: .shortened)`).
pub fn time_of_day<Tz: TimeZone>(d: &DateTime<Tz>) -> String {
    let hour = d.hour();
    let twelve = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!("{twelve}:{:02} {}", d.minute(), if hour < 12 { "am" } else { "pm" })
}

/// "11 Nov 2026 at 2:00 pm": a row's next run in full, for its tooltip
/// (`formatted(date: .abbreviated, time: .shortened)`).
pub fn full_time<Tz: TimeZone>(at_ms: i64, tz: &Tz) -> String {
    let Some(d) = tz.timestamp_millis_opt(at_ms).single() else { return String::new() };
    format!("{} {} {} at {}", d.day(), MONTHS[d.month0() as usize], d.year(), time_of_day(&d))
}

/// A schedule's title as the list shows it.
pub fn title(schedule: &Schedule) -> &str {
    if schedule.title.is_empty() {
        "Untitled"
    } else {
        &schedule.title
    }
}

/// The editor's Repeat choices (`RepeatChoice`). Each preset only builds a
/// cron string; the runner works out what it means (`schedules.preview`), so
/// the app never reads cron beyond recognising its own presets when editing.
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatChoice {
    pub kind: RepeatKind,
    pub hour: u32,
    pub minute: u32,
    /// Cron's day of the week: 0 is Sunday, 6 Saturday.
    pub weekday: u32,
    /// Day of the month, 1 to 31 (a month without it is skipped, as in cron).
    pub day: u32,
    /// What's typed in the cron field for Custom.
    pub custom: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatKind {
    Hourly,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Custom,
}

impl RepeatKind {
    pub const ALL: [RepeatKind; 6] = [RepeatKind::Hourly, RepeatKind::Daily, RepeatKind::Weekdays, RepeatKind::Weekly, RepeatKind::Monthly, RepeatKind::Custom];

    pub fn title(self) -> &'static str {
        match self {
            RepeatKind::Hourly => "Every hour",
            RepeatKind::Daily => "Every day",
            RepeatKind::Weekdays => "Weekdays",
            RepeatKind::Weekly => "Every week",
            RepeatKind::Monthly => "Every month",
            RepeatKind::Custom => "Custom",
        }
    }

    /// Its SF Symbol in the editor's Repeat menu (`RepeatChoice.Kind.symbol`).
    pub fn symbol(self) -> &'static str {
        match self {
            RepeatKind::Hourly => "clock",
            RepeatKind::Daily => "sun.max",
            RepeatKind::Weekdays => "briefcase",
            RepeatKind::Weekly => "calendar",
            RepeatKind::Monthly => "calendar.badge.clock",
            RepeatKind::Custom => "slider.horizontal.3",
        }
    }
}

impl Default for RepeatChoice {
    fn default() -> Self {
        RepeatChoice { kind: RepeatKind::Weekdays, hour: 9, minute: 0, weekday: 1, day: 1, custom: String::new() }
    }
}

impl RepeatChoice {
    /// The cron string the choice stands for. Every hour runs on the hour.
    pub fn cron(&self) -> String {
        let (m, h) = (self.minute, self.hour);
        match self.kind {
            RepeatKind::Hourly => "0 * * * *".into(),
            RepeatKind::Daily => format!("{m} {h} * * *"),
            RepeatKind::Weekdays => format!("{m} {h} * * 1-5"),
            RepeatKind::Weekly => format!("{m} {h} * * {}", self.weekday),
            RepeatKind::Monthly => format!("{m} {h} {} * *", self.day),
            RepeatKind::Custom => self.custom.trim().to_owned(),
        }
    }

    /// The preset a schedule's cron came from, so editing it shows the same
    /// choice; anything else (an agent's "*/15 9-17 * * *") is Custom, with
    /// the cron in the field.
    pub fn from_cron(cron: &str) -> RepeatChoice {
        let mut out = RepeatChoice { custom: cron.to_owned(), kind: RepeatKind::Custom, ..RepeatChoice::default() };
        let fields: Vec<&str> = cron.split_whitespace().collect();
        let Some(minute) = fields.first().and_then(|f| f.parse::<u32>().ok()).filter(|m| *m <= 59).filter(|_| fields.len() == 5) else { return out };
        if fields[1..] == ["*", "*", "*", "*"] {
            if minute == 0 {
                out.kind = RepeatKind::Hourly;
            }
            return out;
        }
        let Some(hour) = fields[1].parse::<u32>().ok().filter(|h| *h <= 23).filter(|_| fields[3] == "*") else { return out };
        out.hour = hour;
        out.minute = minute;
        match (fields[2], fields[4]) {
            ("*", "*") => out.kind = RepeatKind::Daily,
            ("*", "1-5") => out.kind = RepeatKind::Weekdays,
            ("*", dow) => {
                if let Some(weekday) = dow.parse::<u32>().ok().filter(|d| *d <= 7) {
                    out.kind = RepeatKind::Weekly;
                    out.weekday = weekday % 7;
                }
            }
            (dom, "*") => {
                if let Some(day) = dom.parse::<u32>().ok().filter(|d| (1..=31).contains(d)) {
                    out.kind = RepeatKind::Monthly;
                    out.day = day;
                }
            }
            _ => {}
        }
        out
    }
}

/// The editor's Model and Effort menus for a schedule that starts a new
/// thread each time: the list of the login it runs on (`ScheduleModels`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleModels {
    /// "claude" or "codex": the login's.
    pub provider: String,
    /// The login the list comes from. None before the runner has said hello.
    pub account: Option<Account>,
    /// The model a thread that names none runs on: the runner's default for
    /// the provider, else the login's.
    pub default_model: Option<String>,
    /// The level "Default" means for the model, when it's known.
    pub default_effort: Option<String>,
    /// The model rows, as the composer's picker lists them.
    pub rows: Vec<PickerRow>,
    /// The levels for the model (or the default model).
    pub effort_levels: Vec<String>,
}

impl ScheduleModels {
    /// `account_id` pins the login; `pool_id` lets the runner pick one each
    /// run, so the list is the login in the pool for the chosen model's
    /// provider (or its first). Neither is what the runner does with a thread
    /// that names none: its default pool, else its default login.
    pub fn new(account_id: Option<&str>, pool_id: Option<&str>, model: Option<&str>, logins: &RunnerLogins) -> ScheduleModels {
        let named = |c: &ModelChoice| model.filter(|m| !m.is_empty()).is_some_and(|m| c.id == m || c.model == m);
        let pool = pool_id.or(if account_id.is_none() { logins.defaults.pool_id.as_deref() } else { None }).and_then(|id| logins.pool(id));
        let account: Option<&Account> = match (account_id.and_then(|id| logins.account(id)), pool) {
            (Some(pinned), _) => Some(pinned),
            (None, Some(pool)) => {
                let members = pool_logins(pool, &logins.accounts, None);
                members.iter().copied().find(|a| a.models.iter().any(named)).or_else(|| members.first().copied())
            }
            (None, None) => logins.account(&logins.defaults.account_id).or_else(|| logins.accounts.first()),
        };
        let provider = account.map_or("claude", Account::provider_id).to_owned();
        let choices: &[ModelChoice] = account.map_or(&[], |a| a.models.as_slice());
        let default_model = resolved_model(&[logins.defaults.model_for(&provider), account.and_then(|a| a.default_model.as_deref())]).map(str::to_owned);
        let effective = resolved_model(&[model, default_model.as_deref()]);
        let default_effort = implied_effort(effective, choices, account, logins.defaults.efforts.get(&provider).map(String::as_str));
        let rows = picker_rows(choices, default_model.as_deref(), model, "", &provider);
        let effort_levels = effort_levels(effective, choices, &provider);
        ScheduleModels { provider, account: account.cloned(), default_model, default_effort, rows, effort_levels }
    }

    /// "Default (Opus 5.5)", or plain "Default" when it isn't known.
    pub fn default_model_title(&self) -> String {
        let Some(default) = self.default_model.as_deref() else { return "Default".into() };
        let title = self.account.as_ref().and_then(|a| choice_for(default, &a.models)).map_or_else(|| model_title(Some(default)), |c| c.title.clone());
        format!("Default ({title})")
    }

    /// "Default (High)", or "Default effort" when it isn't known: beside the
    /// model menu a bare "Default" didn't say what it was.
    pub fn default_effort_title(&self) -> String {
        self.default_effort.as_deref().map_or_else(|| "Default effort".into(), |e| format!("Default ({})", effort_title(e)))
    }
}

/// Where the editor's Once picker starts: the next whole hour at least a
/// quarter of an hour away (`ScheduleTime.defaultOnce`).
/// The header's Scheduled chip on a thread a schedule started
/// (`StartedBySchedule.tip`): the schedule's title as it was when it started
/// the thread, and what it is now if it's still there: when it runs, or that
/// it's paused. It may since have been renamed or deleted.
pub fn started_by_tip(origin_title: &str, current: Option<&Schedule>) -> String {
    let named = format!("Started by a schedule: {}", if origin_title.is_empty() { "Untitled" } else { origin_title });
    let Some(current) = current else { return format!("{named}. It has since been deleted.") };
    let when = if current.paused { "paused" } else { current.when_text.as_str() };
    if when.is_empty() {
        format!("{named}. Click to edit it.")
    } else {
        format!("{named} ({when}). Click to edit it.")
    }
}

pub fn default_once_ms(now_ms: i64) -> i64 {
    let later = now_ms + 15 * 60_000;
    let hour = later - later.rem_euclid(3_600_000);
    if hour < later {
        hour + 3_600_000
    } else {
        hour
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schedule(v: serde_json::Value) -> Schedule {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn the_scheduled_chip_says_when_it_runs_or_what_happened_to_it() {
        let mut live = Schedule { id: "s1".into(), title: "Renamed".into(), when_text: "Weekdays at 9:00".into(), ..Default::default() };
        assert_eq!(started_by_tip("Morning brief", Some(&live)), "Started by a schedule: Morning brief (Weekdays at 9:00). Click to edit it.");
        live.paused = true;
        assert_eq!(started_by_tip("Morning brief", Some(&live)), "Started by a schedule: Morning brief (paused). Click to edit it.");
        assert_eq!(started_by_tip("", None), "Started by a schedule: Untitled. It has since been deleted.");
    }

    #[test]
    fn schedules_fall_under_active_paused_and_done_in_the_runners_order() {
        let list = vec![
            schedule(json!({ "id": "a", "title": "Brief", "cron": "0 9 * * 1-5" })),
            schedule(json!({ "id": "b", "title": "Later", "paused": true })),
            schedule(json!({ "id": "c", "title": "Once", "finishedAt": "2026-11-01T00:00:00Z", "paused": true })),
            schedule(json!({ "id": "d", "title": "" })),
        ];
        let grouped = ScheduleSection::group(&list);
        let groups: Vec<(&str, Vec<&str>)> = grouped.iter().map(|(s, rows)| (s.title(), rows.iter().map(|r| r.id.as_str()).collect())).collect();
        assert_eq!(groups, vec![("Active", vec!["a", "d"]), ("Paused", vec!["b"]), ("Done", vec!["c"])]);
        assert_eq!(title(&list[3]), "Untitled");
    }

    #[test]
    fn where_a_schedule_sends() {
        let to_thread = schedule(json!({ "id": "a", "threadId": "t1" }));
        assert_eq!(target_text(&to_thread, Some("Release notes"), false, "/Users/m/dev/misc"), "Release notes");
        assert_eq!(target_text(&to_thread, Some(""), false, ""), "Untitled");
        assert_eq!(target_text(&to_thread, None, false, ""), "A thread that's gone");
        let new_each = schedule(json!({ "id": "b" }));
        assert_eq!(target_text(&new_each, None, false, "/Users/m/dev/misc"), "New thread in misc");
        assert_eq!(target_text(&new_each, None, true, "/Users/m/dev/misc"), "New thread", "another Mac's default folder isn't known here");
        let in_folder = schedule(json!({ "id": "c", "cwd": "/Volumes/Media/Photos" }));
        assert_eq!(target_text(&in_folder, None, true, ""), "New thread in Photos");
    }

    #[test]
    fn a_new_threads_models_come_from_the_login_it_runs_on() {
        let hello = json!({
            "accounts": [
                { "id": "claude-main", "provider": "claude", "title": "Mike", "defaultModel": "opus",
                  "models": [{ "id": "opus", "title": "Opus 5.5", "effortLevels": ["low", "high"] }, { "id": "haiku", "title": "Haiku 5.5" }] },
                { "id": "codex-main", "provider": "codex", "title": "Codex", "models": [{ "id": "gpt-6", "title": "GPT-6" }] },
            ],
            "defaults": { "accountId": "claude-main", "cwd": "/Users/m/dev/misc", "efforts": { "claude": "high" } },
        });
        let logins = RunnerLogins::from_hello(&hello);
        let models = ScheduleModels::new(None, None, None, &logins);
        assert_eq!(models.provider, "claude");
        assert_eq!(models.default_model_title(), "Default (Opus 5.5)");
        assert_eq!(models.default_effort_title(), "Default (High)");
        assert_eq!(models.effort_levels, vec!["low", "high"]);
        let pinned = ScheduleModels::new(Some("codex-main"), None, None, &logins);
        assert_eq!(pinned.provider, "codex");
        assert!(pinned.rows.iter().any(|r| r.id == "gpt-6"));
    }

    #[test]
    fn times_in_full_and_of_day() {
        let utc = chrono::Utc;
        let at = chrono::DateTime::parse_from_rfc3339("2026-11-11T14:05:00Z").unwrap().timestamp_millis();
        assert_eq!(full_time(at, &utc), "11 Nov 2026 at 2:05 pm");
        let midnight = chrono::DateTime::parse_from_rfc3339("2026-11-11T00:00:00Z").unwrap().with_timezone(&utc);
        assert_eq!(time_of_day(&midnight), "12:00 am");
    }

    #[test]
    fn a_run_that_didnt_send_says_why() {
        let failed = schedule(json!({ "id": "a", "lastRun": { "outcome": "failed", "note": "the thread was deleted" } }));
        assert_eq!(row_problem(&failed), Some(("Last run failed: the thread was deleted".into(), true)));
        let skipped = schedule(json!({ "id": "a", "lastRun": { "outcome": "skipped" } }));
        assert_eq!(row_problem(&skipped), Some(("Skipped last run".into(), false)));
        assert_eq!(row_problem(&schedule(json!({ "id": "a", "lastRun": { "outcome": "sent" } }))), None);
    }

    #[test]
    fn the_pages_numbers() {
        let now = 1_000_000_000_000;
        let soon = chrono::DateTime::from_timestamp_millis(now + 3_600_000).unwrap().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let list = vec![
            schedule(json!({ "id": "a", "cron": "0 9 * * *", "nextRunAt": soon, "runs": 4, "createdBy": "t1" })),
            schedule(json!({ "id": "b", "paused": true, "runs": 1 })),
            schedule(json!({ "id": "c", "finishedAt": "2026-11-01T00:00:00Z", "runs": 1 })),
        ];
        let tiles: Vec<(String, String)> = stats(&list, now).into_iter().map(|t| (t.value, t.label)).collect();
        assert_eq!(
            tiles,
            vec![("1".into(), "On".into()), ("1".into(), "Due in a day".into()), ("1".into(), "Repeating".into()), ("1".into(), "Set by a thread".into()), ("1".into(), "Paused".into()), ("6".into(), "Runs so far".into())]
        );
        assert_eq!(next(&list).map(|s| s.id.as_str()), Some("a"));
    }

    #[test]
    fn repeat_choices_round_trip_their_cron() {
        for cron in ["0 * * * *", "30 9 * * *", "0 9 * * 1-5", "15 18 * * 5", "0 8 1 * *"] {
            assert_eq!(RepeatChoice::from_cron(cron).cron(), cron, "{cron}");
        }
        let agents = RepeatChoice::from_cron("*/15 9-17 * * *");
        assert_eq!(agents.kind, RepeatKind::Custom);
        assert_eq!(agents.cron(), "*/15 9-17 * * *");
        assert_eq!(RepeatChoice::from_cron("0 9 * * 7").weekday, 0, "Sunday as 7 reads as 0");
        assert_eq!(default_once_ms(3_600_000 * 10 + 5 * 60_000), 3_600_000 * 11, "10:05 starts at 11:00");
        assert_eq!(default_once_ms(3_600_000 * 10 + 50 * 60_000), 3_600_000 * 12, "10:50 starts at 12:00, a quarter of an hour away at least");
    }
}
