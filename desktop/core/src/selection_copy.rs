// What selected words in a reply become when they leave it: the HTML a copy
// puts beside the plain text (the Swift app's SelectionHTML), and the quote
// the selection bar's Quote puts in the composer (`quoteInComposer`).
//
// Why HTML: the plain text loses a reply's links, and Slack, Chrome and most
// web apps only read links and formatting from HTML, so a pasted reply's
// "Fireside Chat shorts" arrived as bare words. Only links that mean
// something off this computer are kept (a file path or one of Threadbear's
// own links pasted into Slack would be dead), and no colours or fonts, so
// the dark theme's white text never pastes into a white document.
//
// The GPUI app hands this the selection as the kit gives it in Markdown (its
// `SelectionFormat::Source`), which keeps the links, bold, italic and code
// the plain text loses.
use markdown::mdast::Node;

/// The tags a piece of the selection is wrapped in, outermost first, as
/// SelectionHTML has them: a link, then code, or bold and italic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tags {
    link: Option<String>,
    code: bool,
    bold: bool,
    italic: bool,
}

impl Tags {
    fn open_close(&self) -> (String, String) {
        let mut open = String::new();
        let mut close: Vec<&str> = Vec::new();
        if let Some(url) = &self.link {
            open.push_str(&format!("<a href=\"{}\">", escape(url)));
            close.push("</a>");
        }
        if self.code {
            open.push_str("<code>");
            close.push("</code>");
        } else {
            if self.bold {
                open.push_str("<b>");
                close.push("</b>");
            }
            if self.italic {
                open.push_str("<i>");
                close.push("</i>");
            }
        }
        close.reverse();
        (open, close.concat())
    }
}

/// The selection as an HTML fragment for the clipboard, from its Markdown.
pub fn selection_html(markdown_source: &str) -> String {
    let mut spans: Vec<(Tags, String)> = Vec::new();
    if let Ok(tree) = markdown::to_mdast(markdown_source, &markdown::ParseOptions::gfm()) {
        let mut out = Spans { spans: &mut spans, started: false };
        walk(&tree, &Tags::default(), &mut out);
    }
    let body: String = spans
        .iter()
        .map(|(tags, text)| {
            let (open, close) = tags.open_close();
            // Each line wraps on its own, so a tag never spans a <br>.
            text.split('\n').map(|line| if line.is_empty() { String::new() } else { format!("{open}{}{close}", escape(line)) }).collect::<Vec<_>>().join("<br>")
        })
        .collect();
    format!("<meta charset=\"utf-8\">{body}")
}

struct Spans<'a> {
    spans: &'a mut Vec<(Tags, String)>,
    /// Something's been written, so the next block starts on a new line.
    started: bool,
}

impl Spans<'_> {
    /// Neighbours with the same tags join up, or an inline code split by
    /// the parser copies as two codes.
    fn push(&mut self, tags: &Tags, text: &str) {
        if text.is_empty() {
            return;
        }
        self.started = true;
        match self.spans.last_mut() {
            Some((last, words)) if last == tags => words.push_str(text),
            _ => self.spans.push((tags.clone(), text.to_owned())),
        }
    }

    fn new_line(&mut self) {
        if self.started {
            self.push(&Tags::default(), "\n");
        }
    }
}

fn walk(node: &Node, tags: &Tags, out: &mut Spans) {
    match node {
        Node::Text(text) => out.push(tags, &text.value),
        Node::InlineCode(code) => out.push(&Tags { code: true, ..tags.clone() }, &code.value),
        Node::InlineMath(math) => out.push(tags, &math.value),
        Node::Break(_) => out.push(&Tags::default(), "\n"),
        Node::Strong(_) => children(node, &Tags { bold: true, ..tags.clone() }, out),
        Node::Emphasis(_) => children(node, &Tags { italic: true, ..tags.clone() }, out),
        Node::Link(link) => {
            let tags = if web_link(&link.url) { Tags { link: Some(link.url.clone()), ..tags.clone() } } else { tags.clone() };
            children(node, &tags, out);
        }
        // A picture and Threadbear's own raw HTML have no words to copy.
        Node::Image(_) | Node::ImageReference(_) | Node::Definition(_) | Node::FootnoteDefinition(_) | Node::FootnoteReference(_) | Node::ThematicBreak(_) => {}
        Node::Html(html) => out.push(tags, &html.value),
        Node::Heading(_) => {
            out.new_line();
            children(node, &Tags { bold: true, ..tags.clone() }, out);
        }
        Node::Paragraph(_) => {
            out.new_line();
            children(node, tags, out);
        }
        Node::Code(code) => {
            out.new_line();
            out.push(&Tags { code: true, ..tags.clone() }, &code.value);
        }
        Node::Math(math) => {
            out.new_line();
            out.push(tags, &math.value);
        }
        Node::List(list) => {
            let mut number = list.start.unwrap_or(1);
            for item in &list.children {
                out.new_line();
                // The marker, as the reply draws it, and the item's words
                // on the same line.
                out.push(&Tags::default(), &if list.ordered { format!("{number}. ") } else { "• ".to_owned() });
                number += 1;
                let mut first = true;
                for child in item.children().into_iter().flatten() {
                    if first && matches!(child, Node::Paragraph(_)) {
                        children(child, tags, out);
                    } else {
                        walk(child, tags, out);
                    }
                    first = false;
                }
            }
        }
        Node::Table(table) => {
            for row in &table.children {
                out.new_line();
                for (i, cell) in row.children().into_iter().flatten().enumerate() {
                    if i > 0 {
                        out.push(&Tags::default(), "\t");
                    }
                    children(cell, tags, out);
                }
            }
        }
        _ => children(node, tags, out),
    }
}

