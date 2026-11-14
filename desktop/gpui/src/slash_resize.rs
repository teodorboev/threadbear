// Sizing the slash palette (SlashPalettePanel's PaletteFrame and
// SlashPaletteView's ResizeHandle): wide bands on its top, sides and top
// corners drag it taller or wider (Mike, 7 Nov: "the hit area for the drag
// handles is small"), lit in the accent while hovered or dragged; the lines
// between its columns drag the column on their left, and a double-click puts
// one back to its own width; a right-click anywhere on it offers Default Size
// and Default Column Widths. What's dragged to is kept for next time, in the
// app's prefs as the Swift app keeps it in UserDefaults. It stays centred over
// the box, so a side grows both sides at once. The sums are
// threadbear_core::slash's (PaletteLayout, PaletteEdge).
use crate::composer::Composer;
use crate::style;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use threadbear_core::slash::{PaletteEdge, PaletteLayout};
use crate::tips::Tip as _;

/// The bands' width (PaletteFrame.band) and the palette's corners.
const BAND: f32 = 10.;
pub(crate) const RADIUS: f32 = 14.;
/// What the bands are called, to VoiceOver and to a script.
const EDGE_NAMES: [(PaletteEdge, &str); 5] = [
    (PaletteEdge::Top, "The palette's top edge"),
    (PaletteEdge::Leading, "The palette's left edge"),
    (PaletteEdge::Trailing, "The palette's right edge"),
    (PaletteEdge::TopLeading, "The palette's top left corner"),
    (PaletteEdge::TopTrailing, "The palette's top right corner"),
];
pub(crate) const COLUMN_NAMES: [&str; 2] = ["The categories' edge", "The list's edge"];
const EDGE_TIP: &str = "Drag to resize the palette; right-click for its default size";
const COLUMN_TIPS: [&str; 2] = ["Drag to size the categories; double-click for their own width", "Drag to size the list; double-click for its own width"];

/// A drag under way: an edge from the size it had, or a column from its width.
#[derive(Clone, Copy)]
enum Drag {
    Edge { edge: PaletteEdge, from: Point<Pixels>, start: (f64, f64) },
    Column { which: usize, from: Point<Pixels>, start: f64, others: f64, total: f64 },
}

thread_local! {
    /// The drag going on, if any: only one pointer, so one at a time.
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
    /// The size a drag has got to, drawn until it's let go.
    static LIVE: Cell<Option<(f64, f64)>> = const { Cell::new(None) };
    /// Where the box is across the window (its left and width), last drawn,
    /// so a wide palette is kept inside the window.
    static BOX: Cell<(f32, f32)> = const { Cell::new((0., 0.)) };
    /// The band the pointer's over, lit in the accent.
    static LIT: Cell<bool> = const { Cell::new(false) };
    /// The palette as last drawn, in points: its width and height, and its
    /// categories' and list's widths (the dump's `palette.size`, `columns`).
    static DRAWN: Cell<Option<[f64; 4]>> = const { Cell::new(None) };
}

/// Keeps what was drawn, for the dump.
pub(crate) fn drawn_as(size: (f64, f64), columns: (f64, f64)) {
    DRAWN.set(Some([size.0, size.1, columns.0, columns.1]));
}

/// The palette as last drawn: its size and its columns' widths, in points.
#[cfg(feature = "automation")]
pub(crate) fn drawn() -> Option<[f64; 4]> {
    DRAWN.get()
}

/// What's kept, from the app's prefs.
pub(crate) fn kept(composer: &Composer, cx: &App) -> PaletteLayout {
    composer.app.upgrade().map(|a| a.read(cx).new_thread.prefs.slash_layout).unwrap_or_default()
}

fn keep(composer: &Composer, layout: PaletteLayout, save: bool, cx: &mut App) {
    if let Some(app) = composer.app.upgrade() {
        app.update(cx, |app, _| {
            app.new_thread.prefs.slash_layout = layout;
            if save {
                crate::prefs::save(&app.new_thread.prefs);
            }
        });
    }
}

