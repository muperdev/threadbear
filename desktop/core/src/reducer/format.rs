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

    pub fn add(self, other: TurnTokens) -> TurnTokens {
        TurnTokens {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            cache_write: self.cache_write.saturating_add(other.cache_write),
        }
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
