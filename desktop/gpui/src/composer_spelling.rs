// Spelling in the message box, as the Swift app's NSTextView had it: a wavy
// line under a misspelled word once it's typed (green under grammar, when
// that's on), the Mac's autocorrection and text replacements as a word ends,
// and the right-click menu's guesses, Ignore Spelling and Learn Spelling, with
// Spelling and Grammar, Substitutions, Transformations and Speech under them.
// The Swift app's menu bar never had those four; its box's right-click menu
// did, and this is that menu.
//
// The Mac's spell checker answers (text_services.rs); what's done with the
// answers is threadbear_core::spelling. Off a Mac there's no spell checker,
// so no marks, and the menu is the kit's Cut, Copy, Paste and Select All.
use crate::composer::Composer;
use crate::text_services::{self, Checks};
use gpui_kit::component::input::{Copy, Cut, Paste, SelectAll, TextDecoration, TextareaState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;
use std::ops::Range;
use threadbear_core::spelling::{self, Transform};

actions!(
    text_services,
    [
        IgnoreSpelling,
        LearnSpelling,
        CheckDocumentNow,
        ToggleSpellingWhileTyping,
        ToggleGrammar,
        ToggleCorrectSpelling,
        ToggleSmartQuotes,
        ToggleSmartDashes,
        ToggleTextReplacement,
        MakeUpperCase,
        MakeLowerCase,
        Capitalize,
        StartSpeaking,
        StopSpeaking,
        PasteAndMatchStyle,
    ]
);

/// One of the spell checker's guesses, chosen from the right-click menu: put
/// `word` where `was` is, at `start..end`, if it's still there.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = text_services, no_json)]
pub(crate) struct UseGuess {
    pub word: String,
    pub was: String,
    pub start: usize,
    pub end: usize,
}

/// The red and green of AppKit's marks.
fn mark_style(grammar: bool) -> HighlightStyle {
    let color = if grammar { hsla(122. / 360., 0.55, 0.42, 1.) } else { hsla(0., 0.85, 0.55, 1.) };
    HighlightStyle { underline: Some(UnderlineStyle { color: Some(color), thickness: gpui_kit::px(1.), wavy: true }), ..Default::default() }
}

/// What the box's spelling knows between edits.
#[derive(Default)]
pub(crate) struct SpellState {
    /// The text as it was after the last edit, to tell a typed space from a
    /// paste.
    last: String,
    /// The marks drawn now, `true` for grammar.
    pub(crate) marks: Vec<(Range<usize>, bool)>,
    /// Counts edits, so a check waiting for typing to pause knows whether
    /// another edit came first.
    edits: u64,
}

/// How long typing pauses before the whole box is checked again. Between
/// checks the marks move with the words (`shift_marks`), so a long message
/// isn't read through on every key, as NSTextView doesn't either.
const CHECK_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

impl Composer {
    pub(crate) fn checks(&self, cx: &App) -> Checks {
        self.app.upgrade().map(|a| a.read(cx).new_thread.prefs.text_checks).unwrap_or_default()
    }

    /// After every change to the box: corrections and substitutions for a
    /// word just ended, then the marks again.
    pub(crate) fn spelling_after_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !text_services::AVAILABLE {
            return;
        }
        let input = self.input.clone();
        let (text, selection) = Self::text_and_selection(&input, cx);
        let old = std::mem::replace(&mut self.spell.last, text.clone());
        let checks = self.checks(cx);
        let caret = selection.end;
        if selection.is_empty() && checks.any_substitution() && text.len() <= text_services::MAX_CHECKED {
            if let Some(typed) = spelling::typed_boundary(&old, &text, caret) {
                let paragraph = text[..caret].rfind('\n').map_or(0, |i| i + 1);
                let found = text_services::check(&text, paragraph..caret, &checks, true);
                let chips: Vec<Range<usize>> = Self::chips_in(&input, cx).into_iter().map(|(r, _)| r).collect();
                let subs: Vec<_> = spelling::substitutions(&found, caret, typed)
                    .into_iter()
                    .filter(|(r, _)| !chips.iter().any(|c| c.start < r.end && r.start < c.end))
                    .collect();
                if !subs.is_empty() {
                    // Last first, so the ranges before stay right; each one
                    // Undo takes back, as AppKit's correction is.
                    let shift: isize = subs.iter().map(|(r, with)| with.len() as isize - r.len() as isize).sum();
                    input.update(cx, |s, cx| {
                        for (range, with) in &subs {
                            s.set_selected_range(range.clone(), cx);
                            s.replace(with.clone(), window, cx);
                        }
                        let caret = (caret as isize + shift).max(0) as usize;
                        s.set_selected_range(caret..caret, cx);
                    });
                    self.spell.last = Self::text_and_selection(&input, cx).0;
                }
            }
        }
        let now = self.spell.last.clone();
        // The box's value set whole since (a send, a draft put back) took
        // its marks away (`value_set`), and they were for other words.
        if input.read(cx).text_marks().is_empty() {
            self.spell.marks.clear();
        }
        self.spell.marks = spelling::shift_marks(&self.spell.marks, &old, &now);
        self.draw_spelling_marks(cx);
        self.spell.edits += 1;
        let edits = self.spell.edits;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHECK_AFTER).await;
            this.update(cx, |this, cx| {
                if this.spell.edits == edits {
                    this.refresh_spelling_marks(false, true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn draw_spelling_marks(&mut self, cx: &mut Context<Self>) {
        let drawn: Vec<TextDecoration> = self.spell.marks.iter().map(|(r, grammar)| TextDecoration::new(r.clone(), mark_style(*grammar))).collect();
        self.input.update(cx, |s, cx| s.set_text_marks(drawn, cx));
    }

    /// Marks the misspelled words (and grammar, when it's on). `now`: Check
    /// Document Now, which marks even with checking as you type off.
    /// `typing`: after an edit, when the word the caret's in is still being
    /// typed and waits; otherwise (leaving the box, a right-click) it's
    /// checked too.
    pub(crate) fn refresh_spelling_marks(&mut self, now: bool, typing: bool, cx: &mut Context<Self>) {
        let checks = self.checks(cx);
        let input = self.input.clone();
        let (text, selection) = Self::text_and_selection(&input, cx);
        let on = text_services::AVAILABLE && (now || checks.spelling || checks.grammar) && text.len() <= text_services::MAX_CHECKED;
        let marks = if on {
            let mut asked = checks;
            asked.spelling = checks.spelling || now;
            let found = text_services::check(&text, 0..text.len(), &asked, false);
            let chips: Vec<Range<usize>> = Self::chips_in(&input, cx).into_iter().map(|(r, _)| r).collect();
            // The word being typed waits until it's finished.
            let caret = (selection.is_empty() && typing && !now).then_some(selection.end);
            spelling::marks(&found, caret, &chips)
        } else {
            Vec::new()
        };
        self.spell.marks = marks;
        self.spell.last = text;
        self.draw_spelling_marks(cx);
    }

    /// The misspelled word the caret or selection is in.
    fn misspelled_here(&self, cx: &App) -> Option<(Range<usize>, String)> {
        let (text, selection) = Self::text_and_selection(&self.input, cx);
        let (range, _) = self.spell.marks.iter().find(|(r, grammar)| !grammar && r.start <= selection.start && selection.end <= r.end)?;
        Some((range.clone(), text.get(range.clone())?.to_owned()))
    }

    fn change_checks(&mut self, change: impl FnOnce(&mut Checks), cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, _| {
                let mut checks = app.new_thread.prefs.text_checks;
                change(&mut checks);
                // Correction and replacement follow the Mac's own settings
                // until they're switched here; then they're this app's.
                app.new_thread.prefs.text_checks = checks;
                crate::prefs::save(&app.new_thread.prefs);
            });
        }
        self.refresh_spelling_marks(false, false, cx);
    }

    fn transform(&mut self, how: Transform, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.input.clone();
        let (text, selection) = Self::text_and_selection(&input, cx);
        let range = if selection.is_empty() { spelling::word_at(&text, selection.start) } else { selection.clone() };
        let Some(words) = text.get(range.clone()) else { return };
        if range.is_empty() {
            return;
        }
        let with = how.apply(words);
        input.update(cx, |s, cx| {
            s.set_selected_range(range.clone(), cx);
            s.replace(with.clone(), window, cx);
            // The words stay selected, as AppKit leaves them.
            s.set_selected_range(range.start..range.start + with.len(), cx);
        });
    }

    /// The text field's handlers for the right-click menu's items and the
    /// Edit menu's Paste and Match Style.
    pub(crate) fn spelling_actions(&self, field: Div, cx: &mut Context<Self>) -> Div {
        field
            .on_action(cx.listener(|this, guess: &UseGuess, window, cx| {
                let input = this.input.clone();
                let (text, _) = Self::text_and_selection(&input, cx);
                if text.get(guess.start..guess.end) != Some(guess.was.as_str()) {
                    return;
                }
                input.update(cx, |s, cx| {
                    s.set_selected_range(guess.start..guess.end, cx);
                    s.replace(guess.word.clone(), window, cx);
                });
                this.refresh_spelling_marks(false, false, cx);
            }))
            .on_action(cx.listener(|this, _: &IgnoreSpelling, _, cx| {
                if let Some((_, word)) = this.misspelled_here(cx) {
                    text_services::ignore(&word);
                    this.refresh_spelling_marks(false, false, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &LearnSpelling, _, cx| {
                if let Some((_, word)) = this.misspelled_here(cx) {
                    text_services::learn(&word);
                    this.refresh_spelling_marks(false, false, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CheckDocumentNow, _, cx| this.refresh_spelling_marks(true, false, cx)))
            .on_action(cx.listener(|this, _: &ToggleSpellingWhileTyping, _, cx| this.change_checks(|c| c.spelling = !c.spelling, cx)))
            .on_action(cx.listener(|this, _: &ToggleGrammar, _, cx| this.change_checks(|c| c.grammar = !c.grammar, cx)))
            .on_action(cx.listener(|this, _: &ToggleCorrectSpelling, _, cx| this.change_checks(|c| c.correct = Some(!c.correct()), cx)))
            .on_action(cx.listener(|this, _: &ToggleSmartQuotes, _, cx| this.change_checks(|c| c.quotes = !c.quotes, cx)))
            .on_action(cx.listener(|this, _: &ToggleSmartDashes, _, cx| this.change_checks(|c| c.dashes = !c.dashes, cx)))
            .on_action(cx.listener(|this, _: &ToggleTextReplacement, _, cx| this.change_checks(|c| c.replace = Some(!c.replace()), cx)))
            .on_action(cx.listener(|this, _: &MakeUpperCase, window, cx| this.transform(Transform::Upper, window, cx)))
            .on_action(cx.listener(|this, _: &MakeLowerCase, window, cx| this.transform(Transform::Lower, window, cx)))
            .on_action(cx.listener(|this, _: &Capitalize, window, cx| this.transform(Transform::Capitalize, window, cx)))
            // NSTextView reads the selection, or everything with none.
            .on_action(cx.listener(|this, _: &StartSpeaking, _, cx| {
                let (text, selection) = Self::text_and_selection(&this.input, cx);
                let words = if selection.is_empty() { text.as_str() } else { text.get(selection).unwrap_or_default() };
                text_services::speak(words);
            }))
            .on_action(cx.listener(|_, _: &StopSpeaking, _, _| text_services::stop_speaking()))
            // Words only: a copied picture or file stays out (the Swift box is
            // plain text, so its Paste and Match Style was the same).
            .on_action(cx.listener(|this, _: &PasteAndMatchStyle, window, cx| {
                let Some(words) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
                this.input.update(cx, |s, cx| s.replace(words.to_string(), window, cx));
            }))
    }
}

/// The box's right-click menu, NSTextView's: the guesses for a misspelled
/// word, then Cut, Copy and Paste, then the four submenus.
pub(crate) fn context_menu(composer: &WeakEntity<Composer>, input: &Entity<TextareaState>, window: &mut Window, cx: &mut App) -> NativeMenu {
    let mut menu = NativeMenu::new();
    let composer = composer.upgrade();
    // Checked now, the word clicked in included: the marks may be waiting for
    // typing to pause, and the click put the caret in the word.
    if let Some(c) = &composer {
        c.update(cx, |c, cx| c.refresh_spelling_marks(false, false, cx));
    }
    let (has_selection, editable) = {
        let state = input.read(cx);
        (!state.selected_range().is_empty(), state.presentation().is_editable())
    };
    if let Some(misspelled) = composer.as_ref().and_then(|c| c.read(cx).misspelled_here(cx)) {
        let (range, was) = misspelled;
        let text = input.read(cx).value().to_string();
        let guesses = text_services::guesses(&text, range.clone());
        if guesses.is_empty() {
            menu = menu.menu_with_disabled("No Guesses Found", true, Box::new(IgnoreSpelling));
        }
        for word in guesses.into_iter().take(8) {
            menu = menu.menu(word.clone(), Box::new(UseGuess { word, was: was.clone(), start: range.start, end: range.end }));
        }
        menu = menu.separator().menu("Ignore Spelling", Box::new(IgnoreSpelling)).menu("Learn Spelling", Box::new(LearnSpelling)).separator();
    }
    menu = menu
        .menu_with_disabled("Cut", !(editable && has_selection), Box::new(Cut))
        .menu_with_disabled("Copy", !has_selection, Box::new(Copy))
        .menu_with_disabled("Paste", !editable, Box::new(Paste))
        .separator()
        .menu("Select All", Box::new(SelectAll));
    if !text_services::AVAILABLE {
        return menu;
    }
    menu = menu.separator()
        .menu_with_disabled("Look Up", !crate::native_text::has_lookup(window, cx), Box::new(crate::native_text::LookUp))
        .menu_with_disabled("Services…", !crate::native_text::has_selection(window, cx), Box::new(crate::native_text::Services));
    if crate::native_text::translate_available() {
        menu = menu.menu_with_disabled("Translate", !has_selection, Box::new(crate::native_text::Translate));
    }
    if crate::native_text::writing_tools_available() {
        menu = menu.menu_with_disabled("Writing Tools", !has_selection, Box::new(crate::native_text::WritingTools));
    }
    let checks = composer.as_ref().map(|c| c.read(cx).checks(cx)).unwrap_or_default();
    // "Show Spelling and Grammar" and "Show Substitutions" are AppKit's
    // panels, which only an NSTextView can answer; they're left out.
    let spelling = NativeMenu::new()
        .menu("Check Document Now", Box::new(CheckDocumentNow))
        .separator()
        .menu_with_check("Check Spelling While Typing", checks.spelling, Box::new(ToggleSpellingWhileTyping))
        .menu_with_check("Check Grammar With Spelling", checks.grammar, Box::new(ToggleGrammar))
        .menu_with_check("Correct Spelling Automatically", checks.correct(), Box::new(ToggleCorrectSpelling));
    let substitutions = NativeMenu::new()
        .menu_with_check("Smart Quotes", checks.quotes, Box::new(ToggleSmartQuotes))
        .menu_with_check("Smart Dashes", checks.dashes, Box::new(ToggleSmartDashes))
        .menu_with_check("Text Replacement", checks.replace(), Box::new(ToggleTextReplacement));
    let transformations = NativeMenu::new()
        .menu_with_disabled("Make Upper Case", !editable, Box::new(MakeUpperCase))
        .menu_with_disabled("Make Lower Case", !editable, Box::new(MakeLowerCase))
        .menu_with_disabled("Capitalize", !editable, Box::new(Capitalize));
    let speech = NativeMenu::new()
        .menu("Start Speaking", Box::new(StartSpeaking))
        .menu_with_disabled("Stop Speaking", !text_services::speaking(), Box::new(StopSpeaking));
    menu.separator()
        .submenu("Spelling and Grammar", spelling)
        .submenu("Substitutions", substitutions)
        .submenu("Transformations", transformations)
        .submenu("Speech", speech)
}

/// `composer-spelling`: the words marked now, each with "spelling" or
/// "grammar".
#[cfg(feature = "automation")]
pub(crate) fn marks_command(composer: &Entity<Composer>, cx: &App) -> serde_json::Value {
    let c = composer.read(cx);
    let (text, _) = Composer::text_and_selection(&c.input, cx);
    let marks: Vec<String> = c.spell.marks.iter().map(|(r, grammar)| format!("{} ({})", text.get(r.clone()).unwrap_or_default(), if *grammar { "grammar" } else { "spelling" })).collect();
    serde_json::json!(format!("ok {}", serde_json::to_string(&marks).unwrap_or_default()))
}

/// `composer-right-click`: the box's right-click menu with the caret in the word
/// `at` (its first time in the box), as titles (`NativeMenu::titles`); with
/// `choose`, that item is chosen instead, as a click on it would.
#[cfg(feature = "automation")]
pub(crate) fn menu_command(composer: &Entity<Composer>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    let input = composer.read(cx).input.clone();
    if let Some(at) = cmd["at"].as_str() {
        let text = input.read(cx).value().to_string();
        let Some(start) = text.find(at) else { return Err(format!("no {at:?} in the box")) };
        let caret = start + at.char_indices().nth(1).map_or(at.len(), |(i, _)| i);
        input.update(cx, |s, cx| s.set_selected_range(caret..caret, cx));
    }
    let menu = context_menu(&composer.downgrade(), &input, window, cx);
    let Some(choose) = cmd["choose"].as_str() else {
        return Ok(serde_json::json!(format!("ok {}", serde_json::to_string(&menu.titles()).unwrap_or_default())));
    };
    let action = menu.action_for(choose).ok_or_else(|| format!("no item {choose:?} to choose"))?;
    let handle = input.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
    crate::web_panels::keyboard_to_app(window);
    window.dispatch_action(action, cx);
    Ok(serde_json::json!("ok"))
}
