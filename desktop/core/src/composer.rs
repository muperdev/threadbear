// The composer's rules with no window in sight: ports of ThreadbearCore's
// ListContinuation, InlinePictures, ComposerChips and Decisions. The Swift is
// the definition of right; each part says which type it comes from.
use crate::reducer::{Ask, AskAnswer, AskNote};
use std::collections::BTreeMap;
use std::ops::Range;

/// The character a picture's chip is in the draft (InlinePictures.mark).
pub const PICTURE_MARK: char = '\u{FFFC}';
pub const PICTURE_MARK_STR: &str = "\u{FFFC}";

// ---- ListContinuation ----

/// A byte range of the text to replace with `text` (ListContinuation.Edit,
/// which counts UTF-16 as NSTextView does; Rust strings count bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEdit {
    pub range: Range<usize>,
    pub text: String,
}

/// ListContinuation.newline: Shift-Return in the box carries a markdown list
/// on, after "1. foo" with "2. ", after "- foo" with "- ", and a checkbox item
/// with an unticked one. On an item with nothing in it, the marker comes off,
/// which is how you get out of the list. `None` means a plain new line.
pub fn list_newline(text: &str, selection: Range<usize>) -> Option<ListEdit> {
    if selection.end > text.len() || !text.is_char_boundary(selection.start) || !text.is_char_boundary(selection.end) {
        return None;
    }
    let line_start = text[..selection.start].rfind('\n').map_or(0, |at| at + 1);
    let before = &text[line_start..selection.start];
    let marker = ListMarker::parse(before)?;
    let line_end = text[selection.end..].find('\n').map_or(text.len(), |at| selection.end + at);
    let after = &text[selection.end..line_end];
    if marker.len == before.len() && after.trim().is_empty() {
        return Some(ListEdit { range: line_start..selection.start, text: String::new() });
    }
    let mut next = before[..marker.indent].to_owned();
    match marker.number {
        Some((number, delimiter)) => {
            next.push_str(&(number + 1).to_string());
            next.push(delimiter);
        }
        None => next.push(before.as_bytes()[marker.indent] as char),
    }
    next.push_str(&before[marker.spaces.clone()]);
    if marker.has_box {
        // Keep the spacing after the box, but the new one starts unticked.
        next.push_str("[ ]");
        next.push_str(&before[marker.after_box.clone()]);
    }
    Some(ListEdit { range: selection, text: format!("\n{next}") })
}

/// `^([ \t]*)(?:([-*+])|([0-9]{1,9})([.)]))([ \t]+)(\[[ xX]\][ \t]+)?`,
/// written out, as this crate has no regex dependency.
struct ListMarker {
    indent: usize,
    number: Option<(i64, char)>,
    spaces: Range<usize>,
    has_box: bool,
    after_box: Range<usize>,
    /// How far the match reaches.
    len: usize,
}

impl ListMarker {
    fn parse(line: &str) -> Option<ListMarker> {
        let bytes = line.as_bytes();
        let is_space = |b: u8| b == b' ' || b == b'\t';
        let indent = bytes.iter().take_while(|&&b| is_space(b)).count();
        let mut at = indent;
        let number = match bytes.get(at)? {
            b'-' | b'*' | b'+' => {
                at += 1;
                None
            }
            b'0'..=b'9' => {
                let start = at;
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
                if at - start > 9 {
                    return None;
                }
                let delimiter = *bytes.get(at)? as char;
                if delimiter != '.' && delimiter != ')' {
                    return None;
                }
                let number: i64 = line[start..at].parse().ok()?;
                at += 1;
                Some((number, delimiter))
            }
            _ => return None,
        };
        let spaces_start = at;
        while at < bytes.len() && is_space(bytes[at]) {
            at += 1;
        }
        if at == spaces_start {
            return None;
        }
        let spaces = spaces_start..at;
        // The optional "[x] " after it.
        let mut has_box = false;
        let mut after_box = at..at;
        if bytes.len() >= at + 4 && bytes[at] == b'[' && bytes[at + 2] == b']' && matches!(bytes[at + 1], b' ' | b'x' | b'X') && is_space(bytes[at + 3]) {
            let mut end = at + 3;
            while end < bytes.len() && is_space(bytes[end]) {
                end += 1;
            }
            has_box = true;
            after_box = at + 3..end;
            at = end;
        }
        Some(ListMarker { indent, number, spaces, has_box, after_box, len: at })
    }
}

