// Settings › Pools' editing (Swift's `Pools`, its "Settings" part): what the
// runner would refuse in `pools.save`, the logins a pool lists, ticking and
// moving them, renaming, and a new pool's id. The list is edited as a draft
// and saved whole when the runner would take it.
use crate::logins::{Account, Pool};
use serde_json::{json, Value};

/// The pool's logins the runner has, in the pool's order (`Pools.logins`).
pub fn logins<'a>(pool: &Pool, accounts: &'a [Account]) -> Vec<&'a Account> {
    pool.account_ids.iter().filter_map(|id| accounts.iter().find(|a| &a.id == id)).collect()
}

/// The providers in the pool, in the order its logins first name them.
pub fn providers(pool: &Pool, accounts: &[Account]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for account in logins(pool, accounts) {
        let id = account.provider_id().to_owned();
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

/// The pool's icon: the one picked, else its first provider's (Claude's
/// sparkle, Codex's brackets), else a stack (`Pools.symbol`).
pub fn symbol(pool: &Pool, accounts: &[Account]) -> String {
    if let Some(icon) = pool.icon.as_deref().filter(|i| !i.is_empty()) {
        return icon.to_owned();
    }
    match providers(pool, accounts).first().map(String::as_str) {
        Some("claude") => "sparkle".into(),
        Some("codex") => "chevron.left.forwardslash.chevron.right".into(),
        Some(_) => "cpu".into(),
        None => "square.stack".into(),
    }
}

/// Why the runner would refuse this list, in words, or None when it's fine
/// (`Pools.problem`): Settings says this rather than send a list it knows
/// will bounce.
pub fn problem(pools: &[Pool], accounts: &[Account]) -> Option<String> {
    if pools.is_empty() {
        return Some("Keep at least one pool.".into());
    }
    let mut seen = std::collections::HashSet::new();
    for pool in pools {
        let name = pool.title.trim();
        if name.is_empty() {
            return Some("Every pool needs a name.".into());
        }
        if !seen.insert(pool.id.as_str()) {
            return Some(format!("Two pools have the id {}.", pool.id));
        }
        if pool.account_ids.is_empty() {
            return Some(format!("{name} needs at least one login."));
        }
        if !accounts.is_empty() {
            if let Some(unknown) = pool.account_ids.iter().find(|id| !accounts.iter().any(|a| &a.id == *id)) {
                return Some(format!("{name} has a login the runner doesn't know ({unknown})."));
            }
        }
    }
    None
}

/// A pool's logins as Settings lists them: its own, ticked, in its order
/// (which decides the provider tried first), then the rest in the runner's.
pub fn login_list<'a>(pool: &Pool, accounts: &'a [Account]) -> Vec<(&'a Account, bool)> {
    let own = logins(pool, accounts);
    let rest = accounts.iter().filter(|a| !own.iter().any(|o| o.id == a.id)).map(|a| (a, false));
    own.iter().map(|a| (*a, true)).chain(rest).collect()
}

/// Ticks or unticks a login; a newly ticked one goes last.
pub fn setting(pool: &Pool, login: &str, included: bool) -> Pool {
    let mut pool = pool.clone();
    pool.account_ids.retain(|id| id != login);
    if included {
        pool.account_ids.push(login.to_owned());
    }
    pool
}

/// Moves a ticked login one place up (`step` -1) or down (1).
pub fn moving_by(pool: &Pool, login: &str, step: i32) -> Pool {
    let mut pool = pool.clone();
    if let Some(index) = pool.account_ids.iter().position(|id| id == login) {
        let target = index as i64 + step as i64;
        if (0..pool.account_ids.len() as i64).contains(&target) {
            pool.account_ids.swap(index, target as usize);
        }
    }
    pool
}

/// A ticked login dropped on another: it takes that one's place, and the
/// rest shift along.
pub fn moving_onto(pool: &Pool, login: &str, target: &str) -> Pool {
    let mut pool = pool.clone();
    let (Some(from), Some(to)) = (pool.account_ids.iter().position(|id| id == login), pool.account_ids.iter().position(|id| id == target)) else { return pool };
    if login == target {
        return pool;
    }
    let moved = pool.account_ids.remove(from);
    pool.account_ids.insert(to, moved);
    pool
}

/// The pool with a new name, trimmed, or None when there's nothing to save:
/// a blank name (the runner refuses those) or the same one. The id stays,
/// so threads and folders on the pool keep it.
pub fn renaming(pool: &Pool, title: &str) -> Option<Pool> {
    let title = title.trim();
    if title.is_empty() || title == pool.title {
        return None;
    }
    let mut pool = pool.clone();
    pool.title = title.to_owned();
    Some(pool)
}

/// An id for a new pool from its title: lowercase words joined by dashes,
/// with "-2", "-3" when it's taken.
pub fn new_id(title: &str, taken: &[String]) -> String {
    let words: Vec<String> = title.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect();
    let base = if words.is_empty() { "pool".to_owned() } else { words.join("-") };
    if !taken.contains(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !taken.contains(id)).unwrap_or(base)
}

/// A pool as `pools.save` takes it.
pub fn to_json(pool: &Pool) -> Value {
    let mut v = json!({ "id": pool.id, "title": pool.title, "accountIds": pool.account_ids, "folders": pool.folders });
    if let Some(icon) = pool.icon.as_deref().filter(|i| !i.is_empty()) {
        v["icon"] = json!(icon);
    }
    v
}

/// A pool's logins and folders under its name in the list, in grey
/// (`PoolListRow`): "mike, work · 2 folders".
pub fn list_detail(pool: &Pool, accounts: &[Account]) -> String {
    let names: Vec<String> = logins(pool, accounts).iter().map(|a| crate::logins::short_account_name(&a.id, accounts)).collect();
    let mut detail = if names.is_empty() { "No logins".to_owned() } else { names.join(", ") };
    match pool.folders.len() {
        0 => {}
        1 => detail.push_str(" · 1 folder"),
        n => detail.push_str(&format!(" · {n} folders")),
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, provider: &str) -> Account {
        serde_json::from_value(json!({ "id": id, "provider": provider })).unwrap()
    }
    fn pool(id: &str, logins: &[&str]) -> Pool {
        Pool { id: id.into(), title: id.to_uppercase(), account_ids: logins.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn says_what_the_runner_would_refuse() {
        let accounts = [account("mike", "claude"), account("codex-1", "codex")];
        assert_eq!(problem(&[], &accounts).as_deref(), Some("Keep at least one pool."));
        assert_eq!(problem(&[pool("a", &[])], &accounts).as_deref(), Some("A needs at least one login."));
        assert_eq!(problem(&[pool("a", &["mike"]), pool("a", &["mike"])], &accounts).as_deref(), Some("Two pools have the id a."));
        assert_eq!(problem(&[pool("a", &["ghost"])], &accounts).as_deref(), Some("A has a login the runner doesn't know (ghost)."));
        let mut blank = pool("a", &["mike"]);
        blank.title = "  ".into();
        assert_eq!(problem(&[blank], &accounts).as_deref(), Some("Every pool needs a name."));
        assert_eq!(problem(&[pool("a", &["mike"])], &accounts), None);
    }

    #[test]
    fn lists_ticks_and_moves_logins() {
        let accounts = [account("a", "claude"), account("b", "claude"), account("c", "codex")];
        let p = pool("p", &["c", "a"]);
        let list: Vec<(&str, bool)> = login_list(&p, &accounts).into_iter().map(|(a, on)| (a.id.as_str(), on)).collect();
        assert_eq!(list, [("c", true), ("a", true), ("b", false)]);
        assert_eq!(providers(&p, &accounts), ["codex", "claude"]);
        assert_eq!(symbol(&p, &accounts), "chevron.left.forwardslash.chevron.right");
        assert_eq!(setting(&p, "b", true).account_ids, ["c", "a", "b"]);
        assert_eq!(setting(&p, "c", false).account_ids, ["a"]);
        assert_eq!(moving_by(&p, "a", -1).account_ids, ["a", "c"]);
        assert_eq!(moving_by(&p, "a", 1).account_ids, ["c", "a"], "already last");
        let three = pool("p", &["a", "b", "c"]);
        assert_eq!(moving_onto(&three, "c", "a").account_ids, ["c", "a", "b"]);
    }

    #[test]
    fn renames_and_names_new_pools() {
        let p = pool("work", &["a"]);
        assert_eq!(renaming(&p, "  Day job ").map(|p| (p.id, p.title)), Some(("work".into(), "Day job".into())));
        assert!(renaming(&p, "   ").is_none());
        assert!(renaming(&p, "WORK").is_none());
        let taken = vec!["new-pool".to_owned(), "new-pool-2".to_owned()];
        assert_eq!(new_id("New pool", &taken), "new-pool-3");
        assert_eq!(new_id("!!", &[]), "pool");
    }
}
