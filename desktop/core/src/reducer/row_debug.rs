// The row's own words for dumps and golden files: a port of
// Transcript/RowDebug.swift (`excerpt`, `text` and `TokenText`).
use super::card_text::PlanCardText;
use super::format::{cost_text, duration, token_line};
use super::groups::BackgroundWork;
use super::row::{Pending, PlanCardState, Row, RowContent, UserMessage};
use super::text;
use std::collections::BTreeSet;

pub struct RowDebug;

impl RowDebug {
    /// The row's text on one line, cut to `limit` characters.
    pub fn excerpt(row: &Row, limit: usize) -> String {
        Self::one_line(&Self::text(row), limit)
    }

    /// The row's words, whatever kind it is.
    pub fn text(row: &Row) -> String {
        match &row.content {
            RowContent::User(message) => Self::user_text(message),
            RowContent::Assistant(item) | RowContent::Reasoning(item) => item.text.clone(),
            RowContent::Tool(tool) => {
                if tool.failed() {
                    format!("{} (failed)", tool.title())
                } else {
                    tool.title()
                }
            }
            RowContent::Images { images, .. } => {
                if images.len() == 1 {
                    "1 image".to_owned()
                } else {
                    format!("{} images", images.len())
                }
            }
            RowContent::Approval(item) => match &item.resolution {
                Some(resolution) => format!("{} ({})", item.request.title, resolution.decision.as_str()),
                None => item.request.title.clone(),
            },
            RowContent::Question(item) => item.request.questions.iter().map(|q| q.question.clone()).collect::<Vec<_>>().join(" / "),
            RowContent::Secret(item) => item.resolved_text().unwrap_or_else(|| item.request.label.clone()),
            RowContent::Review(review) => format!("{} · {} images · {}", review.title, review.options.len(), review.status),
            RowContent::Plan(entries) => entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("; "),
            RowContent::PlanCard(card) => {
                let state = match card.state {
                    PlanCardState::Waiting => "waiting",
                    PlanCardState::Approved => "approved",
                    PlanCardState::SentBack => "sent back",
                };
                if card.entries.is_empty() {
                    // Its title too, as the card draws it over the plan (the
                    // nightly parity run looks for both).
                    match PlanCardText::split(&card.text).0 {
                        Some(title) => format!("plan ({state}): {title}: {}", PlanCardText::summary(&card.text)),
                        None => format!("plan ({state}): {}", PlanCardText::summary(&card.text)),
                    }
                } else {
                    format!("plan ({state}), {}", card.entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("; "))
                }
            }
            RowContent::Asks(card) => {
                let openness = if card.is_open { "open" } else { "closed" };
                if card.is_done() {
                    let loose = match &card.loose_ends {
                        None => String::new(),
                        Some(ends) if ends.is_empty() => ", loose ends: none".to_owned(),
                        Some(ends) => {
                            // A booked check-in is drawn under its own label.
                            let booked: Vec<String> = ends.iter().filter(|e| e.is_booked()).map(|e| e.text.clone()).collect();
                            let coming = if booked.is_empty() { String::new() } else { format!(", Coming back: {}", booked.join("; ")) };
                            format!(", loose ends: {}{coming}", ends.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("; "))
                        }
                    };
                    let next = if card.next.is_empty() {
                        String::new()
                    } else {
                        let items: Vec<String> =
                            card.next.iter().enumerate().map(|(i, n)| format!("{}{n}", if picked(&card.picked_next, i) { "[x] " } else { "" })).collect();
                        format!(", next: {}", items.join("; "))
                    };
                    return format!("{openness}, nothing needed{loose}{next}");
                }
                format!("{openness}, {} of {} ticked: {}", card.ticked_count(), card.asks.len(), card.asks.iter().map(|a| a.action.clone()).collect::<Vec<_>>().join("; "))
            }
            RowContent::Footer(completion) => {
                let mut words = duration(completion.duration_ms);
                if let Some(cost) = cost_text(completion.cost_usd) {
                    words.push_str(&format!(", {cost}"));
                }
                if completion.outcome != super::event::TurnOutcome::Completed {
                    words.push_str(&format!(", {}", completion.outcome.as_str()));
                }
                // Codex's footer shows only the time; its tokens are here.
                if let Some(tokens) = token_line(completion.usage.as_ref()) {
                    words.push_str(&format!(", {tokens}"));
                }
                words
            }
            RowContent::Note(note) => note.clone(),
            RowContent::Error { message, .. } => message.clone(),
            RowContent::Group(group) => group.summary.clone(),
            RowContent::Working(_) => "Working".to_owned(),
            RowContent::Background(status) => BackgroundWork::line_with_detail(&status.tasks),
            RowContent::Changes(card) => card.title(),
            RowContent::Gap(gap) => gap.label(),
            // Another lab's review, as the Swift RowDebug says it.
            RowContent::LabReview(r) => {
                format!("{}{}", r.heading(), r.findings.iter().map(|f| format!("; #{} {}: {}", f.id, f.outcome.as_deref().unwrap_or("no answer"), f.claim)).collect::<String>())
            }
        }
    }

