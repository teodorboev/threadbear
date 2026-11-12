// Settings › Keyboard's rules, a port of AppShortcuts.swift's ShortcutRecorder
// and the parts of Shortcuts.swift that edit keys (ShortcutGroup, `take`,
// `reset`, `takenByTyping`) and of KeyboardSettings.swift that say things
// (the key caps, the question under a row, the search). Click a command's key,
// press the new one, and it's changed; a key something else has is asked
// about rather than taken. Everything changed goes into shortcuts.json, which
// the Swift app reads too (shortcuts.rs).
use crate::shortcuts::{ShortcutBindings, ShortcutCommand, ShortcutKey};

/// The headings on the page, named for the Swift app's menus (`ShortcutGroup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Threads,
    Go,
    View,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Threads, Group::Go, Group::View];

    pub fn title(self) -> &'static str {
        match self {
            Group::Threads => "Threads",
            Group::Go => "Go",
            Group::View => "View",
        }
    }

    /// Its commands, in the order the Swift list has them.
    pub fn commands(self) -> Vec<ShortcutCommand> {
        ShortcutCommand::ALL.into_iter().filter(|c| group(*c) == self).collect()
    }

    /// What it is, behind its ⓘ.
    pub fn about(self) -> String {
        format!("The keys for what's in the {} menu. Click one, then press the new keys. Esc cancels, and Delete removes it.", self.title())
    }
}

/// What to ask your agent, the same for every group.
pub const ASK: &str = "change them for you, or tell you what's free. They're in shortcuts.json in Threadbear's data folder, which holds only what you've changed.";
/// Under a command's name while its key is being pressed.
pub const HINT: &str = "Esc cancels, and Delete removes it";
/// The line by Reset All.
pub const FOOTER: &str = "Saved in a file your agents can read and change too.";
pub const RESET_ALL_TIP: &str = "Every key back to how Threadbear comes";
pub const SEARCH_PROMPT: &str = "Search shortcuts";

/// Which heading a command sits under (`ShortcutCommand.group`).
pub fn group(command: ShortcutCommand) -> Group {
    use ShortcutCommand::*;
    match command {
        NewThread | Archive | Hibernate | Stop | CopyTranscript => Group::Threads,
        Back | Forward | NextInboxThread | ShowInbox | ShowScheduled | ShowProjects | ShowLibrarian | ShowHibernating | ShowArchived | Settings => Group::Go,
        ZoomIn | ZoomOut | ActualSize | ToggleThreads | TogglePanels | ToggleTerminal | ResetPanelLayout | CollapseThreads => Group::View,
    }
}

// ---------------------------------------------------------------- the keys

/// Gives `key` to `command`, leaving whatever had it with none, so two
/// commands are never on one key (`ShortcutBindings.take`).
pub fn take(bindings: &mut ShortcutBindings, key: &ShortcutKey, command: ShortcutCommand) {
    while let Some(other) = bindings.command_using(key, Some(command)) {
        bindings.set(other, None);
    }
    bindings.set(command, Some(key.clone()));
}

/// Back to its default, taking that key back if something else has it.
pub fn reset(bindings: &mut ShortcutBindings, command: ShortcutCommand) {
    match command.default_key() {
        Some(key) => take(bindings, &key, command),
        None => bindings.set(command, None),
    }
}

/// Every key back to how Threadbear comes. Lines for commands this app
/// doesn't have stay in the file, as the Swift app's `resetAll` leaves them.
pub fn reset_all(bindings: &mut ShortcutBindings) {
    for command in ShortcutCommand::ALL {
        bindings.set(command, command.default_key());
    }
}

pub fn any_changed(bindings: &ShortcutBindings) -> bool {
    ShortcutCommand::ALL.iter().any(|c| bindings.is_changed(*c))
}

/// Another command on this one's key, which only comes from editing the file
/// by hand.
pub fn twin(bindings: &ShortcutBindings, command: ShortcutCommand) -> Option<ShortcutCommand> {
    bindings.key(command).and_then(|key| bindings.command_using(&key, Some(command)))
}

/// What a key already does in every text box, which a shortcut would take over
/// (`takenByTyping`): menu keys go first.
pub fn taken_by_typing(key: &ShortcutKey) -> Option<&'static str> {
    if !key.command || key.control || key.option {
        return None;
    }
    match key.key.as_str() {
        "left" | "right" | "up" | "down" => Some(if key.shift { "Selecting Text" } else { "Moving the Cursor" }),
        "delete" if !key.shift => Some("Deleting to the Start of the Line"),
        "return" if !key.shift => Some("Save"),
        _ => None,
    }
}

