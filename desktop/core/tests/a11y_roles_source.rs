// Every element in the GPUI app with an accessibility label says what kind of
// thing it is. GPUI makes an accessibility node only for an element with a
// role other than a plain container (gpui-pre's `Div::a11y_role`), so a
// custom Send button or sidebar row with `.aria_label` and no `.role` was
// invisible to VoiceOver, where the Swift app's native controls were buttons,
// rows and checkboxes (parity re-check, 13 Nov 2026).
//
// The rules, checked on the source: a builder chain that calls `.aria_label(`
// also calls `.role(` (at the chain's own level, not inside a child), and so
// does a custom element built from `div()`, `h_flex()` or `v_flex()` that
// takes a click. A wrapper that only names a kit control for the app's
// scripts, or a backdrop a click dismisses, says so with
// `.a11y_role(Role::GenericContainer)` (gpui/src/a11y.rs): still no node, as what's inside is what
// VoiceOver reads.
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The source with comments, strings and char literals blanked to spaces
/// (lengths kept, so offsets still line up), and everything after a file's
/// `#[cfg(test)]` module dropped.
fn code_only(text: &str) -> String {
    // Windows' checkout has CRLF line ends (git's autocrlf), which would hide
    // `#[cfg(test)]\nmod` and count a unit test's elements as the app's.
    let text = text.replace("\r\n", "\n");
    let b = text.as_bytes();
    let mut out = vec![b' '; b.len()];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            out[i] = b'\n';
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                if b[i] == b'\n' {
                    out[i] = b'\n';
                }
                i += 1;
            }
            i += 2;
        } else if c == b'r' && (b.get(i + 1) == Some(&b'"') || b.get(i + 1) == Some(&b'#')) && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) {
            // A raw string: r"..." or r#"..."#.
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) != Some(&b'"') {
                out[i] = c;
                i += 1;
                continue;
            }
            j += 1;
            loop {
                if j >= b.len() {
                    break;
                }
                if b[j] == b'"' && b[j + 1..].iter().take(hashes).filter(|&&h| h == b'#').count() == hashes {
                    j += 1 + hashes;
                    break;
                }
                if b[j] == b'\n' {
                    out[j] = b'\n';
                }
                j += 1;
            }
            out[i] = b'"';
            out[j - 1] = b'"';
            i = j;
        } else if c == b'"' {
            out[i] = b'"';
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                }
                if i < b.len() && b[i] == b'\n' {
                    out[i] = b'\n';
                }
                i += 1;
            }
            if i < b.len() {
                out[i] = b'"';
            }
            i += 1;
        } else if c == b'\'' {
            // A char literal ('(' or '\n'), not a lifetime ('a).
            if b.get(i + 1) == Some(&b'\\') {
                let mut j = i + 2;
                while j < b.len() && b[j] != b'\'' {
                    j += 1;
                }
                i = j + 1;
            } else if b.get(i + 2) == Some(&b'\'') {
                i += 3;
            } else {
                out[i] = c;
                i += 1;
            }
        } else {
            out[i] = c;
            i += 1;
        }
    }
    let code = String::from_utf8(out).unwrap();
    match code.find("#[cfg(test)]\nmod ").or_else(|| code.find("#[cfg(all(test")) {
        Some(at) => code[..at].to_owned(),
        None => code,
    }
}

fn is_open(c: u8) -> bool {
    matches!(c, b'(' | b'[' | b'{')
}
fn is_close(c: u8) -> bool {
    matches!(c, b')' | b']' | b'}')
}

/// Whether the `=` at `i` is an assignment (not `==`, `=>`, `<=`, `>=`, `!=`).
fn assignment(b: &[u8], i: usize) -> bool {
    b[i] == b'=' && b.get(i + 1) != Some(&b'=') && b.get(i + 1) != Some(&b'>') && (i == 0 || !matches!(b[i - 1], b'=' | b'<' | b'>' | b'!'))
}

