// The composer's login picker and the model picker's pinned line: the logins
// grouped by provider, each with its note and the room it has left
// (AccountMenu and AccountUsageText.menuLine and subtitle in
// AccountUsageText.swift), and where Unpin puts a thread pinned to a login
// (Pools.unpinTarget, ThreadSettings.pinnedLine). The GPUI app draws these
// (desktop/gpui/src/composer_account.rs); its scripts read the same lines.
use crate::logins::{provider_title, Account, Pool};
use crate::new_thread::{chip_badge, login_name, main_windows, new_thread_pool, percent, pool_for_folder, pool_logins, reset_time, room, tightest, usage_of, AccountUsage, ChipBadge, UsageWindow};
use chrono::{DateTime, TimeZone};

/// "5h", "week": the short names the menu uses (AccountUsageText.shortName).
fn short_name(window: &UsageWindow) -> String {
    match window.kind.as_str() {
        "five_hour" => "5h".to_owned(),
        "weekly" => "week".to_owned(),
        _ => window.title.clone(),
    }
}

/// A moment in milliseconds in `now`'s zone.
fn at<Tz: TimeZone>(ms: i64, now: &DateTime<Tz>) -> DateTime<Tz> {
    now.timezone().timestamp_millis_opt(ms).single().unwrap_or_else(|| now.clone())
}

/// The menu's usage line (AccountUsageText.menuLine): "5h 89% · week 78%,
/// resets Thu 1 pm", the reset being the tighter window's; "Limit reached,
/// resets 3:40 pm" for a login at its limit. None with nothing known.
pub fn menu_line<Tz: TimeZone>(usage: &AccountUsage, now: &DateTime<Tz>) -> Option<String> {
    let main = main_windows(usage);
    if usage.blocked {
        let reset = main.iter().filter(|w| w.remaining_percent < 1.).filter_map(|w| w.resets_at).min().or_else(|| main.iter().filter_map(|w| w.resets_at).min());
        return Some(match reset {
            Some(ms) => format!("Limit reached, resets {}", reset_time(&at(ms, now), now)),
            None => "Limit reached".to_owned(),
        });
    }
    if main.is_empty() {
        return None;
    }
    let mut line = main.iter().map(|w| format!("{} {}", short_name(w), percent(w.remaining_percent))).collect::<Vec<_>>().join(" · ");
    if let Some(ms) = tightest(usage).and_then(|w| w.resets_at) {
        line.push_str(&format!(", resets {}", reset_time(&at(ms, now), now)));
    }
    Some(line)
}

/// A login's line under its name (AccountUsageText.subtitle): its note,
/// then the room left.
pub fn subtitle<Tz: TimeZone>(account: &Account, now: &DateTime<Tz>) -> Option<String> {
    let parts: Vec<String> = [account.note.clone(), usage_of(account).and_then(|u| menu_line(&u, now))].into_iter().flatten().filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// A login in the picker (AccountMenu.Item).
#[derive(Clone, Debug, PartialEq)]
pub struct LoginItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub is_selected: bool,
    pub is_blocked: bool,
    /// Under a quarter left (orange) or at its limit (red), as on the chip.
    pub badge: Option<ChipBadge>,
}

/// A provider's logins (AccountMenu.Section).
#[derive(Clone, Debug, PartialEq)]
pub struct LoginSection {
    pub id: String,
    pub title: String,
    pub items: Vec<LoginItem>,
}