/// What has a key outside the list, which can't change (`takenOutsideTheList`):
/// typing first, then the app's own fixed keys. The Swift app reads these from
/// its menu bar; this app's are its standard keys (shell.rs, `standard_keys`)
/// and the editing keys every text box takes, which a shortcut here would win
/// over, as its keys have no context.
pub fn taken_outside(key: &ShortcutKey, macos: bool) -> Option<&'static str> {
    if let Some(typing) = taken_by_typing(key) {
        return Some(typing);
    }
    const EVERYWHERE: [(&str, &str); 7] = [("cmd+q", "Quit Threadbear"), ("cmd+z", "Undo"), ("cmd+shift+z", "Redo"), ("cmd+x", "Cut"), ("cmd+c", "Copy"), ("cmd+v", "Paste"), ("cmd+a", "Select All")];
    const MAC: [(&str, &str); 3] = [("cmd+h", "Hide Threadbear"), ("cmd+opt+h", "Hide Others"), ("cmd+m", "Minimize")];
    let name = key.string();
    EVERYWHERE.iter().chain(if macos { MAC.iter() } else { [].iter() }).find(|(k, _)| *k == name).map(|(_, what)| *what)
}

/// A key press as GPUI names it, as shortcuts.json writes it
/// (`ShortcutKey(keyCode:characters:)`). GPUI gives a shifted symbol as the
/// symbol itself (⇧⌘[ comes as "cmd-{"), where the file keeps the key and
/// Shift ("cmd+shift+["), so a US keyboard's shifted symbols go back to their
/// keys. Elsewhere ⌘'s place is Control's, as `chord` has it. None for a key
/// with nothing to call it (a modifier on its own).
pub fn key_from_press(key: &str, platform: bool, alt: bool, control: bool, shift: bool, macos: bool) -> Option<ShortcutKey> {
    let (command, control) = if macos { (platform, control) } else { (control || platform, false) };
    let named = match key {
        "enter" => Some("return"),
        "backspace" => Some("delete"),
        "delete" => Some("forwarddelete"),
        "escape" | "space" | "tab" | "up" | "down" | "left" | "right" | "home" | "end" | "pageup" | "pagedown" => Some(key),
        _ => None,
    };
    let (name, shift) = if let Some(named) = named {
        (named.to_owned(), shift)
    } else if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        // F1 to F20, the Swift list's.
        if !(1..=20).contains(&n) {
            return None;
        }
        (key.to_owned(), shift)
    } else {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { return None };
        if c.is_whitespace() || c.is_control() {
            return None;
        }
        match unshifted(c) {
            Some(base) => (base.to_string(), true),
            None if c.is_uppercase() => (c.to_lowercase().to_string(), true),
            None => (c.to_string(), shift),
        }
    };
    Some(ShortcutKey { key: name, command, option: alt, control, shift })
}

/// The key a US keyboard types a shifted symbol on.
fn unshifted(c: char) -> Option<char> {
    const PAIRS: [(char, char); 21] = [
        ('!', '1'), ('@', '2'), ('#', '3'), ('$', '4'), ('%', '5'), ('^', '6'), ('&', '7'), ('*', '8'), ('(', '9'), (')', '0'),
        ('_', '-'), ('+', '='), ('{', '['), ('}', ']'), ('|', '\\'), (':', ';'), ('"', '\''), ('<', ','), ('>', '.'), ('?', '/'), ('~', '`'),
    ];
    PAIRS.iter().find(|(shifted, _)| *shifted == c).map(|(_, base)| *base)
}

// ---------------------------------------------------------------- listening

/// What came of the last key pressed while listening (`ShortcutRecorder.Outcome`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Outcome {
    /// Nothing pressed yet.
    #[default]
    Waiting,
    /// A key that would type instead of being a shortcut.
    TypesInstead,
    /// Another command has it, which it can give up if asked.
    TakenByCommand(ShortcutKey, ShortcutCommand),
    /// Something outside the list has it, which can't change.
    TakenOutside(ShortcutKey, &'static str),
}

/// Listens for a command's new key (`ShortcutRecorder`). After a key that
/// can't be used it keeps listening, so another press is another go. Each
/// change it makes comes back as the new bindings, for the caller to save.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorder {
    pub command: Option<ShortcutCommand>,
    pub outcome: Outcome,
}

