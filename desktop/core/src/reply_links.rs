// What a link in a reply does, with no UI in it: the Swift app's FileLinks
// (what a click on a file or web link does, and the right-click menus that
// also set it), ChromeProfiles (a web link's "Open in Chrome: <profile>"
// items) and LinkPeekCard's words (the card that shows after the pointer
// rests on a link). The GPUI app draws and acts on these (gpui/src/reply_links.rs),
// everything it opens going through its open gate.
use crate::highlights::{self, text::address};
use serde_json::Value;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- file links

/// What a file link can do, from its right-click menu; the menu's Default
/// also sets which one a plain click does (`FileLinkAction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FileLinkAction {
    /// Docs and pictures in the pane, anything else (code) in its app: how a
    /// click worked before there was a choice.
    #[default]
    Automatic,
    Panel,
    App,
    Finder,
}

impl FileLinkAction {
    /// The ways to open a file, in the menu's order.
    pub const OPEN: [FileLinkAction; 3] = [FileLinkAction::Panel, FileLinkAction::App, FileLinkAction::Finder];
    /// The choices under Default.
    pub const DEFAULTS: [FileLinkAction; 4] = [FileLinkAction::Automatic, FileLinkAction::Panel, FileLinkAction::App, FileLinkAction::Finder];

    /// The name it's saved under: the Swift app's raw values, so the two
    /// read each other's choice the same way.
    pub fn key(self) -> &'static str {
        match self {
            FileLinkAction::Automatic => "automatic",
            FileLinkAction::Panel => "panel",
            FileLinkAction::App => "app",
            FileLinkAction::Finder => "finder",
        }
    }

    pub fn from_key(key: &str) -> Option<FileLinkAction> {
        FileLinkAction::DEFAULTS.into_iter().find(|a| a.key() == key)
    }

    pub fn symbol(self) -> &'static str {
        match self {
            FileLinkAction::Automatic => "wand.and.stars",
            FileLinkAction::Panel => "sidebar.right",
            FileLinkAction::App => "arrow.up.forward.app",
            FileLinkAction::Finder => "folder",
        }
    }

    /// Its menu item's words. `app` is the app the system opens this file
    /// in ("Xcode"), for "Open in Xcode".
    pub fn title(self, app: Option<&str>) -> String {
        match self {
            FileLinkAction::Automatic => "Panel for Docs and Pictures, App for the Rest".to_owned(),
            FileLinkAction::Panel => "Open in Panel".to_owned(),
            FileLinkAction::App => app.map_or_else(|| "Open in Default App".to_owned(), |name| format!("Open in {name}")),
            FileLinkAction::Finder => finder_title().to_owned(),
        }
    }

    /// What a click on a file link does: ⌘ (Control elsewhere) always means
    /// its app, and so does a project file on this computer with an app to
    /// open it (a copy of another Mac's has none of its media); otherwise
    /// what Default says (`FileLinkAction.forClick`).
    pub fn for_click(command_key: bool, remote: bool, opens_in_its_app: bool, chosen: FileLinkAction) -> FileLinkAction {
        if command_key || (!remote && opens_in_its_app) {
            return FileLinkAction::App;
        }
        chosen
    }

    /// Automatic made definite for one file: the kinds Mike reviews rather
    /// than edits (docs, pictures, pages) and folders in the pane, the rest
    /// in its app (`openTranscriptFile`).
    pub fn resolved(self, path: &str, is_folder: bool) -> FileLinkAction {
        match self {
            FileLinkAction::Automatic if is_folder || highlights::previews_in_pane(path) => FileLinkAction::Panel,
            FileLinkAction::Automatic => FileLinkAction::App,
            other => other,
        }
    }
}

/// "Show in Finder" on a Mac; the system's file browser elsewhere.
pub fn finder_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else {
        "Show in Folder"
    }
}

// ---------------------------------------------------------------- web links

/// What a click on a web link does, in a reply and in Highlights' Links:
/// Mike's browser by default (where his sign-ins are), or the pane. Set
/// from Default in a link's right-click menu (`WebLinkAction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WebLinkAction {
    #[default]
    Browser,
    Panel,
}

impl WebLinkAction {
    pub const ALL: [WebLinkAction; 2] = [WebLinkAction::Browser, WebLinkAction::Panel];

