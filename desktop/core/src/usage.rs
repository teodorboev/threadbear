// The Usage panel's numbers and words, and the honey pot's level, with no UI
// in sight: a port of the Swift app's UsageStats.swift (UsageStatsMath,
// LimitPace, RollingWeek), UsageGroups, HoneyLevel, the parts of
// AccountUsageText the panel reads, LoginColours' palette, and what
// UsagePanel.swift works out for each tile, chart, card, login and meter.
// desktop/gpui/src/usage.rs draws what's here and the automation's dump reads
// the same, so the two can't say different things.
//
// Times are milliseconds since the epoch. Days and hours are the app's own
// time zone's (the Swift app's `Calendar.current`), handed in as `zone` so the
// tests can pin one.
use crate::inbox::parse_ms;
use crate::logins::{provider_title, Account, Pool};
use crate::new_thread::{clock, day_name, login_name, percent, pool_providers, reset_time};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Timelike};
use serde_json::Value;
use std::collections::HashMap;

const HOUR: i64 = 3_600_000;
const DAY: i64 = 86_400_000;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// `AccountUsageText.chipWarning`: under this much room a window's words go
/// orange.
pub const CHIP_WARNING: f64 = 25.;

// ---- What the runner sends ----

/// `UsageWindow`: one limit, "5-hour 89% left, resets at 1 pm".
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// "five_hour", "weekly", "weekly_model", "daily", "monthly", "allowance"
    /// or "other"; kept as it came so a new kind still shows by its title.
    pub kind: String,
    pub title: String,
    /// 0 to 100, may be fractional.
    pub remaining: f64,
    pub resets_at: Option<i64>,
    /// "$132.79 left".
    pub detail: Option<String>,
}

impl Window {
    /// A window with no percentage can't be shown, so it's left out.
    fn decode(v: &Value) -> Option<Window> {
        let remaining = v["remainingPercent"].as_f64()?.clamp(0., 100.);
        let kind = v["kind"].as_str().unwrap_or("other").to_owned();
        let title = v["title"].as_str().map_or_else(|| kind.clone(), str::to_owned);
        Some(Window { kind, title, remaining, resets_at: v["resetsAt"].as_str().and_then(parse_ms), detail: v["detail"].as_str().map(str::to_owned) })
    }

    /// The weekly windows get a chart of the week; the 5-hour one a bar.
    pub fn is_week(&self) -> bool {
        self.kind == "weekly" || self.kind == "weekly_model"
    }
}

/// `AccountUsage`: how much room a login has, from Token Stats' file or the
/// runner's own readings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub windows: Vec<Window>,
    /// When the newest reading in it was taken.
    pub read_at: Option<i64>,
    /// "tokenStats", "live", or "shared".
    pub source: String,
    /// Token Stats has gone quiet and there's no live reading since.
    pub stale: bool,
    /// The provider says this login is at its limit.
    pub blocked: bool,
    /// Token Stats' own note ("64,390 credits · 2 limit resets available").
    pub notice: Option<String>,
}

impl Usage {
    /// Leniently, as the Swift decoder: a field it can't read takes its
    /// default, and a block that isn't an object is no usage at all.
    pub fn decode(v: &Value) -> Option<Usage> {
        v.as_object()?;
        Some(Usage {
            windows: v["windows"].as_array().map(|list| list.iter().filter_map(Window::decode).collect()).unwrap_or_default(),
            read_at: v["readAt"].as_str().and_then(parse_ms),
            source: v["source"].as_str().unwrap_or("tokenStats").to_owned(),
            stale: v["stale"].as_bool().unwrap_or(false),
            blocked: v["blocked"].as_bool().unwrap_or(false),
            notice: v["notice"].as_str().map(str::to_owned),
        })
    }
}

/// A login's usage, if the runner sent any it can read.
pub fn usage(account: &Account) -> Option<Usage> {
    account.usage.as_ref().and_then(Usage::decode)
}

/// One hour of a login's tokens (`TokenHour`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hour {
    pub start: i64,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

impl Hour {
    /// Held at the most an i64 can say: every row can be in range and a
    /// quarter of a million of them not (the Swift app's cross-lab review,
    /// 7 Nov).
    pub fn total(&self) -> i64 {
        self.input.saturating_add(self.output).saturating_add(self.cache_read).saturating_add(self.cache_write)
    }

    fn plus(self, other: &Hour) -> Hour {
        Hour {
            start: self.start,
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            cache_write: self.cache_write.saturating_add(other.cache_write),
        }
    }
}

/// One reading of a window (`LimitSample`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: i64,
    pub remaining: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountTokens {
    pub account_id: String,
    /// Oldest first.
    pub hours: Vec<Hour>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LimitHistory {
    pub account_id: String,
    pub kind: String,
    /// A model's own week's title ("Weekly Fable"); None for the main two.
    pub title: Option<String>,
    /// Oldest first.
    pub samples: Vec<Sample>,
}

/// `UsageStats` (docs/protocol.md, "Usage history"): what the runner has
/// kept about each login's spending, for the panel's numbers and charts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// False until the runner's first read of the transcripts is done.
    pub ready: bool,
    pub tokens: Vec<AccountTokens>,
    pub limits: Vec<LimitHistory>,
}

impl Stats {
    /// `usage.stats`'s answer. A short or odd row is left out rather than
    /// failing the lot, and an entry without its login is skipped.
    pub fn decode(v: &Value) -> Stats {
        let tokens = v["tokens"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let account_id = t["accountId"].as_str()?.to_owned();
                let mut hours: Vec<Hour> = t["hours"].as_array().into_iter().flatten().filter_map(hour_row).collect();
                hours.sort_by_key(|h| h.start);
                Some(AccountTokens { account_id, hours })
            })
            .collect();
        let limits = v["limits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let account_id = l["accountId"].as_str()?.to_owned();
                let kind = l["kind"].as_str()?.to_owned();
                let mut samples: Vec<Sample> = l["samples"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|row| {
                        let row = row.as_array()?;
                        let (at, left) = (row.first()?.as_f64()?, row.get(1)?.as_f64()?);
                        (at.is_finite() && left.is_finite()).then(|| Sample { at: at as i64, remaining: left.clamp(0., 100.) })
                    })
                    .collect();
                samples.sort_by_key(|s| s.at);
                Some(LimitHistory { account_id, kind, title: l["title"].as_str().map(str::to_owned), samples })
            })
            .collect();
        Stats { ready: v["ready"].as_bool().unwrap_or(true), tokens, limits }
    }

    pub fn hours(&self, account: &str) -> &[Hour] {
        self.tokens.iter().find(|t| t.account_id == account).map_or(&[], |t| &t.hours)
    }

    /// A window's readings. A model's own week ("Weekly Fable") is told
    /// apart from the others by its title.
    pub fn samples(&self, account: &str, kind: &str, title: Option<&str>) -> &[Sample] {
        self.limits.iter().find(|l| l.account_id == account && l.kind == kind && (kind != "weekly_model" || l.title.as_deref() == title)).map_or(&[], |l| &l.samples)
    }
}

/// `[startMs, input, output, cacheRead, cacheWrite]`. A count past ten
/// trillion an hour is no real reading, and the sums over a month of them
/// stay well inside an i64.
fn hour_row(row: &Value) -> Option<Hour> {
    let row = row.as_array()?;
    if row.len() < 5 {
        return None;
    }
    let start = row[0].as_f64()?;
    if !start.is_finite() || start.abs() > 8.64e15 {
        return None;
    }
    let count = |i: usize| -> Option<i64> {
        let x = row[i].as_f64()?;
        (x.is_finite() && x >= 0. && x <= 1e13 && x.trunc() == x).then_some(x as i64)
    };
    Some(Hour { start: start as i64, input: count(1)?, output: count(2)?, cache_read: count(3)?, cache_write: count(4)? })
}

// ---- Days and hours in the app's time zone ----

fn local<Tz: TimeZone>(ms: i64, zone: &Tz) -> Option<DateTime<Tz>> {
    zone.timestamp_millis_opt(ms).single()
}

/// A date's first moment: midnight, unless a clock change skips it, when the
/// day starts once the clocks have moved on (`Calendar.startOfDay`).
fn first_moment<Tz: TimeZone>(date: NaiveDate, zone: &Tz) -> Option<i64> {
    let midnight = date.and_time(NaiveTime::MIN);
    (0..=12).find_map(|step| zone.from_local_datetime(&(midnight + Duration::minutes(15 * step))).earliest()).map(|d| d.timestamp_millis())
}

/// `Calendar.startOfDay(for:)`.
pub fn start_of_day<Tz: TimeZone>(ms: i64, zone: &Tz) -> i64 {
    local(ms, zone).and_then(|d| first_moment(d.date_naive(), zone)).unwrap_or(ms)
}

/// The start of the day `days` days after the one `ms` is in (negative for
/// before): `Calendar.date(byAdding: .day, ...)` on a day's start, which keeps
/// to midnight across a clock change, where adding 24 hours wouldn't.
pub fn day_start_after<Tz: TimeZone>(ms: i64, days: i64, zone: &Tz) -> i64 {
    local(ms, zone).and_then(|d| d.date_naive().checked_add_signed(Duration::days(days))).and_then(|date| first_moment(date, zone)).unwrap_or(start_of_day(ms, zone) + days * DAY)
}

/// The start of the hour `ms` is in, on the local clock
/// (`Calendar.dateInterval(of: .hour, for:)`), which a zone half an hour off
/// UTC puts on the half hour.
pub fn hour_start<Tz: TimeZone>(ms: i64, zone: &Tz) -> i64 {
    match local(ms, zone) {
        Some(d) => ms - (i64::from(d.minute()) * 60_000 + i64::from(d.second()) * 1000 + i64::from(d.timestamp_subsec_millis())),
        None => ms - ms.rem_euclid(HOUR),
    }
}

fn same_day<Tz: TimeZone>(a: i64, b: i64, zone: &Tz) -> bool {
    match (local(a, zone), local(b, zone)) {
        (Some(a), Some(b)) => a.date_naive() == b.date_naive(),
        _ => false,
    }
}

/// `AccountUsageText.resetTime`: "now", "3:40 pm", "Thu 1 pm", "12 Nov".
pub fn reset_words<Tz: TimeZone>(at: i64, now: i64, zone: &Tz) -> String {
    match (local(at, zone), local(now, zone)) {
        (Some(at), Some(now)) => reset_time(&at, &now),
        _ => String::new(),
    }
}

/// `AccountUsageText.clock`: "3:40 pm", "1 pm" on the hour.
pub fn clock_words<Tz: TimeZone>(at: i64, zone: &Tz) -> String {
    local(at, zone).map(|d| clock(&d)).unwrap_or_default()
}

/// `AccountUsageText.when`: "6:02 am" today, else "Mon 6:02 am".
pub fn when<Tz: TimeZone>(at: i64, now: i64, zone: &Tz) -> String {
    match (local(at, zone), local(now, zone)) {
        (Some(d), Some(n)) if d.date_naive() == n.date_naive() => clock(&d),
        (Some(d), Some(n)) => format!("{} {}", day_name(&d, &n), clock(&d)),
        _ => String::new(),
    }
}

fn weekday<Tz: TimeZone>(at: i64, zone: &Tz) -> &'static str {
    local(at, zone).map_or("", |d| WEEKDAYS[d.weekday().num_days_from_sunday() as usize])
}

// ---- Sums (UsageStatsMath) ----

/// Tokens the logins spent in [from, to), by whole hours: an hour counts if
/// it starts in the range.
pub fn total(stats: &Stats, accounts: &[impl AsRef<str>], from: i64, to: i64) -> Hour {
    let mut sum = Hour { start: from, ..Default::default() };
    for id in accounts {
        for hour in stats.hours(id.as_ref()) {
            if hour.start >= from && hour.start < to {
                sum = sum.plus(hour);
            }
        }
    }
    sum
}

/// Today so far.
pub fn today<Tz: TimeZone>(stats: &Stats, accounts: &[impl AsRef<str>], now: i64, zone: &Tz) -> Hour {
    total(stats, accounts, start_of_day(now, zone), now + 1000)
}

