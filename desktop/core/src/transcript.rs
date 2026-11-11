// A thread's events folded into what the transcript draws, by the rules in
// docs/protocol.md ("Events" and "Transient events"). Events stay as JSON, so
// a type or field this app doesn't know is simply passed over.
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub item_id: String,
    pub title: String,
    pub kind: String,
    pub ok: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub question: String,
    pub multi: bool,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub action: String,
    pub why: Option<String>,
    pub options: Vec<String>,
    pub suggested: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User { seq: u64, text: String },
    Text { item_id: String, text: String, streaming: bool },
    Reasoning { item_id: String, text: String, streaming: bool },
    Steps(Vec<Step>),
    Approval { approval_id: String, title: String, options: Vec<String>, decision: Option<String> },
    Question { approval_id: String, questions: Vec<Question>, decision: Option<String>, message: Option<String> },
    Note { text: String, error: bool },
    TurnEnd { seq: u64, outcome: String, duration_ms: u64, asks: Vec<Ask>, error: Option<String> },
}

#[derive(Clone, Debug)]
struct Live {
    kind: String,
    text: String,
}

#[derive(Clone, Debug, Default)]
pub struct ThreadEvents {
    events: BTreeMap<u64, Value>,
    live: Vec<(String, Live)>,
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

impl ThreadEvents {
    /// Adds what threads.events, threads.subscribe or a thread.event brought.
    pub fn add(&mut self, incoming: &[Value], in_progress: &[Value]) {
        for p in in_progress {
            let kind = if s(p, "type") == "reasoning" { "reasoning.delta" } else { "assistant.text.delta" };
            self.set_live(s(p, "itemId"), kind, s(p, "text").to_string());
        }
        for e in incoming {
            let ty = s(e, "type");
            if ty.ends_with(".delta") {
                let id = s(e, "itemId");
                let had = self.live.iter().find(|(i, _)| i == id).map(|(_, l)| l.text.clone()).unwrap_or_default();
                self.set_live(id, ty, had + s(e, "text"));
            } else if let Some(seq) = e.get("seq").and_then(Value::as_u64) {
                if ty == "assistant.text" || ty == "reasoning" {
                    let id = s(e, "itemId").to_string();
                    self.live.retain(|(i, _)| *i != id);
                }
                self.events.insert(seq, e.clone());
            }
        }
    }

    fn set_live(&mut self, id: &str, kind: &str, text: String) {
        let live = Live { kind: kind.to_string(), text };
        match self.live.iter_mut().find(|(i, _)| i == id) {
            Some((_, l)) => *l = live,
            None => self.live.push((id.to_string(), live)),
        }
    }

    pub fn fold(&self) -> Vec<Item> {
        let hidden: Vec<(u64, u64)> = self
            .events
            .values()
            .filter(|e| s(e, "type") == "thread.rewound")
            .map(|e| (e["fromSeq"].as_u64().unwrap_or(0), e["toSeq"].as_u64().unwrap_or(0)))
            .collect();
        let mut resolved: HashMap<String, (String, Option<String>)> = HashMap::new();
        for e in self.events.values().filter(|e| s(e, "type") == "approval.resolved") {
            resolved.insert(s(e, "approvalId").to_string(), (s(e, "decision").to_string(), e.get("message").and_then(Value::as_str).map(String::from)));
        }

        let mut items: Vec<Item> = vec![];
        let mut by_item: HashMap<String, usize> = HashMap::new();
        let mut step_at: HashMap<String, (usize, usize)> = HashMap::new();

        for (seq, e) in &self.events {
            if hidden.iter().any(|(from, to)| seq >= from && seq <= to) {
                continue;
            }
            match s(e, "type") {
                "user.message" => items.push(Item::User { seq: *seq, text: s(e, "text").to_string() }),
                ty @ ("assistant.text" | "reasoning") => {
                    let item_id = s(e, "itemId").to_string();
                    let text = s(e, "text").to_string();
                    let row = if ty == "assistant.text" {
                        Item::Text { item_id: item_id.clone(), text, streaming: false }
                    } else {
                        Item::Reasoning { item_id: item_id.clone(), text, streaming: false }
                    };
                    match by_item.get(&item_id) {
                        Some(&at) => items[at] = row,
                        None => {
                            by_item.insert(item_id, items.len());
                            items.push(row);
                        }
                    }
                }
                "tool.call" => {
                    let step = Step { item_id: s(e, "itemId").to_string(), title: s(e, "title").to_string(), kind: s(e, "kind").to_string(), ok: None };
                    if !matches!(items.last(), Some(Item::Steps(_))) {
                        items.push(Item::Steps(vec![]));
                    }
                    let at = items.len() - 1;
                    if let Some(Item::Steps(steps)) = items.last_mut() {
                        step_at.insert(step.item_id.clone(), (at, steps.len()));
                        steps.push(step);
                    }
                }
                "tool.result" => {
                    if let Some(&(at, i)) = step_at.get(s(e, "itemId")) {
                        if let Item::Steps(steps) = &mut items[at] {
                            steps[i].ok = e.get("ok").and_then(Value::as_bool);
                        }
                    }
                }
                "approval.requested" => {
                    let id = s(e, "approvalId").to_string();
                    let options = e["options"].as_array().map(|o| o.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                    items.push(Item::Approval { decision: resolved.get(&id).map(|r| r.0.clone()), approval_id: id, title: s(e, "title").to_string(), options });
                }
                "question.requested" => {
                    let id = s(e, "approvalId").to_string();
                    let questions = e["questions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|q| Question {
                            question: s(q, "question").to_string(),
                            multi: q["multiSelect"].as_bool().unwrap_or(false),
                            options: q["options"].as_array().into_iter().flatten().map(|o| s(o, "label").to_string()).collect(),
                        })
                        .collect();
                    let r = resolved.get(&id).cloned();
                    items.push(Item::Question { approval_id: id, questions, decision: r.as_ref().map(|r| r.0.clone()), message: r.and_then(|r| r.1) });
                }
                "secret.requested" => items.push(Item::Note { text: format!("Asked for {}", s(e, "label")), error: false }),
                "system.note" => items.push(Item::Note { text: s(e, "text").to_string(), error: false }),
                "error" => items.push(Item::Note { text: s(e, "message").to_string(), error: true }),
                "turn.completed" => {
                    let asks = e["asks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|a| Ask {
                            action: s(a, "action").to_string(),
                            why: a.get("why").and_then(Value::as_str).map(String::from),
                            options: a["options"].as_array().into_iter().flatten().filter_map(|o| o.as_str().map(String::from)).collect(),
                            suggested: a.get("suggested").and_then(Value::as_u64).map(|n| n as usize),
                        })
                        .collect();
                    items.push(Item::TurnEnd {
                        seq: *seq,
                        outcome: s(e, "outcome").to_string(),
                        duration_ms: e["durationMs"].as_u64().unwrap_or(0),
                        asks,
                        error: e.get("error").and_then(Value::as_str).map(String::from),
                    });
                }
                _ => {}
            }
        }

