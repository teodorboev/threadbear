// The web runtime for the GPUI app (docs/gpui-port.md, "Choice 1: the web
// view"): CEF's official Chromium build, fetched the first time something
// needs it (download.rs, checked by threadbear_core::web_runtime), started
// from the data folder in the app's own process (mac/host.rs), and the pages
// it shows, all sealed the way the Swift app seals its web views (mac/sealed.rs).
//
// Nothing here draws GPUI; desktop/gpui shows the state and hosts the pages.
// On Windows and Linux there's no runtime yet, so everything says so and the
// app shows source and offers the browser, as it does before an install.
pub mod download;
#[cfg(target_os = "macos")]
mod mac;
pub mod panel_rules;

pub use download::Progress;

#[cfg(target_os = "macos")]
pub use mac::host::{bundle_identifier, start, status, Launch, Status};
#[cfg(target_os = "macos")]
pub use mac::offscreen::{picture, png_size, Offscreen, Options, Reply};
/// The transcript's diagrams and pages, drawn into pictures (mac/rich.rs).
#[cfg(target_os = "macos")]
pub use mac::rich::{data_url, draw_diagram, draw_page, reset as reset_rich, Drawn};
/// A page in a native view, placed from a GPUI layout slot (mac/page.rs).
#[cfg(target_os = "macos")]
pub use mac::page::Page;
/// A web panel's page: a patch's site or the Convex dashboard (mac/panel.rs),
/// and the native menu for controls over one.
#[cfg(target_os = "macos")]
pub use mac::host::{keyboard_holder, popup_menu, send_keys, take_keyboard, MenuItem};
#[cfg(target_os = "macos")]
pub use mac::panel::PanelPage;
pub use panel_rules::{site, Event as PanelEvent, Kind as PanelKind};

/// Who has the keyboard in the window GPUI's view fills (AppKit's first
/// responder), for the dump and for giving it back (web_panels.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardHolder {
    /// GPUI's own view: what GPUI has focused gets the keys.
    App,
    /// A page's view that's showing, after a click in it.
    Page,
    /// A page's view that's hidden.
    HiddenPage,
    /// Nobody, or the window itself: a page's view that had it went.
    Window,
    Other,
}

impl KeyboardHolder {
    pub fn name(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Page => "page",
            Self::HiddenPage => "hidden page",
            Self::Window => "window",
            Self::Other => "other",
        }
    }
}

/// The helper processes' main (the threadbear-web-helper binary).
pub fn helper_main() -> i32 {
    #[cfg(target_os = "macos")]
    return mac::helper::run();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("threadbear-web-helper: there's no web runtime for this system yet");
        1
    }
}

#[cfg(not(target_os = "macos"))]
mod elsewhere {
    use std::path::PathBuf;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Status {
        NotStarted,
        Starting,
        Ready,
        Failed(String),
    }

    pub struct Launch {
        pub bundle: PathBuf,
        pub framework: PathBuf,
        pub helper: PathBuf,
        pub profile: PathBuf,
        pub log_file: PathBuf,
    }

    pub fn status() -> Status {
        Status::Failed("there's no web runtime for this system yet".into())
    }

    pub fn start(_launch: Launch) -> Result<(), String> {
        Err("there's no web runtime for this system yet".into())
    }

    pub fn bundle_identifier() -> Option<String> {
        None
    }

    /// What a drawing gives back (mac/rich.rs has the real one).
    pub struct Drawn {
        pub width: f32,
        pub height: f32,
        pub clipped: bool,
        pub dark: Vec<u8>,
        pub light: Vec<u8>,
        pub window_page: String,
    }

    pub async fn draw_diagram(_source: &str, _zoom: u32, _scale: f32) -> Result<Drawn, String> {
        Err("there's no web runtime for this system yet".into())
    }

    pub async fn draw_page(_html: &str, _zoom: u32, _scale: f32) -> Result<Drawn, String> {
        Err("there's no web runtime for this system yet".into())
    }

    pub fn reset_rich() {}

    /// One item of a native menu (mac/host.rs has the real one).
    #[derive(Clone, Debug, Default)]
    pub struct MenuItem {
        pub title: String,
        pub symbol: String,
        pub tip: String,
        pub checked: bool,
    }

    pub fn popup_menu(_view: *mut std::ffi::c_void, _x: f32, _y: f32, _items: &[MenuItem]) -> Option<usize> {
        None
    }

    /// No pages here to take the keyboard from GPUI's view.
    pub fn keyboard_holder(_view: *mut std::ffi::c_void) -> crate::KeyboardHolder {
        crate::KeyboardHolder::App
    }

    pub fn take_keyboard(_view: *mut std::ffi::c_void) -> bool {
        false
    }

    pub fn send_keys(_view: *mut std::ffi::c_void, _text: &str) {}

    /// A web panel's page (mac/panel.rs has the real one). There's no
    /// runtime here to make one, so none is ever made.
    pub struct PanelPage(());

    impl PanelPage {
        pub fn open(_parent: *mut std::ffi::c_void, _url: &str, _kind: crate::PanelKind) -> Result<(PanelPage, futures::channel::mpsc::UnboundedReceiver<crate::PanelEvent>), String> {
            Err("there's no web runtime for this system yet".into())
        }
        pub fn devtools(&self, _method: &str, _params: serde_json::Value) -> futures::channel::oneshot::Receiver<Result<serde_json::Value, String>> {
            let (tx, rx) = futures::channel::oneshot::channel();
            let _ = tx.send(Err("there's no web runtime for this system yet".into()));
            rx
        }
        pub fn load(&self, _url: &str) {}
        pub fn reload(&self) {}
        pub fn can_go_back(&self) -> bool {
            false
        }
        pub fn loading(&self) -> bool {
            false
        }
        pub fn loads(&self) -> u32 {
            0
        }
        pub fn title(&self) -> String {
            String::new()
        }
        pub fn address(&self) -> String {
            String::new()
        }
        pub fn go_back(&self) {}
        pub fn move_to(&self, _parent: *mut std::ffi::c_void) {}
        pub fn set_bounds(&self, _x: f32, _y: f32, _width: f32, _height: f32, _parent_height: f32) {}
        pub fn bounds(&self) -> (f32, f32, f32, f32) {
            (0., 0., 0., 0.)
        }
        pub fn set_hidden(&self, _hidden: bool) {}
        pub fn hidden(&self) -> bool {
            true
        }
        pub fn muted(&self) -> bool {
            false
        }
        pub fn edit(&self, _command: &str) {}
        pub fn has_keyboard(&self) -> bool {
            false
        }
        pub fn input_count(&self) -> u64 {
            0
        }
        pub fn focus(&self) {}
        pub fn note_input(&self) {}
    }
}
#[cfg(not(target_os = "macos"))]
pub use elsewhere::*;