fn children(node: &Node, tags: &Tags, out: &mut Spans) {
    for child in node.children().into_iter().flatten() {
        walk(child, tags, out);
    }
}

/// Only links that mean something off this computer.
fn web_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---------------------------------------------------------------- quoting

/// The words as a Markdown quote: each line after "> ", blank ones too.
pub fn quote(selection: &str) -> String {
    let words = selection.trim();
    if words.is_empty() {
        return String::new();
    }
    words.split('\n').map(|line| format!("> {line}")).collect::<Vec<_>>().join("\n")
}

/// The thread's draft with the quote at its end, ready for Mike to say what
/// about it: under what's there after a blank line, then a blank line for
/// his words. None when there's nothing to quote.
pub fn quoted_draft(draft: &str, selection: &str) -> Option<String> {
    let (keep, add) = quote_into(draft, selection)?;
    Some(format!("{}{add}", &draft[..keep]))
}

/// The same as an edit: how much of the draft stays (its trailing newlines
/// go, or all of it when it's blank) and what goes after it. What stays
/// keeps its place, so chips in it stay where they are.
pub fn quote_into(draft: &str, selection: &str) -> Option<(usize, String)> {
    let quote = quote(selection);
    if quote.is_empty() {
        return None;
    }
    if draft.trim().is_empty() {
        return Some((0, format!("{quote}\n\n")));
    }
    Some((draft.trim_end_matches('\n').len(), format!("\n\n{quote}\n\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_bold_and_code_are_kept_and_nothing_else() {
        assert_eq!(selection_html("See [the PR](https://github.com/a/b/pull/1) and **ship** it."), r#"<meta charset="utf-8">See <a href="https://github.com/a/b/pull/1">the PR</a> and <b>ship</b> it."#);
        assert_eq!(selection_html("Run `tb land` *now*"), r#"<meta charset="utf-8">Run <code>tb land</code> <i>now</i>"#);
        // A bold link: the link outside, bold within.
        assert_eq!(selection_html("[**big**](https://x.dev)"), r#"<meta charset="utf-8"><a href="https://x.dev"><b>big</b></a>"#);
        // Code is code, whatever it's in.
        assert_eq!(selection_html("**`a`**"), r#"<meta charset="utf-8"><code>a</code>"#);
    }

    #[test]
    fn links_that_mean_nothing_off_this_computer_are_their_words() {
        assert_eq!(selection_html("[`src/a.rs`](tbpath:?p=src/a.rs) and [tests ↑](work:npm%20test) and [f](file:///tmp/a.md)"), r#"<meta charset="utf-8"><code>src/a.rs</code> and tests ↑ and f"#);
        assert_eq!(selection_html("[mail me](mailto:a@b.c)"), r#"<meta charset="utf-8"><a href="mailto:a@b.c">mail me</a>"#);
    }

    #[test]
    fn blocks_are_lines_and_a_tag_never_spans_one() {
        assert_eq!(selection_html("One\n\nTwo"), r#"<meta charset="utf-8">One<br>Two"#);
        assert_eq!(selection_html("# Title\n\n- a\n- **b**\n\n1. c"), r#"<meta charset="utf-8"><b>Title</b><br>• a<br>• <b>b</b><br>1. c"#);
        assert_eq!(selection_html("```\nlet a = 1;\nlet b = 2;\n```"), r#"<meta charset="utf-8"><code>let a = 1;</code><br><code>let b = 2;</code>"#);
        assert_eq!(selection_html("a  \nb"), r#"<meta charset="utf-8">a<br>b"#);
    }

    #[test]
    fn what_it_says_is_escaped() {
        assert_eq!(selection_html(r#"5 < 6 & "x""#), r#"<meta charset="utf-8">5 &lt; 6 &amp; &quot;x&quot;"#);
        assert_eq!(selection_html("[a](https://x.dev/?a=1&b=\"2\")"), r#"<meta charset="utf-8"><a href="https://x.dev/?a=1&amp;b=&quot;2&quot;">a</a>"#);
        assert_eq!(selection_html(""), r#"<meta charset="utf-8">"#);
    }

    #[test]
    fn a_quote_goes_under_the_draft_with_room_after_it() {
        assert_eq!(quote("  one\n\ntwo \n"), "> one\n> \n> two");
        assert_eq!(quoted_draft("", "the words").as_deref(), Some("> the words\n\n"));
        assert_eq!(quoted_draft("  \n", "the words").as_deref(), Some("> the words\n\n"));
        assert_eq!(quoted_draft("About this:\n\n\n", "a\nb").as_deref(), Some("About this:\n\n> a\n> b\n\n"));
        assert_eq!(quoted_draft("Hi", "   "), None);
        // As an edit: what's kept stays put, so a chip in it does too.
        assert_eq!(quote_into("Look \u{FFFC} here\n", "x"), Some(("Look \u{FFFC} here".len(), "\n\n> x\n\n".to_owned())));
        assert_eq!(quote_into(" \n", "x"), Some((0, "> x\n\n".to_owned())));
    }
}