    fn user_text(message: &UserMessage) -> String {
        // One on its way says so, as its spinner or clock does.
        if let Some(pending) = message.pending {
            let word = match pending {
                Pending::Sending => "sending",
                Pending::Queued => "queued",
                Pending::WaitingForRunner => "waiting for the runner",
            };
            let plain = UserMessage { pending: None, ..message.clone() };
            return format!("({word}) {}", Self::user_text(&plain));
        }
        if message.text.is_empty() {
            return if message.images.len() == 1 { "1 image".to_owned() } else { format!("{} images", message.images.len()) };
        }
        // A dump should say it wasn't Mike, as the transcript does.
        if message.is_from_runner() {
            return format!("(sent by the runner) {}", message.text);
        }
        if message.is_from_schedule() {
            return format!("(sent by a schedule) {}", message.text);
        }
        message.text.clone()
    }

    /// `oneLine`: runs of whitespace (Swift's Character.isWhitespace, the
    /// same Unicode property as char::is_whitespace) as one space, cut to
    /// `limit` characters.
    pub fn one_line(words: &str, limit: usize) -> String {
        let joined = words.split(char::is_whitespace).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
        if text::count(&joined) <= limit {
            return joined;
        }
        format!("{}…", text::prefix(&joined, limit.saturating_sub(1)))
    }
}

fn picked(set: &BTreeSet<i64>, index: usize) -> bool {
    set.contains(&(index as i64))
}

/// Where "Copy debug info" says to look for a row's events
/// (`RowDebug.EventsSource`): the `tb` that reaches this app's runner, or a
/// fixture's events file when the app runs on one.
pub enum EventsSource {
    Tb(String),
    Fixture,
}

/// What "Copy debug info" says: enough for an agent to find the row and the
/// events behind it (`RowDebug.info`).
pub fn info(thread_id: &str, row: &Row, seqs: &[i64], runner_version: &str, source: &EventsSource) -> String {
    let mut lines = vec![format!("thread: {thread_id}"), format!("row: {}", row.id), format!("kind: {}", row.kind().as_str())];
    if let Some(turn) = row.turn_id.as_deref().filter(|t| !t.is_empty()) {
        lines.push(format!("turn: {turn}"));
    }
    // The footer shows only the time and the tokens' short form; the turn's
    // counts in full are here.
    if let RowContent::Footer(done) = &row.content {
        if let Some(tokens) = token_line(done.usage.as_ref()) {
            lines.push(format!("tokens: {tokens}"));
        }
        if let Some(kind) = done.error_kind {
            lines.push(format!("error kind: {}", kind.as_str()));
        }
    }
    lines.push(format!("events: {}", if seqs.is_empty() { "none loaded".to_owned() } else { seq_ranges(seqs) }));
    lines.push(format!("runner: {}", if runner_version.is_empty() { "unknown" } else { runner_version }));
    // The command that prints those events, ready to run.
    if let (Some(first), Some(last)) = (seqs.iter().min(), seqs.iter().max()) {
        lines.push(match source {
            // The id quoted as one word: another runner's ids are what it
            // wrote to the backend, and this line is meant to be pasted
            // into a shell.
            EventsSource::Tb(tb) => format!("see: {tb} events {} --from {first} --to {last}", shell_word(thread_id)),
            EventsSource::Fixture => {
                let range = if first == last { format!("line {first}") } else { format!("lines {first}-{last}") };
                format!("see: {range} of events.ndjson in the thread's fixture folder (line N is seq N)")
            }
        });
    }
    lines.join("\n") + "\n"
}

/// `text` as one word a shell takes as it is: bare when it's only safe
/// characters, in single quotes otherwise (the default data folder has a
/// space; another runner's thread id could have anything).
pub fn shell_word(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)) {
        text.to_owned()
    } else {
        crate::sign_in::quoted(text)
    }
}