/// The palette's size in points (zoom aside): what a drag has got to, else
/// what's kept within the window, else as wide as the box and about half the
/// window tall (SlashPaletteCoordinator.place).
pub(crate) fn size(layout: PaletteLayout, box_width: f64, window: Size<Pixels>, auto_height: f64) -> (f64, f64) {
    if let Some(live) = LIVE.get() {
        let (w, h) = live;
        return (PaletteLayout::width_in(w, f64::from(window.width)), PaletteLayout::height_in(h, f64::from(window.height)));
    }
    let width = layout.width.map_or(box_width, |w| PaletteLayout::width_in(w, f64::from(window.width)));
    let height = layout.height.map_or(auto_height, |h| PaletteLayout::height_in(h, f64::from(window.height)));
    (width, height)
}

/// Where the palette's left edge goes, from the box's: centred over it, and
/// never past the window's sides (8 in from each).
pub(crate) fn left_offset(width: f32, window_width: f32) -> f32 {
    let (box_left, box_width) = BOX.get();
    let centred = (box_width - width) / 2.;
    let least = 8. - box_left;
    let most = window_width - 8. - box_left - width;
    if most < least {
        least
    } else {
        centred.clamp(least, most)
    }
}

/// A canvas that keeps where the box is across the window.
pub(crate) fn measure_box() -> impl IntoElement {
    canvas(|bounds, _, _| BOX.set((f32::from(bounds.origin.x), f32::from(bounds.size.width))), |_, _, _, _| {}).absolute().size_full()
}

