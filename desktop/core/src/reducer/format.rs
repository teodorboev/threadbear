// The small pieces of ThreadbearCore that turn numbers into the words a
// transcript row shows: SafeNumbers.swift, RelativeTime.duration, CostText,
// TurnTokens and TokenText.
use serde_json::{json, Value};

/// `Int(checked:)`: rounding toward zero as `as` does, or nil where Swift's
/// `Int(_:)` would stop the app.
pub fn int_checked(value: f64) -> Option<i64> {
    if value.is_finite() && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
        Some(value as i64)
    } else {
        None
    }
}

/// `Int(saturating:)`. Rust's `as` already saturates and makes NaN 0.
pub fn int_saturating(value: f64) -> i64 {
    value as i64
}

/// `Double.wholeCount(atMost:)`: a whole, finite number from 0 up to `most`.
pub fn whole_count(value: f64, most: f64) -> Option<i64> {
    if value.is_finite() && value >= 0.0 && value <= most && value.trunc() == value {
        int_checked(value)
    } else {
        None
    }
}

/// 2^53 - 1, as `wholeCount` and `lenientInt` allow by default: every number
/// here comes from JavaScript.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `RelativeTime.duration`: "1m 12s", "850 ms", "2h 4m".
pub fn duration(milliseconds: f64) -> String {
    if milliseconds < 1000.0 {
        return format!("{} ms", int_saturating(milliseconds));
    }
    let total = int_saturating(milliseconds / 1000.0);
    if total < 60 {
        return format!("{total}s");
    }
    if total < 3600 {
        return format!("{}m {}s", total / 60, total % 60);
    }
    format!("{}h {}m", total / 3600, (total % 3600) / 60)
}

/// `CostText.format`: small models cost well under a cent a turn.
pub fn cost_text(cost: Option<f64>) -> Option<String> {
    let cost = cost.filter(|c| *c > 0.0)?;
    if cost >= 0.01 {
        Some(format!("${cost:.2}"))
    } else if cost >= 0.001 {
        Some(format!("${cost:.3}"))
    } else {
        Some("<$0.001".to_string())
    }
}

/// `TurnTokens`, as far as the footer rows need: a stretch's footer adds up
/// its turns' tokens, and shows them as Claude's `usage` shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TurnTokens {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

impl TurnTokens {
    /// `TurnTokens(usage:)`. Claude's `{inputTokens, outputTokens,
    /// cacheReadTokens, cacheWriteTokens}`, or Codex's, whose inputTokens
    /// includes its cachedInputTokens. None with no tokens at all.
    pub fn from_usage(usage: Option<&Value>) -> Option<TurnTokens> {
        let usage = usage?.as_object()?;
        let count = |key: &str| int_saturating(usage.get(key).and_then(Value::as_f64).unwrap_or(0.0)).max(0);
        let (input, cache_read) = if usage.contains_key("cachedInputTokens") {
            let cached = count("cachedInputTokens");
            ((count("inputTokens") - cached).max(0), cached)
        } else {
            (count("inputTokens"), count("cacheReadTokens"))
        };
        let tokens = TurnTokens { input, output: count("outputTokens"), cache_read, cache_write: count("cacheWriteTokens") };
        if tokens.read() > 0 || tokens.output > 0 {
            Some(tokens)
        } else {
            None
        }
    }

    /// Everything the model read, the cache included.
    pub fn read(self) -> i64 {
        self.input.saturating_add(self.cache_read).saturating_add(self.cache_write)
    }

    /// Claude's shape, which `from_usage` reads back the same.
    pub fn usage(self) -> Value {
        json!({
            "inputTokens": self.input,
            "outputTokens": self.output,
            "cacheReadTokens": self.cache_read,
            "cacheWriteTokens": self.cache_write,
        })
    }

    /// The footer's tip (`TurnTokens.tip`): "1.7k tokens out, 381k in (337k
    /// cache reads, 44k cache writes, 16 uncached). $0.45 at API prices, not
    /// what a subscription charges."
    pub fn tip(self, cost_usd: Option<f64>) -> String {
        let parts: Vec<String> = [(self.cache_read, "cache reads"), (self.cache_write, "cache writes"), (self.input, "uncached")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| format!("{} {what}", short_tokens(n)))
            .collect();
        let mut text = format!("{} tokens out, {} in", short_tokens(self.output), short_tokens(self.read()));
        if parts.len() > 1 {
            text += &format!(" ({})", parts.join(", "));
        }
        text.push('.');
        if let Some(cost) = cost_text(cost_usd) {
            text += &format!(" {cost} at API prices, not what a subscription charges.");
        }
        text
    }

    pub fn add(self, other: TurnTokens) -> TurnTokens {
        TurnTokens {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            cache_write: self.cache_write.saturating_add(other.cache_write),
        }
    }
}

/// `UsageStatsMath.tokens`: 950, "1.7k", "381k", "4.2M".
pub fn short_tokens(n: i64) -> String {
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

/// `TokenText.line`: "24,177 in (12,032 cached), 310 out, 120 reasoning",
/// from either provider's `usage`.
pub fn token_line(usage: Option<&Value>) -> Option<String> {
    let usage = usage?;
    let count = |key: &str| usage.get(key).and_then(Value::as_f64).and_then(|v| whole_count(v, MAX_SAFE_INTEGER));
    let mut parts = Vec::new();
    if let Some(input) = count("inputTokens") {
        let mut part = format!("{} in", thousands(input));
        if let Some(cached) = count("cachedInputTokens").or_else(|| count("cacheReadTokens")) {
            if cached > 0 {
                part.push_str(&format!(" ({} cached)", thousands(cached)));
            }
        }
        parts.push(part);
    }
    if let Some(written) = count("cacheWriteTokens") {
        if written > 0 {
            parts.push(format!("{} cache written", thousands(written)));
        }
    }
    if let Some(output) = count("outputTokens") {
        parts.push(format!("{} out", thousands(output)));
    }
    if let Some(reasoning) = count("reasoningTokens") {
        if reasoning > 0 {
            parts.push(format!("{} reasoning", thousands(reasoning)));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    let body: String = out.chars().rev().collect();
    if n < 0 {
        format!("-{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tip_tests {
    use super::*;

    #[test]
    fn the_footer_tip_breaks_the_tokens_out_with_the_dollars() {
        let t = TurnTokens { input: 16, output: 1_700, cache_read: 337_000, cache_write: 44_000 };
        assert_eq!(t.tip(Some(0.45)), "1.7k tokens out, 381k in (337k cache reads, 44k cache writes, 16 uncached). $0.45 at API prices, not what a subscription charges.");
        let plain = TurnTokens { input: 900, output: 40, ..TurnTokens::default() };
        assert_eq!(plain.tip(None), "40 tokens out, 900 in.", "one kind of input isn't broken out");
    }
}
