// What fills a Claude thread's context, in Claude's own words
// (`threads.contextUsage`, docs/protocol.md "Context breakdown"), as the
// composer's ring's popover shows it (ContextBreakdown.swift, after Ghostex's
// panel): "32% used", "323k / 1M", a stacked bar, then a row each for what's
// in the window, the buffer kept for compacting, the free space, and the tool
// schemas kept out of it. desktop/gpui/src/composer_context.rs draws it.
//
// The numbers come from whichever runner has the thread, which may be another
// Mac's, so nothing in them may crash this one: a count outside 0 to
// COUNT_LIMIT is read as no count (the row is left out), and the sums
// saturate (Daybreak, 10 Nov 2026).
use crate::composer::ContextMeter;
use serde_json::Value;

/// Far above any model's window, and far below where adding a few hundred
/// rows of it could overflow.
pub const COUNT_LIMIT: i64 = 1 << 40;

fn count(v: &Value) -> Option<i64> {
    let n = v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0. && f.abs() < 9e15).map(|f| f as i64))?;
    (0..=COUNT_LIMIT).contains(&n).then_some(n)
}

/// One of the CLI's categories (ContextUsage.Category).
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    /// "messages", "memory_files"...; absent from an older CLI, and a newer
    /// one may add ids this app doesn't know.
    pub id: Option<String>,
    pub name: String,
    pub tokens: i64,
    /// "used", "buffer", "free" or "deferred".
    pub kind: String,
}

/// The CLI's count (ContextUsage), read leniently: a row that isn't a name
/// and a number is left out rather than losing the lot.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextUsage {
    /// Milliseconds since the epoch.
    pub at: Option<i64>,
    pub model: Option<String>,
    pub total_tokens: i64,
    pub max_tokens: i64,
    pub categories: Vec<Category>,
}

impl ContextUsage {
    pub fn decode(v: &Value) -> Option<ContextUsage> {
        v.as_object()?;
        let categories = v["categories"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|c| Some(Category { id: c["id"].as_str().map(str::to_owned), name: c["name"].as_str()?.to_owned(), tokens: count(&c["tokens"])?, kind: c["kind"].as_str()?.to_owned() }))
                    .collect()
            })
            .unwrap_or_default();
        Some(ContextUsage {
            at: v["at"].as_str().and_then(crate::inbox::parse_ms),
            model: v["model"].as_str().map(str::to_owned),
            total_tokens: count(&v["totalTokens"]).unwrap_or(0),
            max_tokens: count(&v["maxTokens"]).unwrap_or(0),
            categories,
        })
    }
}

/// The runner's answer (ContextUsageAnswer): the breakdown, if there is one;
/// whether the thread has a running CLI to ask again; and why it's an old
/// one, when asking failed.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextAnswer {
    pub usage: Option<ContextUsage>,
    pub live: bool,
    pub note: Option<String>,
}

impl ContextAnswer {
    pub fn decode(v: &Value) -> ContextAnswer {
        ContextAnswer { usage: ContextUsage::decode(&v["usage"]), live: v["live"].as_bool().unwrap_or(false), note: v["note"].as_str().map(str::to_owned) }
    }
}

/// Colour groups, so the view picks the colours and like rows share one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    Messages,
    Memory,
    Skills,
    Agents,
    System,
    Mcp,
    Other,
    Buffer,
    Free,
}

/// A row of the popover.
#[derive(Clone, Debug, PartialEq)]
pub struct BreakdownRow {
    pub name: String,
    pub tokens: i64,
    pub swatch: Swatch,
    /// Tool schemas outside the window until loaded: listed, not counted.
    pub deferred: bool,
    /// Its share of the window, None for a deferred row.
    pub fraction: Option<f64>,
}

impl BreakdownRow {
    /// "610k".
    pub fn token_text(&self) -> String {
        ContextMeter::tokens(self.tokens)
    }

    /// "61%", "1.5%", "<0.1%"; None for a deferred row.
    pub fn percent_text(&self) -> Option<String> {
        self.fraction.map(share)
    }
}

/// The breakdown as the popover draws it (ContextBreakdown).
#[derive(Clone, Debug, PartialEq)]
pub struct ContextBreakdown {
    pub used: i64,
    pub window: i64,
    pub at: Option<i64>,
    pub rows: Vec<BreakdownRow>,
}