    pub fn key(self) -> &'static str {
        match self {
            WebLinkAction::Browser => "browser",
            WebLinkAction::Panel => "panel",
        }
    }

    pub fn from_key(key: &str) -> Option<WebLinkAction> {
        WebLinkAction::ALL.into_iter().find(|a| a.key() == key)
    }

    pub fn symbol(self) -> &'static str {
        match self {
            WebLinkAction::Browser => "safari",
            WebLinkAction::Panel => "sidebar.right",
        }
    }

    /// `browser` is the default browser's name ("Google Chrome").
    pub fn title(self, browser: Option<&str>) -> String {
        match self {
            WebLinkAction::Browser => browser.map_or_else(|| "Open in Browser".to_owned(), |name| format!("Open in {name}")),
            WebLinkAction::Panel => "Open in Panel".to_owned(),
        }
    }

    /// What a click does: ⌘ always means the browser, whatever Default says.
    pub fn for_click(command_key: bool, chosen: WebLinkAction) -> WebLinkAction {
        if command_key {
            WebLinkAction::Browser
        } else {
            chosen
        }
    }
}

// ---------------------------------------------------------------- Chrome's profiles

/// Chrome's own id, which the default browser and running apps are matched by.
pub const CHROME_BUNDLE_ID: &str = "com.google.Chrome";

/// A Chrome profile with a window open, for "Open in Chrome: convex.dev":
/// Mike runs a Chrome window per account (personal, Convex, ...), and a
/// plain open goes to whichever he used last, often the wrong sign-in
/// (Mike, 6 Nov).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChromeProfile {
    /// The profile's folder in Chrome's data, "Default" or "Profile 5":
    /// what `--profile-directory` takes.
    pub directory: String,
    /// The name Chrome shows on the profile ("convex.dev").
    pub name: String,
    /// Its Google account picture, when Chrome has saved one.
    pub picture: Option<PathBuf>,
}

impl ChromeProfile {
    pub fn title(&self) -> String {
        format!("Open in Chrome: {}", self.name)
    }
}

/// Where Chrome keeps its profiles' data on this system, under `home`.
pub fn chrome_data_folder(home: &str) -> PathBuf {
    let home = Path::new(home);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Google/Chrome")
    } else if cfg!(windows) {
        std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| home.join("AppData/Local")).join("Google/Chrome/User Data")
    } else {
        home.join(".config/google-chrome")
    }
}

/// The open profiles in Chrome's "Local State": `last_active_profiles` is
/// the ones with a window, which Chrome rewrites as windows open and close
/// (a few seconds late, as it batches writes), named from `info_cache`. In
/// Chrome's order, so the menu doesn't reshuffle (`ChromeProfiles.parse`).
pub fn chrome_profiles(local_state: &str, folder: &Path, exists: &dyn Fn(&Path) -> bool) -> Vec<ChromeProfile> {
    let Ok(root) = serde_json::from_str::<Value>(local_state) else { return Vec::new() };
    let profile = &root["profile"];
    let (Some(active), Some(info)) = (profile["last_active_profiles"].as_array(), profile["info_cache"].as_object()) else { return Vec::new() };
    active
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|directory| {
            let entry = info.get(directory)?;
            let name = entry["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(directory).to_owned();
            let picture = entry["gaia_picture_file_name"].as_str().filter(|f| !f.is_empty()).map(|file| folder.join(directory).join(file)).filter(|p| exists(p));
            Some(ChromeProfile { directory: directory.to_owned(), name, picture })
        })
        .collect()
}