// ---- InlinePictures ----

/// "[Picture 2]", the marker a message carries for its second picture.
pub fn picture_marker(number: usize) -> String {
    format!("[Picture {number}]")
}

pub fn mark_count(text: &str) -> usize {
    text.chars().filter(|&c| c == PICTURE_MARK).count()
}

/// InlinePictures.words: a draft's words without its pictures.
pub fn draft_words(text: &str) -> String {
    text.chars().filter(|&c| c != PICTURE_MARK).collect()
}

/// InlinePictures.normalized: a draft's text put right for `image_count`
/// pictures, one mark each. Spare marks go; a picture with no mark gets one
/// at the end.
pub fn normalized(text: &str, image_count: usize) -> String {
    let marks = mark_count(text);
    if marks == image_count {
        return text.to_owned();
    }
    if marks > image_count {
        let mut kept = 0;
        return text
            .chars()
            .filter(|&c| {
                if c == PICTURE_MARK {
                    kept += 1;
                    kept <= image_count
                } else {
                    true
                }
            })
            .collect();
    }
    let mut out = text.to_owned();
    for _ in marks..image_count {
        out = appending(PICTURE_MARK_STR, &out);
    }
    out
}

/// InlinePictures.appending: `piece` after `text`, with a space between
/// unless `text` is empty or ends in whitespace.
pub fn appending(piece: &str, text: &str) -> String {
    match text.chars().last() {
        Some(last) if !last.is_whitespace() => format!("{text} {piece}"),
        _ => format!("{text}{piece}"),
    }
}

/// InlinePictures.sentText: a draft's words as they're sent, its k-th mark
/// `[Picture first+k]`. Nothing but pictures and spaces sends as "", the
/// pictures alone.
pub fn sent_text(text: &str, first: usize) -> String {
    if !text.contains(PICTURE_MARK) {
        return text.to_owned();
    }
    if text.chars().all(|c| c == PICTURE_MARK || c.is_whitespace()) {
        return String::new();
    }
    message_text(text, first)
}

/// InlinePictures.messageText: a draft's text with its k-th mark as
/// `[Picture first+k]`.
pub fn message_text(text: &str, first: usize) -> String {
    if !text.contains(PICTURE_MARK) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut number = first;
    for c in text.chars() {
        if c == PICTURE_MARK {
            out.push_str(&picture_marker(number));
            number += 1;
        } else {
            out.push(c);
        }
    }
    out
}

/// What's dropped or picked that isn't a picture goes in as its full path in
/// backticks, so one with spaces in its name still reads as one path
/// (ComposerTextView.pathText).
pub fn path_text(paths: &[String]) -> String {
    paths.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(" ")
}

// ---- ComposerChips ----

