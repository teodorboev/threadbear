// What the app gives the dock for each panel and how the dock fits the app
// around it: Swift's DockContent and DockOptions (dock/Sources/BearDock/DockView.swift).
use gpui_kit::*;
use std::rc::Rc;

/// What the app gives the dock for each panel. The dock calls these every
/// frame; `view` should be as cheap as rendering the panel normally.
pub trait DockContent: 'static {
    /// The panel's title, in its tab or bar and on the ghost.
    fn title(&self, panel: &str, cx: &App) -> SharedString;

    /// The icon in front of the title. Nil: none.
    fn icon(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let _ = (panel, window, cx);
        None
    }

    /// A small mark over the bottom corner of the tab's icon (where the panel
    /// came from, say). Nil: none.
    fn badge(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let _ = (panel, window, cx);
        None
    }

    /// The panel's content. `ghost` is true for the second copy drawn in the
    /// ghost that follows a drag. Content that owns a resource only one
    /// element can use (a web view, a video surface) should draw a picture of
    /// it instead when `ghost` is true, or the ghost takes it and the panel is
    /// left blank once the ghost goes, as Swift's `isDockGhost` environment
    /// value was for.
    fn view(&self, panel: &str, ghost: bool, window: &mut Window, cx: &mut App) -> AnyElement;

    /// Drawn in place of the title when the panel is on its own (a thread's
    /// name and folder over the transcript, say). Nil: the title.
    fn header(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let _ = (panel, window, cx);
        None
    }

    /// Buttons at the end of the panel's bar while it's showing.
    fn accessory(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let _ = (panel, window, cx);
        None
    }

    /// False for a panel the app always keeps (the transcript): no ×, no Close.
    fn closable(&self, panel: &str, cx: &App) -> bool {
        let _ = (panel, cx);
        true
    }

    /// False for a panel with nothing to show right now (the selected thread
    /// has no such panel): its tab is left out and another of its group's
    /// shows, without changing the layout, so it's back when it has something
    /// again.
    fn available(&self, panel: &str, cx: &App) -> bool {
        let _ = (panel, cx);
        true
    }

    /// A line about the panel for its tab's tip, before how to move it
    /// (whose a pinned panel is, say). Nil: only the move tip.
    fn tip(&self, panel: &str, cx: &App) -> Option<SharedString> {
        let _ = (panel, cx);
        None
    }

    /// The app's own items for the panel's right-click menu, above Close
    /// (Threadbear's Pin and Unpin). None by default.
    fn menu_items(&self, panel: &str, cx: &App) -> Vec<DockMenuItem> {
        let _ = (panel, cx);
        Vec::new()
    }
}

/// One of the app's items in a panel's right-click menu (`menu_items`).
#[derive(Clone)]
pub struct DockMenuItem {
    pub label: SharedString,
    pub icon: Option<gpui_kit::assets::IconName>,
    pub on_click: Rc<dyn Fn(&mut Window, &mut App)>,
}

/// The app's content, shared by the dock and its windows.
pub type SharedContent = Rc<dyn DockContent>;

/// Wraps a panel's window of its own, to give it what the app's main window
/// gives its views, as a new window doesn't inherit any of that.
pub type WindowWrapper = Rc<dyn Fn(AnyElement, &mut Window, &mut App) -> AnyElement>;

/// Called on a panel's window of its own as it's made, before it shows.
pub type PrepareWindow = Rc<dyn Fn(&mut Window, &mut App)>;

/// Told when a panel's window of its own comes to the front (true) or goes,
/// closing included, with that window.
pub type WindowActivation = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// How the dock fits the app around it.
#[derive(Clone)]
pub struct DockOptions {
    pub header_height: f32,
    pub title_size: f32,
    /// In a window without a title bar: room left at the start of the
    /// top-left panel's bar for the traffic lights. 0 with a title bar.
    pub traffic_lights: f32,
    /// In a window without a title bar: the strip beside the traffic lights
    /// moves the window, as a title bar would. Only that strip: Mike reached
    /// for a panel's bar to move the panel and got the window (28 Oct), so
    /// every other bar moves its panel, empty part and all.
    pub corner_moves_window: bool,
    /// In a window without a title bar: a plain drag on a panel's bar (title,
    /// tabs or empty part) moves the window, as a title bar's would. Picking
    /// a panel up always takes a press held for a moment first, with or
    /// without this.
    pub bars_move_window: bool,
    /// A press held on a panel's background (not its bar) picks the panel up.
    /// A panel with no bar along its top (its title hidden) can always be
    /// picked up from its background, whatever this says. On by default,
    /// as BearDock does it; Threadbear turns it off unless Settings says
    /// otherwise, as Mike found it got in the way more than it helped (13 Nov).
    pub background_moves_panel: bool,
    /// The colour of drop targets and the lit divider. Default: the theme's
    /// primary (Swift's `Color.accentColor`). Apps with an accent of their
    /// own (the demo's macOS blue) set it here.
    pub accent: Option<Hsla>,
    /// Gives a panel's window of its own what the main window gives its views.
    pub window_content: Option<WindowWrapper>,
    /// Called on a panel's window of its own as it's made, before it shows.
    pub prepare_window: Option<PrepareWindow>,
    /// Called as a panel's window of its own comes to the front or goes.
    pub window_activation: Option<WindowActivation>,
}

impl Default for DockOptions {
    fn default() -> Self {
        Self {
            header_height: 32.,
            title_size: 12.,
            traffic_lights: 0.,
            corner_moves_window: false,
            bars_move_window: false,
            background_moves_panel: true,
            accent: None,
            window_content: None,
            prepare_window: None,
            window_activation: None,
        }
    }
}

/// The menu's row for one of the app's items.
pub(crate) fn popup_item(item: DockMenuItem) -> gpui_kit::component::menu::PopupMenuItem {
    let on_click = item.on_click.clone();
    let mut row = gpui_kit::component::menu::PopupMenuItem::new(item.label);
    if let Some(icon) = item.icon {
        row = row.icon(gpui_kit::component::Icon::new(icon));
    }
    row.on_click(move |_, window, cx| on_click(window, cx))
}
