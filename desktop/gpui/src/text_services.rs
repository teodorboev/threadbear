// The Mac's text services for the app's own text: the spell checker that
// marks and corrects words in the message box (NSSpellChecker), Edit >
// Speech (NSSpeechSynthesizer), and which words are selected right now, for
// those and for Services and Look Up (thread 0ff64630 uses
// `front_selection` and `replace_if_unchanged` for those).
//
// The Swift app got all of this from NSTextView. GPUI's text boxes aren't
// one, so the spell checker is asked directly and its answers drawn by the
// kit's textarea (`set_text_marks`, our patch in desktop/vendor). What's done
// with the answers is threadbear_core::spelling. Off a Mac there's no spell
// checker or speech here: every call finds nothing and does nothing, and the
// Edit menu leaves those items out.
use gpui_kit::component::input::TextareaState;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use threadbear_core::spelling::Found;

/// The Edit menu's switches for the message box, kept between runs (prefs).
/// Spelling while typing is on, grammar off, and quotes and dashes off as the
/// Swift composer had them (`isAutomaticQuoteSubstitutionEnabled = false`);
/// correction and text replacement follow the Mac's own Keyboard settings
/// until they're switched here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Checks {
    pub spelling: bool,
    pub grammar: bool,
    pub correct: Option<bool>,
    pub quotes: bool,
    pub dashes: bool,
    pub replace: Option<bool>,
}

impl Default for Checks {
    fn default() -> Self {
        Checks { spelling: true, grammar: false, correct: None, quotes: false, dashes: false, replace: None }
    }
}

impl Checks {
    pub fn correct(&self) -> bool {
        self.correct.unwrap_or_else(system_correction)
    }
    pub fn replace(&self) -> bool {
        self.replace.unwrap_or_else(system_replacement)
    }
    /// Whether anything is checked as words are typed.
    pub fn any_substitution(&self) -> bool {
        self.correct() || self.replace() || self.quotes || self.dashes
    }
}

/// Whether the spell checker and speech are here at all.
pub(crate) const AVAILABLE: bool = cfg!(target_os = "macos");