/// The model chip's name for a model id (ComposerChips.model).
pub fn model_title(model: Option<&str>) -> String {
    let Some(model) = model.filter(|m| !m.is_empty()) else { return "Default".to_owned() };
    if model.chars().all(char::is_alphabetic) {
        return capitalized(model);
    }
    if model.to_lowercase().starts_with("gpt-") {
        return model
            .split('-')
            .enumerate()
            .map(|(i, part)| {
                if i == 0 {
                    part.to_uppercase()
                } else {
                    capitalized(part)
                }
            })
            .collect::<Vec<_>>()
            .join("-");
    }
    let mut parts: Vec<&str> = model.split('-').filter(|p| !p.is_empty()).collect();
    if parts.first() != Some(&"claude") {
        return model.to_owned();
    }
    parts.remove(0);
    if let Some(last) = parts.last() {
        if last.len() == 8 && last.chars().all(|c| c.is_ascii_digit()) {
            parts.pop();
        }
    }
    let Some(family) = parts.first().copied() else { return model.to_owned() };
    if parts.len() < 2 || !family.chars().all(char::is_alphabetic) || !parts[1..].iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        return model.to_owned();
    }
    format!("{} {}", capitalized(family), parts[1..].join("."))
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// ComposerChips.effortTitle.
pub fn effort_title(level: &str) -> String {
    match level {
        "xhigh" => "Extra high".to_owned(),
        "max" => "Max".to_owned(),
        "" => String::new(),
        _ => capitalized(level),
    }
}

/// ComposerChips.effortPorridge: what the porridge is like at each level.
pub fn effort_porridge(level: &str) -> Option<&'static str> {
    match level {
        "low" => Some("Too cold"),
        "medium" => Some("A bit cold"),
        "high" => Some("Warm"),
        "xhigh" => Some("Hot"),
        "max" => Some("Too hot"),
        "ultra" => Some("Scalding"),
        _ => None,
    }
}

pub const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
pub const CODEX_EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultra"];

/// ComposerChips.canSend.
pub fn can_send(text: &str, image_count: usize) -> bool {
    image_count > 0 || !text.trim().is_empty()
}

pub const BOX_MIN_LINES: usize = 2;
pub const BOX_MAX_LINES: usize = 10;

// ---- ContextMeter ----

/// How full the thread's context is, for the ring (ContextMeter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextMeter {
    pub used: i64,
    pub window: i64,
}

impl ContextMeter {
    /// The ring turns orange from here: the CLI compacts not far past it.
    pub const NEARLY_FULL: f64 = 0.8;

    pub fn new(used: Option<i64>, window: Option<i64>) -> Option<ContextMeter> {
        let (used, window) = (used?, window?);
        (window > 0 && used >= 0).then_some(ContextMeter { used, window })
    }

    pub fn fraction(&self) -> f64 {
        (self.used as f64 / self.window as f64).min(1.)
    }

    pub fn percent(&self) -> i64 {
        (self.fraction() * 100.).round() as i64
    }

    pub fn is_nearly_full(&self) -> bool {
        self.fraction() >= Self::NEARLY_FULL
    }

    /// "640k of 1M tokens".
    pub fn amount(&self) -> String {
        format!("{} of {} tokens", Self::tokens(self.used), Self::tokens(self.window))
    }

    /// The ring's tooltip.
    pub fn tip(&self) -> String {
        format!("Context {}% full", self.percent())
    }

    /// 850, 12k, 640k, 1M, 1.2M.
    pub fn tokens(n: i64) -> String {
        if n < 1000 {
            return n.to_string();
        }
        if n < 1_000_000 {
            let k = n as f64 / 1000.;
            return if k < 10. { format!("{}k", Self::trimmed(k)) } else { format!("{}k", k.round() as i64) };
        }
        format!("{}M", Self::trimmed(n as f64 / 1_000_000.))
    }

    /// One decimal place, rounded down, without a trailing ".0".
    fn trimmed(x: f64) -> String {
        let s = format!("{:.1}", (x * 10.).floor() / 10.);
        s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
    }
}

// ---- Decisions ----

/// The pick that means "Something else" (Decisions.ownAnswer).
pub const OWN_ANSWER: i64 = -1;

/// Decisions.choices: what Mike can pick from, the agent's options, or for a
/// plain ask the reply it wants back, or the words its action asks for.
pub fn choices(ask: &Ask) -> Vec<String> {
    if !ask.options.is_empty() {
        return ask.options.clone();
    }
    if let Some(reply) = ask.reply.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        return vec![reply.to_owned()];
    }
    implied_choices(&ask.action)
}

