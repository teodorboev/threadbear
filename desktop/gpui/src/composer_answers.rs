// The picks under a folded card (AnswerChips.swift): with the card of asks
// folded to its heading, the answers picked on it would go with the next
// message with no sign of them, so each shows as a chip over the box: its
// ask's number on a card of several, a pencil for an answer in Mike's own
// words, the answer, and × to take it back. A chip's tip is its ask.
use crate::composer::Composer;
use crate::style::{self, size};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{h_flex, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use threadbear_core::composer::{picked_answer_label, OWN_ANSWER};
use crate::tips::Tip as _;

/// What the × on a chip says, and is called.
const TAKE_BACK: &str = "Take this answer back";

impl Composer {
    /// Each pick's ask, its answer as the chip reads, and the pick; None
    /// when the card is open above (its buttons show the picks), or a
    /// message is being edited.
    fn answer_chips(&self, cx: &App) -> Option<(usize, Vec<(usize, String, i64, String)>)> {
        if self.editing.is_some() || self.card.picks.is_empty() {
            return None;
        }
        let card = self.open_card(cx).filter(|c| !c.is_done())?;
        if !self.card.collapsed {
            return None;
        }
        let chips: Vec<(usize, String, i64, String)> = self
            .card
            .picks
            .iter()
            .filter_map(|(&ask, &pick)| {
                let index = usize::try_from(ask).ok()?;
                let the_ask = card.asks.get(index)?;
                let label = picked_answer_label(the_ask, pick, self.card.notes.get(&ask))?;
                Some((index, label, pick, threadbear_core::sidebar::preview_plain(&the_ask.action)))
            })
            .collect();
        (!chips.is_empty()).then_some((card.asks.len(), chips))
    }

    /// The chips' row over the box, scrolling sideways when they don't fit.
    pub(crate) fn render_answer_chips(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (asks, chips) = self.answer_chips(cx)?;
        let accent = style::accent(cx);
        let secondary = style::secondary_label(cx);
        let mut row = h_flex().id("answer-chips").w_full().gap(px(6.)).overflow_x_scroll();
        for (index, label, pick, action) in chips {
            let take_back = cx.listener(move |this, _: &ClickEvent, window, cx| this.pick_answer(index, pick, window, cx));
            row = row.child(
                h_flex()
                    .id(("answer-chip", index))
                    .flex_none()
                    .gap(px(6.))
                    .px(px(9.))
                    .py(px(4.))
                    .rounded_full()
                    .bg(accent.opacity(0.14))
                    .border(gpui_kit::px(0.5))
                    .border_color(accent.opacity(0.45))
                    .text_size(px(size::CHIP))
                    .line_height(px(style::natural_line(size::CHIP)))
                    .tip(action.clone())
                    .when(asks > 1, |s| s.child(div().text_color(secondary).child((index + 1).to_string())))
                    .when(pick == OWN_ANSWER, |s| s.child(crate::icons::symbol("pencil").with_size(px(10.)).text_color(secondary)))
                    .child(div().max_w(px(260.)).truncate().text_color(style::label_text(cx)).child(label))
                    .child(
                        div()
                            .id(("answer-chip-remove", index))
                            .test_support()
                            .role(Role::Button).aria_label(TAKE_BACK)
                            .cursor_pointer()
                            .child(gpui_kit::component::Icon::from(gpui_kit::assets::IconName::X).with_size(px(10.)).text_color(secondary))
                            .tip(TAKE_BACK)
                            .on_click(take_back),
                    ),
            );
        }
        Some(row.into_any_element())
    }

    /// The chips as the dump's `composer.answers` (both apps): "1: done",
    /// "2: Just the internal testers".
    #[cfg(feature = "automation")]
    pub(crate) fn answer_chip_lines(&self, cx: &App) -> Option<Vec<String>> {
        let (_, chips) = self.answer_chips(cx)?;
        Some(chips.into_iter().map(|(index, label, _, _)| format!("{}: {label}", index + 1)).collect())
    }
}

/// How many beeps a scripted run has heard (the automation's `beeps`).
#[cfg(feature = "automation")]
static BEEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "automation")]
pub(crate) fn beeps() -> usize {
    BEEPS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The system's beep when Return can't go on: the ask still open is the
/// one in front already (TranscriptController.showUnanswered's NSSound.beep).
/// On the Mac the alert sound Mike chose; Windows and Linux have none here
/// yet, and stay quiet.
pub(crate) fn beep() {
    #[cfg(feature = "automation")]
    BEEPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    {
        // AppKit's own, which GPUI links: no feature of objc2-app-kit's
        // needed for one function.
        extern "C" {
            fn NSBeep();
        }
        // SAFETY: NSBeep takes nothing and is called on the main thread,
        // where the app's events are handled.
        unsafe { NSBeep() }
    }
}