/// The picker's sections (AccountMenu.sections): one a provider, Claude then
/// Codex then any other in the order they come, each login by its address
/// with its note and room under it. A login at its limit stays pickable.
pub fn login_sections<Tz: TimeZone>(accounts: &[Account], selected: &str, now: &DateTime<Tz>) -> Vec<LoginSection> {
    let mut sections: Vec<LoginSection> = Vec::new();
    for account in accounts {
        let provider = account.provider_id().to_owned();
        let item = LoginItem {
            id: account.id.clone(),
            title: account.name(),
            subtitle: subtitle(account, now),
            is_selected: account.id == selected,
            is_blocked: usage_of(account).is_some_and(|u| u.blocked),
            badge: chip_badge(Some(account)),
        };
        match sections.iter_mut().find(|s| s.id == provider) {
            Some(section) => section.items.push(item),
            None => sections.push(LoginSection { title: provider_title(&provider), id: provider, items: vec![item] }),
        }
    }
    let rank = |id: &str| ["claude", "codex"].iter().position(|p| *p == id).unwrap_or(2);
    // Stable, so the others keep their order.
    sections.sort_by_key(|s| rank(&s.id));
    sections
}

/// The picker as text, for the automation's `composer-menu` (AccountMenu.
/// lines): `#` on a section, `[x]` on the chosen login, its line after a
/// dash, then where the numbers came from.
pub fn login_lines(sections: &[LoginSection], footer: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    for section in sections {
        lines.push(format!("# {}", section.title));
        for item in &section.items {
            let mut line = format!("{}{}", if item.is_selected { "[x] " } else { "[ ] " }, item.title);
            if let Some(subtitle) = &item.subtitle {
                line.push_str(&format!(" - {subtitle}"));
            }
            lines.push(line);
        }
    }
    if let Some(footer) = footer {
        lines.push(format!("--- {footer}"));
    }
    lines
}

/// "Pinned to gmail", under the model picker's effort levels for a thread
/// pinned to a login (ThreadSettings.pinnedLine).
pub fn pinned_line(account_id: &str, accounts: &[Account]) -> String {
    format!("Pinned to {}", login_name(account_id, accounts))
}

/// The pool Unpin puts a pinned thread in (Pools.unpinTarget): one holding
/// its login, so it stays where it is until that login runs dry, its
/// folder's or the last one picked first; with none holding it, the pool a
/// new thread there would start in.
pub fn unpin_target(pools: &[Pool], account_id: &str, cwd: &str, home: &str, last: Option<&str>) -> Option<String> {
    let holding: Vec<&Pool> = pools.iter().filter(|p| p.account_ids.iter().any(|a| a == account_id)).collect();
    let folder = pool_for_folder(pools, cwd, home);
    for candidate in [folder.as_deref(), last].into_iter().flatten() {
        if holding.iter().any(|p| p.id == candidate) {
            return Some(candidate.to_owned());
        }
    }
    if let Some(first) = holding.first() {
        return Some(first.id.clone());
    }
    new_thread_pool(pools, None, cwd, home, last, None)
}

// ---- The model picker's Login line ----
//
// Mike picked this on 12 Nov 2026 over logins in the rail, a strip of pills
// and a submenu: one quiet line under the effort levels, "Login: Auto
// (gmail for now)", that opens in place to list the pool's logins. Picking
// one pins the thread to it (`threads.update { accountId }`); Auto puts it
// back in the pool. It took the place of the old "Pinned to … · Unpin".

/// What Auto says under its name in the list.
pub const AUTO_HELP: &str = "The pool picks, and moves on when one runs out";

/// The closed line, in two parts: "Login: Auto" and "(acme for now)" for a
/// thread in a pool, the second only when its login is one of the pool's;
/// "Login: gmail" and nothing for one pinned to a login.
pub fn login_line(account_id: &str, accounts: &[Account], pool: Option<&Pool>) -> (String, Option<String>) {
    match pool {
        None => (format!("Login: {}", login_name(account_id, accounts)), None),
        Some(pool) => {
            let on = (!account_id.is_empty() && pool.account_ids.iter().any(|a| a == account_id) && accounts.iter().any(|a| a.id == account_id)).then(|| format!("({} for now)", login_name(account_id, accounts)));
            ("Login: Auto".to_owned(), on)
        }
    }
}

/// A row of the opened list: Auto (no `id`), or one of the pool's logins.
#[derive(Clone, Debug, PartialEq)]
pub struct PoolLoginRow {
    pub id: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    /// The tightest window's room left, 0 at its limit; None when unknown.
    pub room: Option<f64>,
    /// Orange under a quarter, red at its limit, as on the chip.
    pub badge: Option<ChipBadge>,
    pub is_selected: bool,
}