/// The order Ghostex lists them in: the conversation first, then what each
/// session starts with. Ids this app doesn't know come after, in the CLI's
/// order.
fn order(id: &str) -> Option<(usize, Swatch)> {
    Some(match id {
        "messages" => (0, Swatch::Messages),
        "memory_files" => (1, Swatch::Memory),
        "skills" => (2, Swatch::Skills),
        "custom_agents" => (3, Swatch::Agents),
        "system_tools" => (4, Swatch::System),
        "system_prompt" => (5, Swatch::System),
        "mcp_instructions" => (6, Swatch::Mcp),
        "mcp_tools" => (7, Swatch::Mcp),
        "autocompact_buffer" | "compact_buffer" => (0, Swatch::Buffer),
        "free_space" => (0, Swatch::Free),
        "mcp_tools_deferred" => (0, Swatch::Mcp),
        "system_tools_deferred" => (1, Swatch::System),
        _ => return None,
    })
}

impl ContextBreakdown {
    pub fn new(usage: &ContextUsage) -> Option<ContextBreakdown> {
        let counted = usage.categories.iter().filter(|c| c.kind != "deferred").fold(0_i64, |sum, c| sum.saturating_add(c.tokens.max(0)));
        let window = if usage.max_tokens > 0 { usage.max_tokens } else { counted };
        if window <= 0 {
            return None;
        }
        let rows = |kind: &str| -> Vec<BreakdownRow> {
            let mut picked: Vec<(usize, &Category)> = usage.categories.iter().enumerate().filter(|(_, c)| c.kind == kind && (c.tokens > 0 || kind == "free")).collect();
            picked.sort_by_key(|(offset, c)| c.id.as_deref().and_then(order).map_or(100 + offset, |(rank, _)| rank));
            picked
                .into_iter()
                .map(|(_, c)| {
                    let swatch = match kind {
                        "buffer" => Swatch::Buffer,
                        "free" => Swatch::Free,
                        _ => c.id.as_deref().and_then(order).map_or(Swatch::Other, |(_, s)| s),
                    };
                    BreakdownRow { name: c.name.clone(), tokens: c.tokens, swatch, deferred: kind == "deferred", fraction: (kind != "deferred").then(|| c.tokens as f64 / window as f64) }
                })
                .collect()
        };
        let mut all = rows("used");
        all.extend(rows("buffer"));
        all.extend(rows("free"));
        all.extend(rows("deferred"));
        Some(ContextBreakdown { used: usage.total_tokens, window, at: usage.at, rows: all })
    }

    /// "32% used".
    pub fn headline(&self) -> String {
        format!("{}% used", whole_percent(self.used as f64 / self.window as f64))
    }

    /// "323k / 1M".
    pub fn amount(&self) -> String {
        format!("{} / {}", ContextMeter::tokens(self.used), ContextMeter::tokens(self.window))
    }

    /// The bar's pieces, left to right: what's used, then the buffer.
    pub fn bar(&self) -> Vec<&BreakdownRow> {
        self.rows.iter().filter(|r| !r.deferred && r.swatch != Swatch::Free).collect()
    }

    /// The popover as text, for the automation's `composer-menu`:
    /// "64% used 642k / 1M", "Messages 610k 61%", and "Breakdown from just now".
    pub fn lines(&self, now_ms: i64) -> Vec<String> {
        let mut lines = vec![format!("{} {}", self.headline(), self.amount())];
        for row in &self.rows {
            lines.push([Some(row.name.clone()), Some(row.token_text()), row.percent_text()].into_iter().flatten().collect::<Vec<_>>().join(" "));
        }
        if let Some(at) = self.at {
            lines.push(format!("Breakdown from {}", age(at, now_ms)));
        }
        lines
    }
}

/// A fraction as a whole percentage, held to 0 to 9999 first.
fn whole_percent(fraction: f64) -> i64 {
    if fraction.is_nan() {
        return 0;
    }
    (fraction.clamp(0., 99.99) * 100.).round() as i64
}

/// "29%", "1.5%", "0.1%", "<0.1%": whole numbers from 10%, one place below,
/// so the small rows don't all read 0%.
pub fn share(fraction: f64) -> String {
    let p = fraction * 100.;
    // Not `p <= 0`: NaN is neither.
    if !(p > 0.) {
        return "0%".to_owned();
    }
    if p >= 10. {
        return format!("{}%", whole_percent(fraction));
    }
    let tenth = (p * 10.).round() / 10.;
    if tenth < 0.1 {
        return "<0.1%".to_owned();
    }
    let s = format!("{tenth:.1}");
    format!("{}%", s.strip_suffix(".0").unwrap_or(&s))
}

