use gpui::{App, Entity, IntoElement, RenderOnce, Window};

use super::{InputBaseState, TextareaMode};

/// State for editing ordinary multi-line text.
///
/// This is the shared editing engine in its multi-line kind. Code-editor
/// facilities such as languages, diagnostics, folding, and LSP do not exist on
/// this type — those methods live on [`super::EditorState`].
pub type TextareaState = InputBaseState<TextareaMode>;

impl TextareaState {
    /// This app's: replace the marks drawn under the text (a misspelled
    /// word's wavy line). Ranges are UTF-8 byte offsets into the text as it
    /// is now; one past the end or reversed is dropped, and each is clipped
    /// to whole characters.
    pub fn set_text_marks(&mut self, marks: Vec<super::TextDecoration>, cx: &mut gpui::Context<Self>) {
        let len = self.text.len();
        let mut marks: Vec<super::TextDecoration> = marks
            .into_iter()
            .filter(|m| m.range.start < m.range.end && m.range.end <= len)
            .map(|mut m| {
                use super::RopeExt as _;
                m.range = self.text.clip_offset(m.range.start, sum_tree::Bias::Left)..self.text.clip_offset(m.range.end, sum_tree::Bias::Right);
                m
            })
            .collect();
        marks.sort_by_key(|m| m.range.start);
        if marks != self.extras.marks {
            self.extras.marks = marks;
            cx.notify();
        }
    }

    /// This app's: the marks [`Self::set_text_marks`] last set.
    pub fn text_marks(&self) -> &[super::TextDecoration] {
        &self.extras.marks
    }
}

/// An unstyled ordinary multi-line text input.
#[derive(IntoElement)]
pub struct Textarea {
    presentation: super::InlineTokenPresentation,
    state: Entity<TextareaState>,
}

impl Textarea {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            presentation: Default::default(),
        }
    }
    /// The element each atomic token renders as; the input keeps editing and history.
    pub fn token<R: IntoElement>(
        mut self,
        render: impl Fn(&super::InlineTokenContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.presentation = self.presentation.token(render);
        self
    }
    /// Open a reference after a completed, unconsumed token click.
    pub fn on_token_click(
        mut self,
        listener: impl Fn(&super::InlineTokenClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.presentation = self.presentation.on_token_click(listener);
        self
    }
    /// Report pointer presence over a token; hover never selects or edits.
    pub fn on_token_hover(
        mut self,
        listener: impl Fn(&super::InlineTokenHoverEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.presentation = self.presentation.on_token_hover(listener);
        self
    }
}

impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, _| {
            state.set_token_presentation(self.presentation)
        });
        self.state
    }
}
