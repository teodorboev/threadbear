// The Terminal panel's rules with no window in them (TerminalPanel.swift's
// Terminals, TerminalTarget and TerminalSession, ThreadbearCore's ShellBlock
// and TerminalPut): which shell a thread gets and in which folder, the shell's
// own command line, what each key sends to it, and how a command from a reply
// is put at the prompt without ever being run. The GPUI app's terminal.rs and
// terminal_element.rs draw it and hold the shells.
use crate::inbox::ThreadSummary;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------- which shell

/// Which shell: the one shared home-folder shell, or a thread's own. Two kinds
/// rather than one string, so no thread id (another runner's included) can
/// ever name the home shell (Daybreak, 10 Nov).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TerminalKey {
    Home,
    Thread(String),
}

impl std::fmt::Display for TerminalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminalKey::Home => write!(f, "home"),
            TerminalKey::Thread(id) => write!(f, "thread:{id}"),
        }
    }
}

/// Where a terminal runs: its key (the thread, or home) and folder, and what
/// the panel says over it when its folder isn't the thread's.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub key: TerminalKey,
    pub cwd: String,
    pub note: Option<String>,
}

/// `TerminalTarget.for`: a thread's own shell in its folder (its worktree when
/// it has one), or the home shell, with a line saying why, for a thread whose
/// folder isn't here: another machine's, an archived one's (its worktree may
/// be another thread's by now) or one whose folder has gone. With no thread
/// open it's the home shell too. `runner` names another machine's thread's
/// runner, when that's known.
pub fn target_for(thread: Option<&ThreadSummary>, home: &str, runner: Option<&str>, is_dir: impl Fn(&str) -> bool) -> Target {
    let home_shell = |note: Option<String>| Target { key: TerminalKey::Home, cwd: home.to_owned(), note };
    let Some(thread) = thread else { return home_shell(None) };
    if thread.is_archived() {
        return home_shell(Some("This thread is archived, so this terminal is in your home folder.".to_owned()));
    }
    if thread.is_remote() {
        let place = runner.map_or_else(|| "on another machine".to_owned(), |name| format!("on {name}"));
        return home_shell(Some(format!("This thread runs {place}, so this terminal is on this Mac, in your home folder.")));
    }
    if thread.cwd.is_empty() || !is_dir(&thread.cwd) {
        return home_shell(Some("This thread's folder isn't on this Mac any more, so this terminal is in your home folder.".to_owned()));
    }
    Target { key: TerminalKey::Thread(thread.id.clone()), cwd: thread.cwd.clone(), note: None }
}

/// Which of `keys` are shells of this Mac's threads that `threads` says are
/// archived (`Terminals.archived`). Never the home shell. Only this Mac's own
/// threads have shells of their own, so another runner's index can't stop one
/// by calling a row of its own "home" and archiving it (Daybreak, 10 Nov).
pub fn archived<'a, T: std::borrow::Borrow<ThreadSummary>>(keys: impl IntoIterator<Item = &'a TerminalKey>, threads: &HashMap<String, T>) -> Vec<TerminalKey> {
    keys.into_iter()
        .filter(|key| match key {
            TerminalKey::Thread(id) => threads.get(id).map(crate::inbox::summary).is_some_and(|t| !t.is_remote() && t.is_archived()),
            TerminalKey::Home => false,
        })
        .cloned()
        .collect()
}

/// Which of `keys` are thread shells whose thread isn't in `keeping`: the
/// runner's full list came back (on connecting) without it, so it was deleted
/// while the app wasn't listening (`Terminals.gone`). The home shell stays.
pub fn gone<'a>(keys: impl IntoIterator<Item = &'a TerminalKey>, keeping: &HashSet<&str>) -> Vec<TerminalKey> {
    keys.into_iter().filter(|key| matches!(key, TerminalKey::Thread(id) if !keeping.contains(id.as_str()))).cloned().collect()
}

/// Whether a shell block in `block_thread`'s transcript may put its command in
/// the Terminal (`canPutInTerminal(from:)`): only when that thread is the one
/// the main panel shows, so the Terminal panel is its shell, in front of Mike,
/// and its shell is in its own folder on this Mac. Asked of the thread the
/// block is in, never the selection alone: a block in another runner's thread
/// open in a panel of its own got a play icon from a local thread being
/// selected, then pasted into that thread's shell (Daybreak, 10 Nov).
pub fn can_put(block_thread: Option<&str>, selected: Option<&str>, target: &Target) -> bool {
    match (block_thread, selected) {
        (Some(block), Some(selected)) => block == selected && target.note.is_none() && target.key == TerminalKey::Thread(block.to_owned()),
        _ => false,
    }
}

// ---------------------------------------------------------------- the shell

/// The shell's command line and the variables it gets on top of the app's.
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The shell `program` ends up running (`env` execs it), which has to
    /// be what owns the terminal before a command is typed there.
    pub shell: String,
}

/// Variables the shell mustn't inherit. A thread id makes the runner read
/// anything run with it as that thread's own agent (runner/src/cli.ts), so
/// `tb` typed here would refuse to archive the thread with a message meant for
/// agents, and log what Mike does as the agent's doing; the app may have
/// inherited either name if it was started from inside a thread. The other
/// two are the terminal library's own, and would tell programs this is
/// Alacritty.
pub const UNSET: &[&str] = &["THREADBEAR_THREAD_ID", "THREADBARE_THREAD_ID", "THREADBEAR_THREAD_KEY", "THREADBARE_THREAD_KEY", "ALACRITTY_WINDOW_ID", "WINDOWID"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    OtherUnix,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::OtherUnix
        }
    }
}

/// `TerminalSession.start` and `environment`: your login shell (`$SHELL`, as
/// Terminal starts it), so it reads your profile and has the PATH Terminal
/// gives you, which an app's own doesn't. On a Unix the variables above go
/// through `env -u` on the way, as the library that starts it only adds them.
/// `lang` is the app's own LANG, which the shell gets a UTF-8 one in place of
/// when it has none.
pub fn launch(os: Os, shell: Option<&str>, lang: Option<&str>) -> Launch {
    let mut env = vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ("TERM_PROGRAM".to_owned(), "Threadbear".to_owned()),
    ];
    if lang.is_none_or(str::is_empty) {
        env.push(("LANG".to_owned(), "en_US.UTF-8".to_owned()));
    }
    if os == Os::Windows {
        return Launch { program: "powershell.exe".to_owned(), args: vec!["-NoLogo".to_owned()], env, shell: "powershell.exe".to_owned() };
    }
    let fallback = if os == Os::Mac { "/bin/zsh" } else { "/bin/sh" };
    let shell = shell.filter(|s| s.starts_with('/')).unwrap_or(fallback);
    let mut args: Vec<String> = UNSET.iter().flat_map(|name| ["-u".to_owned(), (*name).to_owned()]).collect();
    // `-l` is a login shell in zsh, bash, fish and the rest, as the "-zsh"
    // Terminal and the Swift app start is.
    args.extend([shell.to_owned(), "-l".to_owned()]);
    Launch { program: "/usr/bin/env".to_owned(), args, env, shell: shell.to_owned() }
}

// ---------------------------------------------------------------- keys

/// A key press as the window saw it: GPUI's name for the key ("enter",
/// "left", "a", "f5", "!") and what it would type with its modifiers ("A" for
/// shift-a, "å" for option-a).
#[derive(Clone, Copy, Debug, Default)]
pub struct Key<'a> {
    pub key: &'a str,
    pub typed: Option<&'a str>,
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

