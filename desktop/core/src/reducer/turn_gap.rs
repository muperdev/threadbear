// The wait before a turn the agent started by itself: a port of
// Transcript/TurnGap.swift.
use super::event::{BackgroundKind, BackgroundTask};
use super::format::int_saturating;
use super::text;

/// Times are seconds since 1970, as `f64`: only their differences are shown.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnGap {
    /// The last turn's end and this one's start.
    pub from: f64,
    pub to: f64,
    /// What finished and woke it; empty for a wake with no finished work or
    /// from a runner too old to say.
    pub woke_by: Vec<BackgroundTask>,
}

impl TurnGap {
    pub fn milliseconds(&self) -> f64 {
        ((self.to - self.from) * 1000.0).max(0.0)
    }

    /// "6 min later · Mine the repo finished".
    pub fn label(&self) -> String {
        let time = Self::later(self.milliseconds());
        match Self::cause(&self.woke_by) {
            Some(cause) => format!("{time} · {cause}"),
            None => time,
        }
    }

    /// How long, in the words someone would say it.
    pub fn later(milliseconds: f64) -> String {
        let seconds = int_saturating((milliseconds / 1000.0).round()).max(0);
        if seconds < 60 {
            return if seconds < 2 { "Moments later".to_owned() } else { format!("{seconds}s later") };
        }
        let minutes = (seconds as f64 / 60.0).round() as i64;
        if minutes < 60 {
            return format!("{minutes} min later");
        }
        let hours = minutes / 60;
        let rest = minutes % 60;
        if hours < 24 {
            return if rest == 0 { format!("{hours} h later") } else { format!("{hours} h {rest} min later") };
        }
        let days = hours / 24;
        if days == 1 {
            "A day later".to_owned()
        } else {
            format!("{days} days later")
        }
    }

    /// What woke it. Sub-agents are named over the shells they ran: a
    /// sub-agent's own commands can finish alongside it, and those are its
    /// working, not what the thread was waiting for.
    pub fn cause(tasks: &[BackgroundTask]) -> Option<String> {
        if tasks.is_empty() {
            return None;
        }
        let agents: Vec<&BackgroundTask> = tasks.iter().filter(|t| t.kind == BackgroundKind::Agent).collect();
        let shown: Vec<&BackgroundTask> = if agents.is_empty() { tasks.iter().collect() } else { agents.clone() };
        let names: Vec<String> = shown.iter().map(|t| text::trim_ws_nl(&t.description).to_owned()).filter(|n| !n.is_empty()).collect();
        let noun = if agents.is_empty() { "background job" } else { "sub-agent" };
        Some(match (names.len(), shown.len()) {
            (0, 1) => format!("A {noun} finished"),
            (0, n) => format!("{n} {noun}s finished"),
            (1, 1) => format!("{} finished", names[0]),
            (1, n) => format!("{} and {} more finished", names[0], n - 1),
            (2, 2) => format!("{} and {} finished", names[0], names[1]),
            (_, n) => format!("{} and {} more finished", names[0], n - 1),
        })
    }
}

/// `Date.ISO8601FormatStyle` for a turn's `ts`, as seconds since 1970, with
/// or without fractional seconds. The runner always sends
/// `new Date().toISOString()` (UTC with milliseconds); the rest of this is
/// the shapes Swift's parser takes too, checked against it: the zone is
/// `Z`, or `+HH`, `+HHMM` or `+HH:MM`; field widths are flexible; and an
/// hour of 25 or minute of 60 just rolls over.
pub(crate) fn parse_iso8601(ts: &str) -> Option<f64> {
    let bytes = ts.as_bytes();
    let mut pos = 0usize;
    fn expect(bytes: &[u8], pos: &mut usize, c: u8) -> Option<()> {
        if bytes.get(*pos) == Some(&c) {
            *pos += 1;
            Some(())
        } else {
            None
        }
    }
    fn digits(bytes: &[u8], pos: &mut usize) -> Option<i64> {
        let start = *pos;
        while bytes.get(*pos).is_some_and(u8::is_ascii_digit) {
            *pos += 1;
        }
        std::str::from_utf8(&bytes[start..*pos]).ok()?.parse().ok()
    }
    let year = digits(bytes, &mut pos)?;
    expect(bytes, &mut pos, b'-')?;
    let month = digits(bytes, &mut pos)?;
    expect(bytes, &mut pos, b'-')?;
    let day = digits(bytes, &mut pos)?;
    expect(bytes, &mut pos, b'T')?;
    let hour = digits(bytes, &mut pos)?;
    expect(bytes, &mut pos, b':')?;
    let minute = digits(bytes, &mut pos)?;
    expect(bytes, &mut pos, b':')?;
    let second = digits(bytes, &mut pos)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Fractional seconds, then the zone.
    let mut fraction = 0.0;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if pos == start {
            return None;
        }
        fraction = format!("0.{}", &ts[start..pos]).parse::<f64>().ok()?;
    }
    let offset_seconds = match bytes.get(pos) {
        Some(b'Z') => {
            pos += 1;
            0
        }
        Some(sign @ (b'+' | b'-')) => {
            let sign = if *sign == b'+' { 1 } else { -1 };
            pos += 1;
            let zone: Vec<u8> = bytes[pos..].iter().filter(|b| **b != b':').copied().collect();
            let hours_minutes = match (zone.len(), zone.iter().all(u8::is_ascii_digit)) {
                (2, true) => (std::str::from_utf8(&zone).ok()?.parse::<i64>().ok()?, 0),
                (4, true) => {
                    let text = std::str::from_utf8(&zone).ok()?;
                    (text[..2].parse::<i64>().ok()?, text[2..].parse::<i64>().ok()?)
                }
                _ => return None,
            };
            pos = bytes.len();
            sign * (hours_minutes.0 * 3600 + hours_minutes.1 * 60)
        }
        _ => return None,
    };
    if pos != bytes.len() {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    Some(seconds as f64 + fraction)
}

// Days since 1970-01-01 in the proleptic Gregorian calendar (Howard
// Hinnant's days_from_civil, as Models.swift uses for dates too).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
