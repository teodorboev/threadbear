// A ```slack, ```email or ```draft block in a reply: text the agent has
// written for Mike to send somewhere else (Swift's DraftBlock.swift). It's
// shown the way it'll go out, every line where the agent put it (a reply's
// own Markdown joins lines up, which ran a Slack draft's "•" lines into one
// paragraph, 6 Nov), in a box with a Copy button. A ```slack one also draws
// Slack's own formatting: *bold*, _italic_, ~strike~, `code` and
// <url|label> links.
//
// The Mac app (app/Sources/ThreadbearCore/Markdown/DraftBlock.swift), the
// phone (ios/ThreadbearKit, DraftBlock.swift) and the website
// (web/src/draft.ts) parse it the same way; keep them together, and their
// tests. This works on chars (code points), as the website does; the Swift
// side counts Characters, which only differs for accents and emoji built from
// several.
use std::ops::Range;

/// Past this a fence stays plain code: a draft that size isn't a message,
/// and the box shouldn't be what makes a reply slow.
pub const MAX_SOURCE: usize = 20_000;

/// Past this many characters a line is shown as written, so a huge line
/// can't keep the formatting busy.
pub const MAX_FORMATTED_LINE: usize = 2_000;

/// Where the placeholders for a line's tokens start (Unicode's private use
/// area).
const PLACEHOLDER_BASE: u32 = 0xE000;
const PLACEHOLDER_LAST: u32 = 0xF8FF;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DraftKind {
    Slack,
    Email,
    Draft,
}