/// What the terminal itself is in, for the keys that change with it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modes {
    /// The cursor keys send `ESC O`, as vim and less ask for (DECCKM).
    pub app_cursor: bool,
    /// A full-screen program has the screen (its own, with no scrollback).
    pub alt_screen: bool,
    /// ⌥ is Meta: ⌥B sends ESC b, as the Swift app's SwiftTerm has it,
    /// rather than typing ∫.
    pub option_is_meta: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    /// These bytes, to the shell.
    Send(Vec<u8>),
    /// Page Up or Down with nothing held, at a prompt: the view goes back or
    /// on a page through what's scrolled off, as in Terminal.
    ScrollPage(i32),
    /// A Control key with no meaning to a terminal: nothing is sent, and it
    /// goes no further.
    Nothing,
    /// Not the terminal's to send: a plain character goes through the input
    /// method (so accents and other languages' input work), and a ⌘ key is the
    /// app's.
    Pass,
}

const ESC: u8 = 0x1b;

/// The bytes a terminal sends for `key`, the way xterm and Terminal do.
pub fn encode_key(key: &Key, modes: Modes) -> KeyAction {
    if key.command {
        return KeyAction::Pass;
    }
    let meta = key.alt && modes.option_is_meta;
    // xterm's modifier parameter: 1 plus shift 1, alt 2, control 4.
    let param = 1 + u8::from(key.shift) + 2 * u8::from(meta) + 4 * u8::from(key.control);
    let csi = |rest: &str| -> Vec<u8> { [&[ESC, b'['][..], rest.as_bytes()].concat() };
    let with_meta = |bytes: Vec<u8>| -> Vec<u8> {
        if meta {
            [vec![ESC], bytes].concat()
        } else {
            bytes
        }
    };
    // A cursor key: `ESC [ A`, `ESC O A` in application mode, `ESC [ 1 ; m A`
    // with modifiers.
    let cursor = |letter: char| -> Vec<u8> {
        if param == 1 {
            let lead = if modes.app_cursor { 'O' } else { '[' };
            format!("\x1b{lead}{letter}").into_bytes()
        } else {
            csi(&format!("1;{param}{letter}"))
        }
    };
    // A key with a number: `ESC [ 5 ~`, `ESC [ 5 ; m ~` with modifiers.
    let tilde = |n: u8| -> Vec<u8> {
        if param == 1 {
            csi(&format!("{n}~"))
        } else {
            csi(&format!("{n};{param}~"))
        }
    };
    let action = match key.key {
        "enter" => with_meta(b"\r".to_vec()),
        "tab" if key.shift => csi("Z"),
        "tab" => with_meta(b"\t".to_vec()),
        // Delete as Terminal sends it, and with ⌥ (ESC DEL) the word before.
        "backspace" => with_meta(vec![0x7f]),
        "escape" => with_meta(vec![ESC]),
        "space" if key.control => with_meta(vec![0]),
        "space" if meta => vec![ESC, b' '],
        "space" => return KeyAction::Pass,
        // ⌥← and ⌥→ go back and on a word, as Terminal's do (ESC b, ESC f).
        "left" if meta && !key.shift && !key.control => vec![ESC, b'b'],
        "right" if meta && !key.shift && !key.control => vec![ESC, b'f'],
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "pageup" | "pagedown" if param == 1 && !modes.app_cursor && !modes.alt_screen => {
            return KeyAction::ScrollPage(if key.key == "pageup" { -1 } else { 1 });
        }
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "f1" | "f2" | "f3" | "f4" => {
            let letter = match key.key {
                "f1" => 'P',
                "f2" => 'Q',
                "f3" => 'R',
                _ => 'S',
            };
            if param == 1 {
                format!("\x1bO{letter}").into_bytes()
            } else {
                csi(&format!("1;{param}{letter}"))
            }
        }
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        other => {
            let mut chars = other.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                // A key with a name this doesn't know (f13, a media key).
                return KeyAction::Nothing;
            };
            if key.control {
                return match control_code(c) {
                    Some(code) => KeyAction::Send(with_meta(vec![code])),
                    None => KeyAction::Nothing,
                };
            }
            if meta {
                // The key's own character, shifted, not what ⌥ makes of it.
                let c = if key.shift && c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c };
                let mut bytes = vec![ESC];
                bytes.extend(c.to_string().into_bytes());
                return KeyAction::Send(bytes);
            }
            return KeyAction::Pass;
        }
    };
    KeyAction::Send(action)
}

/// Control and a key, as the C0 code a terminal sends: ^A is 1, ^[ is ESC,
/// ^@ and ^Space are NUL, and the number row stands for the symbols on it as
/// xterm has them (^2 is NUL, ^6 is ^^, ^7 is ^_, ^8 is DEL).
fn control_code(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' => 0,
        '[' | '3' => ESC,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' | '~' => 0x1e,
        '_' | '-' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// A paste, as a terminal sends it: line breaks as Return, inside the brackets
/// when the program asked for them (bracketed paste), so a newline in it runs
/// nothing. Every control character but Tab and Return goes, brackets or not.
/// Without them each would be a key pressed (^C, ^D, ^U, an ESC that starts a
/// vim command), not text. Inside them an ESC could end them early (a pasted
/// `ESC [ 201 ~` lets what follows run by itself), and the rest never reach
/// the program as text anyway: the pty's own line discipline takes ^C, ^\, ^Z,
/// ^S and ^Q as signals before the shell sees the brackets, so a pasted ^\
/// quits bash's paste half way and the line break after it runs the command
/// (Daybreak, 11 Nov). C1 controls (U+0080 to U+009F, U+009B being CSI) go too.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    let inner: String = text.chars().filter(|c| !c.is_control() || matches!(c, '\t' | '\r')).collect();
    if !bracketed {
        return inner.into_bytes();
    }
    [b"\x1b[200~".as_slice(), inner.as_bytes(), b"\x1b[201~".as_slice()].concat()
}

/// Whether pasting `text` with no brackets would run something by itself:
/// each line break goes in as Return, so at a shell's prompt every line before
/// one runs as it arrives, with no chance to read it. That's macOS's own bash,
/// which never asks for brackets, and any program that hasn't (Codex's review,
/// 11 Nov: a shell block copied when the Terminal was busy, pasted there
/// later, ran its first lines). So the app asks first, as Windows Terminal
/// and iTerm do. A Tab is asked about too: it's the Tab key there, and zsh's
/// completion with bracketed paste off expands a `$(...)` before it, which
/// runs it (Codex's review of the paste fix, 11 Nov).
pub fn paste_runs(text: &str) -> bool {
    text.contains(['\n', '\r', '\t'])
}

/// Text that arrives all at once, from an input method or a text expander,
/// that's to be taken as a paste: more than one character with a control in
/// it. Line breaks on their own are Return, as an input method sends it.
pub fn is_bulk_text(text: &str) -> bool {
    text.chars().nth(1).is_some() && text.chars().any(char::is_control) && !text.chars().all(|c| c == '\r' || c == '\n')
}