impl Recorder {
    pub fn start(&mut self, command: ShortcutCommand) {
        self.command = Some(command);
        self.outcome = Outcome::Waiting;
    }

    pub fn cancel(&mut self) {
        self.command = None;
        self.outcome = Outcome::Waiting;
    }

    pub fn listening(&self, command: ShortcutCommand) -> bool {
        self.command == Some(command)
    }

    /// Listening with no question on screen: a click elsewhere means never
    /// mind (with a question up, its buttons need that click).
    pub fn waiting(&self) -> bool {
        self.command.is_some() && self.outcome == Outcome::Waiting
    }

    /// A key while listening (`press`). Its own key again is no change.
    pub fn press(&mut self, key: ShortcutKey, bindings: &ShortcutBindings, macos: bool) -> Option<ShortcutBindings> {
        let command = self.command?;
        if bindings.key(command).as_ref() == Some(&key) {
            self.cancel();
            None
        } else if key.types_instead() {
            self.outcome = Outcome::TypesInstead;
            None
        } else if let Some(other) = bindings.command_using(&key, Some(command)) {
            self.outcome = Outcome::TakenByCommand(key, other);
            None
        } else if let Some(outside) = taken_outside(&key, macos) {
            self.outcome = Outcome::TakenOutside(key, outside);
            None
        } else {
            let mut next = bindings.clone();
            next.set(command, Some(key));
            self.cancel();
            Some(next)
        }
    }

    /// Delete with nothing held: the command keeps no key.
    pub fn remove(&mut self, bindings: &ShortcutBindings) -> Option<ShortcutBindings> {
        let command = self.command?;
        let mut next = bindings.clone();
        next.set(command, None);
        self.cancel();
        Some(next)
    }

    /// Yes to giving the key to this command instead (`useTakenKey`).
    pub fn use_taken_key(&mut self, bindings: &ShortcutBindings) -> Option<ShortcutBindings> {
        let (Some(command), Outcome::TakenByCommand(key, _)) = (self.command, self.outcome.clone()) else { return None };
        let mut next = bindings.clone();
        take(&mut next, &key, command);
        self.cancel();
        Some(next)
    }

    /// A key press as GPUI gives it while listening (`swallows`): Esc with
    /// nothing held stops, Delete with nothing held removes the key, anything
    /// else is a go at a new one. None when the bindings stay as they are.
    pub fn key_down(&mut self, key: &str, platform: bool, alt: bool, control: bool, shift: bool, bindings: &ShortcutBindings, macos: bool) -> Option<ShortcutBindings> {
        self.command?;
        if !platform && !alt && !control && !shift {
            match key {
                "escape" => {
                    self.cancel();
                    return None;
                }
                "backspace" | "delete" => return self.remove(bindings),
                _ => {}
            }
        }
        let pressed = key_from_press(key, platform, alt, control, shift, macos)?;
        self.press(pressed, bindings, macos)
    }
}

/// The line under a row when the key pressed can't simply be used, and
/// whether it offers "Use It for This Instead" (`Question`).
#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub text: String,
    pub can_take: bool,
}

pub fn question(outcome: &Outcome) -> Option<Question> {
    match outcome {
        Outcome::Waiting => None,
        Outcome::TypesInstead => Some(Question { text: "Include ⌘ or ⌃, otherwise that key would type instead.".to_owned(), can_take: false }),
        Outcome::TakenByCommand(key, other) => Some(Question { text: format!("{} is already {}.", key.display(), other.title()), can_take: true }),
        Outcome::TakenOutside(key, what) => Some(Question { text: format!("{} is taken by {what}, which can't be changed.", key.display()), can_take: false }),
    }
}

// ---------------------------------------------------------------- what a row shows

/// A command's key cap (`KeyCap.Look`).
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    Key(String),
    None,
    Listening,
    Taken(String),
}

impl Look {
    pub fn title(&self) -> String {
        match self {
            Look::Key(text) | Look::Taken(text) => text.clone(),
            Look::None => "Add".to_owned(),
            Look::Listening => "Press keys…".to_owned(),
        }
    }
}