impl PoolLoginRow {
    /// What's at the row's end: "19%", or "limit".
    pub fn room_words(&self) -> Option<String> {
        if self.badge == Some(ChipBadge::Blocked) {
            return Some("limit".to_owned());
        }
        self.room.map(percent)
    }
}

/// The opened list for `pool` (the rail's column): Auto, ticked when the
/// thread is in this pool, then the pool's logins, the one it's pinned to
/// ticked. `pinned_login` is the login of a pinned thread, listed first when
/// the pool doesn't hold it, so the tick always has a row.
pub fn pool_login_rows<Tz: TimeZone>(pool: &Pool, accounts: &[Account], thread_pool: Option<&str>, pinned_login: Option<&str>, now: &DateTime<Tz>) -> Vec<PoolLoginRow> {
    let mut rows = vec![PoolLoginRow { id: None, title: "Auto".to_owned(), subtitle: Some(AUTO_HELP.to_owned()), room: None, badge: None, is_selected: thread_pool == Some(pool.id.as_str()) }];
    let mut logins: Vec<&Account> = pool_logins(pool, accounts, None);
    if let Some(pinned) = pinned_login.and_then(|id| accounts.iter().find(|a| a.id == id)) {
        if !logins.iter().any(|a| a.id == pinned.id) {
            logins.insert(0, pinned);
        }
    }
    for account in logins {
        let usage = usage_of(account);
        let left = usage.as_ref().and_then(room);
        // A window run dry is at its limit even before the runner's flag
        // says so, as the room cards have it (Codex's review, 12 Nov).
        let blocked = usage.as_ref().is_some_and(|u| u.blocked) || left.is_some_and(|r| r < 1.);
        rows.push(PoolLoginRow {
            id: Some(account.id.clone()),
            title: login_name(&account.id, accounts),
            subtitle: subtitle(account, now),
            room: if blocked { Some(0.) } else { left },
            badge: if blocked { Some(ChipBadge::Blocked) } else { chip_badge(Some(account)) },
            is_selected: thread_pool.is_none() && pinned_login == Some(account.id.as_str()),
        });
    }
    rows
}