/// The last 60 minutes, as near as hourly buckets allow: this hour so far,
/// plus the part of the last hour that's still inside the 60.
pub fn last_hour<Tz: TimeZone>(stats: &Stats, accounts: &[impl AsRef<str>], now: i64, zone: &Tz) -> i64 {
    let this_hour = hour_start(now, zone);
    let elapsed = (now - this_hour) as f64 / HOUR as f64;
    let current = total(stats, accounts, this_hour, this_hour + HOUR).total();
    let before = total(stats, accounts, this_hour - HOUR, this_hour).total();
    current.saturating_add((before as f64 * (1. - elapsed)).round() as i64)
}

/// What a usual day had spent by this time of day: the average of the
/// previous `days` days from midnight to the same clock time. None when the
/// stats don't go back that far.
pub fn usual_by_now<Tz: TimeZone>(stats: &Stats, accounts: &[impl AsRef<str>], now: i64, days: i64, zone: &Tz) -> Option<i64> {
    let earliest = accounts.iter().filter_map(|id| stats.hours(id.as_ref()).first().map(|h| h.start)).min()?;
    let today = start_of_day(now, zone);
    if earliest >= day_start_after(today, -(days - 1), zone) {
        return None;
    }
    let since_midnight = now - today;
    let mut sum: i64 = 0;
    for back in 1..=days {
        let day = day_start_after(today, -back, zone);
        sum = sum.saturating_add(total(stats, accounts, day, day + since_midnight + 1000).total());
    }
    Some(sum / days)
}

/// One bar of a chart: a login's tokens in an hour or a day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub start: i64,
    pub account_id: String,
    pub tokens: i64,
}

/// Today's hours from midnight to now, one bar per login per hour, zeros
/// left out.
pub fn today_by_hour<Tz: TimeZone>(stats: &Stats, accounts: &[impl AsRef<str>], now: i64, zone: &Tz) -> Vec<Bar> {
    let start = start_of_day(now, zone);
    let mut bars = Vec::new();
    for id in accounts {
        for hour in stats.hours(id.as_ref()) {
            if hour.start >= start && hour.start <= now && hour.total() > 0 {
                bars.push(Bar { start: hour_start(hour.start, zone), account_id: id.as_ref().to_owned(), tokens: hour.total() });
            }
        }
    }
    merge(bars)
}

/// The last `days` days, today included, one bar per login per day.
pub fn by_day<Tz: TimeZone>(stats: &Stats, accounts: &[impl AsRef<str>], days: i64, now: i64, zone: &Tz) -> Vec<Bar> {
    let first = day_start_after(now, -(days - 1), zone);
    let mut bars = Vec::new();
    for id in accounts {
        for hour in stats.hours(id.as_ref()) {
            if hour.start >= first && hour.total() > 0 {
                bars.push(Bar { start: start_of_day(hour.start, zone), account_id: id.as_ref().to_owned(), tokens: hour.total() });
            }
        }
    }
    merge(bars)
}

/// One bar per login per slot, in time order and then by login id, which is
/// the order the Swift chart stacks them in (the first at the bottom).
fn merge(bars: Vec<Bar>) -> Vec<Bar> {
    let mut out: Vec<Bar> = Vec::new();
    let mut at: HashMap<(i64, String), usize> = HashMap::new();
    for bar in bars {
        match at.get(&(bar.start, bar.account_id.clone())) {
            Some(&i) => out[i].tokens = out[i].tokens.saturating_add(bar.tokens),
            None => {
                at.insert((bar.start, bar.account_id.clone()), out.len());
                out.push(bar);
            }
        }
    }
    out.sort_by(|a, b| (a.start, &a.account_id).cmp(&(b.start, &b.account_id)));
    out
}

// ---- Words ----

/// "4.2M", "380k", "12.4k", "912".
pub fn tokens(n: i64) -> String {
    let value = n as f64;
    let short = |v: f64, suffix: &str| {
        let text = if v < 10. { format!("{v:.1}") } else { format!("{}", v.round() as i64) };
        format!("{}{suffix}", text.strip_suffix(".0").unwrap_or(&text))
    };
    if value >= 1e9 {
        short(value / 1e9, "B")
    } else if value >= 1e6 {
        short(value / 1e6, "M")
    } else if value >= 1e3 {
        short(value / 1e3, "k")
    } else {
        n.to_string()
    }
}

/// The tooltip's breakdown: "4.2M tokens: 3.9M cache reads, 210k cache
/// writes, 40k input, 52k output".
pub fn breakdown(sum: &Hour) -> String {
    let parts: Vec<String> = [(sum.cache_read, "cache reads"), (sum.cache_write, "cache writes"), (sum.input, "input"), (sum.output, "output")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, name)| format!("{} {name}", tokens(n)))
        .collect();
    let head = format!("{} tokens", tokens(sum.total()));
    if parts.is_empty() {
        head
    } else {
        format!("{head}: {}", parts.join(", "))
    }
}

// ---- Pace (LimitPace) ----

/// How fast a window is going down, and whether it lasts until it resets.
/// Worked out from the window alone (what's gone since it started, over the
/// time since), or from the readings over the last day when there are
/// enough of them, as the week's average hides a busy day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// Percentage points a day (weekly) or an hour (5-hour) at this rate.
    pub rate: f64,
    /// When it runs out at this rate, if that's before the reset.
    pub runs_out_at: Option<i64>,
    /// What's left when it resets, if it lasts.
    pub left_at_reset: Option<f64>,
}

impl Pace {
    pub fn of(window: &Window, samples: &[Sample], now: i64) -> Option<Pace> {
        let reset = window.resets_at.filter(|r| *r > now)?;
        let span = match window.kind.as_str() {
            "five_hour" => 5 * HOUR,
            // A model's own week ("Weekly Fable") goes like the main one.
            "weekly" | "weekly_model" => 7 * DAY,
            _ => return None,
        };
        let week = span > DAY;
        let start = reset - span;
        let left = window.remaining;
        let hours = |ms: i64| ms as f64 / HOUR as f64;
        let mut per_hour = None;
        if week {
            // The readings of the last day, within this window: the oldest
            // one to now, with at least half a day of them.
            let since = start.max(now - DAY);
            if let Some(oldest) = samples.iter().find(|s| s.at >= since && s.at < now) {
                if now - oldest.at >= 12 * HOUR {
                    per_hour = Some((oldest.remaining - left).max(0.) / hours(now - oldest.at));
                }
            }
        }
        let per_hour = match per_hour {
            Some(p) => p,
            None => {
                let elapsed = hours(now - start);
                // Too early in the window to say anything.
                if elapsed < if week { 2. } else { 0.25 } {
                    return None;
                }
                (100. - left).max(0.) / elapsed
            }
        };
        let mut pace = Pace { rate: per_hour * if week { 24. } else { 1. }, runs_out_at: None, left_at_reset: None };
        let to_reset = hours(reset - now);
        if per_hour > 0. && left / per_hour < to_reset {
            pace.runs_out_at = Some(now + (left / per_hour * HOUR as f64) as i64);
        } else {
            pace.left_at_reset = Some((left - per_hour * to_reset).max(0.));
        }
        Some(pace)
    }

    /// "9% a day", "12% an hour".
    pub fn rate_text(&self, kind: &str) -> String {
        let value = if self.rate < 1. { format!("{:.1}", self.rate) } else { format!("{}", self.rate.round() as i64) };
        format!("{value}{}", if kind == "five_hour" { "% an hour" } else { "% a day" })
    }
}

/// How much of a weekly window left unused at the reset is worth pointing
/// at. Under this it's just about right.
pub const SPARE_WORTH_SAYING: f64 = 15.;

/// What a verdict's words are about, for their colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Used up now.
    Out,
    /// Runs out before it resets: spend it more slowly, or elsewhere.
    Short,
    /// A good part of it will go unused: lean on this one.
    Spare,
    /// Nothing to do.
    Fine,
}

/// What to do about a window, in one line under its meter (Mike, 5 Nov: the
/// graphs are good but he wants something to act on when he looks).
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub text: String,
    pub tone: Tone,
}

/// `LimitPace.verdict`: used up, runs out early, will go unused, on track, or
/// when it resets.
pub fn verdict<Tz: TimeZone>(window: &Window, pace: Option<&Pace>, stale: bool, now: i64, zone: &Tz) -> Option<Verdict> {
    let reset = window.resets_at.map(|at| reset_words(at, now, zone));
    if window.remaining < 1. {
        if let Some(reset) = &reset {
            return Some(Verdict { text: format!("Used up, back {reset}"), tone: Tone::Out });
        }
    }
    // Old numbers can't say how fast it's going now.
    if let (Some(pace), false) = (pace, stale) {
        if let Some(runs_out) = pace.runs_out_at {
            let mut text = format!("Runs out {}", reset_words(runs_out, now, zone));
            if let Some(resets_at) = window.resets_at {
                text.push_str(&format!(", {} before it resets", gap((resets_at - runs_out) as f64 / 1000.)));
            }
            return Some(Verdict { text, tone: Tone::Short });
        }
        // Only the week: a 5-hour window nearly always has room left over,
        // so saying so would be on every row and mean nothing.
        if let (true, Some(left), Some(reset)) = (window.kind == "weekly", pace.left_at_reset, &reset) {
            if left >= SPARE_WORTH_SAYING {
                return Some(Verdict { text: format!("About {} will go unused, resets {reset}", percent(left)), tone: Tone::Spare });
            }
            return Some(Verdict { text: format!("On track, lasts till {reset}"), tone: Tone::Fine });
        }
    }
    reset.map(|reset| Verdict { text: format!("Resets {reset}"), tone: Tone::Fine })
}

/// "2 days", "1 day", "5 hours", "1 hour", "40 min": roughly how long, as
/// nobody plans around "1 day 7 hours".
pub fn gap(seconds: f64) -> String {
    let hours = seconds / 3600.;
    if hours >= 36. {
        format!("{} days", (hours / 24.).round() as i64)
    } else if hours >= 24. {
        "1 day".to_owned()
    } else if hours >= 1.5 {
        format!("{} hours", hours.round() as i64)
    } else if hours >= 1. {
        "1 hour".to_owned()
    } else {
        format!("{} min", ((seconds / 60.).round() as i64).max(1))
    }
}

// ---- The weekly charts' line (RollingWeek) ----

/// The rolling week the weekly charts show.
pub const WEEK: i64 = 7 * DAY;
/// A reading this many points over the one before it is a reset. Smaller
/// rises are a reading being revised, and stay joined.
pub const RESET_RISE: f64 = 5.;

/// The readings from a week ago to now, ending on what's left now, cut into
/// runs at each reset, so a jump back to full isn't drawn as a slope across
/// the hours between two readings. `window_start` is when the current week
/// began (its reset less a week), if known: the line is full there by
/// definition, whatever the readings around it say. A week that hasn't
/// started since its reset still shows the week before (Mike, 11 Nov).
pub fn runs(samples: &[Sample], current: f64, window_start: Option<i64>, now: i64) -> Vec<Vec<Sample>> {
    let from = now - WEEK;
    let mut points: Vec<Sample> = samples.iter().filter(|s| s.at >= from && s.at < now).copied().collect();
    points.sort_by_key(|s| s.at);
    let mut break_at = None;
    if let Some(start) = window_start.filter(|s| *s > from && *s < now) {
        let before = points.iter().filter(|s| s.at < start).copied();
        let after = points.iter().filter(|s| s.at > start).copied();
        points = before.chain(std::iter::once(Sample { at: start, remaining: 100. })).chain(after).collect();
        break_at = Some(start);
    }
    points.push(Sample { at: now, remaining: current });
    let mut out: Vec<Vec<Sample>> = Vec::new();
    for point in points {
        match out.last_mut() {
            Some(run) if Some(point.at) != break_at && run.last().is_some_and(|last| point.remaining - last.remaining < RESET_RISE) => run.push(point),
            _ => out.push(vec![point]),
        }
    }
    out
}

/// Each midnight from `from` to `to`, for the charts' day lines.
pub fn midnights<Tz: TimeZone>(from: i64, to: i64, zone: &Tz) -> Vec<i64> {
    let mut day = start_of_day(from, zone);
    if day < from {
        day = day_start_after(day, 1, zone);
    }
    let mut out = Vec::new();
    while day <= to {
        out.push(day);
        let next = day_start_after(day, 1, zone);
        if next <= day {
            break;
        }
        day = next;
    }
    out
}