/// What's asked before such a paste: its title and the line under it.
pub fn paste_question(text: &str) -> (String, &'static str) {
    let lines = text.lines().count().max(1);
    let title = if lines == 1 { "Paste this line?".to_owned() } else { format!("Paste {lines} lines?") };
    if !text.contains(['\n', '\r']) {
        return (title, "The program in this terminal will take each Tab in it as the Tab key, which a shell can use to complete or expand what's before it.");
    }
    (title, "The program in this terminal will take each line break as Return, so at a shell's prompt each line runs as soon as it's pasted.")
}

// ---------------------------------------------------------------- the mouse

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

/// A click, drag or wheel turn for a program that asked for the mouse (vim
/// with `mouse=a`, htop): xterm's SGR form (`ESC [ < b ; x ; y M`) when it
/// asked for that, else the old one, whose positions stop at 223. `column`
/// and `row` count from 0.
pub fn mouse_report(button: MouseButton, pressed: bool, motion: bool, column: usize, row: usize, shift: bool, alt: bool, control: bool, sgr: bool) -> Option<Vec<u8>> {
    let mut code: u32 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    code += 4 * u32::from(shift) + 8 * u32::from(alt) + 16 * u32::from(control) + 32 * u32::from(motion);
    if sgr {
        let end = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{};{}{end}", column + 1, row + 1).into_bytes());
    }
    // The old form says which button only on its press; a release is 3.
    if !pressed {
        code = 3 + 4 * u32::from(shift) + 8 * u32::from(alt) + 16 * u32::from(control);
    }
    let (x, y) = (column + 1 + 32, row + 1 + 32);
    if x > 255 || y > 255 {
        return None;
    }
    Some(vec![ESC, b'[', b'M', (32 + code) as u8, x as u8, y as u8])
}

// ---------------------------------------------------------------- shell blocks

/// A ```sh, ```bash, ```zsh or ```shell block in a reply: a command the agent
/// is handing over, often one only Mike can run (ShellBlock.swift). The play
/// icon on it types the command into the thread's Terminal and stops short of
/// Return, so a person always reads what's about to run.
///
/// What's in a reply can come from anything the agent read, so this only
/// takes text that means what it looks like. A control character could end a
/// bracketed paste early (ESC [ 201 ~) and let the next line run by itself,
/// and invisible or bidi characters could show a different command from the
/// one typed. A block holding any of those gets no play icon.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShellBlock {
    pub command: String,
}

/// Past this it's a script, not a command to hand someone.
pub const MAX_SOURCE: usize = 8_000;

impl ShellBlock {
    /// The command for a fence's info string (only its first word counts), or
    /// None for any other fence, or one that can't safely be typed.
    pub fn from(language: Option<&str>, code: &str) -> Option<ShellBlock> {
        let word = language?.split([' ', '\t', '{']).find(|w| !w.is_empty())?.to_lowercase();
        if !matches!(word.as_str(), "sh" | "bash" | "zsh" | "shell") {
            return None;
        }
        let mut source = code.replace("\r\n", "\n");
        while source.ends_with('\n') {
            source.pop();
        }
        if source.trim().is_empty() || source.chars().count() > MAX_SOURCE || !is_plain(&source) {
            return None;
        }
        Some(ShellBlock { command: without_prompts(&source) })
    }

    /// Whether a reply's text could hold one, so only those are looked at.
    pub fn may_hold(text: &str) -> bool {
        // sh, bash, zsh and shell all have it.
        (text.contains("```") || text.contains("~~~")) && text.to_lowercase().contains("sh")
    }
}

/// Only ordinary visible text, spaces, tabs and newlines: no control
/// characters, nothing invisible that changes what's shown (Unicode's format
/// characters: zero widths, bidi overrides and isolates, tags), no line or
/// paragraph separators, nothing from the private use areas.
fn is_plain(text: &str) -> bool {
    text.chars().all(|c| c == '\n' || c == '\t' || !(c.is_control() || is_format(c) || matches!(c, '\u{2028}' | '\u{2029}') || is_private_use(c)))
}

/// Unicode's format characters (general category Cf).
fn is_format(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
        | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

fn is_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// Agents sometimes write `$ npm test`. The "$ " only goes when every line
/// has one, so a line that really starts with `$` is left alone.
fn without_prompts(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let all_prompted = lines.iter().filter(|l| !l.trim().is_empty()).all(|l| l.starts_with("$ "));
    if !all_prompted {
        return text.to_owned();
    }
    lines.iter().map(|l| l.strip_prefix("$ ").unwrap_or(l)).collect::<Vec<_>>().join("\n")
}

/// The closed shell blocks in a reply's markdown, in order: what its play and
/// copy icons are on, for the dump. Fences inside quotes count, behind their
/// `>`s; one still streaming in doesn't.
pub fn shell_blocks(text: &str) -> Vec<ShellBlock> {
    if !ShellBlock::may_hold(text) {
        return Vec::new();
    }
    struct Open<'a> {
        mark: char,
        length: usize,
        quotes: usize,
        language: &'a str,
        code: Vec<&'a str>,
    }
    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    for line in text.split('\n') {
        let (quotes, rest) = unquoted(line);
        match open.as_mut() {
            None => {
                let Some(mark) = rest.chars().next().filter(|c| *c == '`' || *c == '~') else { continue };
                let length = rest.chars().take_while(|c| *c == mark).count();
                if length >= 3 {
                    open = Some(Open { mark, length, quotes, language: rest[length..].trim(), code: Vec::new() });
                }
            }
            Some(o) => {
                let closing = rest.trim_end();
                if quotes == o.quotes && closing.chars().count() >= o.length && closing.chars().all(|c| c == o.mark) {
                    if let Some(block) = ShellBlock::from(Some(o.language), &o.code.join("\n")) {
                        out.push(block);
                    }
                    open = None;
                } else {
                    o.code.push(if o.quotes > 0 { rest } else { line });
                }
            }
        }
    }
    out
}

/// A line behind any quote markers, and how many there were.
fn unquoted(line: &str) -> (usize, &str) {
    let mut rest = line.trim_start();
    let mut quotes = 0;
    while let Some(after) = rest.strip_prefix('>') {
        quotes += 1;
        rest = after.trim_start();
    }
    (quotes, rest)
}

/// Whether the code block at `start..end` of `source` (markdown, the block's
/// whole span, fences and all) has its closing fence. One still streaming in
/// hasn't, and half a command mustn't be something you can press. The last
/// line has to be the opening fence's character, at least as many of them,
/// behind as many quote markers as the opening line.
pub fn fence_closed(source: &str, start: usize, end: usize) -> bool {
    let Some(block) = source.get(start..end.min(source.len())) else { return false };
    let lines: Vec<&str> = block.trim_end_matches(['\n', '\r']).split('\n').collect();
    if lines.len() < 2 {
        return false;
    }
    // A fence inside a quote opens and closes behind its `>`s.
    let (quotes, opening) = unquoted(lines[0]);
    let Some(mark) = opening.chars().next().filter(|c| *c == '`' || *c == '~') else { return false };
    let length = opening.chars().take_while(|c| *c == mark).count();
    if length < 3 {
        return false;
    }
    let (closing_quotes, closing) = unquoted(lines[lines.len() - 1].trim_end());
    closing_quotes == quotes && closing.chars().count() >= length && closing.chars().all(|c| c == mark)
}

// ---------------------------------------------------------------- putting a command