const OPENING: &[char] = &['"', '\u{201c}', '\''];
const CLOSING: &[char] = &['"', '\u{201d}', '\''];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Decisions.impliedChoices: 'Reply "export" when…' or "Say go for Short 2",
/// so the word becomes a button; a list of quoted words a button each.
pub fn implied_choices(action: &str) -> Vec<String> {
    let lower = action.to_lowercase();
    // The words 'say', 'reply', 'type', 'send', 'answer' (Decisions.quoted).
    for (start, _) in lower.char_indices() {
        if start > 0 && lower[..start].chars().next_back().is_some_and(is_word_char) {
            continue;
        }
        for word in ["say", "reply", "type", "send", "answer"] {
            if !lower[start..].starts_with(word) {
                continue;
            }
            let mut at = start + word.len();
            if lower[at..].chars().next().is_some_and(is_word_char) {
                continue;
            }
            if lower[at..].starts_with(" with") {
                at += " with".len();
            }
            let spaced = lower[at..].chars().take_while(|c| c.is_whitespace()).count();
            if spaced == 0 {
                continue;
            }
            at += lower[at..].chars().take(spaced).map(char::len_utf8).sum::<usize>();
            let Some((first, mut end)) = quoted_word(action, at) else { continue };
            let mut out = vec![first];
            // Each further quoted choice straight after the first.
            while out.len() < MAX_IMPLIED {
                let Some((word, next)) = next_quoted(action, end) else { break };
                out.push(word);
                end = next;
            }
            return out;
        }
    }
    // Decisions.bare: "Say go for…".
    for start in ["say", "reply"] {
        if let Some(rest) = lower.strip_prefix(start) {
            let spaced = rest.chars().take_while(|c| c.is_whitespace()).count();
            if spaced == 0 {
                continue;
            }
            let rest = rest.trim_start();
            for word in ["go", "yes", "ok", "okay", "done", "ship it", "merge it", "merge"] {
                if rest.starts_with(word) && !rest[word.len()..].chars().next().is_some_and(is_word_char) {
                    return vec![capitalized_first(word)];
                }
            }
        }
    }
    Vec::new()
}

const MAX_IMPLIED: usize = 6;

