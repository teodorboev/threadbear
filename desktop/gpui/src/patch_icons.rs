// The pictures patches give their panels and chips (docs/protocol.md,
// "Patches"): a patch's own picture (`iconFile`, drawn in colour), else its
// SF Symbol from the bundled set, and the small badge saying where a panel or
// chip came from (`PanelOriginBadge`). The rail, the dock's tabs and the chips'
// shelves draw them the same way. Before this every patch panel was the same
// dashed square, on the rail and nowhere else (Mike, 11 Nov).
use crate::icons;
use crate::sidebar::ProjectIcon;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::path::PathBuf;
use threadbear_core::highlights::PanelOrigin;

/// A picture this app can draw as it is.
fn drawable(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
}

/// The symbol, or the dashed square for a name the bundled set hasn't got.
/// With no colour it takes its parent's, so a rail button's hover still
/// brightens it.
pub(crate) fn glyph(symbol: &str, size: f32, color: Option<Hsla>) -> AnyElement {
    let icon = icons::try_symbol(symbol)
        .or_else(|| icons::try_symbol(symbol.trim_end_matches(".fill")))
        .unwrap_or_else(|| icons::symbol("square.dashed"))
        .with_size(px(size));
    match color {
        Some(color) => icon.text_color(color).into_any_element(),
        None => icon.into_any_element(),
    }
}

/// A panel's or a chip's icon: its patch's own picture, in colour, when it has
/// one this app can draw, else its symbol.
pub(crate) fn patch_icon(icon_file: Option<&str>, symbol: &str, size: f32, color: Option<Hsla>, remote: bool) -> AnyElement {
    if remote { return glyph(symbol, size, color); }
    #[cfg(target_os = "macos")]
    if let Some(path) = icon_file.filter(|p| matches!(p.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(), "app" | "icns" | "pdf")) {
        let path = PathBuf::from(path);
        let source = ImageSource::Custom(std::sync::Arc::new(move |window, cx| window.use_asset::<MacIcon>(&path, cx)));
        let symbol = symbol.to_owned();
        return img(source).size(px(size + 1.)).object_fit(ObjectFit::Contain).with_fallback(move || glyph(&symbol, size, color)).into_any_element();
    }
    match icon_file.filter(|p| !p.is_empty() && drawable(p)) {
        Some(path) => {
            let symbol = symbol.to_owned();
            img(PathBuf::from(path))
                .size(px(size + 1.))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || glyph(&symbol, size, color))
                .into_any_element()
        }
        None => glyph(symbol, size, color),
    }
}

// The asset cache does the decode off the drawing thread and shares it among
// the rail, tabs and chips. Only the gate reads the file or bundle.
#[cfg(target_os = "macos")]
struct MacIcon;
#[cfg(target_os = "macos")]
impl Asset for MacIcon {
    type Source = PathBuf;
    type Output = Result<std::sync::Arc<RenderImage>, ImageCacheError>;
    fn load(path: Self::Source, cx: &mut App) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        // All on the asset's worker: an application's icon is a file in its
        // bundle, read like any other (open_gate::application_icon).
        async move {
            let icon = crate::open_gate::read_icon(&path, crate::open_gate::Origin::ThisMac)?;
            let png = crate::open_gate::icon_png(icon)?;
            Image::from_bytes(ImageFormat::Png, png).to_image_data(renderer).map_err(Into::into)
        }
    }
}

/// What the badge draws for a project: its picture or symbol, else its letter.
pub(crate) struct ProjectMark {
    pub(crate) icon: Option<ProjectIcon>,
    pub(crate) letter: SharedString,
}