/// What's on the shell's prompt, from the keys that have gone to it
/// (`TerminalPut.Line`). Read from the keys and not the screen because the
/// screen can't say: it doesn't show where the prompt ends or where the cursor
/// is in the line, and a command edited by hand can still end the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Empty,
    /// Exactly this, cursor at the end, nothing pressed since.
    Put(String),
    /// Someone has typed, moved the cursor or pasted.
    Edited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The shell is still starting; ask again in a moment.
    Wait,
    Type,
    /// The last command put here is still at the prompt, unrun, and this is a
    /// different one. It's copied rather than typed: after the first, the two
    /// would run as a third command nobody wrote, and clearing the first
    /// would mean sending keys (^U) a shell can have bound to anything,
    /// Return included (Codex's review, 11 Nov).
    Occupied,
    /// It's already sitting at the prompt (a second click): only the keyboard
    /// needs to go there.
    AlreadyThere,
    /// Something else has the terminal, or someone's own text is on the
    /// prompt. It isn't typed; it's copied instead.
    Busy,
}

/// Whether a shell's terminal has something a restart would lose, as quitting
/// ends every shell: a program running (a command still going, vim with its
/// buffer, ssh), or anything typed at the prompt, put there or edited
/// (Codex's review, 12 Nov). An idle shell at an empty prompt, or one that's
/// ended, has nothing.
pub fn holds_a_restart(running: bool, shell_alone_in_front: bool, line: &Line) -> bool {
    running && (!shell_alone_in_front || *line != Line::Empty)
}

/// How long a new shell may take to reach its first prompt, after which it's
/// treated as busy and the command is copied.
pub const STARTING_FOR_SECS: f64 = 10.;

/// `TerminalPut.step`.
/// - `shell_in_front`: the shell itself owns the terminal, not a program it's
///   running (vim, a dev server, a command still going).
/// - `bracketed_paste`: the terminal is in bracketed paste mode, which zsh,
///   fish and modern bash switch on only while they're editing a line. Both
///   are needed: vim turns it on too, and a shell running a builtin like
///   `read` owns the terminal with it off. Off means a newline, or simply
///   filling `read -k`, could run something with no Return pressed, so
///   nothing is typed, which includes macOS's bash 3, which never turns it on.
/// - `starting`: the shell has only just been started.
pub fn step(block: &ShellBlock, shell_in_front: bool, bracketed_paste: bool, starting: bool, line: &Line) -> Step {
    if !(shell_in_front && bracketed_paste) {
        return if starting { Step::Wait } else { Step::Busy };
    }
    match line {
        Line::Empty => Step::Type,
        Line::Put(command) if *command == block.command => Step::AlreadyThere,
        Line::Put(_) => Step::Occupied,
        Line::Edited => Step::Busy,
    }
}

/// What's on the prompt after a key goes to the shell (`TerminalPut.line`),
/// `key` being GPUI's name for it. Return (on its own) and ^C leave an empty
/// prompt; ⌘ keys aren't input (⌘V arrives as a paste); anything else edits it.
pub fn line_after_key(line: &Line, key: &str, control: bool, alt: bool, shift: bool, command: bool) -> Line {
    if command {
        return line.clone();
    }
    if key == "enter" && !control && !alt && !shift {
        return Line::Empty;
    }
    if control && key == "c" {
        return Line::Empty;
    }
    Line::Edited
}

/// Whether the program in front of the terminal is the shell that was
/// started there, from the paths of both (symlinks followed): `exec ssh -tt
/// elsewhere` or `exec vim` keeps the shell's process, and its hold on the
/// terminal, but a command typed there would go to that machine or program
/// (Codex's review, 11 Nov).
pub fn is_the_shell(started: &std::path::Path, running: &std::path::Path) -> bool {
    started == running || std::fs::canonicalize(running).is_ok_and(|r| r == started)
}

// ---------------------------------------------------------------- colours

/// A colour as 8-bit sRGB.
pub type Rgb8 = (u8, u8, u8);

/// The 16 colours programs pick by number: Terminal.app's, as the Swift app's
/// SwiftTerm has them (`Color.terminalAppColors`).
pub const ANSI: [Rgb8; 16] = [
    (0, 0, 0),
    (194, 54, 33),
    (37, 188, 36),
    (173, 173, 39),
    (73, 46, 225),
    (211, 56, 211),
    (51, 187, 200),
    (203, 204, 205),
    (129, 131, 131),
    (252, 57, 31),
    (49, 231, 34),
    (234, 236, 35),
    (88, 51, 255),
    (249, 53, 248),
    (20, 240, 240),
    (233, 235, 235),
];

/// All 256 numbered colours for a terminal drawn in `foreground` on
/// `background`, as SwiftTerm makes them by default (its `base16Lab`): the 16
/// above, then the 6×6×6 cube and the grey ramp worked out in Lab between the
/// first eight, with the background and text colours standing in for black and
/// white, so a program's 256 colours sit with the panel's own in either look.
/// On a light background the two swap, so the ramp still runs dark to light.
pub fn palette(background: Rgb8, foreground: Rgb8) -> [Rgb8; 256] {
    let mut base: Vec<Lab> = ANSI[..8].iter().map(|c| Lab::from(*c)).collect();
    base[0] = Lab::from(background);
    base[7] = Lab::from(foreground);
    if base[7].l < base[0].l {
        base.swap(0, 7);
    }
    let mut out = [(0u8, 0u8, 0u8); 256];
    out[..16].copy_from_slice(&ANSI);
    for r in 0..6 {
        let tr = f64::from(r) / 5.;
        let c0 = base[0].lerp(tr, base[1]);
        let c1 = base[2].lerp(tr, base[3]);
        let c2 = base[4].lerp(tr, base[5]);
        let c3 = base[6].lerp(tr, base[7]);
        for g in 0..6 {
            let tg = f64::from(g) / 5.;
            let c4 = c0.lerp(tg, c1);
            let c5 = c2.lerp(tg, c3);
            for b in 0..6 {
                out[16 + 36 * r as usize + 6 * g as usize + b as usize] = c4.lerp(f64::from(b) / 5., c5).rgb();
            }
        }
    }
    for i in 0..24 {
        out[232 + i] = base[0].lerp((i + 1) as f64 / 25., base[7]).rgb();
    }
    out
}

/// CIE Lab (D65), for mixing colours as they look rather than as numbers.
#[derive(Clone, Copy, Debug)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

impl Lab {
    fn from((r, g, b): Rgb8) -> Lab {
        let linear = |c: u8| {
            let c = f64::from(c) / 255.;
            if c > 0.04045 {
                ((c + 0.055) / 1.055).powf(2.4)
            } else {
                c / 12.92
            }
        };
        let (r, g, b) = (linear(r), linear(g), linear(b));
        let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16. / 116. };
        let x = f((r * 0.4124564 + g * 0.3575761 + b * 0.1804375) / 0.95047);
        let y = f(r * 0.2126729 + g * 0.7151522 + b * 0.0721750);
        let z = f((r * 0.0193339 + g * 0.1191920 + b * 0.9503041) / 1.08883);
        Lab { l: 116. * y - 16., a: 500. * (x - y), b: 200. * (y - z) }
    }

    fn lerp(self, t: f64, to: Lab) -> Lab {
        Lab { l: self.l + t * (to.l - self.l), a: self.a + t * (to.a - self.a), b: self.b + t * (to.b - self.b) }
    }

    fn rgb(self) -> Rgb8 {
        let y = (self.l + 16.) / 116.;
        let x = self.a / 500. + y;
        let z = y - self.b / 200.;
        let back = |t: f64| if t * t * t > 0.008856 { t * t * t } else { (t - 16. / 116.) / 7.787 };
        let (x, y, z) = (back(x) * 0.95047, back(y), back(z) * 1.08883);
        let gamma = |c: f64| {
            let c = if c > 0.0031308 { 1.055 * c.powf(1. / 2.4) - 0.055 } else { 12.92 * c };
            (c.clamp(0., 1.) * 255. + 0.5) as u8
        };
        (gamma(x * 3.2404542 - y * 1.5371385 - z * 0.4985314), gamma(-x * 0.9692660 + y * 1.8760108 + z * 0.0415560), gamma(x * 0.0556434 - y * 0.2040259 + z * 1.0572252))
    }
}