/// While a drag goes on: the window's pointer moves resize, and letting go
/// keeps the size (SlashPaletteCoordinator.resizeToPointer and endResize).
pub(crate) fn drag_watch(composer: WeakEntity<Composer>) -> Option<AnyElement> {
    DRAG.with_borrow(|d| d.is_some()).then(|| {
        let up = composer.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let moving = composer.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    if let Some(composer) = moving.upgrade() {
                        composer.update(cx, |c, cx| {
                            dragged_to(c, event.position, cx);
                            cx.notify();
                        });
                    }
                });
                let up = up.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    if let Some(composer) = up.upgrade() {
                        composer.update(cx, |c, cx| {
                            let_go(c, cx);
                            cx.notify();
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    })
}

fn dragged_to(composer: &Composer, at: Point<Pixels>, cx: &mut App) {
    let Some(drag) = DRAG.with_borrow(|d| *d) else { return };
    // Points, as the sizes are kept at 100%.
    let scale = f64::from(style::zoomed(1.));
    match drag {
        Drag::Edge { edge, from, start } => {
            let (dx, dy) = (f64::from(at.x - from.x) / scale, f64::from(at.y - from.y) / scale);
            LIVE.set(Some(edge.resized(start, dx, dy)));
        }
        Drag::Column { which, from, start, others, total } => {
            let dx = f64::from(at.x - from.x) / scale;
            let mut layout = kept(composer, cx);
            if which == 0 {
                layout.categories = Some(PaletteLayout::column(start + dx, 110., others, total));
            } else {
                layout.list = Some(PaletteLayout::column(start + dx, 120., others, total));
            }
            keep(composer, layout, false, cx);
        }
    }
}

fn let_go(composer: &Composer, cx: &mut App) {
    let Some(drag) = DRAG.with_borrow_mut(|d| d.take()) else { return };
    let layout = kept(composer, cx);
    let layout = match (drag, LIVE.take()) {
        (Drag::Edge { edge, .. }, Some(size)) => edge.kept(layout, size),
        _ => layout,
    };
    keep(composer, layout, true, cx);
}

/// The bands on the palette's top, sides and top corners, over everything
/// else in it, and the accent edge while one's hovered or dragged.
pub(crate) fn edges(size: (f64, f64), cx: &mut Context<Composer>) -> Vec<AnyElement> {
    let lit = LIT.get() || DRAG.with_borrow(|d| matches!(d, Some(Drag::Edge { .. })));
    let accent = style::accent(cx);
    let mut out: Vec<AnyElement> = Vec::new();
    if lit {
        out.push(div().absolute().inset_0().rounded(style::px(RADIUS)).border_2().border_color(accent).into_any_element());
    }
    for (edge, name) in EDGE_NAMES {
        let band = div()
            .id(SharedString::from(format!("palette-edge-{name}")))
            .test_support()
            .role(Role::Splitter).aria_label(name)
            .absolute()
            .map(|d| match edge {
                PaletteEdge::Top => d.top_0().left(style::px(BAND)).right(style::px(BAND)).h(style::px(BAND)).cursor(CursorStyle::ResizeUpDown),
                PaletteEdge::Leading => d.left_0().top(style::px(BAND)).bottom_0().w(style::px(BAND)).cursor(CursorStyle::ResizeLeftRight),
                PaletteEdge::Trailing => d.right_0().top(style::px(BAND)).bottom_0().w(style::px(BAND)).cursor(CursorStyle::ResizeLeftRight),
                PaletteEdge::TopLeading => d.top_0().left_0().size(style::px(BAND)).cursor(CursorStyle::ResizeUpLeftDownRight),
                PaletteEdge::TopTrailing => d.top_0().right_0().size(style::px(BAND)).cursor(CursorStyle::ResizeUpRightDownLeft),
            })
            .on_hover(|on, window, _| {
                LIT.set(*on);
                window.refresh();
            })
            .tip(EDGE_TIP)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                    DRAG.with_borrow_mut(|d| *d = Some(Drag::Edge { edge, from: event.position, start: size }));
                    LIVE.set(Some(size));
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        out.push(band.into_any_element());
    }
    out
}

/// The line between two columns, grabbable a few points either side: drags
/// the column on its left; a double-click puts it back to its own width.
/// `which` 0 is the categories', 1 the list's; `width` the column's now,
/// `others` what the other columns need, `total` the palette's width.
pub(crate) fn column_rule(which: usize, width: f64, others: f64, total: f64, cx: &mut Context<Composer>) -> AnyElement {
    let dragging = DRAG.with_borrow(|d| matches!(d, Some(Drag::Column { which: w, .. }) if *w == which));
    let line = if dragging { style::accent(cx) } else { style::hairline(cx) };
    let accent = style::accent(cx);
    let group: SharedString = format!("palette-column-{which}").into();
    div()
        .id(SharedString::from(format!("palette-column-{which}")))
        .test_support()
        .role(Role::Splitter).aria_label(COLUMN_NAMES[which])
        .group(group.clone())
        .flex_none()
        .relative()
        .w(style::px(7.))
        .h_full()
        .flex()
        .justify_center()
        .cursor(CursorStyle::ResizeLeftRight)
        .child(div().w(gpui_kit::px(1.)).h_full().bg(line).group_hover(group, move |s| s.bg(accent)))
        .tip(COLUMN_TIPS[which])
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if event.click_count >= 2 {
                    // Its own width again.
                    let mut layout = kept(this, cx);
                    if which == 0 {
                        layout.categories = None;
                    } else {
                        layout.list = None;
                    }
                    keep(this, layout, true, cx);
                } else {
                    DRAG.with_borrow_mut(|d| *d = Some(Drag::Column { which, from: event.position, start: width, others, total }));
                }
                cx.notify();
            }),
        )
        .into_any_element()
}

/// The palette's right-click menu (its `iconContextMenu`): the default size,
/// or the columns' own widths.
pub(crate) fn menu(composer: WeakEntity<Composer>, menu: gpui_kit::component::menu::PopupMenu) -> gpui_kit::component::menu::PopupMenu {
    use gpui_kit::component::menu::PopupMenuItem;
    let (size, columns) = (composer.clone(), composer);
    menu.item(PopupMenuItem::new("Default Size").icon(gpui_kit::component::Icon::from(gpui_kit::assets::IconName::Shrink)).on_click(move |_, _, cx| {
        if let Some(c) = size.upgrade() {
            c.update(cx, |c, cx| {
                let mut layout = kept(c, cx);
                layout.width = None;
                layout.height = None;
                keep(c, layout, true, cx);
                cx.notify();
            });
        }
    }))
    .item(PopupMenuItem::new("Default Column Widths").icon(gpui_kit::component::Icon::from(gpui_kit::assets::IconName::Columns3)).on_click(move |_, _, cx| {
        if let Some(c) = columns.upgrade() {
            c.update(cx, |c, cx| {
                let mut layout = kept(c, cx);
                layout.categories = None;
                layout.list = None;
                keep(c, layout, true, cx);
                cx.notify();
            });
        }
    }))
}