/// Longer than this, the box isn't checked as it's typed (a pasted log):
/// each check reads the whole text.
pub(crate) const MAX_CHECKED: usize = 20_000;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSRange, NSString, NSTextCheckingType};
    use threadbear_core::spelling::{byte_of_utf16, utf16_of_byte, Kind};

    thread_local! {
        /// One spell document for the app's text boxes, so Ignore Spelling
        /// lasts while the app runs, as an NSTextView's does for its window.
        static TAG: isize = NSSpellChecker::uniqueSpellDocumentTag();
        #[allow(deprecated)]
        static VOICE: Retained<objc2_app_kit::NSSpeechSynthesizer> = objc2_app_kit::NSSpeechSynthesizer::new();
    }

    fn checker() -> Retained<NSSpellChecker> {
        NSSpellChecker::sharedSpellChecker()
    }

    fn ns_range(text: &str, range: Range<usize>) -> NSRange {
        let start = utf16_of_byte(text, range.start);
        NSRange { location: start, length: utf16_of_byte(text, range.end) - start }
    }

    pub fn check(text: &str, within: Range<usize>, checks: &Checks, typing: bool) -> Vec<Found> {
        let mut types = NSTextCheckingType::empty();
        if checks.spelling || !typing {
            types |= NSTextCheckingType::Spelling;
        }
        if checks.grammar {
            types |= NSTextCheckingType::Grammar;
        }
        if typing {
            if checks.correct() {
                types |= NSTextCheckingType::Correction;
            }
            if checks.replace() {
                types |= NSTextCheckingType::Replacement;
            }
            if checks.quotes {
                types |= NSTextCheckingType::Quote;
            }
            if checks.dashes {
                types |= NSTextCheckingType::Dash;
            }
        }
        if types.is_empty() || text.is_empty() {
            return Vec::new();
        }
        let string = NSString::from_str(text);
        let tag = TAG.with(|t| *t);
        // SAFETY: no options, no orthography wanted, and a null word count,
        // which the method takes as "don't count".
        let results = unsafe { checker().checkString_range_types_options_inSpellDocumentWithTag_orthography_wordCount(&string, ns_range(text, within), types.0, None, tag, None, std::ptr::null_mut()) };
        let mut found = Vec::new();
        for result in results.iter() {
            let kind = match result.resultType() {
                t if t == NSTextCheckingType::Spelling => Kind::Spelling,
                t if t == NSTextCheckingType::Grammar => Kind::Grammar,
                t if t == NSTextCheckingType::Correction => Kind::Correction,
                t if t == NSTextCheckingType::Replacement => Kind::Replacement,
                t if t == NSTextCheckingType::Quote => Kind::Quote,
                t if t == NSTextCheckingType::Dash => Kind::Dash,
                _ => continue,
            };
            let r = result.range();
            let range = byte_of_utf16(text, r.location)..byte_of_utf16(text, r.location + r.length);
            let replacement = result.replacementString().map(|s| s.to_string());
            found.push(Found { range, kind, replacement });
        }
        // A plain check doesn't always offer corrections; AppKit's own
        // autocorrect asks for the one of the word just ended, so this does
        // too: the last misspelled word in what was checked.
        if typing && checks.correct() && !found.iter().any(|f| f.kind == Kind::Correction) {
            if let Some(word) = found.iter().filter(|f| f.kind == Kind::Spelling).max_by_key(|f| f.range.end).cloned() {
                let checker = checker();
                let language = checker.language();
                if let Some(with) = checker.correctionForWordRange_inString_language_inSpellDocumentWithTag(ns_range(text, word.range.clone()), &string, &language, tag) {
                    found.push(Found { range: word.range, kind: Kind::Correction, replacement: Some(with.to_string()) });
                }
            }
        }
        found
    }

    pub fn guesses(text: &str, range: Range<usize>) -> Vec<String> {
        let string = NSString::from_str(text);
        let tag = TAG.with(|t| *t);
        checker()
            .guessesForWordRange_inString_language_inSpellDocumentWithTag(ns_range(text, range), &string, None, tag)
            .map(|list| list.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    }

    pub fn learn(word: &str) {
        checker().learnWord(&NSString::from_str(word));
    }

    pub fn ignore(word: &str) {
        let tag = TAG.with(|t| *t);
        checker().ignoreWord_inSpellDocumentWithTag(&NSString::from_str(word), tag);
    }

    #[allow(deprecated)]
    pub fn speak(text: &str) {
        VOICE.with(|v| {
            v.stopSpeaking();
            v.startSpeakingString(&NSString::from_str(text));
        });
    }

    #[allow(deprecated)]
    pub fn stop_speaking() {
        VOICE.with(|v| v.stopSpeaking());
    }

    #[allow(deprecated)]
    pub fn speaking() -> bool {
        VOICE.with(|v| v.isSpeaking())
    }

    pub fn system_correction() -> bool {
        NSSpellChecker::isAutomaticSpellingCorrectionEnabled()
    }

    pub fn system_replacement() -> bool {
        NSSpellChecker::isAutomaticTextReplacementEnabled()
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::*;
    pub fn check(_: &str, _: Range<usize>, _: &Checks, _: bool) -> Vec<Found> {
        Vec::new()
    }
    pub fn guesses(_: &str, _: Range<usize>) -> Vec<String> {
        Vec::new()
    }
    pub fn learn(_: &str) {}
    pub fn ignore(_: &str) {}
    pub fn speak(_: &str) {}
    pub fn stop_speaking() {}
    pub fn speaking() -> bool {
        false
    }
    pub fn system_correction() -> bool {
        false
    }
    pub fn system_replacement() -> bool {
        false
    }
}

/// What the spell checker finds in `within` (byte offsets) of `text`.
/// `typing`: as words are typed, with the corrections and substitutions
/// switched on; otherwise (Check Document Now) spelling, and grammar if on.
pub(crate) fn check(text: &str, within: Range<usize>, checks: &Checks, typing: bool) -> Vec<Found> {
    mac::check(text, within, checks, typing)
}

/// The spell checker's suggestions for the word at `range`, best first.
pub(crate) fn guesses(text: &str, range: Range<usize>) -> Vec<String> {
    mac::guesses(text, range)
}

/// Learn Spelling: never marked again, in any app.
pub(crate) fn learn(word: &str) {
    mac::learn(word)
}

/// Ignore Spelling: not marked again while the app runs.
pub(crate) fn ignore(word: &str) {
    mac::ignore(word)
}

/// Edit > Speech > Start Speaking, in the Mac's spoken-content voice.
pub(crate) fn speak(text: &str) {
    mac::speak(text)
}

pub(crate) fn stop_speaking() {
    mac::stop_speaking()
}

pub(crate) fn speaking() -> bool {
    mac::speaking()
}

/// Keyboard settings' "Correct spelling automatically".
pub(crate) fn system_correction() -> bool {
    mac::system_correction()
}

/// Whether the Mac's text replacements are on (Keyboard > Text Input).
pub(crate) fn system_replacement() -> bool {
    mac::system_replacement()
}

/// Where the selected words are.
#[derive(Clone)]
pub(crate) enum SelectionSource {
    TextBox { input: WeakEntity<TextareaState>, range: Range<usize> },
    TextField { input: WeakEntity<gpui_kit::component::input::InputState>, range: Range<usize> },
    Editor { input: WeakEntity<gpui_kit::component::input::EditorState>, range: Range<usize> },
    Reply,
}

impl SelectionSource {
    pub(crate) fn is_editable(&self, cx: &App) -> bool {
        macro_rules! editable { ($input:expr) => { $input.upgrade().is_some_and(|input| { let state=input.read(cx); !state.is_sensitive() && !state.has_composition() && state.is_single_selection() && state.presentation().is_editable() }) }; }
        match self {
            Self::TextBox {input,..} => editable!(input),
            Self::TextField {input,..} => editable!(input),
            Self::Editor {input,..} => editable!(input),
            Self::Reply => false,
        }
    }
}

thread_local! {
    /// The app's text boxes that take part (each composer registers its own).
    static BOXES: std::cell::RefCell<Vec<WeakEntity<TextareaState>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Count a text box in for `front_selection`.
pub(crate) fn register_text_box(input: &Entity<TextareaState>) {
    BOXES.with_borrow_mut(|boxes| {
        boxes.retain(|b| b.upgrade().is_some());
        boxes.push(input.downgrade());
    });
}

/// The text box with the keyboard in `window`, if it's one of the app's.
pub(crate) fn focused_text_box(window: &Window, cx: &App) -> Option<Entity<TextareaState>> {
    BOXES.with_borrow(|boxes| boxes.iter().filter_map(|b| b.upgrade()).find(|b| b.read(cx).focus_handle(cx).is_focused(window)))
}

/// The words selected now, and where: the text box with the keyboard if it
/// has a selection, otherwise the words selected in a reply. `None` with
/// nothing selected.
/// GPUI keeps a logical focus handle while a native page has the actual
/// keyboard. Its old input selection must not leak into system text menus.
pub(crate) fn app_has_keyboard(window: &Window) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(view) = crate::web_panels::native_view(window) {
        return threadbear_web::keyboard_holder(view) == threadbear_web::KeyboardHolder::App;
    }
    let _ = window;
    true
}

pub(crate) fn front_selection(window: &mut Window, cx: &mut App) -> Option<(String, SelectionSource)> {
    use gpui_kit::component::{input::AnyInputState, WindowExt as _};
    if !app_has_keyboard(window) || crate::terminal::has_focus(window,cx) { return None }
    macro_rules! selection {
        ($input:expr, $variant:ident) => {{
            let input=$input;
            let state=input.read(cx);
            if state.is_sensitive() { return None }
            let range=state.selected_range();
            if range.is_empty() || range.len()>1024*1024 { return None }
            let selected=state.selected_text();
            Some((selected.to_string(),SelectionSource::$variant {input:input.downgrade(),range}))
        }};
    }
    // All plain text fields participate. A secure or one-time-code field
    // must suppress a reply's older selection rather than exposing it.
    if let Some(input)=window.focused_input(cx) {
        return match input {
            AnyInputState::Input(input)=>selection!(input,TextField),
            AnyInputState::Textarea(input)=>selection!(input,TextBox),
            AnyInputState::Editor(input)=>selection!(input,Editor),
            AnyInputState::Otp(_)=>None,
        }
    }
    // A just-focused composer can precede the registry's next paint.
    if let Some(input)=focused_text_box(window,cx) { return selection!(input,TextBox) }
    let words=gpui_kit::base::TextSelection::selected_text(window,cx);
    (!words.is_empty() && words.len()<=1024*1024).then(||(words,SelectionSource::Reply))
}

/// A message's context menu must never use words selected in a different
/// input. The app menu can use front_selection; a reply menu is reply-only.
pub(crate) fn reply_menu_selection(window: &mut Window, cx: &mut App) -> Option<String> {
    match front_selection(window, cx) {
        Some((words, SelectionSource::Reply)) => Some(words),
        _ => None,
    }
}

/// Put `with` in place of the selection `source` names, only if it's still
/// that same text box, with that same range selected, holding `expected`:
/// a Service's answer never lands somewhere the keyboard moved to since.
/// It's one edit Undo takes back. `false` when nothing was changed.
pub(crate) fn replace_if_unchanged(source: &SelectionSource, expected: &str, with: &str, window: &mut Window, cx: &mut App) -> bool {
    if expected.contains('\u{fffc}') { return false }
    macro_rules! replace {
        ($input:expr,$range:expr) => {{
            let Some(input)=$input.upgrade() else{return false};
            input.update(cx,|state,cx|{
                if state.has_composition() || !state.is_single_selection() || state.is_sensitive() || !state.presentation().is_editable() || state.selected_range()!=*$range || state.selected_text().to_string()!=expected {return false}
                state.replace(with.to_owned(),window,cx);
                true
            })
        }};
    }
    match source {
        SelectionSource::TextBox {input,range}=>replace!(input,range),
        SelectionSource::TextField {input,range}=>replace!(input,range),
        SelectionSource::Editor {input,range}=>replace!(input,range),
        SelectionSource::Reply=>false,
    }
}

#[cfg(all(test, feature = "automation"))]
mod selection_tests {
    use super::{front_selection, replace_if_unchanged, SelectionSource};
    use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState, Editor, EditorState, Undo};
    use gpui_kit::prelude::*;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{div, Entity, EntityInputHandler as _, Focusable as _, TestAppContext, Window, WindowOptions};

    struct Fields { input: Entity<InputState>, textarea: Entity<TextareaState>, editor: Entity<EditorState> }
    impl Render for Fields {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().flex_col().child(Input::new(&self.input)).child(Textarea::new(&self.textarea)).child(Editor::new(&self.editor))
        }
    }
    fn fields(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<Fields>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| gpui_kit::open_window(WindowOptions::default(),cx,|window,cx|cx.new(|cx|Fields {
            input:cx.new(|cx|InputState::new(window,cx)),
            textarea:cx.new(|cx|TextareaState::new(window,cx)),
            editor:cx.new(|cx|EditorState::new(window,cx)),
        })).unwrap())
    }

    #[gpui_kit::test]
    fn every_plain_text_field_is_a_selection_source(cx: &mut TestAppContext) {
        let (handle,fields)=fields(cx);
        cx.update_window(handle,|_,window,cx|{
            let (input,textarea,editor)={let f=fields.read(cx);(f.input.clone(),f.textarea.clone(),f.editor.clone())};
            macro_rules! check {
                ($input:expr,$kind:ident)=>{{
                    let input=$input;
                    input.update(cx,|state,cx|{state.set_value("café 🐾",window,cx);state.set_selected_range(0..5,cx);state.focus(window,cx);
                        crate::web_panels::keyboard_to_app(window);
                    });
                    window.render_frame(cx);
                    let (words,source)=front_selection(window,cx).expect("plain selection");
                    assert_eq!(words,"café");assert!(matches!(source,SelectionSource::$kind{..}));
                    assert!(replace_if_unchanged(&source,&words,"coffee",window,cx));
                    assert_eq!(&*input.read(cx).value(),"coffee 🐾");
                    input.read(cx).focus_handle(cx).dispatch_action(&Undo,window,cx);
                    window.render_frame(cx);
                    assert_eq!(&*input.read(cx).value(),"café 🐾");
                }};
            }
            check!(input,TextField);check!(textarea,TextBox);check!(editor,Editor);
        }).unwrap();
    }

    #[gpui_kit::test]
    fn service_returns_refuse_changed_range_text_readonly_and_expired_sources(cx: &mut TestAppContext) {
        let (handle,fields)=fields(cx);
        cx.update_window(handle,|_,window,cx|{
            let input=fields.read(cx).input.clone();
            input.update(cx,|s,cx|{s.set_value("first second",window,cx);s.set_selected_range(0..5,cx);});
            let source=SelectionSource::TextField{input:input.downgrade(),range:0..5};
            input.update(cx,|s,cx|s.set_selected_range(6..12,cx));
            assert!(!replace_if_unchanged(&source,"first","wrong",window,cx));
            input.update(cx,|s,cx|{s.set_value("other second",window,cx);s.set_selected_range(0..5,cx);});
            assert!(!replace_if_unchanged(&source,"first","wrong",window,cx));
            input.update(cx,|s,cx|s.set_readonly(true,cx));
            assert!(!replace_if_unchanged(&source,"other","wrong",window,cx));
            assert!(!replace_if_unchanged(&SelectionSource::Reply,"other","wrong",window,cx));
            let temporary=cx.new(|cx|InputState::new(window,cx));
            let expired=SelectionSource::TextField{input:temporary.downgrade(),range:0..1};
            drop(temporary);
            assert!(!replace_if_unchanged(&expired,"a","wrong",window,cx));
        }).unwrap();
    }

    #[gpui_kit::test]
    fn revealed_secret_fields_do_not_become_text_service_sources(cx: &mut TestAppContext) {
        let (handle,fields)=fields(cx);
        cx.update_window(handle,|_,window,cx|{
            let input=fields.read(cx).input.clone();
            input.update(cx,|s,cx|{s.set_value("secret",window,cx);s.set_selected_range(0..6,cx);s.focus(window,cx);
                crate::web_panels::keyboard_to_app(window);
            });
            window.render_frame(cx);
            input.update(cx,|s,_|s.set_accessibility(gpui_kit::base::input::InputAccessibility::new(None).with_value_exposed(false)));
            assert!(front_selection(window,cx).is_none());
            assert!(!replace_if_unchanged(&SelectionSource::TextField{input:input.downgrade(),range:0..6},"secret","wrong",window,cx));
        }).unwrap();
    }
    #[gpui_kit::test]
    fn a_service_never_replaces_an_extra_caret_or_picture_selection(cx: &mut TestAppContext) {
        let (handle,fields)=fields(cx);
        cx.update_window(handle,|_,window,cx|{
            let input=fields.read(cx).textarea.clone();
            input.update(cx,|s,cx|{
                s.set_value("first\n\u{fffc}\nsecond",window,cx);
                s.set_selected_range(0..5,cx); // the active word
                s.add_selection(6..9,cx); // a second selection on the picture
            });
            let source=SelectionSource::TextBox{input:input.downgrade(),range:0..5};
            assert!(!replace_if_unchanged(&source,"first","rewritten",window,cx));
            assert_eq!(&*input.read(cx).value(),"first\n\u{fffc}\nsecond");
            assert!(!source.is_editable(cx),"one returned string cannot target several selections");
        }).unwrap();
    }

    #[gpui_kit::test]
    fn a_reply_menu_never_takes_a_focused_fields_selection(cx: &mut TestAppContext) {
        let (handle,fields)=fields(cx);
        cx.update_window(handle,|_,window,cx|{
            let input=fields.read(cx).input.clone();
            input.update(cx,|s,cx|{s.set_value("private draft",window,cx);s.set_selected_range(0..13,cx);s.focus(window,cx);
                crate::web_panels::keyboard_to_app(window);
            });
            window.render_frame(cx);
            assert_eq!(front_selection(window,cx).unwrap().0,"private draft");
            assert!(super::reply_menu_selection(window,cx).is_none(),"a message's menu must not search, share or copy a field");
        }).unwrap();
    }

    #[gpui_kit::test]
    fn a_service_never_replaces_a_larger_ime_preedit(cx: &mut TestAppContext) {
        let (handle,fields)=fields(cx);
        cx.update_window(handle,|_,window,cx|{
            let (input,textarea,editor)={let f=fields.read(cx);(f.input.clone(),f.textarea.clone(),f.editor.clone())};
            macro_rules! check { ($input:expr,$kind:ident)=>{{
                let input=$input;
                input.update(cx,|s,cx|{
                    s.set_value("",window,cx);
                    s.replace_and_mark_text_in_range(None,"café 🐾",Some(0..4),window,cx);
                });
                let source=SelectionSource::$kind{input:input.downgrade(),range:0..5};
                assert_eq!(input.read(cx).selected_text().to_string(),"café");
                assert!(!replace_if_unchanged(&source,"café","coffee",window,cx));
                assert_eq!(&*input.read(cx).value(),"café 🐾");
                assert!(!source.is_editable(cx));
                // Once the IME commits, the same selection can be replaced.
                input.update(cx,|s,cx|s.unmark_text(window,cx));
                assert!(replace_if_unchanged(&source,"café","coffee",window,cx));
                assert_eq!(&*input.read(cx).value(),"coffee 🐾");
            }}; }
            check!(input,TextField);check!(textarea,TextBox);check!(editor,Editor);
        }).unwrap();
    }

}