impl DraftKind {
    fn from_word(word: &str) -> Option<DraftKind> {
        match word {
            "slack" => Some(DraftKind::Slack),
            "email" => Some(DraftKind::Email),
            "draft" => Some(DraftKind::Draft),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DraftBlock {
    pub kind: DraftKind,
    pub source: String,
}

/// A run of a line with one look. `link` is http(s) or mailto only, kept as
/// the agent wrote it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Span {
        Span { text: text.into(), ..Default::default() }
    }

    fn same_look(&self, other: &Span) -> bool {
        self.bold == other.bold && self.italic == other.italic && self.strike == other.strike && self.code == other.code && self.link == other.link
    }

    /// The span for a dump (both apps' `drafts`): its looks, then its words,
    /// then where a link goes. Nil for plain words.
    pub fn described(&self) -> Option<String> {
        let mut looks = Vec::new();
        if self.bold {
            looks.push("bold");
        }
        if self.italic {
            looks.push("italic");
        }
        if self.strike {
            looks.push("strike");
        }
        if self.code {
            looks.push("code");
        }
        if self.link.is_some() {
            looks.push("link");
        }
        if looks.is_empty() {
            return None;
        }
        let mut out = format!("{}: {}", looks.join("+"), self.text);
        if let Some(link) = &self.link {
            out.push_str(&format!(" <{link}>"));
        }
        Some(out)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub spans: Vec<Span>,
    /// A Slack "> " line, drawn with a bar.
    pub quote: bool,
}

impl Line {
    /// The line's words as they're shown, the quote's bar included.
    pub fn shown(&self) -> String {
        let words: String = self.spans.iter().map(|s| s.text.as_str()).collect();
        if self.quote {
            format!("\u{2502} {words}")
        } else {
            words
        }
    }
}

impl DraftBlock {
    /// The draft for a fence's info string (only the first word counts), or
    /// None for any other fence.
    pub fn from(language: Option<&str>, code: &str) -> Option<DraftBlock> {
        let word = language?.split([' ', '\t', '{']).find(|w| !w.is_empty())?.to_lowercase();
        let kind = DraftKind::from_word(&word)?;
        let mut source = code.replace("\r\n", "\n");
        while source.ends_with('\n') {
            source.pop();
        }
        if source.trim().is_empty() || source.chars().count() > MAX_SOURCE {
            return None;
        }
        Some(DraftBlock { kind, source })
    }

    /// Whether a reply's text could hold one, so only those are looked at
    /// for a closed fence.
    pub fn may_hold(text: &str) -> bool {
        if !text.contains("```") && !text.contains("~~~") {
            return false;
        }
        let lowered = text.to_lowercase();
        lowered.contains("slack") || lowered.contains("email") || lowered.contains("draft")
    }

    /// What the box's header calls it.
    pub fn label(&self) -> &'static str {
        match self.kind {
            DraftKind::Slack => "Slack draft",
            DraftKind::Email => "Email draft",
            DraftKind::Draft => "Draft",
        }
    }

    /// The draft's lines as they're shown. An email or plain draft is its
    /// text as it is (bare addresses are linked when it's drawn,
    /// `bare_links`); a Slack one has its formatting and <...> tokens
    /// worked out.
    pub fn lines(&self) -> Vec<Line> {
        self.source
            .split('\n')
            .map(|line| {
                if self.kind != DraftKind::Slack {
                    return Line { spans: vec![Span::plain(line)], quote: false };
                }
                if line.starts_with("> ") || line == ">" || line.starts_with("&gt; ") {
                    let rest = if line.starts_with("&gt; ") { &line[5..] } else { &line[line.len().min(2)..] };
                    return Line { spans: slack_spans(rest), quote: true };
                }
                Line { spans: slack_spans(line), quote: false }
            })
            .collect()
    }

    /// What Copy puts on the clipboard as plain text. A Slack draft keeps its
    /// *bold* and _italic_ (Slack's message box formats those as it sends)
    /// but its <url|label> links become "label (url)", which pasted as plain
    /// text would otherwise stay as typed.
    pub fn plain_text(&self) -> String {
        if self.kind != DraftKind::Slack {
            return self.source.clone();
        }
        self.source
            .split('\n')
            .map(|line| {
                let (marked, tokens) = marked(line);
                let Some(tokens) = tokens else { return decode(line) };
                let chars: Vec<char> = marked.chars().collect();
                // A token in `code` is copied as typed, as it's shown.
                let code = code_spans(&chars);
                let mut out = String::new();
                for (offset, c) in chars.iter().enumerate() {
                    let Some(index) = placeholder_index(*c).filter(|i| *i < tokens.len()) else {
                        out.push(*c);
                        continue;
                    };
                    if code.iter().any(|r| r.contains(&offset)) {
                        out.push_str(&tokens[index].1);
                        continue;
                    }
                    match &tokens[index].0 {
                        Token::Link { url, label } if label == url || label.is_empty() => out.push_str(url),
                        Token::Link { url, label } => out.push_str(&format!("{label} ({url})")),
                        Token::Mention(text) => out.push_str(text),
                    }
                }
                decode(&out)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The same as HTML, put on the clipboard beside the plain text, so
    /// pasting into Slack or Mail keeps the bold and the links.
    pub fn html(&self) -> String {
        let body: Vec<String> = self
            .lines()
            .iter()
            .map(|line| {
                let mut after_space = true;
                let inner: String = line
                    .spans
                    .iter()
                    .map(|span| {
                        let out = span_html(span, after_space);
                        after_space = span.text.ends_with(' ');
                        out
                    })
                    .collect();
                if line.quote {
                    format!("<blockquote>{inner}</blockquote>")
                } else {
                    inner
                }
            })
            .collect();
        // Quotes are blocks of their own, so no <br> beside one.
        let mut out = String::new();
        for (index, part) in body.iter().enumerate() {
            out.push_str(part);
            if index + 1 < body.len() && !part.starts_with("<blockquote>") && !body[index + 1].starts_with("<blockquote>") {
                out.push_str("<br>");
            }
        }
        format!("<meta charset=\"utf-8\"><div>{out}</div>")
    }

    /// The lines with an email or plain draft's bare web addresses linked, as
    /// the box draws them (Copy still sends the text as written).
    pub fn drawn_lines(&self) -> Vec<Line> {
        let lines = self.lines();
        if self.kind == DraftKind::Slack {
            return lines;
        }
        lines
            .into_iter()
            .map(|line| {
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                Line { spans: bare_links(&text), quote: line.quote }
            })
            .collect()
    }
}

fn span_html(span: &Span, after_space: bool) -> String {
    let mut text = keep_spaces(&escape(&span.text), after_space);
    if span.code {
        text = format!("<code>{text}</code>");
    }
    if span.strike {
        text = format!("<s>{text}</s>");
    }
    if span.italic {
        text = format!("<i>{text}</i>");
    }
    if span.bold {
        text = format!("<b>{text}</b>");
    }
    if let Some(link) = &span.link {
        text = format!("<a href=\"{}\">{text}</a>", escape(link));
    }
    text
}

/// HTML folds runs of spaces into one, so a line's indent and any spaces
/// lined up in it would go in a paste: each space at the start of a line or
/// after another is a no-break space.
fn keep_spaces(text: &str, after_space: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_space = after_space;
    for c in text.chars() {
        match c {
            ' ' => {
                out.push_str(if previous_space { "&nbsp;" } else { " " });
                previous_space = true;
            }
            '\t' => {
                out.push_str("&nbsp;&nbsp;&nbsp;&nbsp;");
                previous_space = true;
            }
            _ => {
                out.push(c);
                previous_space = false;
            }
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// An email or plain draft's text with its bare http(s) addresses as links,
/// for drawing only. Punctuation that ends a sentence after an address
/// isn't part of it (the website's `bareLinks`; the Mac's asks the system's
/// link detector, which finds the same ones).
pub fn bare_links(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = 0;
    let mut from = 0;
    let lower = text.to_ascii_lowercase();
    while let Some(found) = next_address(&lower, from) {
        // Up to a space, a quote or an angle bracket.
        let end = text[found..].find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).map_or(text.len(), |n| found + n);
        let url = text[found..end].trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"', ')', ']']);
        let scheme_end = found + url.find("//").map_or(0, |n| n + 2);
        from = end.max(found + 1);
        if found + url.len() <= scheme_end {
            continue;
        }
        if found > at {
            spans.push(Span::plain(&text[at..found]));
        }
        spans.push(Span { text: url.to_owned(), link: Some(url.to_owned()), ..Default::default() });
        at = found + url.len();
    }
    if at < text.len() {
        spans.push(Span::plain(&text[at..]));
    }
    spans
}

/// Where the next "http://" or "https://" starts at or after `from`.
fn next_address(lower: &str, from: usize) -> Option<usize> {
    let rest = lower.get(from..)?;
    let http = rest.find("http://");
    let https = rest.find("https://");
    let first = match (http, https) {
        (Some(a), Some(b)) => a.min(b),
        (a, b) => a.or(b)?,
    };
    Some(from + first)
}

/// Whether the fenced code block written at `source[start..end]` (byte
/// offsets: where the parser says the block is) has its closing fence. The
/// block starts at the fence itself, so what's before it on its line is the
/// container (a quote's "> ", a list's "- "). While a reply streams, an
/// unclosed fence runs to the end of the text, and it stays plain code until
/// it's done: half a draft mustn't be something to copy. The closing line
/// has the same quote markers in front, then the same mark at least as many
/// times, and only spaces. How far it's indented isn't checked: a list's
/// indent is the parser's to work out (three tries at it each broke a nested
/// case on the Mac, 6 Nov), and the worst a wrong guess does is show the box
/// a line early (Swift's `MarkdownRenderer.isClosed`, the website's
/// `fenceClosed`).
pub fn fence_closed(source: &str, start: usize, end: usize) -> bool {
    let end = end.min(source.len());
    if start >= end || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return false;
    }
    // The block's last line, not the line break after it.
    let mut end = end;
    while end > start && matches!(source.as_bytes()[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    let opening_start = source[..start].rfind('\n').map_or(0, |n| n + 1);
    let closing_start = source[..end].rfind('\n').map_or(0, |n| n + 1);
    if closing_start <= start {
        return false;
    }
    let line_end = source[start..].find('\n').map_or(source.len(), |n| start + n);
    let opening = &source[start..line_end];
    let mark = match opening.chars().next() {
        Some(c @ ('`' | '~')) => c,
        _ => return false,
    };
    let width = opening.chars().take_while(|&c| c == mark).count();
    if width < 3 {
        return false;
    }
    let quotes = source[opening_start..start].matches('>').count();
    let mut closing = &source[closing_start..end];
    for _ in 0..quotes {
        closing = closing.trim_start_matches([' ', '\t']);
        let Some(rest) = closing.strip_prefix('>') else { return false };
        closing = rest;
    }
    let fence = closing.trim();
    fence.chars().count() >= width && fence.chars().all(|c| c == mark)
}

// ---------------------------------------------------------------- Slack's formatting

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Link { url: String, label: String },
    /// <@U123>, <#C123|general>, <!here>: shown as their words.
    Mention(String),
}

/// A <...> token: a link to a web page or a mail address, or a mention.
/// Anything else between angle brackets is left as written.
fn token(inside: &str) -> Option<Token> {
    let (target, label) = match inside.split_once('|') {
        Some((target, label)) => (target, Some(label)),
        None => (inside, None),
    };
    if let Some(first) = target.chars().next().filter(|c| matches!(c, '@' | '#' | '!')) {
        let name = &target[1..];
        if name.is_empty() || name.contains(' ') {
            return None;
        }
        if let Some(label) = label.filter(|l| !l.is_empty()) {
            return Some(Token::Mention(if first == '#' && !label.starts_with('#') { format!("#{label}") } else { label.to_owned() }));
        }
        if first == '!' {
            // <!subteam^S123> and <!date^...> carry a label or nothing useful.
            let part = name.split('^').find(|p| !p.is_empty()).unwrap_or(name);
            return Some(Token::Mention(format!("@{part}")));
        }
        return Some(Token::Mention(format!("{first}{name}")));
    }
    if target.contains(' ') {
        return None;
    }
    let scheme = scheme_of(target)?.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "mailto") || !is_address(target, &scheme) {
        return None;
    }
    let shown = match label {
        Some(label) => decode(label),
        None if scheme == "mailto" => target["mailto:".len()..].to_owned(),
        None => target.to_owned(),
    };
    // The address is kept as written (Swift's absoluteString is the same for these).
    Some(Token::Link { url: target.to_owned(), label: shown })
}

/// `scheme:` at the start, as a URL's is written.
fn scheme_of(target: &str) -> Option<&str> {
    let colon = target.find(':')?;
    let scheme = &target[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))).then_some(scheme)
}

/// Enough of an address to be one: a web one names a host, a mail one
/// someone, and neither has control characters (the Swift side's
/// `URL(string:)`, the website's `URL.canParse`).
fn is_address(target: &str, scheme: &str) -> bool {
    let rest = &target[scheme.len() + 1..];
    if rest.is_empty() || rest.chars().any(char::is_control) {
        return false;
    }
    if scheme == "mailto" {
        return true;
    }
    rest.trim_start_matches('/').chars().next().is_some_and(|c| !matches!(c, '?' | '#' | '/'))
}

enum Piece {
    Text(String),
    /// The token, and the <...> it was written as.
    Token(Token, String),
}

/// Walks a line once, handing over its words and its <...> tokens in order.
/// A "<" that doesn't start a token is a word like any other. The next ">"
/// is looked for once and kept until it's passed, so a line of many "<" (or
/// one with no ">") takes one pass, not one per "<" (cross-lab review, 6 Nov).
fn scan_tokens(line: &str, mut visit: impl FnMut(Piece)) {
    let chars: Vec<char> = line.chars().collect();
    let mut plain = String::new();
    // Where the next ">" is, once looked for; `looked` and None: there's none.
    let mut close: Option<usize> = None;
    let mut looked = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let stale = match close {
                Some(c) => c <= i,
                None => !looked,
            };
            if stale {
                looked = true;
                close = chars[i + 1..].iter().position(|&c| c == '>').map(|n| i + 1 + n);
            }
            if let Some(end) = close {
                // A token never holds a "<", so with another before the ">"
                // this one isn't a token, and what's between isn't read.
                if !chars[i + 1..end].contains(&'<') {
                    let inside: String = chars[i + 1..end].iter().collect();
                    if let Some(found) = token(&inside) {
                        if !plain.is_empty() {
                            visit(Piece::Text(std::mem::take(&mut plain)));
                        }
                        visit(Piece::Token(found, chars[i..=end].iter().collect()));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        plain.push(chars[i]);
        i += 1;
    }
    if !plain.is_empty() {
        visit(Piece::Text(plain));
    }
}

fn decode(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

type Tokens = Vec<(Token, String)>;

/// The line with each <...> token as one placeholder character, and the
/// tokens; no tokens for a line that's too long, or that has placeholder
/// characters of its own, which is shown as written.
fn marked(line: &str) -> (String, Option<Tokens>) {
    if line.chars().count() > MAX_FORMATTED_LINE || line.chars().any(|c| (PLACEHOLDER_BASE..=PLACEHOLDER_LAST).contains(&(c as u32))) {
        return (line.to_owned(), None);
    }
    let mut out = String::with_capacity(line.len());
    let mut tokens: Tokens = Vec::new();
    scan_tokens(line, |piece| match piece {
        Piece::Text(text) => out.push_str(&text),
        Piece::Token(token, raw) => {
            // A line this short can't have more tokens than placeholders.
            out.push(char::from_u32(PLACEHOLDER_BASE + tokens.len() as u32).unwrap_or('\u{FFFD}'));
            tokens.push((token, raw));
        }
    });
    (out, Some(tokens))
}

fn placeholder_index(c: char) -> Option<usize> {
    let value = c as u32;
    (PLACEHOLDER_BASE..=PLACEHOLDER_LAST).contains(&value).then(|| (value - PLACEHOLDER_BASE) as usize)
}

/// Where a line's `code` is, by char offset, found the way `emphasis` finds it.
fn code_spans(chars: &[char]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' && opens(chars, i) {
            if let Some(end) = closing(chars, i, '`') {
                spans.push(i..end + 1);
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    spans
}

/// A Slack line's spans: *bold*, _italic_, ~strike~, `code`, links and
/// mentions. Each <...> token stands in the line as one placeholder
/// character while the emphasis is worked out, so a link can be bold
/// (*see <url|docs>*) and a token inside `code` stays as written.
pub fn slack_spans(line: &str) -> Vec<Span> {
    let (marked, found) = marked(line);
    let Some(tokens) = found else { return vec![Span::plain(decode(line))] };
    let mut spans = Vec::new();
    for span in emphasis(&marked, &Span::default()) {
        let mut words = String::new();
        let flush = |words: &mut String, spans: &mut Vec<Span>| {
            if !words.is_empty() {
                spans.push(Span { text: std::mem::take(words), ..span.clone() });
            }
        };
        for c in span.text.chars() {
            let Some(index) = placeholder_index(c).filter(|i| *i < tokens.len()) else {
                words.push(c);
                continue;
            };
            if span.code {
                // Code shows what was typed.
                words.push_str(&tokens[index].1);
                continue;
            }
            flush(&mut words, &mut spans);
            let mut piece = span.clone();
            match &tokens[index].0 {
                Token::Link { url, label } => {
                    piece.text = if label.is_empty() { url.clone() } else { label.clone() };
                    piece.link = Some(url.clone());
                }
                Token::Mention(text) => {
                    piece.text = text.clone();
                    piece.bold = true;
                }
            }
            spans.push(piece);
        }
        flush(&mut words, &mut spans);
    }
    merge(spans)
}

/// Joins neighbouring spans that look the same (a "<" that wasn't a token
/// leaves the words around it in pieces).
fn merge(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans.into_iter().filter(|s| !s.text.is_empty()) {
        match out.last_mut() {
            Some(last) if last.same_look(&span) => last.text.push_str(&span.text),
            _ => out.push(span),
        }
    }
    out
}

/// Slack's emphasis: a marker counts when it opens at the start or after a
/// space or punctuation, is followed by a non-space, and closes on the same
/// line before a space, punctuation or the end. So "2*3*4" and
/// "snake_case_name" stay as written.
fn emphasis(text: &str, base: &Span) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, spans: &mut Vec<Span>| {
        if !plain.is_empty() {
            spans.push(Span { text: decode(&std::mem::take(plain)), ..base.clone() });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '*' | '_' | '~' | '`') && opens(&chars, i) {
            if let Some(end) = closing(&chars, i, c) {
                flush(&mut plain, &mut spans);
                let inner: String = chars[i + 1..end].iter().collect();
                let mut span = base.clone();
                match c {
                    '`' => {
                        span.code = true;
                        span.text = decode(&inner);
                        spans.push(span);
                    }
                    _ => {
                        match c {
                            '*' => span.bold = true,
                            '_' => span.italic = true,
                            _ => span.strike = true,
                        }
                        spans.extend(emphasis(&inner, &span));
                    }
                }
                i = end + 1;
                continue;
            }
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, &mut spans);
    spans
}

fn is_wordy(c: char) -> bool {
    c.is_alphanumeric()
}

fn opens(chars: &[char], i: usize) -> bool {
    if i + 1 >= chars.len() || chars[i + 1].is_whitespace() || chars[i + 1] == chars[i] {
        return false;
    }
    i == 0 || !is_wordy(chars[i - 1])
}

fn closing(chars: &[char], start: usize, marker: char) -> Option<usize> {
    (start + 2..chars.len()).find(|&j| chars[j] == marker && !chars[j - 1].is_whitespace() && (j + 1 == chars.len() || !is_wordy(chars[j + 1])))
}

#[cfg(test)]
mod tests {
    // The Mac's DraftBlockTests and the website's draft.test.ts check the
    // same cases.
    use super::*;

    fn span(text: &str) -> Span {
        Span::plain(text)
    }

    fn bold(text: &str) -> Span {
        Span { bold: true, ..span(text) }
    }

    fn joined(draft: &DraftBlock) -> Vec<String> {
        draft.lines().iter().map(|l| l.spans.iter().map(|s| s.text.as_str()).collect()).collect()
    }

    fn draft(kind: DraftKind, source: &str) -> DraftBlock {
        DraftBlock { kind, source: source.to_owned() }
    }

    #[test]
    fn only_the_draft_fences_count() {
        assert_eq!(DraftBlock::from(Some("slack"), "hi\n").map(|d| d.kind), Some(DraftKind::Slack));
        assert_eq!(DraftBlock::from(Some("Email subject=x"), "hi").map(|d| d.kind), Some(DraftKind::Email));
        assert_eq!(DraftBlock::from(Some("draft"), "hi").map(|d| d.kind), Some(DraftKind::Draft));
        assert_eq!(DraftBlock::from(Some("slack{.x}"), "hi").map(|d| d.kind), Some(DraftKind::Slack));
        assert!(DraftBlock::from(Some("slackbot"), "hi").is_none());
        assert!(DraftBlock::from(None, "hi").is_none());
        assert!(DraftBlock::from(Some("slack"), " \n\n").is_none());
        assert_eq!(DraftBlock::from(Some("slack"), "a\r\nb\n\n").map(|d| d.source), Some("a\nb".to_owned()));
        assert_eq!(draft(DraftKind::Slack, "x").label(), "Slack draft");
        assert_eq!(draft(DraftKind::Email, "x").label(), "Email draft");
        assert_eq!(draft(DraftKind::Draft, "x").label(), "Draft");
        assert!(DraftBlock::may_hold("Here:\n\n```slack\nhi\n```"));
        assert!(!DraftBlock::may_hold("```rust\nfn main() {}\n```"));
        assert!(!DraftBlock::may_hold("a slack message"));
    }

    #[test]
    fn every_line_stays_a_line() {
        let d = draft(DraftKind::Draft, "*Threadbear, 6 Nov*\n• one\n• two\n\nThanks");
        assert_eq!(joined(&d), ["*Threadbear, 6 Nov*", "• one", "• two", "", "Thanks"]);
        // Only a Slack draft reads Slack's formatting.
        assert!(!d.lines()[0].spans[0].bold);
        assert_eq!(d.plain_text(), d.source);
    }

    #[test]
    fn slack_formatting() {
        assert_eq!(
            slack_spans("*Shipped* _today_: ~old~ `tb land` and *_both_*"),
            vec![
                bold("Shipped"),
                span(" "),
                Span { italic: true, ..span("today") },
                span(": "),
                Span { strike: true, ..span("old") },
                span(" "),
                Span { code: true, ..span("tb land") },
                span(" and "),
                Span { bold: true, italic: true, ..span("both") },
            ]
        );
    }

    #[test]
    fn markers_inside_words_and_lone_ones_stay_as_written() {
        for text in ["2*3*4", "snake_case_name", "a * b * c", "*open only", "price: $5_000_"] {
            assert_eq!(slack_spans(text), vec![span(text)], "{text}");
        }
        // Markdown's **bold** is Slack's *bold* with a star either side,
        // which is how Slack shows it too.
        assert_eq!(slack_spans("**x**"), vec![span("*"), bold("x"), span("*")]);
    }

    #[test]
    fn links_and_mentions() {
        let spans = slack_spans("See <https://example.com/a?b=1|the docs>, <https://x.dev> or <mailto:m@x.dev>; cc <@U123> in <#C9|general> <!here> & 1 < 2 &amp; <b>");
        let links: Vec<&str> = spans.iter().filter_map(|s| s.link.as_deref()).collect();
        assert_eq!(links, ["https://example.com/a?b=1", "https://x.dev", "mailto:m@x.dev"]);
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "See the docs, https://x.dev or m@x.dev; cc @U123 in #general @here & 1 < 2 & <b>");
        // Other schemes aren't links.
        assert!(slack_spans("<javascript:alert(1)|x>")[0].link.is_none());
        let text: String = slack_spans("<javascript:alert(1)|x>").iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "<javascript:alert(1)|x>");
        // A mention is bold; <!subteam^S1> names the group.
        assert_eq!(slack_spans("<!subteam^S1>"), vec![bold("@subteam")]);
        assert_eq!(slack_spans("<https://>"), vec![span("<https://>")]);
    }

    #[test]
    fn copy_keeps_slacks_markers_but_spells_out_links() {
        let d = draft(DraftKind::Slack, "*Done* <https://x.dev/pr/1|the PR>\n<https://x.dev>\n> quoted &lt;tag&gt;");
        assert_eq!(d.plain_text(), "*Done* the PR (https://x.dev/pr/1)\nhttps://x.dev\n> quoted <tag>");
        assert_eq!(d.html(), "<meta charset=\"utf-8\"><div><b>Done</b> <a href=\"https://x.dev/pr/1\">the PR</a><br><a href=\"https://x.dev\">https://x.dev</a><blockquote>quoted &lt;tag&gt;</blockquote></div>");
        assert!(d.lines()[2].quote);
        assert_eq!(d.lines()[2].shown(), "\u{2502} quoted <tag>");
    }

    #[test]
    fn html_is_escaped() {
        let d = draft(DraftKind::Email, "Hi <script>alert(\"x\")</script> & bye");
        assert_eq!(d.html(), "<meta charset=\"utf-8\"><div>Hi &lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; bye</div>");
    }

    #[test]
    fn bare_addresses_in_an_email_or_plain_draft_are_links_when_drawn() {
        let link = |t: &str| Span { link: Some(t.to_owned()), ..span(t) };
        assert_eq!(
            bare_links("See https://x.dev/pr/1. Or http://a.b/c?d=1), ftp://no"),
            vec![span("See "), link("https://x.dev/pr/1"), span(". Or "), link("http://a.b/c?d=1"), span("), ftp://no")]
        );
        assert_eq!(bare_links("no links here"), vec![span("no links here")]);
        assert_eq!(bare_links(""), Vec::<Span>::new());
        assert_eq!(bare_links("just https:// alone"), vec![span("just https:// alone")]);
        // Copy keeps the text as written.
        let d = draft(DraftKind::Email, "See https://x.dev");
        assert_eq!(d.plain_text(), "See https://x.dev");
        assert_eq!(d.html(), "<meta charset=\"utf-8\"><div>See https://x.dev</div>");
        assert_eq!(d.drawn_lines()[0].spans, vec![span("See "), link("https://x.dev")]);
        // A Slack draft's own links are its tokens, not bare words.
        assert_eq!(draft(DraftKind::Slack, "see https://x.dev").drawn_lines()[0].spans, vec![span("see https://x.dev")]);
    }

    // Cross-lab review, 6 Nov.
    #[test]
    fn tokens_inside_code_stay_as_written_and_a_link_can_be_bold() {
        let code = |t: &str| Span { code: true, ..span(t) };
        assert_eq!(slack_spans("`<@U123>` and `a < b`"), vec![code("<@U123>"), span(" and "), code("a < b")]);
        let url = Some("https://x.dev".to_owned());
        assert_eq!(slack_spans("*See <https://x.dev|docs>*"), vec![bold("See "), Span { link: url.clone(), ..bold("docs") }]);
        assert_eq!(slack_spans("<https://x.dev|a>_b_"), vec![Span { link: url, ..span("a") }, Span { italic: true, ..span("b") }]);
        // Copied as plain text, code keeps its tokens as typed too.
        assert_eq!(draft(DraftKind::Slack, "`<@U1>` <@U1> `<https://x.dev|docs>` <https://x.dev|docs>").plain_text(), "`<@U1>` @U1 `<https://x.dev|docs>` docs (https://x.dev)");
    }

    #[test]
    fn hostile_lines_stay_quick() {
        let started = std::time::Instant::now();
        slack_spans(&"<".repeat(MAX_FORMATTED_LINE));
        slack_spans(&" *a".repeat(MAX_FORMATTED_LINE / 3));
        draft(DraftKind::Slack, &"<".repeat(MAX_SOURCE)).plain_text();
        draft(DraftKind::Slack, &format!("{}>", "<".repeat(MAX_SOURCE - 1))).plain_text();
        bare_links(&"https://".repeat(MAX_SOURCE / 8));
        assert!(started.elapsed().as_secs_f64() < 2., "took {:?}", started.elapsed());
        // A line past the limit is shown as written; a draft past its limit stays code.
        let long = format!("*{}*", "a".repeat(MAX_FORMATTED_LINE));
        assert_eq!(slack_spans(&long), vec![span(&long)]);
        assert!(DraftBlock::from(Some("slack"), &"a".repeat(MAX_SOURCE + 1)).is_none());
        // A placeholder character in the text itself turns formatting off for the line.
        assert_eq!(slack_spans("*a* \u{E000}"), vec![span("*a* \u{E000}")]);
    }

    #[test]
    fn copied_html_keeps_indents_and_lined_up_spaces() {
        let d = draft(DraftKind::Email, "    • nested\nA    B");
        assert_eq!(d.html(), "<meta charset=\"utf-8\"><div>&nbsp;&nbsp;&nbsp;&nbsp;• nested<br>A &nbsp;&nbsp;&nbsp;B</div>");
    }

    #[test]
    fn only_a_closed_fence_makes_a_draft() {
        // The block starts at the fence, after any container, as the parser
        // places a fence in a list or a quote.
        let closed = |text: &str| {
            let start = text.find("```").into_iter().chain(text.find("~~~")).min().unwrap_or(0);
            fence_closed(text, start, text.len())
        };
        for text in [
            "```slack\nhello\n```",
            "````slack\nhello\n`````",
            "~~~email\nhi\n~~~  ",
            "```slack\r\nhello\r\n```",
            "```slack\r\nhello\r\n```\r\n",
            "- ```slack\n  hello\n  ```",
            "> ```slack\n> hello\n> ```",
            "1. a\n\n   > ```slack\n   > hi\n   > ```",
            "```slack\nhello\n   ```",
            "  ```slack\n  hello\n```",
            "1. ```slack\n   hello\n   ```",
            ">```slack\n>hello\n>```",
            "> - ```slack\n>   héllo\n>   ```",
            "- parent\n  - child\n\n    ```slack\n    hello\n    ```",
            "```slack\none\u{2028}two\n```",
            // Indent isn't checked, so this one shows its box a line early: harmless.
            "```slack\nhello\n    ```",
        ] {
            assert!(closed(text), "{text:?} should be closed");
        }
        for text in ["```slack\nhello\n> ```", "> ```slack\n> hello\n```", "```slack\n*unfinished", "```slack\n• one\n• tw", "```slack\nhello\n~~~", "````slack\nhello\n```", "```slack", "```slack\nhello\n``"] {
            assert!(!closed(text), "{text:?} shouldn't be closed");
        }
        // Offsets pick the block out of the whole reply.
        let reply = "Here:\n\n```slack\nhi\n```\n\nThanks";
        let start = reply.find("```").unwrap();
        assert!(fence_closed(reply, start, reply[start + 3..].find("```").unwrap() + start + 6));
        assert!(!fence_closed(reply, start, start + 5));
        assert!(!fence_closed(reply, 99, 120));
    }

    #[test]
    fn a_dump_describes_what_each_span_looks_like() {
        let spans = slack_spans("*Hi* <https://x.dev|there> `x`");
        let described: Vec<String> = spans.iter().filter_map(Span::described).collect();
        assert_eq!(described, ["bold: Hi", "link: there <https://x.dev>", "code: x"]);
    }
}
