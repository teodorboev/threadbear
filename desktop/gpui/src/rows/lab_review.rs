// Another lab's review as a row over the turn's reply (RowRenderer.reviewText):
// a quiet line saying who reviewed and how it came out, which opens the
// review's thread, then each finding with what the agent did about it
// (fixed, disputed and why, left for Mike, or no answer) and where. It was on
// the done card until 12 Nov 2026; Mike moved it into the transcript, as the
// review runs in the background and needn't draw his eye. The ledger is
// `turn.completed`'s `review` (docs/protocol.md, "Review by another lab").
use super::row_wrap;
use super::metrics::ROW_TOP;
use crate::style::{self, size};
use crate::thread::ThreadPane;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use threadbear_core::reducer::{LabReview, ReviewFinding, Row};
use crate::tips::Tip as _;

/// A finding's mark (SF's checkmark.circle, exclamationmark.bubble,
/// person.crop.circle and circle, as Lucide draws them), its word and its
/// colour, by what was done about it.
fn outcome(finding: &ReviewFinding, cx: &App) -> (IconName, &'static str, Hsla) {
    match finding.outcome.as_deref() {
        Some("fixed") => (IconName::CircleCheck, "Fixed", style::tick_green(cx)),
        Some("disputed") => (IconName::MessageSquareWarning, "Disputed", style::needs_you(cx)),
        Some("left") => (IconName::CircleUser, "Left for you", style::accent(cx)),
        _ => (IconName::Circle, "No answer", style::faint_text(cx)),
    }
}

/// The line under a finding: what was done, the agent's note, how serious,
/// and where.
fn finding_detail(finding: &ReviewFinding, word: &str) -> String {
    let mut detail = word.to_owned();
    if let Some(note) = finding.note.as_deref().filter(|n| !n.is_empty()) {
        detail.push_str(&format!(": {note}"));
    }
    if finding.severity == "high" {
        detail.push_str(" · serious");
    }
    if let Some(place) = finding.where_.as_deref().filter(|w| !w.is_empty()) {
        detail.push_str(&format!(" · {place}"));
    }
    detail
}

impl ThreadPane {
    /// The ledger's row: all of it small and quiet, the heading grey rather
    /// than in the link colour, as the Swift line's `.tbAction` run is.
    pub(crate) fn lab_review_row(&mut self, row: &Row, review: &LabReview, cx: &mut Context<Self>) -> AnyElement {
        let (quiet, secondary) = (style::quiet_text(cx), style::secondary_label(cx));
        // The words start past the mark.
        let words = (size::SMALL * 1.2).ceil() + 8.;
        let thread = review.thread_id.clone();
        let row_id = row.id.clone();
        let heading = div()
            .id(SharedString::from(format!("lab-review-{}", row.id)))
            .text_size(px(size::SMALL))
            .line_height(px(19.))
            .text_color(quiet)
            .child(review.heading())
            .when(!thread.is_empty(), |s| {
                s.cursor_pointer()
                    .hover(|s| s.text_color(secondary))
                    .tip("Open the review's thread")
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.link_clicked(&format!("thread:{thread}"), &row_id, window, cx);
                    }))
            });
        let mut column = v_flex().w_full().py(px(ROW_TOP)).child(heading);
        for finding in &review.findings {
            let (symbol, word, color) = outcome(finding, cx);
            column = column.child(
                h_flex()
                    .w_full()
                    .items_start()
                    .mt(px(4.))
                    .child(div().flex_none().w(px(words)).h(px(19.)).flex().items_center().child(Icon::from(symbol).with_size(px(13.)).text_color(color)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_size(px(size::SMALL)).line_height(px(19.)).text_color(secondary).child(finding.claim.clone()))
                            .child(div().text_size(px(size::SMALL)).line_height(px(17.)).text_color(quiet).child(finding_detail(finding, word))),
                    ),
            );
        }
        row_wrap(row, column)
    }
}