/// "3, 7-9, 12": runs of seqs written as ranges, so a big group's events
/// stay one short line.
pub fn seq_ranges(seqs: &[i64]) -> String {
    let sorted: Vec<i64> = seqs.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
    let mut parts = Vec::new();
    let mut index = 0;
    while index < sorted.len() {
        let mut end = index;
        while end + 1 < sorted.len() && sorted[end + 1] == sorted[end] + 1 {
            end += 1;
        }
        parts.push(if end > index { format!("{}-{}", sorted[index], sorted[end]) } else { sorted[index].to_string() });
        index = end + 1;
    }
    parts.join(", ")
}

/// The seqs of the loaded events a row was built from, oldest first
/// (`TranscriptModel.sourceSeqs`): a tool's call, result and diff; an
/// approval's request and its answer; a group's members; a changes card's
/// edits; the turn's start for the working line.
pub fn source_seqs(model: &super::model::TranscriptModel, row_id: &str) -> Vec<i64> {
    let mut wanted: BTreeSet<String> = BTreeSet::from([row_id.to_owned()]);
    match model.row(row_id).or_else(|| model.item(row_id)).map(|r| &r.content) {
        Some(RowContent::Group(group)) => wanted = group.member_ids.iter().cloned().collect(),
        Some(RowContent::Changes(card)) => {
            wanted = model.items().iter().filter(|i| i.turn_id.as_deref() == Some(card.turn_id.as_str()) && i.kind() == super::row::RowKind::Diff).map(|i| i.id.clone()).collect();
        }
        _ => {}
    }
    let mut plan_rows = std::collections::HashMap::new();
    model.events().iter().filter(|e| row_ids_for(e, &mut plan_rows).iter().any(|id| wanted.contains(id))).map(|e| e.seq).collect()
}

/// The item and derived rows an event feeds, with the ids the reducer gives
/// them. `plan_rows` carries each turn's plan row along, since every update
/// of a turn's plan goes to its first one.
fn row_ids_for(event: &super::event::Event, plan_rows: &mut std::collections::HashMap<String, String>) -> Vec<String> {
    use super::event::{ActionRun, EventPayload as P};
    let seq = event.seq;
    match &event.payload {
        P::UserMessage { .. } => vec![format!("u:{seq}")],
        P::AssistantText { item_id, .. } => vec![format!("a:{item_id}")],
        P::Reasoning { item_id, .. } => vec![format!("r:{item_id}")],
        P::ToolCall(call) => vec![format!("t:{}", call.item_id)],
        P::ToolResult(result) => {
            let mut ids = vec![format!("t:{}", result.item_id)];
            if !result.images.is_empty() {
                ids.push(format!("i:{}", result.item_id));
            }
            ids
        }
        P::EditDiff(diff) => vec![format!("t:{}", diff.item_id)],
        P::ApprovalRequested(request) => vec![format!("ap:{}", request.approval_id)],
        P::ApprovalQueued(queue) => vec![format!("ap:{}", queue.approval_id)],
        P::QuestionRequested(request) => vec![format!("q:{}", request.approval_id)],
        P::SecretRequested(request) => vec![format!("s:{}", request.approval_id)],
        P::ReviewUpdated(review) => vec![format!("review:{}", review.id)],
        P::ApprovalResolved(resolution) => vec![format!("ap:{}", resolution.approval_id), format!("q:{}", resolution.approval_id), format!("s:{}", resolution.approval_id)],
        P::PlanProposed { item_id, .. } | P::PlanResolved { item_id, .. } => vec![format!("pl:{item_id}")],
        P::PlanUpdated(_) => {
            let key = event.turn_id.clone().unwrap_or_else(|| format!("seq{seq}"));
            let id = plan_rows.entry(key).or_insert_with(|| format!("p:{seq}")).clone();
            vec![id]
        }
        P::TurnStarted { .. } => vec![format!("w:{}", event.turn_id.as_deref().unwrap_or(""))],
        P::TurnCompleted(done) => vec![format!("f:{}", if done.turn_id.is_empty() { event.turn_id.clone().unwrap_or_else(|| format!("seq{seq}")) } else { done.turn_id.clone() })],
        P::SystemNote(_) => vec![format!("n:{seq}")],
        P::ThreadRewound(_) => vec![format!("rw:{seq}")],
        P::Error(_) => vec![format!("e:{seq}")],
        P::ActionRun(_) => vec![format!("t:{}", ActionRun::item_id(seq))],
        _ => vec![],
    }
}

#[cfg(test)]
mod info_tests {
    use super::*;

    #[test]
    fn seq_runs_are_written_as_ranges() {
        assert_eq!(seq_ranges(&[12, 3, 7, 8, 9, 3]), "3, 7-9, 12");
        assert_eq!(seq_ranges(&[]), "");
    }
}