fn capitalized_first(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The quoted word (one to forty characters) opening at byte `at`, and the
/// byte after its closing quote.
fn quoted_word(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = text.get(at..)?;
    let first = rest.chars().next()?;
    if !OPENING.contains(&first) {
        return None;
    }
    let inner = &rest[first.len_utf8()..];
    let close = inner.find(|c: char| CLOSING.contains(&c))?;
    let word = &inner[..close];
    if word.is_empty() || word.chars().count() > 40 || word.chars().any(|c| CLOSING.contains(&c)) {
        return None;
    }
    let closing = inner[close..].chars().next()?;
    Some((word.to_owned(), at + first.len_utf8() + close + closing.len_utf8()))
}

/// Decisions.more: a comma, "or" or "/" and then another quoted word.
fn next_quoted(text: &str, at: usize) -> Option<(String, usize)> {
    let mut rest = text.get(at..)?;
    let mut consumed = at;
    let eat_spaces = |rest: &mut &str, consumed: &mut usize| {
        let trimmed = rest.trim_start();
        *consumed += rest.len() - trimmed.len();
        *rest = trimmed;
    };
    eat_spaces(&mut rest, &mut consumed);
    let mut matched_join = false;
    if rest.starts_with(',') {
        rest = &rest[1..];
        consumed += 1;
        eat_spaces(&mut rest, &mut consumed);
        if rest.to_lowercase().starts_with("or") && rest[2..].starts_with(char::is_whitespace) {
            consumed += 2;
            rest = &rest[2..];
            eat_spaces(&mut rest, &mut consumed);
        }
        matched_join = true;
    } else if rest.to_lowercase().starts_with("or") && rest[2..].starts_with(char::is_whitespace) {
        consumed += 2;
        rest = &rest[2..];
        eat_spaces(&mut rest, &mut consumed);
        matched_join = true;
    } else if rest.starts_with('/') {
        consumed += 1;
        rest = &rest[1..];
        eat_spaces(&mut rest, &mut consumed);
        matched_join = true;
    }
    if !matched_join {
        return None;
    }
    quoted_word(text, consumed)
}

/// Decisions.nextUnanswered: the next ask after `index` (wrapping round) with
/// nothing picked, or `None` when every other has a pick. `or_this` counts
/// ask `index` too, last.
pub fn next_unanswered(index: usize, count: usize, picks: &BTreeMap<i64, i64>, or_this: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let mut order: Vec<usize> = (1..count).map(|step| (index + step) % count).collect();
    if or_this {
        order.push(index);
    }
    order.into_iter().find(|i| !picks.contains_key(&(*i as i64)))
}

/// Decisions.filled: picks that will send something. Something else with
/// nothing written or pasted in its box doesn't count.
pub fn filled_picks(picks: &BTreeMap<i64, i64>, notes: &BTreeMap<i64, AskNote>) -> BTreeMap<i64, i64> {
    picks
        .iter()
        .filter(|(index, n)| {
            if **n != OWN_ANSWER {
                return true;
            }
            notes.get(index).is_some_and(|note| !note.images.is_empty() || !note.text.trim().is_empty())
        })
        .map(|(k, v)| (*k, *v))
        .collect()
}

/// Decisions.answers: the picked asks as answers, in the card's order, each
/// with its index on the card.
pub fn answers(asks: &[Ask], picks: &BTreeMap<i64, i64>, notes: &BTreeMap<i64, AskNote>) -> Vec<(usize, AskAnswer)> {
    let mut picture = 0;
    asks.iter()
        .enumerate()
        .filter_map(|(i, ask)| {
            let n = *picks.get(&(i as i64))?;
            if n == OWN_ANSWER {
                let note = notes.get(&(i as i64)).cloned().unwrap_or_default();
                let marked = normalized(&note.text, note.images.len());
                let first = picture + 1;
                picture += note.images.len();
                let answer = message_text(&marked, first).trim().to_owned();
                return (!answer.is_empty()).then(|| (i, AskAnswer { action: ask.action.clone(), answer }));
            }
            let options = choices(ask);
            options.get(n as usize).map(|choice| (i, AskAnswer { action: ask.action.clone(), answer: choice.clone() }))
        })
        .collect()
}

/// Decisions.images: the pictures from the answered asks' boxes, in the
/// card's order; the composer's own go after them.
pub fn note_images(asks: &[Ask], picks: &BTreeMap<i64, i64>, notes: &BTreeMap<i64, AskNote>) -> Vec<crate::reducer::ImageRef> {
    (0..asks.len() as i64)
        .filter(|i| picks.get(i) == Some(&OWN_ANSWER))
        .flat_map(|i| notes.get(&i).map(|note| note.images.clone()).unwrap_or_default())
        .collect()
}

/// Decisions.block: one ask's question then its answer. Numbered, every line
/// of the answer is indented under it.
fn answer_block(answer: &AskAnswer, number: Option<usize>) -> String {
    match number {
        None => format!("{}\n{}", answer.action, answer.answer),
        Some(n) => format!("{n}. {}\n   {}", answer.action, answer.answer.replace('\n', "\n   ")),
    }
}

/// Decisions.message: one ordinary message, each picked ask's question then
/// its answer, numbered as on the card, then whatever Mike wrote.
pub fn answers_message(asks: &[Ask], picks: &BTreeMap<i64, i64>, notes: &BTreeMap<i64, AskNote>, extra: &str) -> String {
    let parts: Vec<String> = answers(asks, picks, notes).into_iter().map(|(i, answer)| answer_block(&answer, (asks.len() != 1).then_some(i + 1))).collect();
    let mut body = parts.join("\n\n");
    let more = extra.trim();
    if !more.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(more);
    }
    body
}