/// "just now", "5 min ago", "3 hours ago", "2 days ago".
pub fn age(at_ms: i64, now_ms: i64) -> String {
    let seconds = (now_ms - at_ms) as f64 / 1000.;
    if seconds < 60. {
        return "just now".to_owned();
    }
    let minutes = (seconds / 60.) as i64;
    if minutes < 60 {
        return format!("{minutes} min ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return if hours == 1 { "1 hour ago".to_owned() } else { format!("{hours} hours ago") };
    }
    let days = hours / 24;
    if days == 1 {
        "1 day ago".to_owned()
    } else {
        format!("{days} days ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mock_answer() -> Value {
        // What the mock runner answers for a thread at 642k of 1M.
        json!({ "live": true, "usage": { "at": "2026-11-11T09:00:00Z", "model": "claude-opus-5-5", "totalTokens": 642_000, "maxTokens": 1_000_000, "categories": [
            { "id": "system_prompt", "name": "System prompt", "tokens": 4500, "kind": "used" },
            { "id": "mcp_tools_deferred", "name": "MCP tools (deferred)", "tokens": 173_000, "kind": "deferred" },
            { "id": "memory_files", "name": "Memory files", "tokens": 15_000, "kind": "used" },
            { "id": "messages", "name": "Messages", "tokens": 610_000, "kind": "used" },
            { "id": "new_thing", "name": "Something new", "tokens": 1200, "kind": "used" },
            { "id": "autocompact_buffer", "name": "Autocompact buffer", "tokens": 33_000, "kind": "buffer" },
            { "id": "free_space", "name": "Free space", "tokens": 0, "kind": "free" },
            { "name": "No count" , "kind": "used" },
            { "id": "huge", "name": "Too big", "tokens": 5_000_000_000_000_000_i64, "kind": "used" },
        ] } })
    }

    #[test]
    fn the_breakdown_lists_what_fills_the_context_in_ghostexs_order() {
        let answer = ContextAnswer::decode(&mock_answer());
        assert!(answer.live);
        let breakdown = ContextBreakdown::new(answer.usage.as_ref().unwrap()).unwrap();
        assert_eq!(breakdown.headline(), "64% used");
        assert_eq!(breakdown.amount(), "642k / 1M");
        let names: Vec<&str> = breakdown.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Messages", "Memory files", "System prompt", "Something new", "Autocompact buffer", "Free space", "MCP tools (deferred)"], "rows it can't count are left out");
        assert_eq!(breakdown.rows[3].swatch, Swatch::Other);
        assert!(breakdown.rows[6].deferred && breakdown.rows[6].percent_text().is_none());
        assert_eq!(breakdown.bar().len(), 5, "the bar has what's used and the buffer");
        let at = breakdown.at.unwrap();
        assert_eq!(breakdown.lines(at + 5_000)[..3], ["64% used 642k / 1M".to_owned(), "Messages 610k 61%".to_owned(), "Memory files 15k 1.5%".to_owned()]);
        assert_eq!(breakdown.lines(at + 5_000).last().map(String::as_str), Some("Breakdown from just now"));
        assert!(breakdown.lines(at).contains(&"MCP tools (deferred) 173k".to_owned()));
    }

    #[test]
    fn shares_and_ages_read_as_the_swift_app_says_them() {
        assert_eq!(share(0.61), "61%");
        assert_eq!(share(0.015), "1.5%");
        assert_eq!(share(0.02), "2%");
        assert_eq!(share(0.0004), "<0.1%");
        assert_eq!(share(0.), "0%");
        assert_eq!(share(f64::NAN), "0%");
        assert_eq!(age(0, 59_000), "just now");
        assert_eq!(age(0, 5 * 60_000), "5 min ago");
        assert_eq!(age(0, 3_600_000), "1 hour ago");
        assert_eq!(age(0, 3 * 86_400_000), "3 days ago");
    }

    #[test]
    fn nothing_to_count_is_no_breakdown() {
        let empty = ContextUsage::decode(&json!({ "totalTokens": 5, "maxTokens": 0, "categories": [] })).unwrap();
        assert_eq!(ContextBreakdown::new(&empty), None);
        assert_eq!(ContextAnswer::decode(&json!({ "live": false })).usage, None);
    }
}
