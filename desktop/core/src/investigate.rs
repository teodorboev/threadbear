//! The error bar's Investigate button: the brief for a new thread in the
//! app's own checkout that finds why an error came up and fixes it, as the
//! Swift app's `InvestigateText` writes it. The bar's text alone rarely says
//! enough, so it carries what the app knew when he pressed it, and says it
//! was this app (the GPUI one, in desktop/), so the thread looks there.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub message: String,
    /// When, as the local time it'd be read in ("2026-11-11T15:20:00+08:00").
    pub at: String,
    /// The thread it came up in: the one open.
    pub thread_id: Option<String>,
    pub thread_title: Option<String>,
    pub thread_folder: Option<String>,
    pub app_version: Option<String>,
    pub runner_version: Option<String>,
}

/// Now, as `Report::at` has it.
pub fn now_text() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// The new thread's title: the error's first words.
pub fn title(r: &Report) -> String {
    let flat = r.message.lines().collect::<Vec<_>>().join(" ");
    let short = if flat.chars().count() > 50 { format!("{}…", flat.chars().take(49).collect::<String>().trim_end()) } else { flat };
    format!("Error: {short}")
}

pub fn prompt(r: &Report) -> String {
    let mut lines = vec![
        "The Threadbear desktop app (the GPUI one, in desktop/) showed this in its error bar:".to_owned(),
        String::new(),
        format!("> {}", r.message.replace('\n', "\n> ")),
        String::new(),
        format!("- When: {}", r.at),
    ];
    match &r.thread_id {
        Some(id) => {
            let mut thread = format!("- In thread {id}");
            if let Some(title) = r.thread_title.as_deref().filter(|t| !t.is_empty()) {
                thread += &format!(" (\"{title}\")");
            }
            if let Some(folder) = r.thread_folder.as_deref().filter(|f| !f.is_empty()) {
                thread += &format!(", folder {folder}");
            }
            lines.push(thread);
        }
        None => lines.push("- No thread was open".to_owned()),
    }
    if let Some(version) = r.app_version.as_deref().filter(|v| !v.is_empty()) {
        lines.push(format!("- The app {version}"));
    }
    if let Some(runner) = r.runner_version.as_deref().filter(|v| !v.is_empty()) {
        lines.push(format!("- Runner {runner}"));
    }
    lines.push(String::new());
    lines.push("Find the cause in this checkout and fix it. Start from where the app sets that text (`error_message`, in desktop/gpui/src), follow it to whatever failed, and check `tb logs` around that time. Test first, then land it as AGENTS.md says. If it turns out not to be a bug (something the user has to do, or another service), say what and why instead of changing code.".to_owned());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brief_says_what_the_app_knew() {
        let r = Report {
            message: "Couldn't trust poddle:\nThread not found".into(),
            at: "2026-11-11T15:20:00+08:00".into(),
            thread_id: Some("t1".into()),
            thread_title: Some("Poddle".into()),
            thread_folder: Some("/Users/x/dev/poddle".into()),
            app_version: Some("0.1.0".into()),
            runner_version: Some("0.1.0+20261111".into()),
        };
        assert_eq!(title(&r), "Error: Couldn't trust poddle: Thread not found");
        let p = prompt(&r);
        assert!(p.contains("> Couldn't trust poddle:\n> Thread not found"), "{p}");
        assert!(p.contains("- In thread t1 (\"Poddle\"), folder /Users/x/dev/poddle"), "{p}");
        assert!(p.contains("- The app 0.1.0\n- Runner 0.1.0+20261111"), "{p}");
        assert!(p.contains("desktop/gpui/src"), "it points at this app's code: {p}");
        let none = Report { message: "x".into(), ..Default::default() };
        assert!(prompt(&none).contains("- No thread was open"));
    }

    #[test]
    fn a_long_error_is_cut_for_the_title() {
        let r = Report { message: "a".repeat(80), ..Default::default() };
        assert_eq!(title(&r), format!("Error: {}…", "a".repeat(49)));
    }
}