// ---------------------------------------------------------------- links

/// What a link in the terminal names (`PromptWatchingTerminalView.linkURL`):
/// a file, if it's the path of one (read from `folder` when relative, with a
/// `:12:4` line and column left off when the whole isn't a file), else a URL
/// with a scheme. The path comes first as `main.swift:12` parses as a URL
/// whose scheme is `main.swift`. None when it's neither. Whichever it is, the
/// open gate decides what opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    File(String),
    Url(String),
}

/// GNU tools put this machine's hostname in OSC 8 file URLs. Strip only an
/// exact local host; every other authority still goes to the gate to refuse.
pub fn local_file_url(link: &str, hostname: &str) -> Option<String> {
    if hostname.is_empty() || !link.get(..7)?.eq_ignore_ascii_case("file://") { return None; }
    let rest = &link[7..];
    let (host, _) = rest.split_once('/')?;
    (!host.is_empty() && host.eq_ignore_ascii_case(hostname)).then(|| format!("file://{}", &rest[host.len()..]))
}

/// The printed address/path under a terminal cell. OSC 8 links use their
/// explicit URI instead; this is for ordinary build output and shell text.
pub fn link_word(chars: &[char], at: usize) -> Option<String> {
    let boundary = |c: char| c.is_whitespace() || "\"'<>`".contains(c) || c.is_control();
    if at >= chars.len() || boundary(chars[at]) { return None; }
    let mut first = at;
    let mut last = at + 1;
    while first > 0 && !boundary(chars[first - 1]) { first -= 1; }
    while last < chars.len() && !boundary(chars[last]) { last += 1; }
    while first < last && "([{".contains(chars[first]) { first += 1; }
    loop {
        let tail = *chars.get(last.checked_sub(1)?)?;
        let unmatched = [(')', '('), (']', '['), ('}', '{')].iter().any(|(close, open)| tail == *close && chars[first..last].iter().filter(|c| **c == *close).count() > chars[first..last].iter().filter(|c| **c == *open).count());
        if first < last && (".,;!".contains(tail) || unmatched) { last -= 1; } else { break; }
    }
    (first <= at && at < last && last - first <= 4096).then(|| chars[first..last].iter().collect())
}

pub fn link_target(link: &str, folder: &str, home: &str, exists: impl Fn(&str) -> bool) -> Option<Link> {
    if link.is_empty() || link.len() > 4096 {
        return None;
    }
    let expanded = match link.strip_prefix("~/") {
        Some(rest) if !home.is_empty() => format!("{}/{rest}", home.trim_end_matches('/')),
        _ if link == "~" && !home.is_empty() => home.to_owned(),
        _ => link.to_owned(),
    };
    let path = if expanded.starts_with('/') { expanded } else { format!("{}/{expanded}", folder.trim_end_matches('/')) };
    if exists(&path) {
        return Some(Link::File(path));
    }
    if let Some(bare) = without_line_number(&path) {
        if exists(bare) {
            return Some(Link::File(bare.to_owned()));
        }
    }
    let (scheme, _) = link.split_once(':')?;
    let valid_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    valid_scheme.then(|| Link::Url(link.to_owned()))
}