        for (item_id, l) in &self.live {
            if l.kind == "tool.output.delta" {
                continue;
            }
            let row = if l.kind == "assistant.text.delta" {
                Item::Text { item_id: item_id.clone(), text: l.text.clone(), streaming: true }
            } else {
                Item::Reasoning { item_id: item_id.clone(), text: l.text.clone(), streaming: true }
            };
            match by_item.get(item_id) {
                Some(&at) => items[at] = row,
                None => items.push(row),
            }
        }
        items
    }
}

/// approvals.respond's answers for a question card, as the Mac app makes
/// them: each question's picks joined with ", ", and the whole lot in one
/// line, in the order asked.
pub fn question_answers(questions: &[Question], chosen: &HashMap<usize, Vec<String>>) -> (serde_json::Map<String, Value>, String) {
    let mut answers = serde_json::Map::new();
    let mut line = vec![];
    for (i, q) in questions.iter().enumerate() {
        let picks: Vec<&str> = q.options.iter().filter(|o| chosen.get(&i).is_some_and(|c| c.contains(o))).map(String::as_str).collect();
        if !picks.is_empty() {
            let joined = picks.join(", ");
            answers.insert(q.question.clone(), Value::String(joined.clone()));
            line.push(joined);
        }
    }
    (answers, line.join("; "))
}

