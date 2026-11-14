//! The board in a thread's Highlights (Swift's BoardLayout and ProjectTasks.board).
use crate::projects::{Task, TaskColumn, TaskKind, Tasks};

pub struct Column<'a> {
    pub column: &'a TaskColumn,
    pub cards: Vec<&'a Task>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingMention {
    pub reference: String,
    /// None is the unsent draft; an edit keeps its own queued clicks.
    pub edit: Option<i64>,
}

pub fn take_mentions(pending: &mut Vec<PendingMention>, edit: Option<i64>, restoring: bool) -> Vec<String> {
    let mut ready = vec![];
    for mention in std::mem::take(pending) {
        if mention.edit == edit && (edit.is_some() || !restoring) {
            ready.push(mention.reference);
        } else if mention.edit.is_none() {
            pending.push(mention);
        }
        // A queued edit click goes with that edit; Cancel or choosing a
        // different old message discards it, like changes already typed.
    }
    ready
}

impl Tasks {
    /// Keep Notion's order within each group; this thread's cards come first.
    /// An entirely finished board has nothing to show beside a working thread.
    pub fn board(&self, thread_id: &str) -> Vec<Column<'_>> {
        if self.open_count() == 0 { return vec![] }
        let mut columns: Vec<_> = self.columns.iter().collect();
        columns.sort_by_key(|c| match c.kind { TaskKind::Doing => 0, TaskKind::Todo => 1, TaskKind::Done => 2 });
        columns.into_iter().filter_map(|column| {
            let mut cards = self.items_in(column);
            if cards.is_empty() { return None }
            cards.sort_by_key(|c| !c.is_worked_by(thread_id));
            Some(Column { column, cards })
        }).collect()
    }
}

impl Task {
    pub fn reference(&self) -> String {
        let title = self.title.replace('[', "\\[").replace(']', "\\]");
        if self.url.is_empty() { title } else { format!("[{title}]({})", self.url) }
    }

    /// A complete id wins over prefixes; an ambiguous prefix opens nothing.
    pub fn working_thread<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
        let prefix = self.thread.as_deref()?.trim().to_ascii_lowercase();
        if prefix.chars().count() < 6 { return None }
        let (mut found, mut count) = (None, 0);
        for id in ids {
            if id.eq_ignore_ascii_case(&prefix) { return Some(id) }
            if id.to_ascii_lowercase().starts_with(&prefix) { found = Some(id); count += 1; }
        }
        if count == 1 { found } else { None }
    }
}

pub fn adding(reference: &str, draft: &str) -> String {
    if draft.contains(reference) { return draft.to_owned() }
    if draft.trim().is_empty() { return format!("{reference} ") }
    let separator = if draft.ends_with(char::is_whitespace) { "" } else { " " };
    format!("{draft}{separator}{reference} ")
}

pub fn column_width(available: f32, columns: usize, folded: usize) -> f32 {
    if columns == 0 || available <= 0. { return 146. }
    let gaps = (columns + folded).saturating_sub(1) as f32 * 6.;
    ((available - gaps - folded as f32 * 28.) / columns as f32).max(146.)
}

pub fn page(offset: f32, direction: i32, step: f32, maximum: f32) -> f32 {
    if step <= 0. { return 0. }
    let target = if direction < 0 && offset >= maximum && maximum > 0. {
        (maximum / step).floor() * step
    } else if direction > 0 {
        ((offset / step + 0.001).floor() + 1.) * step
    } else {
        ((offset / step - 0.001).ceil() - 1.) * step
    };
    target.clamp(0., maximum.max(0.))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn column(name: &str, kind: TaskKind) -> TaskColumn { TaskColumn { name: name.into(), kind, ..Default::default() } }
    fn card(id: &str, status: &str, thread: Option<&str>) -> Task { Task { id: id.into(), status: status.into(), thread: thread.map(str::to_owned), ..Default::default() } }

    #[test]
    fn board_prioritises_the_thread_without_reordering_notion_peers() {
        let mut tasks = Tasks { columns: vec![column("To do", TaskKind::Todo), column("Doing", TaskKind::Doing), column("Waiting", TaskKind::Doing), column("Done", TaskKind::Done), column("Empty", TaskKind::Todo)], items: vec![card("a", "Doing", None), card("b", "Doing", Some("ABCDEF")), card("c", "Doing", None), card("d", "Waiting", None), card("e", "To do", None), card("f", "Done", None)], ..Default::default() };
        let board = tasks.board("abcdef-123");
        assert_eq!(board.iter().map(|c| c.column.name.as_str()).collect::<Vec<_>>(), ["Doing", "Waiting", "To do", "Done"]);
        assert_eq!(board[0].cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        tasks.items.retain(|c| c.status == "Done");
        assert!(tasks.board("abcdef-123").is_empty());
    }

    #[test]
    fn mention_is_an_escaped_link_added_once_at_the_end() {
        let task = Task { title: "Fix [share]".into(), url: "https://www.notion.so/card".into(), ..Default::default() };
        let reference = task.reference();
        assert_eq!(reference, "[Fix \\[share\\]](https://www.notion.so/card)");
        let draft = adding(&reference, "Please check");
        assert_eq!(draft, format!("Please check {reference} "));
        assert_eq!(adding(&reference, &draft), draft);
        assert_eq!(adding(&reference, "  \n"), format!("{reference} "));
        assert_eq!(adding(&reference, "Check\n"), format!("Check\n{reference} "));
    }

    #[test]
    fn columns_share_room_then_scroll_by_whole_columns() {
        assert_eq!(column_width(450., 3, 1), 146.);
        assert_eq!(column_width(800., 3, 1), (800. - 18. - 28.) / 3.);
        assert_eq!(page(0., 1, 152., 250.), 152.);
        assert_eq!(page(152., 1, 152., 250.), 250.);
        assert_eq!(page(250., -1, 152., 250.), 152.);
        assert_eq!(page(152., -1, 152., 250.), 0.);
    }

    #[test]
    fn working_thread_prefers_an_exact_id_and_refuses_ambiguous_prefixes() {
        let mut task = Task { thread: Some(" ABC123 ".into()), ..Default::default() };
        assert_eq!(task.working_thread(["abc123-one"]), Some("abc123-one"));
        assert_eq!(task.working_thread(["abc123-one", "abc123-two"]), None);
        assert_eq!(task.working_thread(["abc123-one", "abc123", "abc123-two"]), Some("abc123"));
        task.thread = Some("abc".into());
        assert_eq!(task.working_thread(["abc123"]), None);
    }

    #[test]
    fn a_draft_mention_waits_through_an_edit_while_a_picture_restore_finishes() {
        let mut pending = vec![PendingMention { reference: "draft card".into(), edit: None }];
        assert!(take_mentions(&mut pending, Some(7), true).is_empty());
        pending.push(PendingMention { reference: "edit card".into(), edit: Some(7) });
        assert_eq!(take_mentions(&mut pending, Some(7), true), ["edit card"]);
        assert_eq!(pending.len(), 1);
        assert!(take_mentions(&mut pending, None, true).is_empty());
        assert_eq!(take_mentions(&mut pending, None, false), ["draft card"]);
        assert!(pending.is_empty());
        pending.push(PendingMention { reference: "cancelled edit".into(), edit: Some(8) });
        assert!(take_mentions(&mut pending, None, false).is_empty());
        assert!(pending.is_empty());
    }
}