/// How a command's cap looks: listening, the key pressed that's taken, its
/// key (orange when another command has it too), or Add.
pub fn look(recorder: &Recorder, command: ShortcutCommand, bindings: &ShortcutBindings) -> Look {
    if recorder.listening(command) {
        return match &recorder.outcome {
            Outcome::Waiting | Outcome::TypesInstead => Look::Listening,
            Outcome::TakenByCommand(pressed, _) | Outcome::TakenOutside(pressed, _) => Look::Taken(pressed.display()),
        };
    }
    match bindings.key(command) {
        None => Look::None,
        Some(key) if twin(bindings, command).is_some() => Look::Taken(key.display()),
        Some(key) => Look::Key(key.display()),
    }
}

/// The cap's tooltip.
pub fn cap_tip(command: ShortcutCommand, bindings: &ShortcutBindings) -> String {
    match (bindings.key(command), twin(bindings, command)) {
        (Some(key), Some(other)) => format!("{} is also {}", key.display(), other.title()),
        _ => "Click, then press the new keys".to_owned(),
    }
}

/// The cap's name for VoiceOver and the specs: "Archive, ⌘E".
pub fn cap_label(command: ShortcutCommand, bindings: &ShortcutBindings) -> String {
    format!("{}, {}", command.title(), bindings.key(command).map_or_else(|| "no key".to_owned(), |k| k.display()))
}

/// The row's Reset's tooltip.
pub fn reset_tip(command: ShortcutCommand) -> String {
    command.default_key().map_or_else(|| "Back to no key".to_owned(), |k| format!("Back to {}", k.display()))
}

/// The search: a command shows if its name, its heading or its key has the
/// words in it, whatever their case.
pub fn matches(command: ShortcutCommand, search: &str, bindings: &ShortcutBindings) -> bool {
    let words = search.trim().to_lowercase();
    if words.is_empty() {
        return true;
    }
    command.title().to_lowercase().contains(&words) || group(command).title().to_lowercase().contains(&words) || bindings.key(command).is_some_and(|k| k.display().to_lowercase().contains(&words))
}

/// A command in the dump: "Archive: ⌘K (changed)", "Hibernate: none".
pub fn line(command: ShortcutCommand, bindings: &ShortcutBindings) -> String {
    let key = bindings.key(command).map_or_else(|| "none".to_owned(), |k| k.display());
    let mut out = format!("{}: {key}", command.title());
    if bindings.is_changed(command) {
        out.push_str(" (changed)");
    }
    if let Some(other) = twin(bindings, command) {
        out.push_str(&format!(" (also {})", other.title()));
    }
    out
}

