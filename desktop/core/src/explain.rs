// Explain (docs/protocol.md, "Explain"): words Mike selected in a thread,
// explained by the small model in the pane beside it, with follow-ups. A mini
// fork of the thread that lives in the app alone, one per thread: nothing
// from it goes into the thread, so the agent never sees it and its context
// doesn't grow. A port of the Swift app's ExplainSession.
use serde_json::{json, Map, Value};

/// One answer: the first explanation has no question, a follow-up has one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplainTurn {
    pub question: Option<String>,
    pub answer: Option<String>,
    pub error: Option<String>,
}

impl ExplainTurn {
    pub fn is_pending(&self) -> bool {
        self.answer.is_none() && self.error.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainSession {
    /// Bumped for each new selection, so an answer that comes back after
    /// Mike has explained something else is dropped.
    pub id: u64,
    pub selection: String,
    /// The whole message the selection came from.
    pub message: Option<String>,
    pub turns: Vec<ExplainTurn>,
}

impl ExplainSession {
    pub fn new(id: u64, selection: &str, message: Option<String>) -> ExplainSession {
        ExplainSession { id, selection: selection.trim().to_owned(), message, turns: vec![ExplainTurn::default()] }
    }

    pub fn is_busy(&self) -> bool {
        self.turns.last().is_some_and(ExplainTurn::is_pending)
    }

    /// The selection, short and on one line, in quotes.
    pub fn title(&self) -> String {
        let line = self.selection.lines().next().unwrap_or(&self.selection);
        let short = if line.chars().count() > 40 { format!("{}…", line.chars().take(39).collect::<String>().trim_end()) } else { line.to_owned() };
        format!("\u{201C}{short}\u{201D}")
    }

    /// Adds a follow-up waiting for its answer; None while one is under way
    /// or the question is blank.
    pub fn ask(&mut self, question: &str) -> Option<usize> {
        let question = question.trim();
        if question.is_empty() || self.is_busy() {
            return None;
        }
        self.turns.push(ExplainTurn { question: Some(question.to_owned()), ..Default::default() });
        Some(self.turns.len() - 1)
    }

    /// Tries a failed turn again.
    pub fn retry(&mut self, index: usize) -> bool {
        match self.turns.get_mut(index) {
            Some(turn) if turn.error.is_some() => {
                turn.error = None;
                true
            }
            _ => false,
        }
    }

    /// The `threads.explain` params for turn `index`: the answers before it
    /// as `earlier`, its question if it has one.
    pub fn params(&self, thread_id: &str, index: usize) -> Value {
        let mut params = Map::new();
        params.insert("threadId".into(), json!(thread_id));
        params.insert("selection".into(), json!(self.selection));
        if let Some(message) = &self.message {
            params.insert("message".into(), json!(message));
        }
        let earlier: Vec<Value> = self
            .turns
            .iter()
            .take(index)
            .filter_map(|turn| {
                let answer = turn.answer.as_ref()?;
                let mut t = Map::new();
                t.insert("answer".into(), json!(answer));
                if let Some(q) = &turn.question {
                    t.insert("question".into(), json!(q));
                }
                Some(Value::Object(t))
            })
            .collect();
        if !earlier.is_empty() {
            params.insert("earlier".into(), Value::Array(earlier));
        }
        if let Some(question) = self.turns.get(index).and_then(|t| t.question.as_ref()) {
            params.insert("question".into(), json!(question));
        }
        Value::Object(params)
    }

    /// What a script reads (the Swift dump's preview line): what was asked
    /// and where each answer is up to.
    pub fn summary(&self) -> String {
        let turns: Vec<String> = self.turns.iter().map(|t| t.answer.clone().or_else(|| t.error.as_ref().map(|e| format!("error: {e}"))).unwrap_or_else(|| "pending".to_owned())).collect();
        format!("{} | {}", self.title(), turns.join(" | "))
    }
}

/// What the runner's error says when it has no Explain yet.
pub const NOT_YET: &str = "The runner doesn't know Explain yet. It will once it updates.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_asks_once_then_follows_up() {
        let mut s = ExplainSession::new(1, "  the smallest piece  ", Some("the whole reply".into()));
        assert!(s.is_busy());
        assert_eq!(s.params("t1", 0), json!({ "threadId": "t1", "selection": "the smallest piece", "message": "the whole reply" }));
        assert_eq!(s.ask("why?"), None, "not while the first answer is coming");
        s.turns[0].answer = Some("It means this.".into());
        assert_eq!(s.ask("  "), None);
        assert_eq!(s.ask("and then?"), Some(1));
        assert_eq!(s.params("t1", 1), json!({ "threadId": "t1", "selection": "the smallest piece", "message": "the whole reply", "earlier": [{ "answer": "It means this." }], "question": "and then?" }));
        assert_eq!(s.summary(), "\u{201C}the smallest piece\u{201D} | It means this. | pending");
    }

    #[test]
    fn a_failed_turn_can_be_tried_again() {
        let mut s = ExplainSession::new(2, "x", None);
        assert!(!s.retry(0), "nothing failed");
        s.turns[0].error = Some(NOT_YET.into());
        assert!(!s.is_busy());
        assert!(s.summary().ends_with("error: The runner doesn't know Explain yet. It will once it updates."));
        assert!(s.retry(0));
        assert!(s.is_busy());
        assert!(!s.retry(5));
    }

    #[test]
    fn the_title_is_the_selections_first_line_kept_short() {
        assert_eq!(ExplainSession::new(1, "one line\nand more", None).title(), "\u{201C}one line\u{201D}");
        let long = ExplainSession::new(1, "a b c d e f g h i j k l m n o p q r s t u v w x y z", None);
        assert_eq!(long.title(), "\u{201C}a b c d e f g h i j k l m n o p q r s t\u{2026}\u{201D}");
    }
}