// ---- Windows in words (AccountUsageText) ----

/// The 5-hour and weekly windows, the two that decide whether there's room.
/// Codex Pro Lite and Claude Max (since the end of Oct 2026) have only the
/// weekly one.
pub fn main_windows(usage: &Usage) -> Vec<&Window> {
    ["five_hour", "weekly"].iter().filter_map(|kind| usage.windows.iter().find(|w| w.kind == *kind)).collect()
}

/// Everything else (a model's own weekly limit, an allowance, Codex's other
/// buckets), each once.
pub fn extra_windows(usage: &Usage) -> Vec<&Window> {
    let main: Vec<&str> = main_windows(usage).iter().map(|w| w.kind.as_str()).collect();
    let mut seen: Vec<(String, String)> = Vec::new();
    usage
        .windows
        .iter()
        .filter(|w| {
            if main.contains(&w.kind.as_str()) || seen.iter().any(|(k, t)| *k == w.kind && *t == w.title) {
                return false;
            }
            seen.push((w.kind.clone(), w.title.clone()));
            true
        })
        .collect()
}

/// A model's own weeks ("Weekly Fable"), each once. The panel gives them
/// meters of their own under the main two, as Claude Code's /usage does
/// (Ayush, 6 Nov). They never decide the room: a login out of Fable can still
/// run other models.
pub fn model_weeks(usage: &Usage) -> Vec<&Window> {
    extra_windows(usage).into_iter().filter(|w| w.kind == "weekly_model").collect()
}

/// The tighter of the main windows, the one that runs out first.
pub fn tightest(usage: &Usage) -> Option<&Window> {
    main_windows(usage).into_iter().min_by(|a, b| a.remaining.partial_cmp(&b.remaining).unwrap_or(std::cmp::Ordering::Equal))
}

/// The room left: the tighter of the 5-hour and weekly windows.
pub fn room(usage: &Usage) -> Option<f64> {
    tightest(usage).map(|w| w.remaining)
}

/// One window in words: "89% left, resets 3:40 pm". The window's detail
/// ("64,390 credits") is left out when `notice` already says it.
pub fn window_line<Tz: TimeZone>(window: &Window, now: i64, zone: &Tz, notice: Option<&str>) -> String {
    let mut line = format!("{} left", percent(window.remaining));
    if let Some(detail) = window.detail.as_deref().filter(|d| !d.is_empty() && !notice.unwrap_or("").contains(*d)) {
        line.push_str(&format!(" ({detail})"));
    }
    if let Some(reset) = window.resets_at {
        line.push_str(&format!(", resets {}", reset_words(reset, now, zone)));
    }
    line
}

/// Where the numbers came from: "Usage from Token Stats at 7:15 am", or
/// "Token Stats hasn't updated since 6:02 am" when every reading from it is
/// stale. None when no login has a reading from Token Stats.
pub fn source_line<Tz: TimeZone>(accounts: &[Account], now: i64, zone: &Tz) -> Option<String> {
    let from_token_stats: Vec<Usage> = accounts.iter().filter_map(usage).filter(|u| u.source == "tokenStats").collect();
    let newest = from_token_stats.iter().filter_map(|u| u.read_at).max()?;
    let day = when(newest, now, zone);
    if from_token_stats.iter().all(|u| u.stale) {
        Some(format!("Token Stats hasn't updated since {day}"))
    } else {
        Some(format!("Usage from Token Stats at {day}"))
    }
}

/// The quiet lines under a login, after its meters. A bare percentage
/// (Codex's other buckets) only when it's tighter than the main windows,
/// since otherwise it never decides anything; any other window by its detail
/// ("$250 allowance: $132.79 left"); then the login's own part of Token
/// Stats' notice. A model's own week has a meter of its own.
pub fn extra_lines<Tz: TimeZone>(usage: &Usage, notice: Option<&str>, now: i64, zone: &Tz) -> Vec<String> {
    let room = room(usage).unwrap_or(100.);
    // The panel shows the main windows' resets just above these lines.
    let main_resets: Vec<i64> = main_windows(usage).iter().filter_map(|w| w.resets_at).collect();
    let mut lines = Vec::new();
    let mut money = Vec::new();
    for window in extra_windows(usage).into_iter().filter(|w| w.kind != "weekly_model") {
        let detail = window.detail.as_deref().unwrap_or("");
        if detail.is_empty() && window.kind != "allowance" {
            if window.remaining >= room {
                continue;
            }
            let mut line = format!("{} {} left", window.title, percent(window.remaining));
            if let Some(reset) = window.resets_at.filter(|r| !main_resets.iter().any(|m| (m - r).abs() < 60_000)) {
                line.push_str(&format!(", resets {}", reset_words(reset, now, zone)));
            }
            lines.push(line);
        } else if !detail.is_empty() {
            if !notice.unwrap_or("").contains(detail) {
                money.push(format!("{}: {detail}", window.title));
            }
        } else {
            money.push(format!("{}: {} left", window.title, percent(window.remaining)));
        }
    }
    if let Some(notice) = notice.filter(|n| !n.is_empty()) {
        money.insert(0, notice.to_owned());
    }
    if !money.is_empty() {
        lines.push(money.join(" · "));
    }
    lines
}

/// Token Stats' notices, with what several logins say said once: "Couldn't
/// check usage just now, so these are from 1:19 pm." under each of four
/// logins read as four problems. A notice is split on " · "; a part two or
/// more logins have goes in the first, in the order it first comes, and the
/// map keeps each login's other parts ("$48.85 extra usage spent").
pub fn notices(accounts: &[Account]) -> (Option<String>, HashMap<String, String>) {
    let trim = |s: &str| s.trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r').to_owned();
    let mut parts: Vec<(String, Vec<String>)> = Vec::new();
    for account in accounts {
        let Some(notice) = usage(account).and_then(|u| u.notice).map(|n| trim(&n)).filter(|n| !n.is_empty()) else { continue };
        parts.push((account.id.clone(), notice.split(" · ").map(trim).filter(|p| !p.is_empty()).collect()));
    }
    let mut common: Vec<String> = Vec::new();
    for (_, own) in &parts {
        for part in own {
            if !common.contains(part) && parts.iter().filter(|(_, p)| p.contains(part)).count() > 1 {
                common.push(part.clone());
            }
        }
    }
    let mut own = HashMap::new();
    for (id, list) in &parts {
        let rest: Vec<&str> = list.iter().filter(|p| !common.contains(p)).map(String::as_str).collect();
        if !rest.is_empty() {
            own.insert(id.clone(), rest.join(" · "));
        }
    }
    ((!common.is_empty()).then(|| common.join(" · ")), own)
}

// ---- Pools (UsageGroups) ----

/// The id of the group for logins in no pool.
pub const UNPOOLED: &str = "threadbear.unpooled";

/// One card: a pool, or a provider's logins on a runner without pools.
#[derive(Clone, Debug)]
pub struct Group<'a> {
    pub id: String,
    pub title: String,
    /// The pool, for its icon. None for a provider's group and for "Not in a
    /// pool".
    pub pool: Option<&'a Pool>,
    pub providers: Vec<ProviderLogins<'a>>,
}

impl Group<'_> {
    pub fn accounts(&self) -> Vec<&Account> {
        self.providers.iter().flat_map(|p| p.accounts.iter().copied()).collect()
    }
}

/// One provider's logins within a card.
#[derive(Clone, Debug)]
pub struct ProviderLogins<'a> {
    pub provider: String,
    pub accounts: Vec<&'a Account>,
}

/// How the panel groups the logins: by pool, as threads run on pools (Mike,
/// 28 Oct: "pools and then the providers within them"), each pool's logins
/// split by provider, as a Claude and a Codex login have different windows.
/// Logins in no pool get a card of their own at the end. A runner without
/// pools gets one card per provider.
pub fn groups<'a>(pools: &'a [Pool], accounts: &'a [Account]) -> Vec<Group<'a>> {
    if pools.is_empty() {
        return by_provider(accounts.iter().collect(), false).into_iter().map(|p| Group { id: p.provider.clone(), title: provider_title(&p.provider), pool: None, providers: vec![p] }).collect();
    }
    let mut out: Vec<Group<'a>> = pools
        .iter()
        .filter_map(|pool| {
            // In the pool's order: it decides which provider comes first.
            let members: Vec<&Account> = pool.account_ids.iter().filter_map(|id| accounts.iter().find(|a| a.id == *id)).collect();
            (!members.is_empty()).then(|| Group { id: pool.id.clone(), title: pool.title.clone(), pool: Some(pool), providers: by_provider(members, true) })
        })
        .collect();
    let rest: Vec<&Account> = accounts.iter().filter(|a| !pools.iter().any(|p| p.account_ids.contains(&a.id))).collect();
    if !rest.is_empty() {
        out.push(Group { id: UNPOOLED.to_owned(), title: "Not in a pool".to_owned(), pool: None, providers: by_provider(rest, false) });
    }
    out
}

/// Claude, then Codex, then any other; or, with `keep_order`, in the order
/// the providers first turn up.
pub fn by_provider(accounts: Vec<&Account>, keep_order: bool) -> Vec<ProviderLogins<'_>> {
    let mut order: Vec<String> = Vec::new();
    for a in &accounts {
        if !order.iter().any(|p| p == a.provider_id()) {
            order.push(a.provider_id().to_owned());
        }
    }
    if !keep_order {
        let rank = |p: &str| match p {
            "claude" => 0,
            "codex" => 1,
            _ => 2,
        };
        let mut indexed: Vec<(usize, String)> = order.into_iter().enumerate().collect();
        indexed.sort_by_key(|(i, p)| (rank(p), *i));
        order = indexed.into_iter().map(|(_, p)| p).collect();
    }
    order.into_iter().map(|provider| ProviderLogins { accounts: accounts.iter().copied().filter(|a| a.provider_id() == provider).collect(), provider }).collect()
}

/// `Pools.symbol`: the pool's icon, the one Mike picked, else its first
/// provider's (Claude's sparkle, Codex's brackets), else a stack.
pub fn pool_symbol(pool: &Pool, accounts: &[Account]) -> String {
    if let Some(icon) = pool.icon.as_deref().filter(|i| !i.is_empty()) {
        return icon.to_owned();
    }
    match pool_providers(pool, accounts).first().map(String::as_str) {
        Some("claude") => "sparkle".to_owned(),
        Some("codex") => "chevron.left.forwardslash.chevron.right".to_owned(),
        Some(_) => "cpu".to_owned(),
        None => "square.stack".to_owned(),
    }
}

/// The Usage icon's tooltip: one line per card with the most room any of its
/// logins has (what its next thread would get), or when it's back if every
/// login is at its limit. The panel has the rest (Mike, 28 Oct).
pub fn icon_tooltip<Tz: TimeZone>(groups: &[Group], now: i64, zone: &Tz) -> String {
    std::iter::once("Usage".to_owned()).chain(groups.iter().filter_map(|g| tooltip_line(g, now, zone))).collect::<Vec<_>>().join("\n")
}

fn tooltip_line<Tz: TimeZone>(group: &Group, now: i64, zone: &Tz) -> Option<String> {
    let known: Vec<Usage> = group.accounts().into_iter().filter_map(usage).filter(|u| room(u).is_some() || u.blocked).collect();
    if known.is_empty() {
        return None;
    }
    let best = known.iter().filter(|u| !u.blocked).filter_map(room).fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))));
    if let Some(best) = best {
        return Some(format!("{}: {} left", group.title, percent(best)));
    }
    // All at their limit: the soonest any of them comes back.
    let back = known.iter().filter_map(|u| tightest(u).and_then(|w| w.resets_at)).filter(|at| *at > now).min();
    Some(match back {
        None => format!("{}: at its limit", group.title),
        Some(back) => format!("{}: at its limit, back {}{}", group.title, if same_day(back, now, zone) { "at " } else { "" }, reset_words(back, now, zone)),
    })
}

// ---- The honey pot (HoneyLevel) ----