/// The builder chain around offset `at`, with what's inside its own
/// brackets (children, closures, arguments) dropped: from where the
/// expression starts to where it ends.
fn chain_at(code: &str, at: usize) -> String {
    let b = code.as_bytes();
    let mut start = at;
    let mut depth = 0i32;
    while start > 0 {
        let c = b[start - 1];
        if is_close(c) {
            depth += 1;
        } else if is_open(c) {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 && (c == b';' || c == b',' || assignment(b, start - 1)) {
            break;
        }
        start -= 1;
    }
    let mut end = at;
    depth = 0;
    while end < b.len() {
        let c = b[end];
        if is_open(c) {
            depth += 1;
        } else if is_close(c) {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 && (c == b';' || c == b',') {
            break;
        }
        end += 1;
    }
    let mut flat = String::new();
    depth = 0;
    for &c in &b[start..end] {
        if is_close(c) {
            depth -= 1;
        }
        if depth == 0 {
            flat.push(c as char);
        }
        if is_open(c) {
            depth += 1;
        }
    }
    flat
}

/// A chain that says its role: `.role(`, or the app's `a11y_role` (gpui/src/a11y.rs),
/// which takes `Role::GenericContainer` without gpui-pre's debug assertion.
fn has_role(chain: &str) -> bool {
    chain.contains(".role(") || chain.contains("a11y_role(")
}

/// Each `.aria_label(`, and each click on a custom element, whose chain
/// sets no `.role(`, as `file:line`.
fn unroled(code: &str, rel: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices(".aria_label(") {
        let chain = chain_at(code, at);
        if !has_role(&chain) {
            let line = code[..at].matches('\n').count() + 1;
            found.push(format!("{rel}:{line}"));
        }
    }
    for (at, _) in code.match_indices(".on_click(") {
        let chain = chain_at(code, at);
        let custom = ["div()", "h_flex()", "v_flex()"].iter().any(|start| chain.trim_start().starts_with(start));
        if custom && !has_role(&chain) {
            let line = code[..at].matches('\n').count() + 1;
            found.push(format!("{rel}:{line} (a click)"));
        }
    }
    found
}

#[test]
fn every_labelled_element_has_a_role() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    let mut files = Vec::new();
    rust_files(&desktop.join("gpui/src"), &mut files);
    assert!(files.iter().any(|f| f.ends_with("gpui/src/composer.rs")), "the composer wasn't found, so this test isn't looking in the right place");
    let mut found = Vec::new();
    let mut labels = 0;
    for file in &files {
        let rel = file.strip_prefix(&desktop).unwrap().to_string_lossy().replace('\\', "/");
        let code = code_only(&std::fs::read_to_string(file).unwrap());
        labels += code.matches(".aria_label(").count();
        found.extend(unroled(&code, &rel));
    }
    assert!(labels > 50, "only {labels} labels found, so this test isn't reading the app's controls");
    assert!(
        found.is_empty(),
        "these have an accessibility label, or take a click, but have no role, so VoiceOver never sees them. Give each `.role(Role::Button)` (or ListItem, CheckBox, Tab...) and the state Swift's control has (`.aria_selected`, `.aria_toggled`, `.aria_expanded`); a wrapper naming a kit control for scripts gets `.a11y_role(Role::GenericContainer)`, never `.role(` with it, which panics in a debug build:\n{}",
        found.join("\n")
    );
}

/// gpui-pre's `role` asserts it's never handed a plain container, so a debug
/// or scripted app panicked as soon as one of these drew (13 Nov): every app
/// spec with a done turn's footer, a banner or the Terminal's note.
#[test]
fn no_plain_container_goes_through_gpuis_own_role() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    let mut files = Vec::new();
    rust_files(&desktop.join("gpui/src"), &mut files);
    let mut found = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&desktop).unwrap().to_string_lossy().replace('\\', "/");
        let code = std::fs::read_to_string(file).unwrap();
        for (n, line) in code.lines().enumerate() {
            let squashed: String = line.split_whitespace().collect();
            if squashed.contains(".role(Role::GenericContainer)") || squashed.contains(".role(gpui_kit::Role::GenericContainer)") || squashed.contains(".role($role)") {
                found.push(format!("{rel}:{}", n + 1));
            }
        }
    }
    assert!(found.is_empty(), "use `.a11y_role(` (gpui/src/a11y.rs) for these, which leaves a plain container out rather than panicking:\n{}", found.join("\n"));
}

#[test]
fn a_test_module_is_left_out_with_either_line_end() {
    let source = "fn a() { div().id(\"x\").role(Role::Button).aria_label(\"X\"); }\n#[cfg(test)]\nmod tests {\n    fn b() { div().id(\"y\").aria_label(\"Y\"); }\n}\n";
    assert!(unroled(&code_only(source), "f.rs").is_empty());
    assert!(unroled(&code_only(&source.replace('\n', "\r\n")), "f.rs").is_empty());
}

#[test]
fn the_scanner_reads_chains() {
    let code = code_only(
        r#"
fn a() {
    let send = div().id("send").role(Role::Button).aria_label("Send").on_click(|_, _, _| {});
    let row = div().id("row").aria_label("Row").child(div().role(Role::Button));
    let s = "x.aria_label(";
    // .aria_label( in a comment
    let ch = '(';
    tipped(div().aria_label(t).role(Role::Button), "tip");
    let close = div().id("x").on_click(|_, _, _| {});
    let kit = Button::new("b").on_click(|_, _, _| {});
}
"#,
    );
    assert_eq!(unroled(&code, "f.rs"), vec!["f.rs:4".to_owned(), "f.rs:9 (a click)".to_owned()]);
}