/// The automation's `palette-drag` (GPUI only: the Swift palette's edges
/// follow the real pointer): a press on an edge or a column's line, a move
/// `dx` and `dy` points with the button held, and the release, through real
/// events and hit testing.
#[cfg(feature = "automation")]
pub(crate) fn drag_command(cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    let name = match cmd["edge"].as_str().unwrap_or("") {
        "top" => EDGE_NAMES[0].1,
        "left" => EDGE_NAMES[1].1,
        "right" => EDGE_NAMES[2].1,
        "topLeft" => EDGE_NAMES[3].1,
        "topRight" => EDGE_NAMES[4].1,
        "categories" => COLUMN_NAMES[0],
        "list" => COLUMN_NAMES[1],
        other => return Err(format!("palette-drag takes top, left, right, topLeft, topRight, categories or list, not {other:?}")),
    };
    window.render_frame(cx);
    let Some(handle) = window.find_all_by_label(name).into_iter().find(|e| e.visible()) else { return Err("the palette isn't open".into()) };
    let from = handle.visible_bounds().center();
    let scale = style::zoomed(1.);
    let to = point(from.x + gpui_kit::px(cmd["dx"].as_f64().unwrap_or(0.) as f32 * scale), from.y + gpui_kit::px(cmd["dy"].as_f64().unwrap_or(0.) as f32 * scale));
    window.pointer_move(from, None, cx);
    window.pointer_down(from, MouseButton::Left, cx);
    window.render_frame(cx);
    // In a few steps, as a hand moves.
    for step in 1..=4 {
        let t = step as f32 / 4.;
        window.pointer_move(point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t), Some(MouseButton::Left), cx);
        window.render_frame(cx);
    }
    window.pointer_up(to, MouseButton::Left, cx);
    window.render_frame(cx);
    Ok(serde_json::json!("ok"))
}

/// The automation's `palette-menu`: a right-click on the palette through
/// real events, then its menu read back, or `click` one of its items.
#[cfg(feature = "automation")]
pub(crate) async fn menu_command(window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    cx.update_window(window, |_, window, cx| {
        crate::thread::scripted_input(|| {
            window.render_frame(cx);
            let Some(top) = window.find_all_by_label(EDGE_NAMES[0].1).into_iter().find(|e| e.visible()) else { return Err("the palette isn't open".to_owned()) };
            // Inside the palette, under its top edge.
            let at = point(top.bounds().center().x, top.bounds().bottom() + gpui_kit::px(30.));
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Right, cx);
            window.pointer_up(at, MouseButton::Right, cx);
            Ok(())
        })
    })
    .map_err(|e| e.to_string())??;
    cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
    let items = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            crate::reply_links::open_menu_items(window, 0)
        })
        .map_err(|e| e.to_string())?;
    if let Some(title) = cmd["click"].as_str() {
        let Some((_, item)) = items.iter().find(|(t, _)| t == title) else { return Err(format!("no item {title} in {items:?}")) };
        let at = item.center();
        cx.update_window(window, |_, window, cx| {
            crate::thread::scripted_input(|| {
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Left, cx);
                window.pointer_up(at, MouseButton::Left, cx);
            })
        })
        .map_err(|e| e.to_string())?;
        return Ok(serde_json::json!("ok"));
    }
    let titles: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
    if !titles.is_empty() {
        cx.update_window(window, |_, window, cx| window.press("escape", cx)).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!(format!("ok {}", serde_json::to_string(&titles).unwrap_or_default())))
}