/// How full the honey pot is, 0 to 1: the room left on the login the next
/// new thread would use, the tighter of its 5-hour and weekly windows.
/// Unknown or at its limit is an empty pot; the tooltip and the panel say
/// the rest.
pub fn honey_level(usage: Option<&Usage>) -> f64 {
    match usage {
        Some(u) if !u.blocked => room(u).map_or(0., |r| (r / 100.).clamp(0., 1.)),
        _ => 0.,
    }
}

/// The same login's window has reset, rather than the pot just changing
/// hands to a login with more room, or room coming back a bit at a time as
/// an old 5-hour window slides off. Worth the rise and the drip.
pub fn refilled(old: f64, new: f64, same_login: bool) -> bool {
    same_login && new >= 0.95 && new - old >= 0.1
}

// ---- Colours (LoginColours) ----

/// The reference categorical palette, light and dark, as 0xRRGGBB.
const LIGHT: [u32; 8] = [0x2A78D6, 0xEB6834, 0x1BAF7A, 0xEDA100, 0xE87BA4, 0x008300, 0x6250D6, 0xE34948];
const DARK: [u32; 8] = [0x3987E5, 0xD95926, 0x199E70, 0xC98500, 0xD55181, 0x008300, 0x9085E9, 0xE66767];

/// A login's colour in the charts: the palette's slots in the logins' own
/// order, so a login keeps its colour whichever card or chart it's in. A
/// ninth login gets None (grey) rather than repeating a colour. Text never
/// wears them; a dot beside a name does.
pub fn login_colour(account: &str, accounts: &[Account], dark: bool) -> Option<u32> {
    let slot = accounts.iter().position(|a| a.id == account)?;
    (if dark { DARK } else { LIGHT }).get(slot).copied()
}

// ---- What the panel shows ----

/// A number with its name over it and a quiet line under it (`StatTile`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub title: &'static str,
    pub value: String,
    /// Shown only when it fits on its line; the tip says it either way.
    pub detail: Option<String>,
    pub tip: String,
}

/// Today's tokens, the last hour's and the week's (`SpendSection`). One
/// short line under each number, the sentence in its tip: two lines of
/// "Usually 120M by n..." in the docked panel read as clutter (Mike, 2 Nov).
pub fn tiles<Tz: TimeZone>(stats: &Stats, accounts: &[Account], now: i64, zone: &Tz) -> Vec<Tile> {
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    let today_sum = today(stats, &ids, now, zone);
    let last = last_hour(stats, &ids, now, zone);
    let usual = usual_by_now(stats, &ids, now, 7, zone);
    let week = total(stats, &ids, day_start_after(now, -6, zone), now + 1000);
    let hours_today = ((now - start_of_day(now, zone)) as f64 / HOUR as f64).max(1.);
    let per_hour = tokens((today_sum.total() as f64 / hours_today) as i64);
    vec![
        Tile {
            title: "Today",
            value: tokens(today_sum.total()),
            detail: usual.map(|u| format!("usually {}", tokens(u))),
            tip: format!("{}{}", usual.map(|u| format!("Usually {} by this time of day. ", tokens(u))).unwrap_or_default(), breakdown(&today_sum)),
        },
        Tile {
            title: "Last hour",
            value: tokens(last),
            detail: Some(format!("{per_hour}/h today")),
            tip: format!("Tokens in the last 60 minutes, across every login. Today has averaged {per_hour} an hour."),
        },
        Tile { title: "7 days", value: tokens(week.total()), detail: Some(format!("{} a day", tokens(week.total() / 7))), tip: breakdown(&week) },
    ]
}

/// Which the chart shows: today by the hour, or the last 7 days by the day.
/// Remembered as the Swift app's `usage.chartRange` is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Range {
    #[default]
    Today,
    Week,
}

impl Range {
    pub fn name(self) -> &'static str {
        match self {
            Range::Today => "today",
            Range::Week => "week",
        }
    }

    pub fn from_name(name: &str) -> Option<Range> {
        match name {
            "today" => Some(Range::Today),
            "week" => Some(Range::Week),
            _ => None,
        }
    }
}

/// The chart over the cards (`TokensChart`): tokens by the hour today or by
/// the day for the last week, stacked by login.
#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub range: Range,
    pub title: &'static str,
    /// In time order, then by login id: the order they stack in, the first
    /// at the bottom.
    pub bars: Vec<Bar>,
    /// The time axis: midnight to midnight, or the week to the end of today.
    pub start: i64,
    pub end: i64,
    /// Where each bar's slot starts and how long it is: hours, or days (a
    /// day's slot runs to the next midnight).
    pub slots: Vec<(i64, i64)>,
    /// The time axis's marks and their words: every 6 hours, or each day.
    pub ticks: Vec<(i64, String)>,
    /// The dashed line at now, on today's chart.
    pub now: Option<i64>,
    /// The logins that spent something here, in the logins' own order, for
    /// the colours and the key.
    pub spending: Vec<String>,
    /// The value axis's marks, 0 first.
    pub value_ticks: Vec<i64>,
    /// What the empty chart says.
    pub empty: Option<&'static str>,
}

impl Chart {
    /// The slot a moment is in, for the pointer's hover.
    pub fn slot_at(&self, at: i64) -> Option<i64> {
        self.slots.iter().find(|(start, length)| at >= *start && at < start + length).map(|(start, _)| *start)
    }

    /// The bars in one slot.
    pub fn bars_at(&self, slot: i64) -> Vec<&Bar> {
        self.bars.iter().filter(|b| b.start == slot).collect()
    }

    /// The tallest slot, every login's bars stacked.
    pub fn tallest(&self) -> i64 {
        let mut sums: HashMap<i64, i64> = HashMap::new();
        for bar in &self.bars {
            let sum = sums.entry(bar.start).or_default();
            *sum = sum.saturating_add(bar.tokens);
        }
        sums.into_values().max().unwrap_or(0)
    }

    /// A slot's name, for the hover's note: "1 pm", or "Mon" or "Today".
    pub fn slot_words<Tz: TimeZone>(&self, slot: i64, now: i64, zone: &Tz) -> String {
        match self.range {
            Range::Today => clock_words(slot, zone),
            Range::Week => day_label(slot, now, zone),
        }
    }
}

/// "Mon", "Today".
pub fn day_label<Tz: TimeZone>(at: i64, now: i64, zone: &Tz) -> String {
    if same_day(at, now, zone) {
        "Today".to_owned()
    } else {
        weekday(at, zone).to_owned()
    }
}

pub fn chart<Tz: TimeZone>(stats: &Stats, accounts: &[Account], range: Range, now: i64, zone: &Tz) -> Chart {
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    let midnight = start_of_day(now, zone);
    let (title, bars, start, end) = match range {
        Range::Today => ("Tokens by hour", today_by_hour(stats, &ids, now, zone), midnight, day_start_after(midnight, 1, zone)),
        Range::Week => ("Tokens by day", by_day(stats, &ids, 7, now, zone), day_start_after(midnight, -6, zone), day_start_after(midnight, 1, zone)),
    };
    let slots: Vec<(i64, i64)> = match range {
        Range::Today => (0..)
            .map(|h| start + h * HOUR)
            .take_while(|s| *s < end)
            .map(|s| hour_start(s, zone))
            .fold(Vec::new(), |mut out: Vec<i64>, s| {
                if out.last() != Some(&s) {
                    out.push(s);
                }
                out
            })
            .into_iter()
            .map(|s| (s, HOUR))
            .collect(),
        Range::Week => (0..7).map(|d| day_start_after(start, d, zone)).map(|s| (s, day_start_after(s, 1, zone) - s)).collect(),
    };
    let ticks = match range {
        // Every 6 hours on the clock, as `.stride(by: .hour, count: 6)`.
        Range::Today => slots.iter().map(|(s, _)| *s).filter(|s| local(*s, zone).is_some_and(|d| d.hour() % 6 == 0 && d.minute() == 0)).map(|s| (s, clock_words(s, zone))).collect(),
        Range::Week => slots.iter().map(|(s, _)| (*s, day_label(*s, now, zone))).collect(),
    };
    let spending: Vec<String> = ids.iter().filter(|id| bars.iter().any(|b| b.account_id == **id)).map(|id| (*id).to_owned()).collect();
    let empty = bars.is_empty().then_some(if stats.ready { "Nothing spent yet" } else { "Reading the transcripts…" });
    let mut chart = Chart { range, title, bars, start, end, slots, ticks, now: (range == Range::Today).then_some(now), spending, value_ticks: Vec::new(), empty };
    chart.value_ticks = value_ticks(chart.tallest(), 3);
    chart
}

/// The value axis: about `count` marks from 0 at a round step (1, 2 or 5
/// times a power of ten), the top one at or over `max`, as Swift Charts'
/// `.automatic(desiredCount: 3)` puts them (100M over a 72M bar, at 50M).
pub fn value_ticks(max: i64, count: usize) -> Vec<i64> {
    if max <= 0 || count < 2 {
        return vec![0];
    }
    let raw = max as f64 / (count - 1) as f64;
    let power = 10f64.powf(raw.log10().floor());
    let step = [1., 2., 5., 10.].iter().map(|m| m * power).find(|s| *s >= raw).unwrap_or(10. * power).max(1.);
    let top = (max as f64 / step).ceil() * step;
    let mut out = Vec::new();
    let mut at = 0.;
    while at <= top + step / 2. {
        out.push(at.round() as i64);
        at += step;
    }
    out
}

/// One window's meter (`WindowMeter`): its name and what's left, a bar (the
/// 5-hour) or a chart of the week, and what to do about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub title: String,
    pub kind: String,
    pub remaining: f64,
    /// "78% left".
    pub left: String,
    pub resets_at: Option<i64>,
    /// The words are coloured: red used up, orange under a quarter.
    pub short: bool,
    pub verdict: Option<Verdict>,
    pub tip: String,
    /// What the chart draws on from now (None for old numbers, which can't
    /// say how fast it's going now).
    pub pace: Option<Pace>,
    /// The window's readings, for its chart.
    pub samples: Vec<Sample>,
}

impl Meter {
    pub fn new<Tz: TimeZone>(window: &Window, samples: &[Sample], stale: bool, now: i64, zone: &Tz) -> Meter {
        let pace = Pace::of(window, samples, now);
        // The tooltip says how fast it's going even from old numbers, as the
        // Swift meter's does; only the chart and the line under it don't.
        let mut tip = vec![window_line(window, now, zone, None)];
        if let Some(pace) = &pace {
            let mut line = format!("Going at about {}", pace.rate_text(&window.kind));
            if let Some(at) = pace.runs_out_at {
                line.push_str(&format!(", so it runs out {}", reset_words(at, now, zone)));
            } else if let Some(left) = pace.left_at_reset {
                line.push_str(&format!(", so about {} is left when it resets", percent(left)));
            }
            tip.push(line);
        }
        Meter {
            title: window.title.clone(),
            kind: window.kind.clone(),
            remaining: window.remaining,
            left: format!("{} left", percent(window.remaining)),
            resets_at: window.resets_at,
            short: window.remaining < CHIP_WARNING,
            verdict: verdict(window, pace.as_ref(), stale, now, zone),
            tip: tip.join(". "),
            pace: pace.filter(|_| !stale),
            samples: samples.to_vec(),
        }
    }

    pub fn is_week(&self) -> bool {
        self.kind == "weekly" || self.kind == "weekly_model"
    }

    /// The weekly chart's pieces (`WeekChart`).
    pub fn week_chart<Tz: TimeZone>(&self, now: i64, zone: &Tz) -> WeekChart {
        let from = now - WEEK;
        let reset = self.resets_at.filter(|r| *r > now);
        let to = reset.unwrap_or(now);
        let runs = runs(&self.samples, self.remaining, reset.map(|r| r - WEEK), now);
        // Each day's name under the middle of its day, where that's on the
        // chart.
        let noons = midnights(start_of_day(from, zone), to, zone).into_iter().map(|m| m + 12 * HOUR).filter(|n| *n > from && *n < to).map(|n| (n, weekday(n, zone).to_owned())).collect();
        let pace = self.pace.filter(|_| reset.is_some());
        let ahead = pace.map(|p| vec![Sample { at: now, remaining: self.remaining }, p.runs_out_at.map_or(Sample { at: to, remaining: p.left_at_reset.unwrap_or(self.remaining) }, |at| Sample { at, remaining: 0. })]).unwrap_or_default();
        WeekChart { from, to, runs, ahead, short: self.pace.is_some_and(|p| p.runs_out_at.is_some()), now: Sample { at: now, remaining: self.remaining }, runs_out: pace.and_then(|p| p.runs_out_at), midnights: midnights(from, to, zone), noons }
    }
}