/// `PanelOriginBadge`: a small circle over the icon's bottom corner with the
/// project's own icon for a project's folder, else a glyph (a house for
/// ~/.threadbear, a folder for the thread's own, a wrench for a tool patch, a
/// box for a built-in). Grey and small, like the counts: it answers "where's
/// this from?" and isn't there to be noticed. `side` is its width at 100%:
/// 13 on the rail, smaller on a tab.
pub(crate) fn origin_badge(origin: &PanelOrigin, project: Option<ProjectMark>, side: f32, cx: &App) -> Option<AnyElement> {
    let secondary = cx.theme().muted_foreground;
    let inner: AnyElement = match origin {
        PanelOrigin::Project(_) => {
            let mark = project?;
            let inner_side = side * 10. / 13.;
            match mark.icon {
                Some(ProjectIcon { image_path: Some(path), .. }) => {
                    let letter = mark.letter.clone();
                    img(PathBuf::from(path))
                        .size(px(inner_side))
                        .rounded_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || div().text_size(px(inner_side * 0.7)).font_weight(FontWeight::SEMIBOLD).text_color(secondary).child(letter.clone()).into_any_element())
                        .into_any_element()
                }
                Some(ProjectIcon { symbol: Some(symbol), .. }) if icons::try_symbol(&symbol).is_some() => glyph(&symbol, inner_side * 0.8, Some(secondary)),
                _ => div().text_size(px(inner_side * 0.75)).font_weight(FontWeight::SEMIBOLD).text_color(secondary).child(mark.letter).into_any_element(),
            }
        }
        other => glyph(other.symbol()?, side / 2., Some(secondary)),
    };
    Some(
        div()
            .absolute()
            // Over the icon's bottom corner, a little out from it, as
            // Swift's `.offset(x: side * 2 / 13, y: side / 13)` puts it.
            .right(px(-side / 2. + side * 2. / 13.))
            .bottom(px(-side / 2. + side / 13.))
            .size(px(side))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(cx.theme().background)
            .border(gpui_kit::px(0.5))
            .border_color(crate::style::hairline(cx))
            .child(inner)
            .into_any_element(),
    )
}

/// A panel's icon with its origin badge over the corner, for the rail and the
/// dock's tabs (`PanelIcon` with `PanelOriginBadge`).
pub(crate) fn panel_face(panel: &threadbear_core::highlights::Panel, thread: &threadbear_core::inbox::ThreadSummary, project_icons: &std::collections::HashMap<String, ProjectIcon>, size: f32, badge_side: f32, cx: &App) -> AnyElement {
    let symbol = threadbear_core::highlights::panel_symbol(panel).to_owned();
    let origin = PanelOrigin::of_panel(panel, &thread.patches());
    let badge = origin_badge(&origin, project_mark(&origin, thread, project_icons), badge_side, cx);
    div()
        .relative()
        .size(px(size + 1.))
        .flex()
        .items_center()
        .justify_center()
        .child(patch_icon(panel.icon_file.as_deref(), &symbol, size, None, thread.is_remote()))
        .children(badge)
        .into_any_element()
}

/// The project a badge stands for, by the thread's own list of its projects.
pub(crate) fn project_mark(origin: &PanelOrigin, thread: &threadbear_core::inbox::ThreadSummary, project_icons: &std::collections::HashMap<String, ProjectIcon>) -> Option<ProjectMark> {
    let PanelOrigin::Project(id) = origin else { return None };
    let title = thread.projects.iter().flatten().find(|p| &p.id == id).map(|p| p.title.clone()).unwrap_or_default();
    let letter: SharedString = title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "·".into()).into();
    Some(ProjectMark { icon: project_icons.get(id).cloned(), letter })
}

/// "Convex, from the Threadbear project": the rail button's and the tab's tip.
pub(crate) fn panel_tip(panel: &threadbear_core::highlights::Panel, thread: &threadbear_core::inbox::ThreadSummary) -> String {
    let origin = PanelOrigin::of_panel(panel, &thread.patches());
    let project_title = match &origin {
        PanelOrigin::Project(id) => thread.projects.iter().flatten().find(|p| &p.id == id).map(|p| p.title.clone()),
        _ => None,
    };
    origin.tip(&panel.title, project_title.as_deref())
}