/// What's wrong with shortcuts.json, for under the list (`AppShortcuts.problem`).
pub fn file_problem(text: Option<&str>) -> Option<String> {
    let text = text?;
    match ShortcutBindings::parse(text) {
        Err(_) => Some("shortcuts.json isn't valid JSON, so these are the last keys that worked.".to_owned()),
        Ok(read) if !read.problems.is_empty() => Some(format!("shortcuts.json has lines that couldn't be used: {}", read.problems.join(", "))),
        Ok(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ShortcutCommand::*;

    fn key(text: &str) -> ShortcutKey {
        ShortcutKey::parse(text).unwrap()
    }

    #[test]
    fn the_groups_are_the_swift_pages() {
        let titles = |g: Group| g.commands().into_iter().map(ShortcutCommand::title).collect::<Vec<_>>();
        assert_eq!(titles(Group::Threads), ["New Thread", "Archive", "Hibernate", "Stop", "Copy Transcript as Markdown"]);
        assert_eq!(titles(Group::Go)[..3], ["Back", "Forward", "Next Inbox Thread"]);
        assert_eq!(titles(Group::Go).last(), Some(&"Settings…"));
        assert_eq!(titles(Group::View).first(), Some(&"Zoom In"));
        assert_eq!(titles(Group::View).last(), Some(&"Collapse Threads"));
        let all: usize = Group::ALL.iter().map(|g| g.commands().len()).sum();
        assert_eq!(all, ShortcutCommand::ALL.len(), "every command is under a heading");
        assert!(Group::View.about().starts_with("The keys for what's in the View menu."));
    }

    #[test]
    fn a_free_key_is_taken_at_once() {
        let mut r = Recorder::default();
        let b = ShortcutBindings::default();
        r.start(Archive);
        assert!(r.waiting());
        let next = r.press(key("cmd+k"), &b, true).unwrap();
        assert_eq!(next.key(Archive), Some(key("cmd+k")));
        assert_eq!(r, Recorder::default(), "and it stops listening");
        assert!(any_changed(&next));
        assert_eq!(line(Archive, &next), "Archive: ⌘K (changed)");
        assert_eq!(line(Hibernate, &next), "Hibernate: none");
    }

    #[test]
    fn its_own_key_again_changes_nothing() {
        let mut r = Recorder::default();
        r.start(Archive);
        assert_eq!(r.press(key("cmd+e"), &ShortcutBindings::default(), true), None);
        assert_eq!(r.command, None);
    }

    #[test]
    fn a_key_that_types_is_asked_about_and_it_keeps_listening() {
        let mut r = Recorder::default();
        let b = ShortcutBindings::default();
        r.start(Archive);
        let typed = key_from_press("k", false, true, false, false, true).unwrap();
        assert_eq!(r.press(typed, &b, true), None);
        assert_eq!(r.outcome, Outcome::TypesInstead);
        assert_eq!(question(&r.outcome).unwrap().text, "Include ⌘ or ⌃, otherwise that key would type instead.");
        assert_eq!(look(&r, Archive, &b), Look::Listening);
        assert!(!r.waiting(), "a click elsewhere doesn't cancel with a question up");
        // Another go.
        assert!(r.press(key("cmd+k"), &b, true).is_some());
    }

    #[test]
    fn another_commands_key_is_asked_about_and_can_be_taken() {
        let mut r = Recorder::default();
        let b = ShortcutBindings::default();
        r.start(Archive);
        assert_eq!(r.press(key("cmd+n"), &b, true), None);
        assert_eq!(r.outcome, Outcome::TakenByCommand(key("cmd+n"), NewThread));
        let q = question(&r.outcome).unwrap();
        assert_eq!((q.text.as_str(), q.can_take), ("⌘N is already New Thread.", true));
        assert_eq!(look(&r, Archive, &b), Look::Taken("⌘N".into()));
        let next = r.use_taken_key(&b).unwrap();
        assert_eq!(next.key(Archive), Some(key("cmd+n")));
        assert_eq!(next.key(NewThread), None, "the other one gives it up");
        assert_eq!(line(NewThread, &next), "New Thread: none (changed)");
    }

    #[test]
    fn a_key_outside_the_list_cant_be_had() {
        let mut r = Recorder::default();
        let b = ShortcutBindings::default();
        r.start(Archive);
        r.press(key("cmd+c"), &b, true);
        let q = question(&r.outcome).unwrap();
        assert_eq!((q.text.as_str(), q.can_take), ("⌘C is taken by Copy, which can't be changed.", false));
        assert_eq!(r.use_taken_key(&b), None);
        r.press(key("cmd+left"), &b, true);
        assert_eq!(question(&r.outcome).unwrap().text, "⌘← is taken by Moving the Cursor, which can't be changed.");
        r.press(key("cmd+shift+down"), &b, true);
        assert_eq!(question(&r.outcome).unwrap().text, "⇧⌘↓ is taken by Selecting Text, which can't be changed.");
        r.press(key("cmd+m"), &b, true);
        assert_eq!(r.outcome, Outcome::TakenOutside(key("cmd+m"), "Minimize"));
        assert_eq!(taken_outside(&key("cmd+m"), false), None, "Minimize's key is the Mac's own");
        assert_eq!(taken_outside(&key("ctrl+cmd+left"), true), None, "typing only takes ⌘ alone");
    }

    #[test]
    fn esc_stops_and_delete_takes_the_key_away() {
        let mut r = Recorder::default();
        let b = ShortcutBindings::default();
        r.start(Stop);
        assert_eq!(r.key_down("escape", false, false, false, false, &b, true), None);
        assert_eq!(r.command, None);
        r.start(Stop);
        let next = r.key_down("backspace", false, false, false, false, &b, true).unwrap();
        assert_eq!(next.key(Stop), None);
        assert_eq!(r.command, None);
        // Held with something, they're keys like any other.
        r.start(Stop);
        assert_eq!(r.key_down("escape", false, false, false, true, &b, true), None);
        assert_eq!(r.outcome, Outcome::TypesInstead);
        let next = r.key_down("backspace", true, true, false, false, &b, true).unwrap();
        assert_eq!(next.key(Stop), Some(key("cmd+opt+delete")));
        // A modifier on its own is nothing.
        r.start(Stop);
        assert_eq!(r.key_down("shift", false, false, false, false, &b, true), None);
        assert!(r.waiting());
    }

    #[test]
    fn a_press_reads_as_the_file_writes_it() {
        assert_eq!(key_from_press("k", true, false, false, false, true), Some(key("cmd+k")));
        assert_eq!(key_from_press("{", true, false, false, false, true), Some(key("cmd+shift+[")), "GPUI gives ⇧⌘[ as cmd-{{");
        assert_eq!(key_from_press("c", true, false, false, true, true), Some(key("cmd+shift+c")));
        assert_eq!(key_from_press("down", true, true, false, false, true), Some(key("cmd+opt+down")));
        assert_eq!(key_from_press("enter", true, false, false, false, true), Some(key("cmd+return")));
        assert_eq!(key_from_press("backspace", true, false, false, false, true), Some(key("cmd+delete")));
        assert_eq!(key_from_press("delete", true, false, false, false, true), Some(key("cmd+forwarddelete")));
        assert_eq!(key_from_press("f5", false, false, false, false, true), Some(key("f5")));
        assert_eq!(key_from_press("f21", false, false, false, false, true), None);
        assert_eq!(key_from_press("s", false, false, true, false, true), Some(key("ctrl+s")));
        assert_eq!(key_from_press("k", false, false, true, false, false), Some(key("cmd+k")), "Control is ⌘'s place off the Mac");
        assert_eq!(key_from_press("platform", false, false, false, false, true), None);
        assert_eq!(key_from_press("=", true, false, false, false, true).map(|k| k.display()), Some("⌘=".to_owned()));
    }

    #[test]
    fn reset_takes_the_default_back() {
        let mut b = ShortcutBindings::default();
        b.set(NewThread, Some(key("cmd+e")));
        b.set(Archive, None);
        assert!(b.is_changed(Archive));
        reset(&mut b, Archive);
        assert_eq!(b.key(Archive), Some(key("cmd+e")));
        assert!(!b.is_changed(Archive));
        assert_eq!(b.key(NewThread), None, "New Thread had ⌘E, so it gives it back");
        reset(&mut b, Hibernate);
        assert_eq!(b.key(Hibernate), None);
        reset_all(&mut b);
        assert!(!any_changed(&b));
        assert_eq!(b.json(), None, "the file goes");
        // Lines for commands this app doesn't have stay (the Swift app's
        // Quit, which here is the standard key).
        let mut kept = ShortcutBindings::parse(r#"{"quit": "cmd+k", "archive": "cmd+j"}"#).unwrap();
        reset_all(&mut kept);
        assert!(kept.json().unwrap().contains("quit"));
        assert!(!kept.json().unwrap().contains("archive"));
    }

    #[test]
    fn two_commands_on_one_key_show_it() {
        let b = ShortcutBindings::parse(r#"{"archive": "cmd+n"}"#).unwrap();
        assert_eq!(twin(&b, Archive), Some(NewThread));
        assert_eq!(look(&Recorder::default(), Archive, &b), Look::Taken("⌘N".into()));
        assert_eq!(cap_tip(Archive, &b), "⌘N is also New Thread");
        assert_eq!(cap_tip(Stop, &b), "Click, then press the new keys");
        assert_eq!(line(NewThread, &b), "New Thread: ⌘N (also Archive)");
        assert_eq!(look(&Recorder::default(), Hibernate, &b), Look::None);
        assert_eq!(Look::None.title(), "Add");
        assert_eq!(cap_label(Hibernate, &b), "Hibernate, no key");
        assert_eq!(cap_label(Stop, &b), "Stop, ⌘.");
        assert_eq!(reset_tip(Archive), "Back to ⌘E");
        assert_eq!(reset_tip(Hibernate), "Back to no key");
    }

    #[test]
    fn the_search_looks_at_names_headings_and_keys() {
        let b = ShortcutBindings::default();
        assert!(matches(Archive, "  ", &b));
        assert!(matches(Archive, "arch", &b));
        assert!(matches(ZoomIn, "view", &b), "its heading");
        assert!(matches(Archive, "⌘e", &b), "its key, whatever the case");
        assert!(!matches(Archive, "zoom", &b));
    }

    #[test]
    fn a_bad_file_says_so() {
        assert_eq!(file_problem(None), None);
        assert_eq!(file_problem(Some("{}")), None);
        assert_eq!(file_problem(Some("{ nope")).as_deref(), Some("shortcuts.json isn't valid JSON, so these are the last keys that worked."));
        assert_eq!(file_problem(Some(r#"{"archive": "banana"}"#)).as_deref(), Some("shortcuts.json has lines that couldn't be used: archive: banana"));
    }
}
