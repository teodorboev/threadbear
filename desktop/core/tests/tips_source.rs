// Every control's tooltip in the GPUI app goes through tips.rs (`Tip::tip`, or
// highlights.rs's `tipped`, which uses it), as TipsTests keeps the Swift app on
// `.tip`: the activity log names a click by the tooltip of what's under the
// pointer, and one made another way names nothing (desktop/gpui/src/tips.rs).
// A kit component's own `.tooltip("words")` (a Button, a Switch) is the kit's,
// and stays.
use std::path::{Path, PathBuf};

/// What makes a tooltip without the helper.
const OTHER_WAYS: &[&str] = &["Tooltip::new(", ".tooltip(|", ".tooltip(move |", ".hoverable_tooltip("];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_tooltip_goes_through_the_tip_helper() {
    let gpui = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("gpui/src");
    let mut files = Vec::new();
    rust_files(&gpui, &mut files);
    assert!(files.iter().any(|f| f.ends_with("tips.rs")), "tips.rs wasn't found, so this test isn't looking in the right place");
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.ends_with("tips.rs")) {
        let text = std::fs::read_to_string(file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let squeezed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if OTHER_WAYS.iter().any(|w| squeezed.contains(w) || line.contains(w)) {
                found.push(format!("{}:{}: {}", file.strip_prefix(&gpui).unwrap().display(), n + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "give these their tooltip with `.tip(...)` (tips.rs) or highlights.rs's `tipped(...)`, so the activity log can name a click on them:\n{}", found.join("\n"));
}