/// The last seven days of a weekly window, rolling: what was left over time,
/// last week's readings too, breaking at each reset, then this pace dashed
/// on from now to the reset, with the days along the bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct WeekChart {
    pub from: i64,
    pub to: i64,
    pub runs: Vec<Vec<Sample>>,
    /// Now, then where this pace leads: empty without a pace or a reset.
    pub ahead: Vec<Sample>,
    /// The pace runs out before the reset: the dashed line is orange.
    pub short: bool,
    pub now: Sample,
    /// Where it runs out, marked in orange.
    pub runs_out: Option<i64>,
    /// A faint line at each.
    pub midnights: Vec<i64>,
    /// Each day's name, under its middle.
    pub noons: Vec<(i64, String)>,
}

/// A login's row in its card (`LoginRow`): its name and what it's spent
/// today, then its windows' meters, the main two then a model's own week
/// ("Weekly Fable"), then only the extras that decide something.
#[derive(Clone, Debug, PartialEq)]
pub struct LoginRow {
    pub id: String,
    /// Its email, or its title.
    pub name: String,
    pub note: Option<String>,
    /// "86M today" and its breakdown, once the stats have come and it's spent
    /// any.
    pub today: Option<(String, String)>,
    /// The stats have come, so its colour's dot goes beside its name.
    pub dot: bool,
    /// Its name and plan, "mike@example.com, Max".
    pub tip: String,
    /// "Nothing known yet", or "Limit reached" in red.
    pub status: Option<LoginStatus>,
    pub meters: Vec<Meter>,
    /// Old numbers' "Numbers from 6:02 am", then the extra lines.
    pub more: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginStatus {
    NothingKnown,
    LimitReached,
}

impl LoginStatus {
    pub fn words(self) -> &'static str {
        match self {
            LoginStatus::NothingKnown => "Nothing known yet",
            LoginStatus::LimitReached => "Limit reached",
        }
    }
}

/// One provider's logins in a card, named only when the card mixes
/// providers.
#[derive(Clone, Debug, PartialEq)]
pub struct CardSection {
    pub title: Option<String>,
    pub logins: Vec<LoginRow>,
}

/// One pool's card (`PoolCard`): its name and what it's spent today, then
/// each login in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub symbol: String,
    pub today: Option<(String, String)>,
    pub sections: Vec<CardSection>,
}

fn spent_today<Tz: TimeZone>(stats: Option<&Stats>, ids: &[&str], now: i64, zone: &Tz) -> Option<(String, String)> {
    let sum = today(stats?, ids, now, zone);
    (sum.total() > 0).then(|| (format!("{} today", tokens(sum.total())), breakdown(&sum)))
}