/// The line and list as text, for the automation's `composer-menu`: the
/// line after `---`, then (opened or not) each row as `[x] title - line`.
pub fn pool_login_lines(line: &(String, Option<String>), rows: &[PoolLoginRow]) -> Vec<String> {
    let mut lines = vec![match &line.1 {
        Some(on) => format!("--- {} {on}", line.0),
        None => format!("--- {}", line.0),
    }];
    for row in rows {
        let mut text = format!("{}{}", if row.is_selected { "[x] " } else { "[ ] " }, row.title);
        if let Some(sub) = &row.subtitle {
            text.push_str(&format!(" - {sub}"));
        }
        lines.push(text);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use serde_json::json;

    #[test]
    fn the_login_line_says_auto_and_who_for_now_or_the_pinned_login() {
        let accounts = vec![account("claude-main", "claude", "mike@acme.io", None, serde_json::Value::Null), account("claude-1", "claude", "mike@globex.io", None, serde_json::Value::Null)];
        let pool = Pool { id: "claude".into(), title: "Claude".into(), account_ids: vec!["claude-main".into(), "claude-1".into()], icon: None, folders: vec![] };
        assert_eq!(login_line("claude-main", &accounts, Some(&pool)), ("Login: Auto".to_owned(), Some("(acme for now)".to_owned())));
        let other = Pool { account_ids: vec!["claude-1".into()], ..pool.clone() };
        assert_eq!(login_line("claude-main", &accounts, Some(&other)), ("Login: Auto".to_owned(), None), "not one of the pool's logins");
        assert_eq!(login_line("", &accounts, Some(&pool)), ("Login: Auto".to_owned(), None));
        assert_eq!(login_line("claude-1", &accounts, None), ("Login: globex".to_owned(), None));
    }

    #[test]
    fn the_list_is_auto_then_the_pools_logins_with_their_room() {
        let accounts = vec![
            account("claude-main", "claude", "mike@acme.io", None, json!({ "windows": [{ "kind": "five_hour", "title": "5-hour", "remainingPercent": 19 }, { "kind": "weekly", "title": "Weekly", "remainingPercent": 61 }] })),
            account("claude-1", "claude", "mike@globex.io", None, json!({ "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 80 }] })),
            account("claude-2", "claude", "mike@initech.io", None, json!({ "blocked": true, "windows": [] })),
            account("claude-9", "claude", "mike@umbrella.io", None, serde_json::Value::Null),
        ];
        let pool = Pool { id: "claude".into(), title: "Claude".into(), account_ids: vec!["claude-main".into(), "claude-1".into(), "claude-2".into()], icon: None, folders: vec![] };
        let rows = pool_login_rows(&pool, &accounts, Some("claude"), None, &now());
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Auto", "acme", "globex", "initech"]);
        assert!(rows[0].is_selected && rows.iter().skip(1).all(|r| !r.is_selected), "in the pool: Auto");
        assert_eq!(rows[1].room_words().as_deref(), Some("19%"));
        assert_eq!(rows[1].badge, Some(ChipBadge::Low("19%".into())));
        assert_eq!((rows[2].room_words().as_deref(), &rows[2].badge), (Some("80%"), &None));
        assert_eq!((rows[3].room_words().as_deref(), &rows[3].badge), (Some("limit"), &Some(ChipBadge::Blocked)));
        // A window at 0% with no flag yet is at its limit too.
        let dry = vec![account("claude-main", "claude", "mike@acme.io", None, json!({ "windows": [{ "kind": "five_hour", "title": "5-hour", "remainingPercent": 0.4 }] }))];
        let one = Pool { account_ids: vec!["claude-main".into()], ..pool.clone() };
        assert_eq!(pool_login_rows(&one, &dry, Some("claude"), None, &now())[1].room_words().as_deref(), Some("limit"));
        // Pinned to one of them: that one ticked, Auto not.
        let pinned = pool_login_rows(&pool, &accounts, None, Some("claude-1"), &now());
        assert_eq!(pinned.iter().filter(|r| r.is_selected).map(|r| r.title.as_str()).collect::<Vec<_>>(), ["globex"]);
        // Pinned to a login the pool hasn't got: listed first, ticked.
        let outside = pool_login_rows(&pool, &accounts, None, Some("claude-9"), &now());
        assert_eq!((outside[1].title.as_str(), outside[1].is_selected, outside[1].room_words()), ("umbrella", true, None));
        // In another pool: nothing here ticked.
        assert!(pool_login_rows(&pool, &accounts, Some("convex"), None, &now()).iter().all(|r| !r.is_selected));
        assert_eq!(
            pool_login_lines(&login_line("claude-main", &accounts, Some(&pool)), &rows[..2]),
            ["--- Login: Auto (acme for now)", &format!("[x] Auto - {AUTO_HELP}"), "[ ] acme - 5h 19% · week 61%"]
        );
    }

    fn account(id: &str, provider: &str, email: &str, note: Option<&str>, usage: serde_json::Value) -> Account {
        Account { id: id.into(), provider: provider.into(), title: email.into(), email: Some(email.into()), note: note.map(str::to_owned), usage: (!usage.is_null()).then_some(usage), ..Account::default() }
    }

    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap()
    }

    #[test]
    fn a_logins_line_says_its_note_and_room() {
        let usage = json!({ "windows": [
            { "kind": "five_hour", "title": "5-hour", "remainingPercent": 89.6, "resetsAt": "2026-11-11T13:40:00Z" },
            { "kind": "weekly", "title": "Weekly", "remainingPercent": 78, "resetsAt": "2026-11-12T13:00:00Z" },
            { "kind": "weekly_model", "title": "Weekly Fable", "remainingPercent": 2, "resetsAt": "2026-11-12T13:00:00Z" },
        ] });
        let main = account("claude-main", "claude", "mike@example.com", Some("Convex work only"), usage);
        // The tighter main window's reset; a model's own week never decides it.
        assert_eq!(subtitle(&main, &now()).as_deref(), Some("Convex work only · 5h 89% · week 78%, resets tomorrow 1 pm"));
        let blocked = account("claude-4", "claude", "mike+4@example.com", None, json!({ "blocked": true, "windows": [
            { "kind": "five_hour", "title": "5-hour", "remainingPercent": 0, "resetsAt": "2026-11-11T10:30:00Z" },
            { "kind": "weekly", "title": "Weekly", "remainingPercent": 31, "resetsAt": "2026-11-13T09:00:00Z" },
        ] }));
        assert_eq!(subtitle(&blocked, &now()).as_deref(), Some("Limit reached, resets 10:30 am"));
        assert_eq!(subtitle(&account("x", "claude", "x@y.z", None, serde_json::Value::Null), &now()), None);
        assert_eq!(subtitle(&account("x", "claude", "x@y.z", Some(""), json!({ "windows": [] })), &now()), None);
    }

    #[test]
    fn the_picker_groups_logins_by_provider_claude_first() {
        let accounts = vec![
            account("codex-main", "codex", "mike@example.com", None, json!({ "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 12 }] })),
            account("claude-main", "claude", "mike@example.com", None, serde_json::Value::Null),
            account("gemini", "gemini", "g@example.com", None, serde_json::Value::Null),
            account("claude-2", "", "work@example.com", Some("Convex work only"), json!({ "blocked": true, "windows": [] })),
        ];
        let sections = login_sections(&accounts, "claude-2", &now());
        assert_eq!(sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Claude", "Codex", "Gemini"]);
        assert_eq!(sections[0].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["claude-main", "claude-2"], "a login with no provider is Claude's");
        assert!(sections[0].items[1].is_selected && sections[0].items[1].is_blocked);
        assert_eq!(sections[0].items[1].badge, Some(ChipBadge::Blocked));
        assert_eq!(sections[1].items[0].badge, Some(ChipBadge::Low("12%".into())));
        assert_eq!(
            login_lines(&sections, Some("Usage from Token Stats at 9 am")),
            [
                "# Claude",
                "[ ] mike@example.com",
                "[x] work@example.com - Convex work only · Limit reached",
                "# Codex",
                "[ ] mike@example.com - week 12%",
                "# Gemini",
                "[ ] g@example.com",
                "--- Usage from Token Stats at 9 am",
            ]
        );
    }

    #[test]
    fn unpin_puts_a_thread_in_a_pool_with_its_login() {
        let pool = |id: &str, accounts: &[&str], folders: &[&str]| Pool { id: id.into(), title: id.into(), account_ids: accounts.iter().map(|a| a.to_string()).collect(), icon: None, folders: folders.iter().map(|f| f.to_string()).collect() };
        let pools = vec![pool("claude", &["claude-main", "claude-3"], &[]), pool("convex", &["claude-2"], &["~/dev/convex"]), pool("side", &["codex-main", "claude-3"], &[])];
        assert_eq!(unpin_target(&pools, "claude-3", "/Users/m/dev/me", "/Users/m", None).as_deref(), Some("claude"), "the first holding its login");
        assert_eq!(unpin_target(&pools, "claude-3", "/Users/m/dev/me", "/Users/m", Some("side")).as_deref(), Some("side"), "the last picked, when it holds the login");
        assert_eq!(unpin_target(&pools, "claude-3", "/Users/m/dev/me", "/Users/m", Some("convex")).as_deref(), Some("claude"), "not one without the login");
        assert_eq!(unpin_target(&pools, "claude-9", "/Users/m/dev/convex/app", "/Users/m", None).as_deref(), Some("convex"), "no pool holds it: where a new thread there starts");
        assert_eq!(unpin_target(&[], "claude-3", "/", "/Users/m", None), None);
    }
}