/// The profiles a web link's menu offers: only when Chrome is the default
/// browser and running, and more than one has a window open; with one
/// there's nothing to choose, so the menu keeps its single browser item.
pub fn offered_profiles(profiles: Vec<ChromeProfile>, chrome_is_default_and_running: bool) -> Vec<ChromeProfile> {
    if chrome_is_default_and_running && profiles.len() > 1 {
        profiles
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------- the menus

/// What choosing a link menu's item does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkChoice {
    OpenFile(FileLinkAction),
    SetFileClick(FileLinkAction),
    OpenWeb(WebLinkAction),
    /// The `index`-th offered Chrome profile.
    OpenInProfile(usize),
    SetWebClick(WebLinkAction),
    CopyPath,
    CopyLink,
}

/// One line of a link's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkMenuEntry {
    Item { title: String, symbol: &'static str, choice: LinkChoice },
    /// Greyed: said, with nothing to do.
    Note { title: String, symbol: &'static str },
    Separator,
    /// "Default", which sets what a plain click does: its choices, the
    /// current one ticked.
    Defaults { choices: Vec<(String, &'static str, LinkChoice, bool)> },
}

impl LinkMenuEntry {
    /// The words a script names it by.
    pub fn title(&self) -> &str {
        match self {
            LinkMenuEntry::Item { title, .. } | LinkMenuEntry::Note { title, .. } => title,
            LinkMenuEntry::Separator => "---",
            LinkMenuEntry::Defaults { .. } => "Default",
        }
    }
}

pub const DEFAULT_TITLE: &str = "Default";
pub const DEFAULT_SYMBOL: &str = "cursorarrow.click";

/// A file link's right-click menu (`FileLinkMenu`): the ways to open it,
/// Copy Path, and Default. `found` is whether the link names a file; `app`
/// is the app the system opens it in.
pub fn file_link_menu(found: bool, app: Option<&str>, click: FileLinkAction) -> Vec<LinkMenuEntry> {
    if !found {
        return vec![LinkMenuEntry::Note { title: "Couldn't Find This File".to_owned(), symbol: "questionmark.folder" }];
    }
    let mut menu: Vec<LinkMenuEntry> = FileLinkAction::OPEN.into_iter().map(|a| LinkMenuEntry::Item { title: a.title(app), symbol: a.symbol(), choice: LinkChoice::OpenFile(a) }).collect();
    menu.push(LinkMenuEntry::Separator);
    menu.push(LinkMenuEntry::Item { title: "Copy Path".to_owned(), symbol: "doc.on.doc", choice: LinkChoice::CopyPath });
    menu.push(LinkMenuEntry::Separator);
    // "Default App" rather than this file's app: the choice is for every
    // file link, and they open in different apps.
    let choices = FileLinkAction::DEFAULTS.into_iter().map(|a| (a.title(None), a.symbol(), LinkChoice::SetFileClick(a), a == click)).collect();
    menu.push(LinkMenuEntry::Defaults { choices });
    menu
}

/// A web link's right-click menu (`WebLinkMenu`): the browser, or one item
/// per open Chrome profile, the pane, Copy Link, and Default.
pub fn web_link_menu(browser: Option<&str>, profiles: &[ChromeProfile], click: WebLinkAction) -> Vec<LinkMenuEntry> {
    let mut menu = Vec::new();
    if profiles.is_empty() {
        menu.push(LinkMenuEntry::Item { title: WebLinkAction::Browser.title(browser), symbol: WebLinkAction::Browser.symbol(), choice: LinkChoice::OpenWeb(WebLinkAction::Browser) });
    } else {
        for (index, profile) in profiles.iter().enumerate() {
            menu.push(LinkMenuEntry::Item { title: profile.title(), symbol: WebLinkAction::Browser.symbol(), choice: LinkChoice::OpenInProfile(index) });
        }
    }
    menu.push(LinkMenuEntry::Item { title: WebLinkAction::Panel.title(browser), symbol: WebLinkAction::Panel.symbol(), choice: LinkChoice::OpenWeb(WebLinkAction::Panel) });
    menu.push(LinkMenuEntry::Separator);
    menu.push(LinkMenuEntry::Item { title: "Copy Link".to_owned(), symbol: "link", choice: LinkChoice::CopyLink });
    menu.push(LinkMenuEntry::Separator);
    menu.push(LinkMenuEntry::Defaults { choices: web_defaults(browser, click) });
    menu
}

/// Default's choices for web links, the current one ticked: the same
/// choice in a reply's menu and a Highlights link's.
pub fn web_defaults(browser: Option<&str>, click: WebLinkAction) -> Vec<(String, &'static str, LinkChoice, bool)> {
    WebLinkAction::ALL.into_iter().map(|a| (a.title(browser), a.symbol(), LinkChoice::SetWebClick(a), a == click)).collect()
}

// ---------------------------------------------------------------- the hover card

/// What a link's hover card says (`LinkPeekCard`): a page's title and
/// address; a file's name and folder, and whether it's here to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeekWords {
    pub title: String,
    pub detail: String,
    pub footer: String,
}

/// A page's card: the link's words, unless they're just the address again,
/// then the page's host.
pub fn page_peek(url: &str, words: &str) -> PeekWords {
    let shown = address(url);
    let words = words.trim();
    let title = if !words.is_empty() && !shown.starts_with(words) && !url.starts_with(words) { words.to_owned() } else { host(url).unwrap_or_else(|| shown.clone()) };
    PeekWords { title, detail: shown, footer: "Click to open".to_owned() }
}

/// A file's card: its name, its folder with `~` for home, and what a click
/// does. `away` names another Mac whose file it is.
pub fn file_peek(path: &str, exists: bool, away: Option<&str>, home: &str) -> PeekWords {
    let name = highlights::last_component(path).to_owned();
    let parent = path.trim_end_matches('/').rsplit_once('/').map_or("", |(dir, _)| if dir.is_empty() { "/" } else { dir });
    let detail = match parent.strip_prefix(home.trim_end_matches('/')) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => format!("~{rest}"),
        _ => parent.to_owned(),
    };
    let footer = match (exists, away) {
        (true, _) => "Click to open".to_owned(),
        (false, Some(away)) => format!("On {away}. Click to open."),
        (false, None) => "Not found on this Mac".to_owned(),
    };
    PeekWords { title: name, detail, footer }
}

/// What a file's card shows over its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeekPicture {
    /// The picture itself.
    Image,
    /// Its first lines: text, code, Markdown and pages.
    Lines,
    /// Its kind's symbol.
    Symbol,
}

/// The pictures and text the pane previews get a look at their contents;
/// anything else (a folder, a PDF, a project) its symbol.
pub fn peek_picture(path: &str) -> PeekPicture {
    let lower = path.to_lowercase();
    if highlights::looks_like_picture(path) || lower.ends_with(".svg") {
        return PeekPicture::Image;
    }
    match highlights::viewer_for_path(path) {
        highlights::Viewer::Text | highlights::Viewer::Web => PeekPicture::Lines,
        highlights::Viewer::Picture => PeekPicture::Symbol,
    }
}

/// The first `lines` lines of a file's start, for its card: tabs as spaces,
/// none longer than `width` characters, a cut-off character at the end of
/// the sample dropped. None for one that isn't text.
pub fn first_lines(sample: &[u8], lines: usize, width: usize) -> Option<Vec<String>> {
    if !highlights::looks_like_text(sample) {
        return None;
    }
    let text = match std::str::from_utf8(sample) {
        Ok(text) => text.to_owned(),
        Err(e) => String::from_utf8_lossy(&sample[..e.valid_up_to()]).into_owned(),
    };
    Some(text.lines().take(lines).map(|line| line.replace('\t', "    ").chars().take(width).collect::<String>().trim_end().to_owned()).collect())
}

/// A web address's host, without its port or sign-in (`URL.host()`).
pub fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if authority.starts_with('[') { authority.split_inclusive(']').next().unwrap_or(authority) } else { authority.split(':').next().unwrap_or(authority) };
    (!host.is_empty()).then(|| host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_on_a_file_link_does_what_default_says_unless_command_is_held() {
        assert_eq!(FileLinkAction::for_click(false, false, false, FileLinkAction::Automatic), FileLinkAction::Automatic);
        assert_eq!(FileLinkAction::for_click(true, false, false, FileLinkAction::Panel), FileLinkAction::App, "⌘ always means its app");
        assert_eq!(FileLinkAction::for_click(false, false, true, FileLinkAction::Panel), FileLinkAction::App, "a project file goes to its app");
        assert_eq!(FileLinkAction::for_click(false, true, true, FileLinkAction::Panel), FileLinkAction::Panel, "another Mac's copy has none of its media");
        // Automatic made definite: docs and pictures in the pane, code in its app.
        assert_eq!(FileLinkAction::Automatic.resolved("/w/notes.md", false), FileLinkAction::Panel);
        assert_eq!(FileLinkAction::Automatic.resolved("/w/shot.PNG", false), FileLinkAction::Panel);
        assert_eq!(FileLinkAction::Automatic.resolved("/w/main.rs", false), FileLinkAction::App);
        assert_eq!(FileLinkAction::Automatic.resolved("/w/src", true), FileLinkAction::Panel, "a folder is browsed in the pane");
        assert_eq!(FileLinkAction::Finder.resolved("/w/notes.md", false), FileLinkAction::Finder);
        assert_eq!(WebLinkAction::for_click(true, WebLinkAction::Panel), WebLinkAction::Browser);
        assert_eq!(WebLinkAction::for_click(false, WebLinkAction::Panel), WebLinkAction::Panel);
    }

    #[test]
    fn choices_are_saved_by_the_swift_apps_names() {
        for a in FileLinkAction::DEFAULTS {
            assert_eq!(FileLinkAction::from_key(a.key()), Some(a));
        }
        assert_eq!(FileLinkAction::from_key("automatic"), Some(FileLinkAction::Automatic));
        assert_eq!(WebLinkAction::from_key("panel"), Some(WebLinkAction::Panel));
        assert_eq!(WebLinkAction::from_key("nonsense"), None);
    }

    #[test]
    fn a_file_links_menu_has_its_ways_to_open_copy_path_and_default() {
        let menu = file_link_menu(true, Some("Xcode"), FileLinkAction::Panel);
        let titles: Vec<&str> = menu.iter().map(LinkMenuEntry::title).collect();
        assert_eq!(titles, ["Open in Panel", "Open in Xcode", finder_title(), "---", "Copy Path", "---", "Default"]);
        let LinkMenuEntry::Defaults { choices } = menu.last().unwrap() else { panic!() };
        let named: Vec<(&str, bool)> = choices.iter().map(|(t, _, _, on)| (t.as_str(), *on)).collect();
        // Default App, not this file's app: the choice is for every file link.
        assert_eq!(named, [("Panel for Docs and Pictures, App for the Rest", false), ("Open in Panel", true), ("Open in Default App", false), (finder_title(), false)]);
        assert_eq!(file_link_menu(false, None, FileLinkAction::Automatic).iter().map(LinkMenuEntry::title).collect::<Vec<_>>(), ["Couldn't Find This File"]);
        // With no app found for it.
        assert_eq!(file_link_menu(true, None, FileLinkAction::Automatic)[1].title(), "Open in Default App");
    }

    #[test]
    fn a_web_links_menu_has_one_item_a_profile_when_chrome_has_several_open() {
        let titles = |menu: Vec<LinkMenuEntry>| menu.iter().map(|e| e.title().to_owned()).collect::<Vec<_>>();
        assert_eq!(titles(web_link_menu(Some("Safari"), &[], WebLinkAction::Browser)), ["Open in Safari", "Open in Panel", "---", "Copy Link", "---", "Default"]);
        let profiles = vec![
            ChromeProfile { directory: "Default".into(), name: "Mike".into(), picture: None },
            ChromeProfile { directory: "Profile 5".into(), name: "convex.dev".into(), picture: None },
        ];
        let menu = web_link_menu(Some("Google Chrome"), &profiles, WebLinkAction::Panel);
        assert_eq!(titles(menu.clone()), ["Open in Chrome: Mike", "Open in Chrome: convex.dev", "Open in Panel", "---", "Copy Link", "---", "Default"]);
        assert!(matches!(&menu[1], LinkMenuEntry::Item { choice: LinkChoice::OpenInProfile(1), .. }));
        let LinkMenuEntry::Defaults { choices } = menu.last().unwrap() else { panic!() };
        assert_eq!(choices.iter().map(|(t, _, _, on)| (t.as_str(), *on)).collect::<Vec<_>>(), [("Open in Google Chrome", false), ("Open in Panel", true)]);
        assert_eq!(web_link_menu(None, &[], WebLinkAction::Browser)[0].title(), "Open in Browser");
    }

    #[test]
    fn chromes_open_profiles_come_from_its_local_state_in_its_order() {
        let folder = Path::new("/Chrome");
        let state = r#"{"profile":{"last_active_profiles":["Profile 5","Default","Gone"],"info_cache":{
            "Default":{"name":"Mike","gaia_picture_file_name":"Google Profile Picture.png"},
            "Profile 5":{"name":"","gaia_picture_file_name":""},
            "Profile 9":{"name":"Closed"}}}}"#;
        let exists = |p: &Path| p == Path::new("/Chrome/Default/Google Profile Picture.png");
        let profiles = chrome_profiles(state, folder, &exists);
        assert_eq!(profiles.len(), 2, "a profile Chrome no longer has is left out");
        assert_eq!((profiles[0].directory.as_str(), profiles[0].name.as_str(), profiles[0].picture.is_none()), ("Profile 5", "Profile 5", true), "no name: its folder's");
        assert_eq!(profiles[1].picture.as_deref(), Some(Path::new("/Chrome/Default/Google Profile Picture.png")));
        assert_eq!(profiles[1].title(), "Open in Chrome: Mike");
        assert!(chrome_profiles("not json", folder, &exists).is_empty());
        assert!(chrome_profiles(r#"{"profile":{}}"#, folder, &exists).is_empty());
        // Offered only with more than one, and Chrome the browser in use.
        assert_eq!(offered_profiles(profiles.clone(), true).len(), 2);
        assert!(offered_profiles(profiles.clone(), false).is_empty());
        assert!(offered_profiles(profiles[..1].to_vec(), true).is_empty());
    }

    #[test]
    fn a_pages_card_is_titled_by_its_words_unless_theyre_its_address() {
        let card = page_peek("https://github.com/mikecann/threadbear/pull/12", "the PR");
        assert_eq!((card.title.as_str(), card.detail.as_str(), card.footer.as_str()), ("the PR", "github.com/mikecann/threadbear/pull/12", "Click to open"));
        assert_eq!(page_peek("https://www.example.com/a", "example.com/a").title, "www.example.com");
        assert_eq!(page_peek("https://example.com/a", "https://example.com/a").title, "example.com");
        assert_eq!(page_peek("https://example.com/", "").detail, "example.com");
    }

    #[test]
    fn a_files_card_names_it_its_folder_and_whether_its_here() {
        let card = file_peek("/Users/mike/dev/app/docs/plan.md", true, None, "/Users/mike");
        assert_eq!((card.title.as_str(), card.detail.as_str(), card.footer.as_str()), ("plan.md", "~/dev/app/docs", "Click to open"));
        assert_eq!(file_peek("/tmp/a.png", false, None, "/Users/mike").footer, "Not found on this Mac");
        assert_eq!(file_peek("/Users/mike/a.png", false, Some("bruce"), "/Users/mike").footer, "On bruce. Click to open.");
        assert_eq!(file_peek("/Users/mike/a.png", true, None, "/Users/mike").detail, "~");
        assert_eq!(file_peek("/Users/mikey/a.png", true, None, "/Users/mike").detail, "/Users/mikey", "not home, just a name like it");
        assert_eq!(file_peek("/a.png", true, None, "").detail, "/");
    }

    #[test]
    fn a_files_card_shows_a_picture_its_first_lines_or_its_symbol() {
        assert_eq!(peek_picture("/a/shot.png"), PeekPicture::Image);
        assert_eq!(peek_picture("/a/logo.svg"), PeekPicture::Image);
        assert_eq!(peek_picture("/a/plan.md"), PeekPicture::Lines);
        assert_eq!(peek_picture("/a/main.rs"), PeekPicture::Lines);
        assert_eq!(peek_picture("/a/page.html"), PeekPicture::Lines);
        assert_eq!(peek_picture("/a/report.pdf"), PeekPicture::Symbol);
        assert_eq!(peek_picture("/a/project.tandem"), PeekPicture::Symbol);
        assert_eq!(first_lines(b"one\n\ttwo\nthree\nfour", 3, 80).unwrap(), ["one", "    two", "three"]);
        assert_eq!(first_lines("caf\u{e9}".as_bytes().split_last().unwrap().1, 2, 80).unwrap(), ["caf"], "a character cut off at the end");
        assert_eq!(first_lines(b"0123456789", 1, 4).unwrap(), ["0123"]);
        assert!(first_lines(b"\x00\x01png", 3, 80).is_none());
    }

    #[test]
    fn hosts() {
        assert_eq!(host("https://user:pw@example.com:8080/a?b").as_deref(), Some("example.com"));
        assert_eq!(host("http://[::1]:3000/").as_deref(), Some("[::1]"));
        assert_eq!(host("mailto:a@b.c"), None);
    }
}