pub fn duration(ms: u64) -> String {
    let s = (ms + 500) / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s / 60) % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Seq(u64);
    impl Seq {
        fn ev(&mut self, mut body: Value) -> Value {
            self.0 += 1;
            body["seq"] = json!(self.0);
            body
        }
    }
    fn kinds(items: &[Item]) -> Vec<&'static str> {
        items
            .iter()
            .map(|i| match i {
                Item::User { .. } => "user",
                Item::Text { .. } => "text",
                Item::Reasoning { .. } => "reasoning",
                Item::Steps(_) => "steps",
                Item::Approval { .. } => "approval",
                Item::Question { .. } => "question",
                Item::Note { .. } => "note",
                Item::TurnEnd { .. } => "turnEnd",
            })
            .collect()
    }

    #[test]
    fn a_turn_groups_work_into_steps() {
        let mut q = Seq(0);
        let mut t = ThreadEvents::default();
        t.add(
            &[
                q.ev(json!({ "type": "user.message", "text": "hi" })),
                q.ev(json!({ "type": "turn.started", "turnId": "a" })),
                q.ev(json!({ "type": "tool.call", "itemId": "c1", "kind": "command", "title": "npm test" })),
                q.ev(json!({ "type": "tool.call", "itemId": "c2", "kind": "read", "title": "Read a.ts" })),
                q.ev(json!({ "type": "tool.result", "itemId": "c1", "ok": false, "text": "boom" })),
                q.ev(json!({ "type": "assistant.text", "itemId": "m1", "text": "Done." })),
                q.ev(json!({ "type": "turn.completed", "turnId": "a", "outcome": "completed", "durationMs": 1200 })),
            ],
            &[],
        );
        let items = t.fold();
        assert_eq!(kinds(&items), ["user", "steps", "text", "turnEnd"]);
        let Item::Steps(steps) = &items[1] else { panic!() };
        assert_eq!((steps.len(), steps[0].ok, steps[1].ok), (2, Some(false), None));
    }

    #[test]
    fn deltas_stream_and_the_persisted_text_replaces_them() {
        let mut q = Seq(0);
        let mut t = ThreadEvents::default();
        t.add(&[q.ev(json!({ "type": "user.message", "text": "hi" }))], &[]);
        t.add(&[json!({ "type": "assistant.text.delta", "itemId": "m1", "text": "Hel", "seq": 1 }), json!({ "type": "assistant.text.delta", "itemId": "m1", "text": "lo", "seq": 1 })], &[]);
        assert_eq!(t.fold()[1], Item::Text { item_id: "m1".into(), text: "Hello".into(), streaming: true });
        t.add(&[q.ev(json!({ "type": "assistant.text", "itemId": "m1", "text": "Hello there" }))], &[]);
        assert_eq!(kinds(&t.fold()), ["user", "text"]);
        assert_eq!(t.fold()[1], Item::Text { item_id: "m1".into(), text: "Hello there".into(), streaming: false });
    }

    #[test]
    fn reasoning_that_lands_as_text_takes_its_place() {
        let mut q = Seq(0);
        let mut t = ThreadEvents::default();
        t.add(&[q.ev(json!({ "type": "user.message", "text": "hi" }))], &[json!({ "itemId": "n1", "type": "reasoning", "text": "thinking" })]);
        assert_eq!(kinds(&t.fold()), ["user", "reasoning"]);
        t.add(&[q.ev(json!({ "type": "assistant.text", "itemId": "n1", "text": "the reply" }))], &[]);
        assert_eq!(kinds(&t.fold()), ["user", "text"]);
    }

    #[test]
    fn a_later_text_with_the_same_item_replaces_the_earlier_one() {
        let mut q = Seq(0);
        let mut t = ThreadEvents::default();
        t.add(
            &[
                q.ev(json!({ "type": "assistant.text", "itemId": "m1", "text": "summary", "summary": true })),
                q.ev(json!({ "type": "tool.call", "itemId": "c1", "kind": "other", "title": "x" })),
                q.ev(json!({ "type": "assistant.text", "itemId": "m1", "text": "the whole reply" })),
            ],
            &[],
        );
        let items = t.fold();
        assert_eq!(kinds(&items), ["text", "steps"]);
        assert!(matches!(&items[0], Item::Text { text, .. } if text == "the whole reply"));
    }

    #[test]
    fn an_edit_hides_what_it_rewound() {
        let mut q = Seq(0);
        let mut t = ThreadEvents::default();
        t.add(
            &[
                q.ev(json!({ "type": "user.message", "text": "first try" })),
                q.ev(json!({ "type": "assistant.text", "itemId": "m1", "text": "answer" })),
                q.ev(json!({ "type": "thread.rewound", "fromSeq": 1, "toSeq": 2 })),
                q.ev(json!({ "type": "user.message", "text": "second try", "editOf": 1 })),
            ],
            &[],
        );
        assert_eq!(t.fold(), [Item::User { seq: 4, text: "second try".into() }]);
    }

    #[test]
    fn approvals_and_questions_show_their_answers() {
        let mut q = Seq(0);
        let mut t = ThreadEvents::default();
        t.add(
            &[
                q.ev(json!({ "type": "approval.requested", "approvalId": "p1", "title": "rm -rf build", "options": ["allow", "deny"] })),
                q.ev(json!({ "type": "approval.resolved", "approvalId": "p1", "decision": "deny" })),
                q.ev(json!({ "type": "question.requested", "approvalId": "q1", "questions": [
                    { "question": "Which colour?", "multiSelect": false, "options": [{ "label": "Blue" }, { "label": "Green" }] },
                    { "question": "Where?", "multiSelect": true, "options": [{ "label": "Toolbar" }, { "label": "Menu" }] }
                ] })),
            ],
            &[],
        );
        let items = t.fold();
        assert!(matches!(&items[0], Item::Approval { decision: Some(d), .. } if d == "deny"));
        let Item::Question { questions, decision: None, .. } = &items[1] else { panic!() };
        let chosen = HashMap::from([(0, vec!["Green".to_string()]), (1, vec!["Menu".to_string(), "Toolbar".to_string()])]);
        let (answers, line) = question_answers(questions, &chosen);
        assert_eq!(Value::Object(answers), json!({ "Which colour?": "Green", "Where?": "Toolbar, Menu" }));
        assert_eq!(line, "Green; Toolbar, Menu");
    }

    #[test]
    fn the_same_events_twice_change_nothing() {
        let mut q = Seq(0);
        let batch = [q.ev(json!({ "type": "user.message", "text": "hi" }))];
        let mut t = ThreadEvents::default();
        t.add(&batch, &[]);
        t.add(&batch, &[]);
        assert_eq!(t.fold().len(), 1);
    }
}
