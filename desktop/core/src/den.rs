// The About window's few lines (the Den, Swift's DenLines): about you, from
// the threads the app already has, and the build. A line with nothing to say
// is left out, so a new install's About is short. The tour's made-up threads
// don't count.
use crate::inbox::{parse_ms, ThreadSummary};
use chrono::{Datelike, NaiveDate, TimeZone};
use std::collections::HashMap;

pub const CREDIT: &str = "Made in Busselton by Mike and a lot of bears.";

/// "212 threads since 3 Oct.", the busiest day, and how many are hibernating.
pub fn lines<'a, Tz: TimeZone>(threads: impl IntoIterator<Item = &'a ThreadSummary>, now_ms: i64, tz: &Tz) -> Vec<String> {
    let real: Vec<&ThreadSummary> = threads.into_iter().filter(|t| t.sample != Some(true)).collect();
    let days: Vec<NaiveDate> = real.iter().filter_map(|t| t.created_at.as_deref().and_then(parse_ms)).filter_map(|ms| tz.timestamp_millis_opt(ms).single()).map(|d| d.date_naive()).collect();
    let mut out = Vec::new();
    if let Some(first) = days.iter().min() {
        let count = real.len();
        out.push(format!("{} {} since {}.", number(count), if count == 1 { "thread" } else { "threads" }, day(*first)));
    }
    let mut by_day: HashMap<NaiveDate, usize> = HashMap::new();
    for d in &days {
        *by_day.entry(*d).or_default() += 1;
    }
    // The most recent of any tie, so the answer doesn't change between looks.
    if let Some((busiest, count)) = by_day.into_iter().max_by_key(|(d, n)| (*n, *d)) {
        if count > 1 {
            out.push(format!("Busiest day: {}, {} threads. Sorry.", day(busiest), number(count)));
        }
    }
    let hibernating = real.iter().filter(|t| !t.is_archived() && t.snoozed(now_ms)).count();
    if hibernating > 0 {
        out.push(format!("{} hibernating. Snoring, mostly.", number(hibernating)));
    }
    out
}

/// "Stitched together from fa772646 on 7 Nov." None when the build doesn't
/// say (a debug build).
pub fn build<Tz: TimeZone>(short: &str, built_at: &str, tz: &Tz) -> Option<String> {
    if short.is_empty() {
        return None;
    }
    Some(match parse_ms(built_at).and_then(|ms| tz.timestamp_millis_opt(ms).single()) {
        Some(at) => format!("Stitched together from {short} on {}.", day(at.date_naive())),
        None => format!("Stitched together from {short}."),
    })
}

/// "7 Nov", as en_AU's day and short month.
fn day(d: NaiveDate) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", d.day(), MONTHS[d.month0() as usize])
}

/// "1,234".
fn number(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn thread(id: &str, created: &str) -> ThreadSummary {
        ThreadSummary { id: id.into(), created_at: Some(created.into()), ..Default::default() }
    }

    fn perth() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    #[test]
    fn counts_threads_since_the_first_and_the_busiest_day() {
        let threads = vec![thread("a", "2026-10-03T01:00:00.000Z"), thread("b", "2026-11-07T02:00:00.000Z"), thread("c", "2026-11-07T03:00:00.000Z")];
        let now = parse_ms("2026-11-12T00:00:00.000Z").unwrap();
        assert_eq!(lines(&threads, now, &perth()), vec!["3 threads since 3 Oct.", "Busiest day: 7 Nov, 2 threads. Sorry."]);
    }

    #[test]
    fn days_are_local_and_ties_go_to_the_latest() {
        // 4 Oct 18:00 UTC is 5 Oct in Perth.
        let threads = vec![thread("a", "2026-10-04T18:00:00.000Z"), thread("b", "2026-10-05T01:00:00.000Z"), thread("c", "2026-10-09T01:00:00.000Z"), thread("d", "2026-10-09T02:00:00.000Z")];
        let out = lines(&threads, 0, &perth());
        assert_eq!(out[0], "4 threads since 5 Oct.");
        assert_eq!(out[1], "Busiest day: 9 Oct, 2 threads. Sorry.");
    }

    #[test]
    fn samples_dont_count_and_one_thread_says_thread() {
        let mut sample = thread("s", "2026-01-01T00:00:00.000Z");
        sample.sample = Some(true);
        let threads = vec![sample, thread("a", "2026-11-01T00:00:00.000Z")];
        assert_eq!(lines(&threads, 0, &perth()), vec!["1 thread since 1 Nov."]);
    }

    #[test]
    fn hibernating_threads_are_counted() {
        let mut asleep = thread("a", "2026-11-01T00:00:00.000Z");
        asleep.snoozed_until = Some("2026-11-20T00:00:00.000Z".into());
        let now = parse_ms("2026-11-12T00:00:00.000Z").unwrap();
        let out = lines(&[asleep], now, &perth());
        assert_eq!(out.last().map(String::as_str), Some("1 hibernating. Snoring, mostly."));
    }

    #[test]
    fn nothing_to_say_says_nothing() {
        assert!(lines(&[], 0, &perth()).is_empty());
    }

    #[test]
    fn the_build_line() {
        assert_eq!(build("fa772646", "2026-11-07T03:00:00Z", &perth()).as_deref(), Some("Stitched together from fa772646 on 7 Nov."));
        assert_eq!(build("fa772646", "", &perth()).as_deref(), Some("Stitched together from fa772646."));
        assert_eq!(build("", "2026-11-07T03:00:00Z", &perth()), None);
    }

    #[test]
    fn big_numbers_have_commas() {
        assert_eq!(number(1234567), "1,234,567");
        assert_eq!(number(999), "999");
    }
}