/// `show_model_weeks` is the panel's own switch (`usage.showModelWeeks`): off,
/// the logins leave out a model's own week ("Weekly Fable").
pub fn cards<Tz: TimeZone>(pools: &[Pool], accounts: &[Account], stats: Option<&Stats>, show_model_weeks: bool, now: i64, zone: &Tz) -> Vec<Card> {
    let (_, own) = notices(accounts);
    groups(pools, accounts)
        .iter()
        .map(|group| {
            let ids: Vec<&str> = group.accounts().iter().map(|a| a.id.as_str()).collect();
            let mixed = group.providers.len() > 1;
            Card {
                id: group.id.clone(),
                title: group.title.clone(),
                symbol: group.pool.map_or_else(|| "tray".to_owned(), |p| pool_symbol(p, accounts)),
                today: spent_today(stats, &ids, now, zone),
                sections: group
                    .providers
                    .iter()
                    .map(|section| CardSection {
                        title: mixed.then(|| provider_title(&section.provider)),
                        logins: section.accounts.iter().map(|a| login_row(a, own.get(&a.id).map(String::as_str), stats, show_model_weeks, now, zone)).collect(),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn login_row<Tz: TimeZone>(account: &Account, notice: Option<&str>, stats: Option<&Stats>, show_model_weeks: bool, now: i64, zone: &Tz) -> LoginRow {
    let usage = usage(account);
    let status = match &usage {
        None => Some(LoginStatus::NothingKnown),
        Some(u) if u.blocked => Some(LoginStatus::LimitReached),
        _ => None,
    };
    let mut meters = Vec::new();
    let mut more = Vec::new();
    if let Some(u) = &usage {
        let model_weeks = if show_model_weeks { model_weeks(u) } else { Vec::new() };
        for window in main_windows(u).into_iter().chain(model_weeks) {
            let samples = stats.map_or(&[][..], |s| s.samples(&account.id, &window.kind, Some(&window.title)));
            meters.push(Meter::new(window, samples, u.stale, now, zone));
        }
        // When the numbers are old, since when.
        if let (true, Some(read_at)) = (u.stale, u.read_at) {
            more.push(format!("Numbers from {}", when(read_at, now, zone)));
        }
        more.extend(extra_lines(u, notice, now, zone));
    }
    let name = account.name();
    LoginRow {
        id: account.id.clone(),
        tip: [Some(name.clone()), account.plan.clone()].into_iter().flatten().collect::<Vec<_>>().join(", "),
        name,
        note: account.note.clone().filter(|n| !n.is_empty()),
        today: spent_today(stats, &[account.id.as_str()], now, zone),
        dot: stats.is_some(),
        status,
        meters,
        more,
    }
}

/// The line at the foot of the panel: where the numbers came from, and what
/// Token Stats says about all of them, once.
pub fn footer<Tz: TimeZone>(accounts: &[Account], now: i64, zone: &Tz) -> Option<String> {
    let parts: Vec<String> = [source_line(accounts, now, zone), notices(accounts).0].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(". "))
}

/// "Weekly Fable": each model's own week any login has, once each, in the
/// logins' order (`UsagePanel.modelWeekTitles`). The panel's Settings card
/// only shows when there's one, or its switch would do nothing.
pub fn model_week_titles(accounts: &[Account]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for u in accounts.iter().filter_map(usage) {
        for window in model_weeks(&u) {
            if !seen.contains(&window.title) {
                seen.push(window.title.clone());
            }
        }
    }
    seen
}

/// The Settings card's switch: "Show Weekly Fable".
pub fn model_weeks_switch(titles: &[String]) -> String {
    format!("Show {}", titles.join(", "))
}

/// Its tooltip, for someone who never runs that model.
pub const MODEL_WEEKS_TIP: &str = "A login's own limit on one model. Hide it if you don't use that model";

/// A runner from before usage sends none: the panel says so over the cards.
pub fn needs_newer_runner(accounts: &[Account]) -> bool {
    !accounts.iter().any(|a| a.usage.as_ref().is_some_and(|u| !u.is_null()))
}

pub const NEWER_RUNNER: &str = "The usage numbers need a newer runner.";
pub const PARTIAL: &str = "Still reading the transcripts, so these are partial.";

/// The panel as text, for the automation (the Swift app's `usageLines`, less
/// the CLIs, which are the Updates panel's): the spend, then each login's
/// room by pool (`#`) and provider (`##`), then where the numbers came from.
pub fn lines<Tz: TimeZone>(pools: &[Pool], accounts: &[Account], stats: Option<&Stats>, now: i64, zone: &Tz) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(stats) = stats {
        let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
        let mut spend = format!("Today {}, last hour {}", tokens(today(stats, &ids, now, zone).total()), tokens(last_hour(stats, &ids, now, zone)));
        if !stats.ready {
            spend.push_str(" (still reading)");
        }
        out.push(spend);
    }
    let groups = groups(pools, accounts);
    let pooled = groups.iter().any(|g| g.pool.is_some());
    for group in &groups {
        if pooled {
            out.push(format!("# {}", group.title));
        }
        for section in &group.providers {
            out.push(format!("{}{}", if pooled { "## " } else { "# " }, provider_title(&section.provider)));
            for account in &section.accounts {
                let mut line = account.name();
                if let Some(plan) = &account.plan {
                    line.push_str(&format!(" ({plan})"));
                }
                if let Some(note) = &account.note {
                    line.push_str(&format!(", {note}"));
                }
                match usage(account) {
                    Some(u) => {
                        if u.blocked {
                            line.push_str(", limit reached");
                        }
                        if u.stale {
                            line.push_str(", stale");
                        }
                        let windows: Vec<String> = u.windows.iter().map(|w| format!("{} {}", w.title, percent(w.remaining))).collect();
                        if !windows.is_empty() {
                            line.push_str(&format!(": {}", windows.join(", ")));
                        }
                        if let Some(notice) = &u.notice {
                            line.push_str(&format!(" [{notice}]"));
                        }
                    }
                    None => line.push_str(": nothing known"),
                }
                out.push(line);
            }
        }
    }
    if let Some(source) = source_line(accounts, now, zone) {
        out.push(source);
    }
    out
}

/// The names in the chart's key and the hover's note (`AccountName.login`).
pub fn login_label(account: &str, accounts: &[Account]) -> String {
    login_name(account, accounts)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::iso_time;
    use crate::logins::RunnerLogins;
    use chrono::{FixedOffset, TimeZone};
    use serde_json::json;

    // The Swift tests' clock: Perth, which has no clock changes.
    fn zone() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    /// A moment in October 2026, Perth time. The 29th is a Thursday.
    fn at_on(day: u32, hour: u32, minute: u32) -> i64 {
        zone().with_ymd_and_hms(2026, 10, day, hour, minute, 0).unwrap().timestamp_millis()
    }

    fn at(hour: u32, minute: u32) -> i64 {
        at_on(29, hour, minute)
    }

    fn hour_of(start: i64, tokens: i64) -> Hour {
        Hour { start, output: tokens / 10, cache_read: tokens - tokens / 10, ..Default::default() }
    }

    fn stats(tokens: Vec<(&str, Vec<Hour>)>) -> Stats {
        Stats { ready: true, tokens: tokens.into_iter().map(|(id, hours)| AccountTokens { account_id: id.into(), hours }).collect(), limits: vec![] }
    }

    fn window(kind: &str, title: &str, left: f64, resets_at: Option<i64>) -> Window {
        Window { kind: kind.into(), title: title.into(), remaining: left, resets_at, detail: None }
    }

    fn weekly(left: f64, resets_at: i64) -> Window {
        window("weekly", "Weekly", left, Some(resets_at))
    }

    /// A login with a 5-hour window (if any) and a weekly one.
    fn login(id: &str, provider: &str, five: Option<f64>, week: f64, blocked: bool, reset: Option<i64>) -> Account {
        let mut windows = vec![];
        if let Some(five) = five {
            windows.push(json!({ "kind": "five_hour", "title": "5-hour", "remainingPercent": five, "resetsAt": reset.map(iso_time) }));
        }
        windows.push(json!({ "kind": "weekly", "title": "Weekly", "remainingPercent": week }));
        Account { id: id.into(), provider: provider.into(), title: id.into(), usage: Some(json!({ "windows": windows, "blocked": blocked })), ..Default::default() }
    }

    fn pool(id: &str, title: &str, logins: &[&str]) -> Pool {
        Pool { id: id.into(), title: title.into(), account_ids: logins.iter().map(|l| l.to_string()).collect(), ..Default::default() }
    }

    // ---- Ported from the Swift app's UsageStatsTests ----

    #[test]
    fn decodes_the_runners_shape_and_skips_bad_rows() {
        let s = Stats::decode(&json!({
            "generatedAt": "2026-10-29T02:00:00.000Z", "ready": false,
            "tokens": [{ "accountId": "c1", "hours": [[1761703200000u64, 10, 20, 300, 4], [1], [1761699600000u64, 1, 2, 3, 4]] }, { "hours": [] }],
            "limits": [{ "accountId": "c1", "kind": "weekly", "samples": [[1761703200000u64, 40.5], [1761699600000u64, 120]] }],
            "somethingNew": 1
        }));
        assert!(!s.ready);
        assert_eq!(s.tokens.len(), 1);
        let hours = s.hours("c1");
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0].total(), 10, "oldest first");
        assert_eq!(hours[1].total(), 334);
        assert_eq!(s.samples("c1", "weekly", None).iter().map(|s| s.remaining).collect::<Vec<_>>(), vec![100., 40.5], "sorted, and clamped to 100");
        assert!(s.samples("c1", "five_hour", None).is_empty());
    }

    #[test]
    fn odd_rows_are_left_out_rather_than_failing_the_lot() {
        let s = Stats::decode(&json!({ "tokens": [{ "accountId": "a", "hours": [[0, -1, 0, 0, 0], [0, 1.5, 0, 0, 0], [1e20, 1, 1, 1, 1], [3600000, 10, 0, 0, 0]] }] }));
        assert!(s.ready, "a runner that doesn't say is taken to be ready");
        assert_eq!(s.hours("a").iter().map(|h| (h.start, h.total())).collect::<Vec<_>>(), vec![(3_600_000, 10)]);
    }

    #[test]
    fn today_counts_from_local_midnight() {
        let z = zone();
        let s = stats(vec![("a", vec![hour_of(at_on(28, 23, 0), 500), hour_of(at(0, 0), 1_000), hour_of(at(9, 0), 2_000)]), ("b", vec![hour_of(at(9, 0), 300)])]);
        let now = at(9, 30);
        assert_eq!(today(&s, &["a", "b"], now, &z).total(), 3_300);
        assert_eq!(today(&s, &["b"], now, &z).total(), 300);
    }

    #[test]
    fn the_last_hour_takes_the_tail_of_the_previous_hour() {
        let s = stats(vec![("a", vec![hour_of(at(8, 0), 1_000), hour_of(at(9, 0), 400)])]);
        // At 9:15, three quarters of 8 o'clock's hour is still inside the 60.
        assert_eq!(last_hour(&s, &["a"], at(9, 15), &zone()), 400 + 750);
    }

    #[test]
    fn a_usual_day_averages_the_same_time_on_previous_days() {
        let z = zone();
        let mut hours = vec![];
        for day in 22..=28 {
            hours.push(hour_of(at_on(day, 8, 0), 700)); // before 9:30, counts
            hours.push(hour_of(at_on(day, 15, 0), 9_000)); // after, doesn't
        }
        let s = stats(vec![("a", hours)]);
        assert_eq!(usual_by_now(&s, &["a"], at(9, 30), 7, &z), Some(700));
        // Only two days of history: no usual day yet.
        let short = stats(vec![("a", vec![hour_of(at_on(28, 8, 0), 700)])]);
        assert_eq!(usual_by_now(&short, &["a"], at(9, 30), 7, &z), None);
    }

    #[test]
    fn bars_by_hour_and_by_day() {
        let z = zone();
        let s = stats(vec![
            ("a", vec![hour_of(at_on(27, 10, 0), 100), hour_of(at_on(28, 9, 0), 50), hour_of(at_on(28, 10, 0), 50), hour_of(at(8, 0), 10), hour_of(at(9, 0), 0)]),
            ("b", vec![hour_of(at(8, 0), 5)]),
        ]);
        let now = at(9, 30);
        let today = today_by_hour(&s, &["a", "b"], now, &z);
        assert_eq!(today.iter().map(|b| b.account_id.as_str()).collect::<Vec<_>>(), vec!["a", "b"], "zeros left out");
        assert_eq!(today.iter().map(|b| b.tokens).collect::<Vec<_>>(), vec![10, 5]);
        let days = by_day(&s, &["a", "b"], 2, now, &z);
        assert_eq!(days.iter().map(|b| b.tokens).collect::<Vec<_>>(), vec![100, 10, 5], "the 27th is outside two days; the 28th's hours add up");
        assert_eq!(days[0].start, at_on(28, 0, 0));
    }

    #[test]
    fn token_words() {
        assert_eq!(tokens(912), "912");
        assert_eq!(tokens(12_400), "12k");
        assert_eq!(tokens(4_400), "4.4k");
        assert_eq!(tokens(380_000), "380k");
        assert_eq!(tokens(4_000_000), "4M");
        assert_eq!(tokens(4_210_000), "4.2M");
        assert_eq!(tokens(1_500_000_000), "1.5B");
        assert_eq!(tokens(9_960_000), "10M", "9.96 rounds up to a whole one");
        assert_eq!(breakdown(&Hour { input: 40_000, output: 52_000, cache_read: 3_900_000, ..Default::default() }), "4M tokens: 3.9M cache reads, 40k input, 52k output");
        assert_eq!(breakdown(&Hour::default()), "0 tokens");
    }

    #[test]
    fn a_week_going_too_fast_runs_out_before_it_resets() {
        // Three and a half days in, 30% left: 20% a day, 1.5 days to go and
        // 3.5 until the reset.
        let now = at(12, 0);
        let pace = Pace::of(&weekly(30., now + 84 * HOUR), &[], now).unwrap();
        assert!((pace.rate - 20.).abs() < 0.01);
        assert!((pace.runs_out_at.unwrap() - (now + 36 * HOUR)).abs() < 1000);
        assert_eq!(pace.left_at_reset, None);
        assert_eq!(pace.rate_text("weekly"), "20% a day");
    }

    #[test]
    fn a_week_that_lasts_says_whats_left_at_the_reset() {
        let now = at(12, 0);
        let pace = Pace::of(&weekly(80., now + 5 * DAY), &[], now).unwrap();
        assert!((pace.rate - 10.).abs() < 0.01);
        assert_eq!(pace.runs_out_at, None);
        assert!((pace.left_at_reset.unwrap() - 30.).abs() < 0.01);
    }

    #[test]
    fn the_last_days_readings_win_over_the_weeks_average() {
        // A quiet start to the week, then a busy last day: 60% to 40% in the
        // last 20 hours is 24% a day, not the week's 15%.
        let now = at(12, 0);
        let w = weekly(40., now + 3 * DAY);
        let samples = [Sample { at: now - 30 * HOUR, remaining: 62. }, Sample { at: now - 20 * HOUR, remaining: 60. }, Sample { at: now - HOUR, remaining: 41. }];
        assert!((Pace::of(&w, &samples, now).unwrap().rate - 24.).abs() < 0.01);
        // Under half a day of readings: the week's average instead.
        let recent = [Sample { at: now - 6 * HOUR, remaining: 45. }];
        assert!((Pace::of(&w, &recent, now).unwrap().rate - 15.).abs() < 0.01);
    }

    #[test]
    fn fables_own_week_goes_like_a_week_but_never_says_going_spare() {
        let z = zone();
        let now = at(12, 0);
        let fable = window("weekly_model", "Weekly Fable", 30., Some(now + 84 * HOUR));
        let pace = Pace::of(&fable, &[], now).unwrap();
        assert!((pace.rate - 20.).abs() < 0.01);
        assert_eq!(pace.rate_text("weekly_model"), "20% a day");
        assert_eq!(verdict(&fable, Some(&pace), false, now, &z).map(|v| v.tone), Some(Tone::Short));
        // Untouched: most logins never run out of Fable, so "100% will go
        // unused" on each would be noise. Just when it resets.
        let untouched = window("weekly_model", "Weekly Fable", 100., Some(now + 5 * DAY));
        let v = verdict(&untouched, Pace::of(&untouched, &[], now).as_ref(), false, now, &z).unwrap();
        assert_eq!((v.tone, v.text.as_str()), (Tone::Fine, "Resets Tue 12 pm"));
    }

    #[test]
    fn a_models_weeks_readings_are_found_by_title() {
        let s = Stats::decode(&json!({ "ready": true, "tokens": [], "limits": [
            { "accountId": "a", "kind": "weekly", "samples": [[1793000000000u64, 50]] },
            { "accountId": "a", "kind": "weekly_model", "title": "Weekly Opus", "samples": [[1793000000000u64, 80]] },
            { "accountId": "a", "kind": "weekly_model", "title": "Weekly Fable", "samples": [[1793000000000u64, 70]] }
        ]}));
        assert_eq!(s.samples("a", "weekly", None)[0].remaining, 50.);
        assert_eq!(s.samples("a", "weekly_model", Some("Weekly Fable"))[0].remaining, 70.);
        assert!(s.samples("a", "weekly_model", Some("Weekly Sonnet")).is_empty());
    }

    #[test]
    fn a_five_hour_pace_is_per_hour_and_needs_some_time() {
        let now = at(12, 0);
        let pace = Pace::of(&window("five_hour", "5-hour", 40., Some(now + 3 * HOUR)), &[], now).unwrap();
        assert!((pace.rate - 30.).abs() < 0.01);
        assert!((pace.runs_out_at.unwrap() - (now + 80 * 60_000)).abs() < 1000);
        assert_eq!(pace.rate_text("five_hour"), "30% an hour");
        assert_eq!(Pace::of(&window("five_hour", "5-hour", 99., Some(now + 5 * HOUR - 60_000)), &[], now), None, "a minute in is too early to say");
        assert_eq!(Pace::of(&window("weekly", "Weekly", 50., None), &[], now), None, "no reset, no pace");
    }

    #[test]
    fn running_out_says_how_long_before_the_reset() {
        // 20% a day with 30% left: out in 1.5 days, 2 days before the reset.
        let now = at(12, 0);
        let w = weekly(30., now + 84 * HOUR);
        let v = verdict(&w, Pace::of(&w, &[], now).as_ref(), false, now, &zone()).unwrap();
        assert_eq!((v.tone, v.text.as_str()), (Tone::Short, "Runs out Sat 12 am, 2 days before it resets"));
    }

    #[test]
    fn a_week_with_lots_left_over_says_it_will_go_unused() {
        let now = at(12, 0);
        let w = weekly(80., now + 5 * DAY);
        let v = verdict(&w, Pace::of(&w, &[], now).as_ref(), false, now, &zone()).unwrap();
        assert_eq!((v.tone, v.text.as_str()), (Tone::Spare, "About 30% will go unused, resets Tue 12 pm"));
    }

    #[test]
    fn a_week_about_right_is_on_track() {
        // 5 days in with 35% left: 13% a day, about 9% left at the reset.
        let now = at(12, 0);
        let w = weekly(35., now + 2 * DAY);
        let v = verdict(&w, Pace::of(&w, &[], now).as_ref(), false, now, &zone()).unwrap();
        assert_eq!((v.tone, v.text.as_str()), (Tone::Fine, "On track, lasts till Sat 12 pm"));
    }

    #[test]
    fn a_five_hour_window_with_room_just_says_when_it_resets() {
        let z = zone();
        let now = at(12, 0);
        let w = window("five_hour", "5-hour", 90., Some(now + 2 * HOUR));
        assert_eq!(verdict(&w, Pace::of(&w, &[], now).as_ref(), false, now, &z), Some(Verdict { text: "Resets 2 pm".into(), tone: Tone::Fine }));
        let used = window("five_hour", "5-hour", 0., Some(now + 2 * HOUR));
        assert_eq!(verdict(&used, None, false, now, &z), Some(Verdict { text: "Used up, back 2 pm".into(), tone: Tone::Out }));
    }

    #[test]
    fn old_numbers_dont_guess_at_the_pace() {
        let now = at(12, 0);
        let w = weekly(30., now + 84 * HOUR);
        assert_eq!(verdict(&w, Pace::of(&w, &[], now).as_ref(), true, now, &zone()), Some(Verdict { text: "Resets Mon 12 am".into(), tone: Tone::Fine }));
    }

    #[test]
    fn gaps_read_roughly() {
        assert_eq!(gap(2.1 * 86400.), "2 days");
        assert_eq!(gap(30. * 3600.), "1 day");
        assert_eq!(gap(5.2 * 3600.), "5 hours");
        assert_eq!(gap(70. * 60.), "1 hour");
        assert_eq!(gap(40. * 60.), "40 min");
    }

    #[test]
    fn the_rolling_week_keeps_last_week_and_breaks_at_the_reset() {
        // Last week ran down to 5% and reset 20 hours ago; the new week
        // hasn't been touched, so there's no reset time yet.
        let now = at(12, 0);
        let h = |n: i64| now + n * HOUR;
        let samples = [
            Sample { at: h(-200), remaining: 90. }, // over a week ago
            Sample { at: h(-150), remaining: 60. },
            Sample { at: h(-30), remaining: 5. },
            Sample { at: h(-19), remaining: 100. },
            Sample { at: h(-2), remaining: 100. },
        ];
        let r = runs(&samples, 100., None, now);
        assert_eq!(r.iter().map(|run| run.iter().map(|s| s.remaining).collect::<Vec<_>>()).collect::<Vec<_>>(), vec![vec![60., 5.], vec![100., 100., 100.]]);
        assert_eq!(r.last().and_then(|run| run.last()).map(|s| s.at), Some(now));
    }

    #[test]
    fn the_rolling_week_starts_the_current_week_full() {
        // The week began 30 hours ago: full there whatever the readings
        // around it, and a small rise in it (a revised reading) stays joined.
        let now = at(12, 0);
        let h = |n: i64| now + n * HOUR;
        let samples = [Sample { at: h(-40), remaining: 97. }, Sample { at: h(-20), remaining: 90. }, Sample { at: h(-10), remaining: 92. }];
        let r = runs(&samples, 85., Some(h(-30)), now);
        assert_eq!(r.iter().map(|run| run.iter().map(|s| s.remaining).collect::<Vec<_>>()).collect::<Vec<_>>(), vec![vec![97.], vec![100., 90., 92., 85.]]);
        assert_eq!(r[1][0].at, h(-30));
    }

    #[test]
    fn the_rolling_weeks_day_lines_are_midnights() {
        let now = at(12, 0);
        let lines = midnights(now - 7 * DAY, now, &zone());
        assert_eq!(lines.len(), 7);
        assert!(lines.iter().all(|m| zone().timestamp_millis_opt(*m).unwrap().format("%H:%M").to_string() == "00:00"));
    }

    // ---- Ported from the Swift app's UsageGroupsTests and AccountUsageTests ----

    #[test]
    fn groups_by_pool_then_provider_in_the_pools_order() {
        let accounts = [login("c1", "claude", Some(90.), 80., false, None), login("c2", "claude", Some(50.), 40., false, None), login("x1", "codex", None, 70., false, None), login("c3", "claude", None, 10., false, None)];
        let pools = [pool("mixed", "Mixed", &["x1", "c2"]), pool("claude", "Claude", &["c1", "c2"])];
        let g = groups(&pools, &accounts);
        assert_eq!(g.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), vec!["Mixed", "Claude", "Not in a pool"]);
        // The pool's order decides which provider comes first.
        assert_eq!(g[0].providers.iter().map(|p| p.provider.as_str()).collect::<Vec<_>>(), vec!["codex", "claude"]);
        assert_eq!(g[1].accounts().iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["c1", "c2"]);
        assert_eq!(g[2].accounts().iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["c3"]);
    }

    #[test]
    fn without_pools_each_provider_is_a_group_claude_first() {
        let accounts = [login("x1", "codex", None, 70., false, None), login("c1", "claude", None, 80., false, None)];
        let g = groups(&[], &accounts);
        assert_eq!(g.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), vec!["Claude", "Codex"]);
        assert!(g[0].pool.is_none());
        let gone = [pool("gone", "Gone", &["nobody"])];
        assert_eq!(groups(&gone, &accounts[1..]).iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), vec!["Not in a pool"], "a pool with no logins here is left out");
    }

    #[test]
    fn the_usage_tooltip_says_each_pools_best_room() {
        let accounts = [login("c1", "claude", Some(94.), 93., false, None), login("c2", "claude", Some(100.), 16., false, None), login("x1", "codex", None, 79., false, None)];
        let pools = [pool("claude", "Claude", &["c1", "c2"]), pool("codex", "Codex", &["x1"])];
        assert_eq!(icon_tooltip(&groups(&pools, &accounts), at_on(28, 10, 0), &zone()), "Usage\nClaude: 93% left\nCodex: 79% left");
    }

    #[test]
    fn the_usage_tooltip_says_when_a_pool_at_its_limit_comes_back() {
        let accounts = [login("c1", "claude", Some(0.), 50., true, Some(at_on(28, 13, 26))), login("c2", "claude", Some(0.), 40., true, Some(at_on(28, 12, 56)))];
        let pools = [pool("claude", "Claude", &["c1", "c2"])];
        assert_eq!(icon_tooltip(&groups(&pools, &accounts), at_on(28, 10, 0), &zone()), "Usage\nClaude: at its limit, back at 12:56 pm");
    }

    fn reading(read_at: i64, stale: bool, source: &str) -> Account {
        Account { id: format!("a{read_at}{stale}"), usage: Some(json!({ "windows": [], "readAt": iso_time(read_at), "source": source, "stale": stale })), ..Default::default() }
    }

    #[test]
    fn the_source_line_says_where_the_numbers_came_from() {
        let z = zone();
        let now = at_on(27, 10, 0);
        assert_eq!(source_line(&[reading(at_on(27, 7, 15), false, "tokenStats"), reading(at_on(27, 6, 2), false, "tokenStats")], now, &z).as_deref(), Some("Usage from Token Stats at 7:15 am"));
        assert_eq!(source_line(&[reading(at_on(27, 6, 2), true, "tokenStats")], now, &z).as_deref(), Some("Token Stats hasn't updated since 6:02 am"));
        // One fresh reading is enough for "from Token Stats".
        assert_eq!(source_line(&[reading(at_on(27, 6, 2), true, "tokenStats"), reading(at_on(27, 7, 15), false, "tokenStats")], now, &z).as_deref(), Some("Usage from Token Stats at 7:15 am"));
        assert_eq!(source_line(&[reading(at_on(27, 9, 0), false, "live")], now, &z), None);
        assert_eq!(source_line(&[], now, &z), None);
    }

    #[test]
    fn extra_windows_leave_out_the_main_ones() {
        let u = Usage { windows: vec![window("five_hour", "5-hour", 89., None), window("weekly_model", "Weekly Fable", 71., None), window("weekly", "Weekly", 78., None)], ..Default::default() };
        assert_eq!(main_windows(&u).iter().map(|w| w.kind.as_str()).collect::<Vec<_>>(), vec!["five_hour", "weekly"]);
        assert_eq!(extra_windows(&u).iter().map(|w| w.title.as_str()).collect::<Vec<_>>(), vec!["Weekly Fable"]);
        assert_eq!(room(&u), Some(78.));
    }

    #[test]
    fn a_notice_several_logins_share_is_said_once() {
        let with = |id: &str, notice: Option<&str>| Account { id: id.into(), usage: Some(json!({ "windows": [], "notice": notice })), ..Default::default() };
        let couldnt = "Couldn't check usage just now, so these are from 1:19 pm.";
        let accounts = [
            with("claude-main", Some(couldnt)),
            with("claude-1", Some(&format!("{couldnt} · $48.85 extra usage spent"))),
            with("claude-3", Some(couldnt)),
            with("codex-main", Some("64,390 credits · 2 limit resets available")),
            with("claude-4", None),
        ];
        let (shared, own) = notices(&accounts);
        assert_eq!(shared.as_deref(), Some(couldnt));
        assert_eq!(own, HashMap::from([("claude-1".to_string(), "$48.85 extra usage spent".to_string()), ("codex-main".to_string(), "64,390 credits · 2 limit resets available".to_string())]));
        // One login's notice stays its own.
        let (shared, own) = notices(&[with("codex-main", Some("64,390 credits"))]);
        assert_eq!((shared, own.get("codex-main").map(String::as_str)), (None, Some("64,390 credits")));
    }

    #[test]
    fn credits_show_once() {
        let z = zone();
        let mut w = window("weekly", "Weekly", 93., None);
        w.detail = Some("64,390 credits".into());
        assert_eq!(window_line(&w, at(10, 0), &z, Some("64,390 credits · 2 limit resets available")), "93% left");
        assert_eq!(window_line(&w, at(10, 0), &z, None), "93% left (64,390 credits)");
    }

    #[test]
    fn the_extra_lines_keep_only_what_decides_something() {
        let z = zone();
        let now = at_on(27, 10, 0);
        let usage_of = |five: Option<f64>, week: f64, weekly_reset: Option<i64>| {
            let mut windows = vec![];
            if let Some(five) = five {
                windows.push(window("five_hour", "5-hour", five, None));
            }
            windows.push(window("weekly", "Weekly", week, weekly_reset));
            Usage { windows, ..Default::default() }
        };
        // Mike's gmail login: Fable's own week has a meter, not a line; the
        // allowance and the spend are one line.
        let mut gmail = usage_of(Some(90.), 16., Some(at_on(30, 1, 0)));
        gmail.windows.push(window("weekly_model", "Weekly Fable", 96., Some(at_on(30, 1, 0))));
        gmail.windows.push(Window { detail: Some("$132.79 left".into()), ..window("allowance", "$250 allowance", 53.1, Some(at_on(30, 8, 0) + 5 * DAY)) });
        assert_eq!(extra_lines(&gmail, Some("$48.85 extra usage spent"), now, &z), vec!["$48.85 extra usage spent · $250 allowance: $132.79 left"]);
        // A model's own week has its own meter, so it's never a line here,
        // however tight.
        let mut tight = usage_of(Some(90.), 60., None);
        tight.windows.push(window("weekly_model", "Weekly Fable", 5., Some(at_on(29, 13, 0))));
        assert!(extra_lines(&tight, None, now, &z).is_empty());
        assert_eq!(model_weeks(&tight).iter().map(|w| w.title.as_str()).collect::<Vec<_>>(), vec!["Weekly Fable"]);
        // A bare bucket tighter than the main windows still is.
        let mut codex_tight = usage_of(None, 60., None);
        codex_tight.windows.push(window("other", "GPT-6.1-Sol", 5., None));
        assert_eq!(extra_lines(&codex_tight, None, now, &z), vec!["GPT-6.1-Sol 5% left"]);
        // Codex's credits come in the notice; a window repeating them doesn't
        // say them again.
        let mut codex = usage_of(None, 89., None);
        codex.windows.push(Window { detail: Some("64,390 credits".into()), ..window("other", "Credits", 100., None) });
        assert_eq!(extra_lines(&codex, Some("64,390 credits · 2 limit resets available"), now, &z), vec!["64,390 credits · 2 limit resets available"]);
    }

    // ---- The panel's own pieces ----

    #[test]
    fn usage_is_read_as_the_swift_app_reads_it() {
        let u = Usage::decode(&json!({ "windows": [
            { "kind": "weekly", "title": "Weekly", "remainingPercent": 130 },
            { "kind": "five_hour", "remainingPercent": 50, "resetsAt": "2026-11-11T05:00:00.000Z" },
            { "kind": "other", "title": "No number" }
        ], "stale": true })).unwrap();
        assert_eq!(u.windows.len(), 2, "a window with no percentage is left out");
        assert_eq!(u.windows[0].remaining, 100.);
        assert_eq!(u.windows[1].title, "five_hour", "the kind stands in for a missing title");
        assert_eq!(u.source, "tokenStats");
        assert!(u.stale && !u.blocked);
        assert_eq!(Usage::decode(&json!("nonsense")), None);
        assert_eq!(tightest(&u).map(|w| w.kind.as_str()), Some("five_hour"));
    }

    /// Mike's logins, much as the mock runner has them.
    fn logins() -> RunnerLogins {
        RunnerLogins::from_hello(&json!({
            "accounts": [
                { "id": "claude-main", "provider": "claude", "email": "mike@example.com", "plan": "Max", "usage": {
                    "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 78 }, { "kind": "weekly_model", "title": "Weekly Fable", "remainingPercent": 71 }],
                    "readAt": iso_time(at(11, 15)), "notice": "Couldn't check usage just now. · $48.85 extra usage spent" } },
                { "id": "claude-2", "provider": "claude", "email": "work@example.com", "note": "Convex work only", "usage": {
                    "windows": [{ "kind": "five_hour", "title": "5-hour", "remainingPercent": 95 }, { "kind": "weekly", "title": "Weekly", "remainingPercent": 100 }],
                    "notice": "Couldn't check usage just now." } },
                { "id": "claude-4", "provider": "claude", "email": "mike+4@example.com", "usage": { "blocked": true,
                    "windows": [{ "kind": "five_hour", "title": "5-hour", "remainingPercent": 0, "resetsAt": iso_time(at(15, 40)) }, { "kind": "weekly", "title": "Weekly", "remainingPercent": 31 }] } },
                { "id": "codex-main", "provider": "codex", "title": "Codex · mike@example.com", "email": "mike@example.com", "usage": { "source": "live",
                    "notice": "64,390 credits · 2 limit resets available",
                    "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 64, "detail": "64,390 credits" }, { "kind": "other", "title": "GPT-6.1-Sol", "remainingPercent": 82 }] } },
                { "id": "new", "provider": "claude", "email": "new@example.com" }
            ],
            "pools": [
                { "id": "claude", "title": "Claude", "accountIds": ["claude-main", "claude-4"] },
                { "id": "mixed", "title": "Side projects", "icon": "hammer", "accountIds": ["codex-main", "claude-main"] },
                { "id": "evenings", "title": "Evenings", "accountIds": ["claude-4"] },
                { "id": "gone", "title": "Gone", "accountIds": ["nobody"] }
            ]
        }))
    }

    #[test]
    fn the_icons_tooltip_has_each_pools_best_room() {
        let l = logins();
        let tip = icon_tooltip(&groups(&l.pools, &l.accounts), at(13, 0), &zone());
        assert_eq!(tip, "Usage\nClaude: 78% left\nSide projects: 78% left\nEvenings: at its limit, back at 3:40 pm\nNot in a pool: 95% left");
        assert_eq!(pool_symbol(&l.pools[0], &l.accounts), "sparkle");
        assert_eq!(pool_symbol(&l.pools[1], &l.accounts), "hammer");
    }

    #[test]
    fn the_pot_fills_to_the_room_and_refills_only_on_a_reset() {
        let l = logins();
        assert_eq!(honey_level(usage(&l.accounts[0]).as_ref()), 0.78);
        assert_eq!(honey_level(usage(&l.accounts[2]).as_ref()), 0., "at its limit");
        assert_eq!(honey_level(None), 0.);
        assert!(refilled(0.4, 1., true));
        assert!(!refilled(0.4, 1., false), "a change of login isn't a refill");
        assert!(!refilled(0.9, 0.95, true), "nor is a little room coming back");
    }

    #[test]
    fn each_login_keeps_its_colour() {
        let l = logins();
        assert_eq!(login_colour("claude-main", &l.accounts, true), Some(0x3987E5));
        assert_eq!(login_colour("claude-2", &l.accounts, false), Some(0xEB6834));
        assert_eq!(login_colour("nobody", &l.accounts, false), None);
        assert_eq!(login_label("claude-main", &l.accounts), "mike");
        assert_eq!(login_label("codex-main", &l.accounts), "Codex");
    }

    #[test]
    fn the_cards_say_what_each_login_has_left() {
        let l = logins();
        let z = zone();
        let now = at(13, 0);
        let c = cards(&l.pools, &l.accounts, None, true, now, &z);
        assert_eq!(c.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Claude", "Side projects", "Evenings", "Not in a pool"]);
        let main = &c[0].sections[0].logins[0];
        assert_eq!(main.name, "mike@example.com");
        assert_eq!(main.tip, "mike@example.com, Max");
        assert_eq!(main.meters.iter().map(|m| format!("{} {}", m.title, m.left)).collect::<Vec<_>>(), vec!["Weekly 78% left", "Weekly Fable 71% left"], "the model's own week under the main one");
        assert_eq!(main.more, vec!["$48.85 extra usage spent"], "the shared part of the notice goes to the foot");
        assert!(!main.dot, "no colours before the stats");
        let blocked = &c[0].sections[0].logins[1];
        assert_eq!(blocked.status, Some(LoginStatus::LimitReached));
        assert_eq!(blocked.meters[0].verdict.as_ref().map(|v| v.text.as_str()), Some("Used up, back 3:40 pm"));
        assert!(blocked.meters[0].short);
        // The mixed pool names its providers.
        assert_eq!(c[1].sections.iter().map(|s| s.title.clone()).collect::<Vec<_>>(), vec![Some("Codex".to_string()), Some("Claude".to_string())]);
        assert_eq!(c[0].sections[0].title, None);
        let codex = &c[1].sections[0].logins[0];
        assert_eq!(codex.more, vec!["64,390 credits · 2 limit resets available"], "the credits once, and the roomier bucket not at all");
        assert_eq!(c[3].sections[0].logins[1].status, Some(LoginStatus::NothingKnown));
        assert_eq!((c[0].symbol.as_str(), c[3].symbol.as_str()), ("sparkle", "tray"));
        assert_eq!(footer(&l.accounts, now, &z).as_deref(), Some("Usage from Token Stats at 11:15 am. Couldn't check usage just now."));
        assert!(!needs_newer_runner(&l.accounts));
        assert!(needs_newer_runner(&l.accounts[4..]));
    }

    #[test]
    fn a_stale_login_says_since_when() {
        let mut l = logins();
        l.accounts[0].usage = Some(json!({ "stale": true, "readAt": iso_time(at(11, 55)), "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 40 }] }));
        let c = cards(&l.pools, &l.accounts, None, true, at(13, 0), &zone());
        assert_eq!(c[0].sections[0].logins[0].more, vec!["Numbers from 11:55 am"]);
    }

    #[test]
    fn the_settings_switch_hides_a_models_own_week() {
        let mut l = logins();
        assert_eq!(model_week_titles(&l.accounts), vec!["Weekly Fable"]);
        // A second login with the same model's week names it once.
        l.accounts[1].usage = Some(json!({ "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 90 }, { "kind": "weekly_model", "title": "Weekly Fable", "remainingPercent": 97 }] }));
        let titles = model_week_titles(&l.accounts);
        assert_eq!(titles, vec!["Weekly Fable"]);
        assert_eq!(model_weeks_switch(&titles), "Show Weekly Fable");
        let on = cards(&l.pools, &l.accounts, None, true, at(13, 0), &zone());
        let off = cards(&l.pools, &l.accounts, None, false, at(13, 0), &zone());
        let main = &off[0].sections[0].logins[0];
        assert_eq!(on[0].sections[0].logins[0].meters.len(), 2);
        assert_eq!(main.meters.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), vec!["Weekly"], "only the main windows");
        assert_eq!(main.more, on[0].sections[0].logins[0].more, "and nothing else changes");
        // No login with a model's own week: no switch to show.
        assert!(model_week_titles(&l.accounts[2..]).is_empty());
    }

    #[test]
    fn the_tiles_say_today_the_last_hour_and_the_week() {
        let z = zone();
        let l = logins();
        let now = at(14, 30);
        let s = stats(vec![("claude-main", vec![hour_of(at_on(26, 0, 0), 7_000_000), hour_of(at(9, 0), 2_000_000), hour_of(at(14, 0), 500_000)])]);
        let t = tiles(&s, &l.accounts, now, &z);
        assert_eq!(t.iter().map(|t| (t.title, t.value.as_str())).collect::<Vec<_>>(), vec![("Today", "2.5M"), ("Last hour", "500k"), ("7 days", "9.5M")]);
        assert_eq!(t[0].detail, None, "no usual day without a week of stats");
        assert_eq!(t[1].detail.as_deref(), Some("172k/h today"));
        assert_eq!(t[2].detail.as_deref(), Some("1.4M a day"));
        assert_eq!(t[1].tip, "Tokens in the last 60 minutes, across every login. Today has averaged 172k an hour.");
    }

    #[test]
    fn the_chart_says_its_range_and_marks() {
        let z = zone();
        let now = at(14, 30);
        let l = logins();
        let s = stats(vec![("claude-main", vec![hour_of(at(9, 0), 72_000_000)]), ("claude-2", vec![hour_of(at_on(26, 0, 0), 10)])]);
        let today_chart = chart(&s, &l.accounts, Range::Today, now, &z);
        assert_eq!(today_chart.title, "Tokens by hour");
        assert_eq!(today_chart.ticks.iter().map(|(_, w)| w.as_str()).collect::<Vec<_>>(), vec!["12 am", "6 am", "12 pm", "6 pm"]);
        assert_eq!(today_chart.slots.len(), 24);
        assert_eq!(today_chart.spending, vec!["claude-main".to_string()], "claude-2 spent nothing today");
        assert_eq!(today_chart.value_ticks, vec![0, 50_000_000, 100_000_000], "a 72M bar under a 100M top, as the Swift chart has it");
        assert_eq!(today_chart.slot_at(at(9, 40)), Some(at(9, 0)));
        assert_eq!(today_chart.now, Some(now));
        assert_eq!(today_chart.slot_words(at(13, 0), now, &z), "1 pm");
        let week = chart(&s, &l.accounts, Range::Week, now, &z);
        assert_eq!(week.title, "Tokens by day");
        assert_eq!(week.ticks.first().map(|(_, w)| w.as_str()), Some("Fri"), "six days before a Thursday");
        assert_eq!(week.ticks.last().map(|(_, w)| w.as_str()), Some("Today"));
        assert_eq!(week.spending, vec!["claude-main".to_string(), "claude-2".to_string()], "in the logins' own order");
        assert_eq!(week.now, None);
        let empty = chart(&Stats::default(), &l.accounts, Range::Today, now, &z);
        assert_eq!(empty.empty, Some("Reading the transcripts…"));
        assert_eq!(value_ticks(40_000_000, 3), vec![0, 20_000_000, 40_000_000]);
        assert_eq!(value_ticks(0, 3), vec![0]);
        assert_eq!(Range::from_name("week"), Some(Range::Week));
    }

    #[test]
    fn the_week_chart_runs_to_the_reset() {
        let z = zone();
        let now = at(12, 0);
        // Half gone three days in: runs out a day before the reset.
        let w = weekly(50., now + 4 * DAY);
        let last_week = [Sample { at: now - 6 * DAY, remaining: 70. }, Sample { at: now - 4 * DAY, remaining: 30. }];
        let meter = Meter::new(&w, &last_week, false, now, &z);
        assert_eq!(meter.verdict.as_ref().map(|v| v.text.as_str()), Some("Runs out Sun 12 pm, 1 day before it resets"));
        assert_eq!(meter.tip, "50% left, resets Mon 12 pm. Going at about 17% a day, so it runs out Sun 12 pm");
        let c = meter.week_chart(now, &z);
        assert_eq!((c.from, c.to), (now - WEEK, now + 4 * DAY));
        assert!(c.short, "the dashed line is orange");
        let end = c.ahead.last().unwrap();
        assert!((end.at - (now + 3 * DAY)).abs() < 1000 && end.remaining == 0.);
        assert_eq!(c.runs.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2], "last week, then this one from full");
        assert_eq!(c.noons.first().map(|(_, d)| d.as_str()), Some("Fri"));
        assert_eq!(c.noons.len(), 10);
        // Old numbers draw no pace.
        let stale = Meter::new(&w, &last_week, true, now, &z).week_chart(now, &z);
        assert!(stale.ahead.is_empty() && stale.runs_out.is_none());
    }

    #[test]
    fn the_last_days_readings_can_turn_spare_into_short() {
        let z = zone();
        let now = at(12, 0);
        // 80 left three days in is a quiet week, but 20 points in the last
        // 15 hours runs it out before the reset.
        let w = weekly(80., now + 4 * DAY);
        assert_eq!(verdict(&w, Pace::of(&w, &[], now).as_ref(), false, now, &z).map(|v| v.tone), Some(Tone::Spare));
        let busy = [Sample { at: now - 15 * HOUR, remaining: 100. }];
        let pace = Pace::of(&w, &busy, now).unwrap();
        assert_eq!(pace.rate_text("weekly"), "32% a day");
        assert_eq!(verdict(&w, Some(&pace), false, now, &z).map(|v| v.tone), Some(Tone::Short));
    }

    #[test]
    fn the_lines_are_the_swift_apps() {
        let l = logins();
        let lines = lines(&l.pools, &l.accounts, None, at(13, 0), &zone());
        assert_eq!(&lines[..3], &["# Claude", "## Claude", "mike@example.com (Max): Weekly 78%, Weekly Fable 71% [Couldn't check usage just now. · $48.85 extra usage spent]"]);
        assert!(lines.contains(&"new@example.com: nothing known".to_string()));
        assert!(lines.contains(&"mike+4@example.com, limit reached: 5-hour 0%, Weekly 31%".to_string()));
        assert_eq!(lines.last().map(String::as_str), Some("Usage from Token Stats at 11:15 am"));
    }
}