/// Decisions.followUpMessage: the ones ticked on a done card, one on its own
/// or several as a list, then any asked about, then what he typed.
pub fn follow_up_message(follow_ups: &[String], explain: &[String], extra: &str) -> String {
    let mut body = if follow_ups.len() == 1 { follow_ups[0].clone() } else { follow_ups.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n") };
    if !explain.is_empty() {
        let question = if explain.len() == 1 {
            format!("What do you mean by \"{}\"? Please explain.", explain[0])
        } else {
            format!("What do you mean by these? Please explain.\n{}", explain.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"))
        };
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&question);
    }
    let more = extra.trim();
    if !more.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(more);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newline(text: &str, caret: usize) -> Option<(Range<usize>, String)> {
        list_newline(text, caret..caret).map(|e| (e.range, e.text))
    }

    #[test]
    fn numbers_carry_on_and_bullets_stay() {
        assert_eq!(newline("1. foo", 6), Some((6..6, "\n2. ".into())));
        assert_eq!(newline("- foo", 5), Some((5..5, "\n- ".into())));
        assert_eq!(newline("  * bar", 7), Some((7..7, "\n  * ".into())));
        assert_eq!(newline("9) nine", 7), Some((7..7, "\n10) ".into())));
    }

    #[test]
    fn checkbox_items_start_unticked() {
        assert_eq!(newline("- [x] done", 10), Some((10..10, "\n- [ ] ".into())));
    }

    #[test]
    fn an_empty_item_takes_its_marker_off() {
        assert_eq!(newline("1. foo\n2. ", 10), Some((7..10, String::new())));
        assert_eq!(newline("- ", 2), Some((0..2, String::new())));
    }

    #[test]
    fn plain_lines_are_plain() {
        assert_eq!(newline("just words", 10), None);
        assert_eq!(newline("**bold**", 8), None);
        assert_eq!(newline("2.5 apples", 3), None);
    }

    #[test]
    fn a_marker_in_the_middle_of_a_line_is_no_list() {
        // The caret mid-line still continues: only what's before it counts.
        assert_eq!(newline("1. foo bar", 6), Some((6..6, "\n2. ".into())));
        assert_eq!(newline("foo 1. bar", 6), None);
    }

    #[test]
    fn pictures_read_as_markers_when_sent() {
        let draft = format!("look at {PICTURE_MARK} and {PICTURE_MARK}");
        assert_eq!(sent_text(&draft, 1), "look at [Picture 1] and [Picture 2]");
        assert_eq!(sent_text(&draft, 3), "look at [Picture 3] and [Picture 4]");
        assert_eq!(sent_text(&format!("{PICTURE_MARK} \n{PICTURE_MARK}"), 1), "");
        assert_eq!(sent_text("no pictures", 1), "no pictures");
    }

    #[test]
    fn normalized_drops_spare_marks_and_adds_missing_ones() {
        assert_eq!(normalized(&format!("a{PICTURE_MARK}b{PICTURE_MARK}"), 1), format!("a{PICTURE_MARK}b"));
        assert_eq!(normalized("text", 1), format!("text {PICTURE_MARK}"));
        assert_eq!(normalized("text ", 2), format!("text {PICTURE_MARK} {PICTURE_MARK}"));
    }

    #[test]
    fn model_titles_read_like_the_swift_app() {
        assert_eq!(model_title(Some("claude-opus-5-5")), "Opus 5.5");
        assert_eq!(model_title(Some("claude-opus-5-5-20261101")), "Opus 5.5");
        assert_eq!(model_title(Some("opus")), "Opus");
        assert_eq!(model_title(Some("gpt-6.1-sol")), "GPT-6.1-Sol");
        assert_eq!(model_title(None), "Default");
        assert_eq!(model_title(Some("weird-model")), "weird-model");
    }

    #[test]
    fn effort_titles() {
        assert_eq!(effort_title("xhigh"), "Extra high");
        assert_eq!(effort_title("max"), "Max");
        assert_eq!(effort_title("low"), "Low");
    }

    #[test]
    fn context_meter_reads_like_the_swift_app() {
        let m = ContextMeter::new(Some(640_000), Some(1_000_000)).unwrap();
        assert_eq!(m.percent(), 64);
        assert_eq!(m.amount(), "640k of 1M tokens");
        assert_eq!(m.tip(), "Context 64% full");
        assert!(!m.is_nearly_full());
        assert_eq!(ContextMeter::tokens(850), "850");
        assert_eq!(ContextMeter::tokens(12_345), "12k");
        assert_eq!(ContextMeter::tokens(1_200_000), "1.2M");
        assert_eq!(ContextMeter::tokens(9_500), "9.5k");
        assert!(ContextMeter::new(Some(1), None).is_none());
    }

    fn ask(action: &str, options: &[&str]) -> Ask {
        Ask { options: options.iter().map(|s| s.to_string()).collect(), ..Ask::new(action) }
    }

    #[test]
    fn implied_choices_find_the_words_asked_for() {
        assert_eq!(implied_choices("Reply \"export\" when you're ready"), vec!["export"]);
        assert_eq!(implied_choices("Say go for Short 2, or name another"), vec!["Go"]);
        assert_eq!(implied_choices("Reply with \"Land it\", \"Website, then land it\", or \"Hold\""), vec!["Land it", "Website, then land it", "Hold"]);
        assert!(implied_choices("Have a look at the build").is_empty());
    }

    #[test]
    fn the_message_numbers_several_asks_but_not_one() {
        let asks = vec![ask("Ship it?", &["Yes", "No"]), ask("Which colour?", &["Blue"])];
        let picks = BTreeMap::from([(0, 0), (1, 0)]);
        assert_eq!(answers_message(&asks, &picks, &BTreeMap::new(), "thanks"), "1. Ship it?\n   Yes\n\n2. Which colour?\n   Blue\n\nthanks");
        let one = vec![ask("Ship it?", &["Yes", "No"])];
        assert_eq!(answers_message(&one, &BTreeMap::from([(0, 1)]), &BTreeMap::new(), ""), "Ship it?\nNo");
    }

    #[test]
    fn something_else_goes_as_his_own_words() {
        let asks = vec![ask("Ship it?", &["Yes"])];
        let notes = BTreeMap::from([(0, AskNote { text: "wait for me".into(), images: vec![] })]);
        let picks = BTreeMap::from([(0, OWN_ANSWER)]);
        assert_eq!(answers_message(&asks, &picks, &notes, ""), "Ship it?\nwait for me");
        // With nothing written it's no answer at all.
        assert_eq!(answers_message(&asks, &picks, &BTreeMap::new(), ""), "");
    }

    #[test]
    fn follow_up_messages() {
        assert_eq!(follow_up_message(&["Fix the logo".into()], &[], ""), "Fix the logo");
        assert_eq!(follow_up_message(&["A".into(), "B".into()], &["C".into()], "and hurry"), "- A\n- B\n\nWhat do you mean by \"C\"? Please explain.\n\nand hurry");
    }

    #[test]
    fn next_unanswered_wraps_and_counts_this_one_last() {
        let picks = BTreeMap::from([(0, 0), (2, 1)]);
        assert_eq!(next_unanswered(0, 3, &picks, false), Some(1));
        assert_eq!(next_unanswered(1, 3, &BTreeMap::from([(0, 0), (1, 0), (2, 0)]), true), None);
        assert_eq!(next_unanswered(1, 3, &BTreeMap::from([(0, 0), (2, 0)]), true), Some(1));
    }
}