/// `path` without a `:12` or `:12:4` at its end.
fn without_line_number(path: &str) -> Option<&str> {
    let mut rest = path;
    for _ in 0..2 {
        match rest.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => rest = head,
            _ => break,
        }
    }
    (rest.len() < path.len()).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, cwd: &str) -> ThreadSummary {
        ThreadSummary { id: id.into(), cwd: cwd.into(), ..Default::default() }
    }

    #[test]
    fn a_shell_holds_a_restart_while_busy_or_typed_in() {
        // Idle at an empty prompt, or ended: nothing to lose.
        assert!(!holds_a_restart(true, true, &Line::Empty));
        assert!(!holds_a_restart(false, false, &Line::Edited));
        // A command running, vim, ssh: the shell isn't the one in front.
        assert!(holds_a_restart(true, false, &Line::Empty));
        // Words at the prompt, typed or put there from a reply.
        assert!(holds_a_restart(true, true, &Line::Edited));
        assert!(holds_a_restart(true, true, &Line::Put("ls".into())));
    }

    #[test]
    fn a_thread_gets_its_own_shell_in_its_folder() {
        let t = thread("t1", "/work/repo");
        let target = target_for(Some(&t), "/Users/me", None, |_| true);
        assert_eq!(target, Target { key: TerminalKey::Thread("t1".into()), cwd: "/work/repo".into(), note: None });
        assert_eq!(target_for(None, "/Users/me", None, |_| true), Target { key: TerminalKey::Home, cwd: "/Users/me".into(), note: None });
    }

    #[test]
    fn a_thread_whose_folder_isnt_here_gets_the_home_shell_and_says_why() {
        let mut t = thread("t1", "/work/repo");
        t.runner_id = Some("bruce".into());
        let target = target_for(Some(&t), "/Users/me", Some("Bruce"), |_| true);
        assert_eq!(target.key, TerminalKey::Home);
        assert_eq!(target.note.as_deref(), Some("This thread runs on Bruce, so this terminal is on this Mac, in your home folder."));
        assert!(target_for(Some(&t), "/h", None, |_| true).note.unwrap().contains("on another machine"));
        let mut archived = thread("t2", "/work/repo");
        archived.archived_at = Some("2026-11-10T00:00:00Z".into());
        assert!(target_for(Some(&archived), "/h", None, |_| true).note.unwrap().contains("archived"));
        let gone_folder = thread("t3", "/nowhere");
        let target = target_for(Some(&gone_folder), "/h", None, |_| false);
        assert_eq!(target.key, TerminalKey::Home);
        assert!(target.note.unwrap().contains("isn't on this Mac any more"));
    }

    #[test]
    fn only_this_macs_archived_threads_stop_their_shells() {
        let mut threads = HashMap::new();
        let mut archived_local = thread("a", "/x");
        archived_local.archived_at = Some("2026-11-10T00:00:00Z".into());
        let mut archived_remote = thread("b", "/x");
        archived_remote.archived_at = Some("2026-11-10T00:00:00Z".into());
        archived_remote.runner_id = Some("bruce".into());
        // Another runner's row called "home", archived.
        let mut home_row = thread("home", "/x");
        home_row.archived_at = Some("2026-11-10T00:00:00Z".into());
        home_row.runner_id = Some("bruce".into());
        for t in [archived_local, archived_remote, home_row, thread("c", "/x")] {
            threads.insert(t.id.clone(), t);
        }
        let keys = [TerminalKey::Home, TerminalKey::Thread("a".into()), TerminalKey::Thread("b".into()), TerminalKey::Thread("c".into()), TerminalKey::Thread("missing".into())];
        assert_eq!(archived(&keys, &threads), vec![TerminalKey::Thread("a".into())]);
        let keeping: HashSet<&str> = ["a", "b", "c"].into_iter().collect();
        assert_eq!(gone(&keys, &keeping), vec![TerminalKey::Thread("missing".into())]);
    }

    #[test]
    fn only_the_open_threads_own_shell_takes_a_command() {
        let own = Target { key: TerminalKey::Thread("t1".into()), cwd: "/w".into(), note: None };
        assert!(can_put(Some("t1"), Some("t1"), &own));
        // A block in a thread open in a panel of its own, not the main panel.
        assert!(!can_put(Some("t2"), Some("t1"), &own));
        assert!(!can_put(None, Some("t1"), &own));
        let home = Target { key: TerminalKey::Home, cwd: "/h".into(), note: Some("This thread runs on Bruce…".into()) };
        assert!(!can_put(Some("t1"), Some("t1"), &home));
    }

    #[test]
    fn the_shell_is_a_login_shell_without_a_thread_id() {
        let l = launch(Os::Mac, Some("/opt/homebrew/bin/fish"), Some("en_GB.UTF-8"));
        assert_eq!(l.program, "/usr/bin/env");
        assert_eq!(&l.args[l.args.len() - 2..], ["/opt/homebrew/bin/fish", "-l"]);
        assert_eq!(l.shell, "/opt/homebrew/bin/fish");
        for name in ["THREADBEAR_THREAD_ID", "THREADBARE_THREAD_ID", "THREADBEAR_THREAD_KEY"] {
            assert!(l.args.windows(2).any(|w| w[0] == "-u" && w[1] == name), "{name}");
        }
        assert!(l.env.contains(&("TERM".into(), "xterm-256color".into())));
        assert!(l.env.contains(&("TERM_PROGRAM".into(), "Threadbear".into())));
        assert!(!l.env.iter().any(|(k, _)| k == "LANG"), "keeps the app's own LANG");
        // No SHELL, or a relative one: the system's own.
        assert_eq!(launch(Os::Mac, None, None).args.iter().rev().nth(1).unwrap(), "/bin/zsh");
        assert_eq!(launch(Os::OtherUnix, Some("zsh"), None).args.iter().rev().nth(1).unwrap(), "/bin/sh");
        assert!(launch(Os::Mac, None, Some("")).env.contains(&("LANG".into(), "en_US.UTF-8".into())));
    }

    fn send(key: &str, f: impl FnOnce(&mut Key)) -> KeyAction {
        send_in(key, Modes { option_is_meta: true, ..Default::default() }, f)
    }

    fn send_in(key: &str, modes: Modes, f: impl FnOnce(&mut Key)) -> KeyAction {
        let mut k = Key { key, ..Default::default() };
        f(&mut k);
        encode_key(&k, modes)
    }

    fn bytes(s: &str) -> KeyAction {
        KeyAction::Send(s.as_bytes().to_vec())
    }

    #[test]
    fn keys_send_what_a_terminal_sends() {
        assert_eq!(send("enter", |_| {}), bytes("\r"));
        assert_eq!(send("backspace", |_| {}), bytes("\x7f"));
        assert_eq!(send("tab", |_| {}), bytes("\t"));
        assert_eq!(send("tab", |k| k.shift = true), bytes("\x1b[Z"));
        assert_eq!(send("escape", |_| {}), bytes("\x1b"));
        assert_eq!(send("up", |_| {}), bytes("\x1b[A"));
        assert_eq!(send("left", |_| {}), bytes("\x1b[D"));
        assert_eq!(send("delete", |_| {}), bytes("\x1b[3~"));
        assert_eq!(send("f1", |_| {}), bytes("\x1bOP"));
        assert_eq!(send("f5", |_| {}), bytes("\x1b[15~"));
        assert_eq!(send("f12", |_| {}), bytes("\x1b[24~"));
        assert_eq!(send("home", |_| {}), bytes("\x1b[H"));
        assert_eq!(send("end", |_| {}), bytes("\x1b[F"));
    }

    #[test]
    fn full_screen_programs_get_the_cursor_keys_they_ask_for() {
        let app = Modes { app_cursor: true, alt_screen: true, option_is_meta: true };
        assert_eq!(send_in("up", app, |_| {}), bytes("\x1bOA"));
        assert_eq!(send_in("home", app, |_| {}), bytes("\x1bOH"));
        assert_eq!(send_in("pageup", app, |_| {}), bytes("\x1b[5~"));
        // At a prompt Page Up scrolls back instead, as in Terminal.
        assert_eq!(send("pageup", |_| {}), KeyAction::ScrollPage(-1));
        assert_eq!(send("pagedown", |_| {}), KeyAction::ScrollPage(1));
        assert_eq!(send("pageup", |k| k.shift = true), bytes("\x1b[5;2~"));
    }

    #[test]
    fn modifiers_go_in_the_sequence() {
        assert_eq!(send("up", |k| k.shift = true), bytes("\x1b[1;2A"));
        assert_eq!(send("right", |k| k.control = true), bytes("\x1b[1;5C"));
        assert_eq!(send("f2", |k| k.control = true), bytes("\x1b[1;5Q"));
        assert_eq!(send("delete", |k| k.alt = true), bytes("\x1b[3;3~"));
    }

    #[test]
    fn control_keys_are_c0_codes() {
        assert_eq!(send("c", |k| k.control = true), bytes("\x03"));
        assert_eq!(send("d", |k| k.control = true), bytes("\x04"));
        assert_eq!(send("a", |k| {
            k.control = true;
            k.shift = true
        }), bytes("\x01"));
        assert_eq!(send("space", |k| k.control = true), bytes("\0"));
        assert_eq!(send("[", |k| k.control = true), bytes("\x1b"));
        assert_eq!(send("-", |k| k.control = true), bytes("\x1f"));
        assert_eq!(send("1", |k| k.control = true), KeyAction::Nothing);
    }

    #[test]
    fn option_is_meta() {
        assert_eq!(send("b", |k| {
            k.alt = true;
            k.typed = Some("∫")
        }), bytes("\x1bb"));
        assert_eq!(send("b", |k| {
            k.alt = true;
            k.shift = true
        }), bytes("\x1bB"));
        assert_eq!(send(".", |k| k.alt = true), bytes("\x1b."));
        assert_eq!(send("left", |k| k.alt = true), bytes("\x1bb"));
        assert_eq!(send("right", |k| k.alt = true), bytes("\x1bf"));
        assert_eq!(send("backspace", |k| k.alt = true), bytes("\x1b\x7f"));
        assert_eq!(send("up", |k| k.alt = true), bytes("\x1b[1;3A"));
        // Off, ⌥ makes its character through the input method.
        assert_eq!(send_in("b", Modes::default(), |k| k.alt = true), KeyAction::Pass);
    }

    #[test]
    fn plain_characters_and_command_keys_pass() {
        assert_eq!(send("a", |k| k.typed = Some("a")), KeyAction::Pass);
        assert_eq!(send("space", |_| {}), KeyAction::Pass);
        assert_eq!(send("c", |k| k.command = true), KeyAction::Pass);
        assert_eq!(send("f13", |_| {}), KeyAction::Nothing);
    }

    #[test]
    fn a_paste_is_bracketed_and_cant_close_its_own_brackets() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb");
        // With no brackets a control character would be a key pressed.
        assert_eq!(paste_bytes("ls\x03\x1b:q!\x15\x7f\u{9b}\tx", false), b"ls:q!\tx");
        assert_eq!(paste_bytes("ls\r\n", true), b"\x1b[200~ls\r\x1b[201~");
        assert_eq!(paste_bytes("echo\x1b[201~\nrm -rf ~", true), b"\x1b[200~echo[201~\rrm -rf ~\x1b[201~");
    }

    /// Daybreak's finding 84: inside the brackets a ^\\ (or ^C, ^Z) is still a
    /// signal to the pty, which flushes the opening bracket, so the line break
    /// after it ran the command. Every control but Tab and Return goes, C0,
    /// DEL and C1 alike; tests/paste_pty.rs plays it against a real bash.
    #[test]
    fn a_bracketed_paste_has_no_control_characters_in_it() {
        assert_eq!(
            paste_bytes("\u{1c}printf \"TB_%s_RAN\\n\" PASTE\n", true),
            b"\x1b[200~printf \"TB_%s_RAN\\n\" PASTE\r\x1b[201~"
        );
        let every: String = (0u32..0x20).chain(0x7f..0xa0).filter_map(char::from_u32).collect();
        assert_eq!(paste_bytes(&format!("a{every}b"), true), b"\x1b[200~a\t\r\rb\x1b[201~");
        assert_eq!(paste_bytes(&format!("a{every}b"), false), b"a\t\r\rb");
        // Text that isn't control stays as it is, either way.
        assert_eq!(paste_bytes("caf\u{e9} \u{65e5}\u{672c} \u{1f43b}", true), "\x1b[200~caf\u{e9} \u{65e5}\u{672c} \u{1f43b}\x1b[201~".as_bytes());
    }

    #[test]
    fn mouse_reports() {
        assert_eq!(mouse_report(MouseButton::Left, true, false, 0, 0, false, false, false, true).unwrap(), b"\x1b[<0;1;1M");
        assert_eq!(mouse_report(MouseButton::Left, false, false, 4, 2, false, false, false, true).unwrap(), b"\x1b[<0;5;3m");
        assert_eq!(mouse_report(MouseButton::WheelUp, true, false, 0, 0, false, false, true, true).unwrap(), b"\x1b[<80;1;1M");
        assert_eq!(mouse_report(MouseButton::Left, true, true, 1, 1, false, false, false, true).unwrap(), b"\x1b[<32;2;2M");
        assert_eq!(mouse_report(MouseButton::Right, true, false, 0, 0, false, false, false, false).unwrap(), vec![0x1b, b'[', b'M', 34, 33, 33]);
        assert_eq!(mouse_report(MouseButton::Right, false, false, 0, 0, false, false, false, false).unwrap(), vec![0x1b, b'[', b'M', 35, 33, 33]);
        assert_eq!(mouse_report(MouseButton::Left, true, false, 300, 0, false, false, false, false), None);
    }

    #[test]
    fn only_shell_fences_count() {
        for language in ["sh", "bash", "zsh", "shell", "Bash title=x", "bash{.x}"] {
            assert_eq!(ShellBlock::from(Some(language), "ls\n").map(|b| b.command), Some("ls".to_owned()), "{language}");
        }
        for language in [Some("python"), Some("bashful"), Some("console"), Some(""), None] {
            assert_eq!(ShellBlock::from(language, "ls"), None, "{language:?}");
        }
        assert_eq!(ShellBlock::from(Some("bash"), " \n\n"), None);
    }

    #[test]
    fn prompts_come_off_only_when_every_line_has_one() {
        assert_eq!(ShellBlock::from(Some("sh"), "$ a\n\n$ b\n").unwrap().command, "a\n\nb");
        assert_eq!(ShellBlock::from(Some("sh"), "$ a\nb").unwrap().command, "$ a\nb");
        assert_eq!(ShellBlock::from(Some("sh"), "$HOME/x").unwrap().command, "$HOME/x");
    }

    // Each of these could make what runs differ from what's on screen, or run
    // a line without Return being pressed.
    #[test]
    fn refuses_what_cant_be_typed_safely() {
        let bad = ["echo hi\u{1b}[201~\nrm -rf ~", "echo\u{07}", "a\rb", "a\u{7f}b", "a\u{9b}201~", "rm\u{200b} x", "echo \u{202e}cod", "a\u{2028}b", "x\u{e000}", "x\u{2066}y"];
        for code in bad {
            assert_eq!(ShellBlock::from(Some("bash"), code), None, "{code:?}");
        }
        assert!(ShellBlock::from(Some("bash"), "echo 'héllo 🐻'\n\tls").is_some());
        assert!(ShellBlock::from(Some("bash"), &"a".repeat(MAX_SOURCE + 1)).is_none());
    }

    #[test]
    fn may_hold() {
        assert!(ShellBlock::may_hold("x\n```bash\nls\n```"));
        assert!(!ShellBlock::may_hold("no fence, just bash"));
        assert!(!ShellBlock::may_hold("```python\nx\n```"));
    }

    #[test]
    fn a_fence_still_streaming_in_isnt_closed() {
        let closed = "Run this:\n\n```bash\nnpm test\n```\n";
        let start = closed.find("```").unwrap();
        assert!(fence_closed(closed, start, closed.len()));
        let open = "Run this:\n\n```bash\nnpm test\n";
        assert!(!fence_closed(open, open.find("```").unwrap(), open.len()));
        // A shorter run of backticks doesn't close a longer fence.
        let longer = "````sh\nls\n```";
        assert!(!fence_closed(longer, 0, longer.len()));
        let tildes = "~~~sh\nls\n~~~~";
        assert!(fence_closed(tildes, 0, tildes.len()));
        let quoted = "> ```sh\n> ls\n> ```";
        assert!(fence_closed(quoted, 0, quoted.len()));
        assert!(!fence_closed("```sh", 0, 5));
    }

    #[test]
    fn a_replys_closed_shell_blocks_are_found() {
        let text = "Run these:\n\n```bash\nnpm test\n```\n\nand\n\n> ```sh\n> echo quoted\n> ```\n\n```python\nprint(1)\n```\n\n```zsh\nstill streaming";
        let commands: Vec<String> = shell_blocks(text).into_iter().map(|b| b.command).collect();
        assert_eq!(commands, ["npm test", "echo quoted"]);
        assert!(shell_blocks("no fences here").is_empty());
    }

    fn put_step(command: &str, front: bool, bracketed: bool, starting: bool, line: Line) -> Step {
        step(&ShellBlock { command: command.into() }, front, bracketed, starting, &line)
    }

    #[test]
    fn types_on_an_empty_prompt() {
        assert_eq!(put_step("ls", true, true, false, Line::Empty), Step::Type);
        assert_eq!(put_step("a\nb", true, true, false, Line::Empty), Step::Type);
    }

    #[test]
    fn never_types_into_a_running_program() {
        // vim turns bracketed paste on too, so that alone isn't enough.
        assert_eq!(put_step("ls", false, true, false, Line::Empty), Step::Busy);
    }

    // `read -k 11 answer; eval "$answer"` owns the terminal from inside the
    // shell, so eleven typed characters would have run with no Return.
    #[test]
    fn needs_the_shell_to_be_editing_a_line() {
        assert_eq!(put_step("ls", true, false, false, Line::Empty), Step::Busy);
    }

    #[test]
    fn waits_for_a_shell_thats_starting() {
        assert_eq!(put_step("ls", true, false, true, Line::Empty), Step::Wait);
        assert_eq!(put_step("ls", false, true, true, Line::Empty), Step::Wait);
        assert_eq!(put_step("ls", true, true, true, Line::Empty), Step::Type);
    }

    #[test]
    fn never_adds_to_what_someone_typed() {
        assert_eq!(put_step("ls", true, true, false, Line::Edited), Step::Busy);
    }

    #[test]
    fn a_different_block_is_copied_while_one_waits_at_the_prompt() {
        assert_eq!(put_step("ls", true, true, false, Line::Put("npm test".into())), Step::Occupied);
        assert_eq!(put_step("npm test", true, true, false, Line::Put("npm test".into())), Step::AlreadyThere);
        assert_eq!(put_step("echo \"a b\"", true, true, false, Line::Put("echo \"ab\"".into())), Step::Occupied);
    }

    #[test]
    fn a_paste_that_would_run_lines_is_asked_about() {
        assert!(!paste_runs("echo one"));
        assert!(paste_runs("touch /tmp/x\n"));
        assert!(paste_runs("a\rb"));
        assert!(paste_runs("echo $(touch /tmp/x)\t"), "a Tab is the Tab key without brackets");
        assert!(paste_question("ls\t").1.contains("Tab"));
        assert!(paste_question("ls\n").1.contains("Return"));
        assert_eq!(paste_question("a\nb\n").0, "Paste 2 lines?");
        assert_eq!(paste_question("ls\n").0, "Paste this line?");
    }

    #[test]
    fn a_snippet_with_a_line_break_is_a_paste_not_keys() {
        assert!(is_bulk_text("touch /tmp/x\n"));
        assert!(is_bulk_text("ls\x01"));
        assert!(!is_bulk_text("日本語"), "an input method's words are typed");
        assert!(!is_bulk_text("\r"), "Return");
        assert!(!is_bulk_text("\r\n"), "Return");
        assert!(!is_bulk_text("é"));
    }

    #[test]
    fn the_shell_is_known_by_its_program() {
        let dir = std::env::temp_dir().join(format!("tb-shell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("zsh");
        std::fs::write(&real, "").unwrap();
        let real = std::fs::canonicalize(&real).unwrap();
        assert!(is_the_shell(&real, &real));
        #[cfg(unix)]
        {
            let link = dir.join("link");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(is_the_shell(&real, &link), "a link to it is it");
        }
        assert!(!is_the_shell(&real, std::path::Path::new("/usr/bin/ssh")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keys_track_whats_on_the_prompt() {
        let put = Line::Put("ls".into());
        assert_eq!(line_after_key(&put, "enter", false, false, false, false), Line::Empty);
        assert_eq!(line_after_key(&Line::Edited, "enter", false, false, false, false), Line::Empty);
        assert_eq!(line_after_key(&Line::Edited, "c", true, false, false, false), Line::Empty);
        assert_eq!(line_after_key(&Line::Empty, "a", false, false, false, false), Line::Edited);
        assert_eq!(line_after_key(&put, "a", true, false, false, false), Line::Edited, "Ctrl-A moves the cursor");
        assert_eq!(line_after_key(&put, "enter", false, false, true, false), Line::Edited);
        assert_eq!(line_after_key(&put, "up", false, false, false, false), Line::Edited, "the up arrow brings back a command");
        assert_eq!(line_after_key(&put, "j", false, false, false, true), put, "a shortcut isn't input");
        assert_eq!(line_after_key(&put, "c", false, false, false, false), Line::Edited, "a plain c is just a c");
    }


    #[test]
    fn the_256_colours_run_from_the_panels_background_to_its_text() {
        let near = |a: Rgb8, b: Rgb8| (i16::from(a.0) - i16::from(b.0)).abs() <= 1 && (i16::from(a.1) - i16::from(b.1)).abs() <= 1 && (i16::from(a.2) - i16::from(b.2)).abs() <= 1;
        let dark = palette((28, 28, 31), (230, 230, 230));
        assert_eq!(&dark[..16], &ANSI);
        // The cube's corners are the panel's own colours.
        assert!(near(dark[16], (28, 28, 31)), "{:?}", dark[16]);
        assert!(near(dark[231], (230, 230, 230)), "{:?}", dark[231]);
        let lightness = |c: Rgb8| u32::from(c.0) + u32::from(c.1) + u32::from(c.2);
        assert!(lightness(dark[232]) < lightness(dark[255]));
        // On white the ramp still runs dark to light.
        let light = palette((255, 255, 255), (31, 31, 31));
        assert!(near(light[16], (31, 31, 31)), "{:?}", light[16]);
        assert!(lightness(light[232]) < lightness(light[255]));
        // Pure red on the cube is near Terminal's red, not xterm's.
        let red = dark[16 + 36 * 5];
        assert!(red.0 > 150 && red.1 < 90, "{red:?}");
    }

    #[test]
    fn a_link_is_a_file_when_its_one() {
        let exists = |p: &str| p == "/w/src/main.rs" || p == "/Users/me/notes.md";
        assert_eq!(link_target("src/main.rs", "/w", "/Users/me", exists), Some(Link::File("/w/src/main.rs".into())));
        assert_eq!(link_target("src/main.rs:12:4", "/w", "/Users/me", exists), Some(Link::File("/w/src/main.rs".into())));
        assert_eq!(link_target("~/notes.md", "/w", "/Users/me", exists), Some(Link::File("/Users/me/notes.md".into())));
        assert_eq!(link_target("https://example.com/a", "/w", "/Users/me", exists), Some(Link::Url("https://example.com/a".into())));
        assert_eq!(link_target("file:///w/x.command", "/w", "/Users/me", exists), Some(Link::Url("file:///w/x.command".into())));
        assert_eq!(link_target("main.swift:12", "/w", "/Users/me", exists), Some(Link::Url("main.swift:12".into())));
        assert_eq!(link_target("no scheme here", "/w", "/Users/me", exists), None);
        assert_eq!(link_target("", "/w", "/Users/me", exists), None);
    }

    #[test]
    fn printed_terminal_links_keep_url_punctuation_and_file_positions() {
        let word = |line: &str, at| link_word(&line.chars().collect::<Vec<_>>(), at);
        assert_eq!(word("see (https://example.com/a(b)).", 12).as_deref(), Some("https://example.com/a(b)"));
        assert_eq!(word("error: src/main.rs:12:4", 12).as_deref(), Some("src/main.rs:12:4"));
        assert_eq!(word("雪 https://example.com", 8).as_deref(), Some("https://example.com"));
        assert_eq!(word("https://example.com foo", 19), None);
        assert_eq!(word("[x]", 0), None);
        assert_eq!(word("()", 1), None);
    }

    #[test]
    fn a_file_hyperlink_can_name_only_this_exact_machine() {
        assert_eq!(local_file_url("file://MY-MAC.local/tmp/a%20b.command", "my-mac.local").as_deref(), Some("file:///tmp/a%20b.command"));
        for uri in ["file://my-mac.local.evil/tmp/a", "file://my-mac/tmp/a", "file://user@my-mac.local/tmp/a", "file://my-mac.local:1/tmp/a", "file://other/tmp/a", "https://my-mac.local/a"] {
            assert_eq!(local_file_url(uri, "my-mac.local"), None, "{uri}");
        }
        assert_eq!(local_file_url("file:///tmp/a", ""), None);
    }
}
