// The Terminal's screen as a GPUI element of its own: the grid of cells in the
// panel's colours and the system's monospaced font, the cursor, the selection,
// and what an input method is still composing. The screen itself is
// alacritty_terminal's, held by terminal.rs's Session; this only reads it, and
// hands the pointer and the input method back to the session.
use crate::style;
use crate::terminal::{DrawnAt, GridSize, Session};
use alacritty_terminal::index::Point as GridPoint;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors as TermColours;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use std::sync::OnceLock;
use threadbear_core::terminal::{self as rules, Rgb8};

/// The terminal's colours (`TerminalStyle.apply`): the transcript's own
/// background and text, light or dark with the app, the accent for the
/// cursor and the system's selection colour, and the 256 numbered colours
/// worked out from them as SwiftTerm does.
pub(crate) struct Colours {
    pub(crate) background: Rgb8,
    pub(crate) foreground: Rgb8,
    cursor: Hsla,
    selection: Hsla,
    palette: [Rgb8; 256],
}

impl Colours {
    pub(crate) fn current(cx: &App) -> Colours {
        // Made once per look: the palette takes a few hundred colour conversions.
        static DARK: OnceLock<[Rgb8; 256]> = OnceLock::new();
        static LIGHT: OnceLock<[Rgb8; 256]> = OnceLock::new();
        let dark = cx.theme().is_dark();
        // sRGB(0.11, 0.11, 0.12) and white at 0.9 on dark; white and 0.12 on light.
        let (background, foreground) = if dark { ((28, 28, 31), (230, 230, 230)) } else { ((255, 255, 255), (31, 31, 31)) };
        let palette = *(if dark { &DARK } else { &LIGHT }).get_or_init(|| rules::palette(background, foreground));
        // NSColor.selectedTextBackgroundColor in each look.
        let selection = if dark { rgb_hsla((63, 99, 139)) } else { rgb_hsla((179, 215, 255)) };
        Colours { background, foreground, cursor: style::control_accent(cx), selection, palette }
    }

    pub(crate) fn background_hsla(&self) -> Hsla {
        rgb_hsla(self.background)
    }

    /// A numbered colour, or the terminal's own (256 the text, 257 the
    /// background), for a program that asks (OSC 4, 10 and 11).
    pub(crate) fn indexed(&self, index: usize) -> Rgb8 {
        match index {
            0..=255 => self.palette[index],
            257 => self.background,
            _ => self.foreground,
        }
    }

    /// A cell's colour: one the program set with an escape code (OSC 4) wins
    /// over ours.
    fn resolve(&self, colour: Color, set: &TermColours) -> Rgb8 {
        let from_set = |i: usize| set[i].map(|c| (c.r, c.g, c.b));
        match colour {
            Color::Spec(c) => (c.r, c.g, c.b),
            Color::Indexed(i) => from_set(i as usize).unwrap_or(self.palette[i as usize]),
            Color::Named(name) => from_set(name as usize).unwrap_or_else(|| self.named(name)),
        }
    }

    /// A cell's text colour: bold text in one of the first eight colours is
    /// drawn in its bright one, as SwiftTerm and Terminal draw it (a zsh
    /// theme's bold cyan prompt comes out bright cyan).
    fn text(&self, colour: Color, bold: bool, set: &TermColours) -> Rgb8 {
        let colour = match colour {
            Color::Named(name) if bold && (name as usize) < 8 => Color::Indexed(name as u8 + 8),
            Color::Indexed(i) if bold && i < 8 => Color::Indexed(i + 8),
            other => other,
        };
        self.resolve(colour, set)
    }

    fn named(&self, name: NamedColor) -> Rgb8 {
        use NamedColor::*;
        match name {
            Foreground | BrightForeground => self.foreground,
            Background => self.background,
            Cursor => hsla_rgb(self.cursor),
            DimForeground => dim(self.foreground, self.background),
            DimBlack | DimRed | DimGreen | DimYellow | DimBlue | DimMagenta | DimCyan | DimWhite => {
                let base = name as usize - DimBlack as usize;
                dim(self.palette[base], self.background)
            }
            other => self.palette[(other as usize).min(15)],
        }
    }
}

/// Faint text (SGR 2): two thirds of the way from the background.
fn dim(colour: Rgb8, background: Rgb8) -> Rgb8 {
    let mix = |c: u8, b: u8| ((f32::from(c) * 2. + f32::from(b)) / 3.).round() as u8;
    (mix(colour.0, background.0), mix(colour.1, background.1), mix(colour.2, background.2))
}

fn rgb_hsla((r, g, b): Rgb8) -> Hsla {
    Hsla::from(Rgba { r: f32::from(r) / 255., g: f32::from(g) / 255., b: f32::from(b) / 255., a: 1. })
}

fn hsla_rgb(colour: Hsla) -> Rgb8 {
    let c = Rgba::from(colour);
    ((c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8)
}

/// The font: the Mac's own monospaced system font (SF Mono), as the Swift
/// app's NSFont.monospacedSystemFont, one point over the transcript's code. It
/// can only be named, not listed, so it's checked to have loaded as itself;
/// elsewhere, or if it hasn't, the theme's monospaced font.
fn family(cx: &App) -> SharedString {
    static FAMILY: OnceLock<SharedString> = OnceLock::new();
    FAMILY
        .get_or_init(|| {
            if cfg!(target_os = "macos") {
                let system = font(".AppleSystemUIFontMonospaced");
                let text = cx.text_system();
                let id = text.resolve_font(&system);
                if text.get_font_for_id(id).is_some_and(|f| f.family == system.family) {
                    return system.family;
                }
            }
            cx.theme().mono_font_family.clone()
        })
        .clone()
}

struct Metrics {
    font: Font,
    size: Pixels,
    cell_width: f32,
    cell_height: f32,
}

/// The cell, as SwiftTerm measures it: a W's advance, and the font's ascent
/// and descent rounded up, each snapped to the screen's pixels.
fn metrics(window: &Window, cx: &App) -> Metrics {
    let font = font(family(cx));
    let size = gpui_kit::px(style::zoomed(style::size::CODE + 1.));
    let text = window.text_system();
    let id = text.resolve_font(&font);
    let advance = text.advance(id, size, 'W').map(|s| f32::from(s.width)).unwrap_or(f32::from(size) * 0.6);
    let height = (f32::from(text.ascent(id, size)) + f32::from(text.descent(id, size)).abs()).ceil();
    let scale = window.scale_factor();
    Metrics { font, size, cell_width: ((advance * scale).round() / scale).max(1.), cell_height: ((height * scale).ceil() / scale).max(1.) }
}

pub(crate) struct TerminalElement {
    session: Entity<Session>,
    focused: bool,
    /// The blink's shown half (Session::blink_shown).
    blink_shown: bool,
}

impl TerminalElement {
    pub(crate) fn new(session: Entity<Session>, focused: bool, blink_shown: bool) -> Self {
        TerminalElement { session, focused, blink_shown }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

enum CursorLook {
    /// The cell drawn in the accent, its character in the background colour.
    Block,
    Outline(Bounds<Pixels>, Hsla),
    Bar(Bounds<Pixels>, Hsla),
}

pub(crate) struct Prepainted {
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Option<CursorLook>,
    marked: Option<(Bounds<Pixels>, ShapedLine)>,
    hitbox: Hitbox,
    cell_height: Pixels,
}

/// Cells in a row that draw alike, as one piece of text.
struct Run {
    column: usize,
    cells: usize,
    text: String,
    colour: Rgb8,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    /// A double-width character (CJK, most emoji), drawn on its own.
    wide: bool,
}

impl Run {
    fn same_look(&self, colour: Rgb8, bold: bool, italic: bool, underline: bool, strike: bool) -> bool {
        !self.wide && self.colour == colour && self.bold == bold && self.italic == italic && self.underline == underline && self.strike == strike
    }
}

/// Neighbouring cells with the same background are one rectangle.
fn push_background(row: usize, col: usize, cells: usize, colour: Rgb8, backgrounds: &mut Vec<(usize, usize, usize, Rgb8)>) {
    if let Some(last) = backgrounds.last_mut() {
        if last.0 == row && last.1 + last.2 == col && last.3 == colour {
            last.2 += cells;
            return;
        }
    }
    backgrounds.push((row, col, cells, colour));
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Prepainted {
        let m = metrics(window, cx);
        let (cw, ch) = (m.cell_width, m.cell_height);
        let cols = ((f32::from(bounds.size.width) / cw).floor() as u16).max(2);
        let rows = ((f32::from(bounds.size.height) / ch).floor() as u16).max(1);
        let grid = GridSize { cols, rows, cell_width: cw, cell_height: ch };
        // The panel's size is the shell's: the grid follows it, and the pty
        // tells the program (SIGWINCH).
        self.session.update(cx, |s, _| {
            s.resize(grid);
            s.drawn_at = Some(DrawnAt { origin: bounds.origin, cell_width: gpui_kit::px(cw), cell_height: gpui_kit::px(ch) });
        });
        crate::terminal::note_size(grid, cx);
        let colours = Colours::current(cx);
        let default_bg = colours.background;
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let cell_rect = |col: usize, row: usize, cells: usize| Bounds { origin: point(bounds.origin.x + gpui_kit::px(cw * col as f32), bounds.origin.y + gpui_kit::px(ch * row as f32)), size: size(gpui_kit::px(cw * cells as f32), gpui_kit::px(ch)) };

        let session = self.session.read(cx);
        let marked = session.marked.clone();
        let term = session.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let selection = content.selection;
        let cursor = content.cursor;
        let cursor_row = cursor.point.line.0 + offset;
        // Its blink's gone half, unless a program asked for a steady cursor
        // or another view has the keyboard (then it's a steady outline).
        let blinked_out = self.focused && !self.blink_shown && term.cursor_style().blinking;
        let cursor_shown = cursor.shape != CursorShape::Hidden && (0..rows as i32).contains(&cursor_row) && !blinked_out;
        // Focused, the usual block in the accent; not, an outline of it, as
        // SwiftTerm draws it when another view has the keyboard.
        let solid_cursor = cursor_shown && self.focused && cursor.shape == CursorShape::Block && marked.is_empty();
        let set = content.colors;

        let mut backgrounds: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
        let mut runs: Vec<(usize, Run)> = Vec::new();
        let mut bg_cells: Vec<(usize, usize, usize, Rgb8)> = Vec::new();
        for indexed in content.display_iter {
            let point = indexed.point;
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = (point.line.0 + offset) as usize;
            let col = point.column.0;
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            let cells = if wide { 2 } else { 1 };
            let mut fg = colours.text(cell.fg, cell.flags.contains(Flags::BOLD), set);
            let mut bg = colours.resolve(cell.bg, set);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg = dim(fg, bg);
            }
            let selected = selection.is_some_and(|s| s.contains(point) || (wide && s.contains(GridPoint::new(point.line, point.column + 1))));
            if selected {
                bg = hsla_rgb(colours.selection);
            }
            if cell.flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            if solid_cursor && point == cursor.point {
                bg = hsla_rgb(colours.cursor);
                fg = default_bg;
            }
            if bg != default_bg {
                push_background(row, col, cells, bg, &mut bg_cells);
            }
            let c = if cell.c.is_control() || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) { ' ' } else { cell.c };
            let (bold, italic) = (cell.flags.contains(Flags::BOLD), cell.flags.contains(Flags::ITALIC));
            let (underline, strike) = (cell.flags.intersects(Flags::ALL_UNDERLINES), cell.flags.contains(Flags::STRIKEOUT));
            let extend = runs.last().is_some_and(|(r, run)| *r == row && run.column + run.cells == col && run.same_look(fg, bold, italic, underline, strike) && !wide);
            if !extend {
                runs.push((row, Run { column: col, cells: 0, text: String::new(), colour: fg, bold, italic, underline, strike, wide }));
            }
            let (_, run) = runs.last_mut().expect("a run");
            run.text.push(c);
            for extra in cell.zerowidth().into_iter().flatten() {
                run.text.push(*extra);
            }
            run.cells += cells;
        }
        for (row, col, cells, colour) in bg_cells {
            backgrounds.push((cell_rect(col, row, cells), rgb_hsla(colour)));
        }
        let cursor_look = if !cursor_shown || !marked.is_empty() {
            None
        } else if solid_cursor {
            Some(CursorLook::Block)
        } else {
            let rect = cell_rect(cursor.point.column.0, cursor_row as usize, 1);
            match cursor.shape {
                CursorShape::Beam => Some(CursorLook::Bar(Bounds { origin: rect.origin, size: size(gpui_kit::px(2.), rect.size.height) }, colours.cursor)),
                CursorShape::Underline => Some(CursorLook::Bar(Bounds { origin: point(rect.origin.x, rect.origin.y + rect.size.height - gpui_kit::px(2.)), size: size(rect.size.width, gpui_kit::px(2.)) }, colours.cursor)),
                _ => Some(CursorLook::Outline(rect, colours.cursor)),
            }
        };
        let cursor_cell = (cursor.point.column.0, cursor_row.max(0) as usize);
        drop(term);

        // Shaped once the lock's let go: the reader thread waits on it.
        let text = window.text_system().clone();
        let mut lines = Vec::with_capacity(runs.len());
        for (row, run) in runs {
            // A run of plain spaces draws nothing.
            if !run.underline && !run.strike && run.text.chars().all(|c| c == ' ') {
                continue;
            }
            let mut font = m.font.clone();
            if run.bold {
                font = font.bold();
            }
            if run.italic {
                font = font.italic();
            }
            let colour = rgb_hsla(run.colour);
            let style = TextRun {
                len: run.text.len(),
                font,
                color: colour,
                background_color: None,
                underline: run.underline.then_some(UnderlineStyle { thickness: gpui_kit::px(1.), color: Some(colour), wavy: false }),
                strikethrough: run.strike.then_some(StrikethroughStyle { thickness: gpui_kit::px(1.), color: Some(colour) }),
            };
            // Every character in its own cell, whatever its glyph's width.
            let force = (!run.wide).then_some(gpui_kit::px(cw));
            let shaped = text.shape_line(run.text.into(), m.size, &[style], force);
            lines.push((cell_rect(run.column, row, 1).origin, shaped));
        }
        let marked = (!marked.is_empty()).then(|| {
            let colour = rgb_hsla(colours.foreground);
            let style = TextRun { len: marked.len(), font: m.font.clone(), color: colour, background_color: None, underline: Some(UnderlineStyle { thickness: gpui_kit::px(1.), color: Some(colour), wavy: false }), strikethrough: None };
            let shaped = text.shape_line(marked.into(), m.size, &[style], None);
            let rect = cell_rect(cursor_cell.0, cursor_cell.1, 1);
            (Bounds { origin: rect.origin, size: size(shaped.width.max(rect.size.width), rect.size.height) }, shaped)
        });
        Prepainted { backgrounds, lines, cursor: cursor_look, marked, hitbox, cell_height: gpui_kit::px(ch) }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), p: &mut Prepainted, window: &mut Window, cx: &mut App) {
        let background = Colours::current(cx).background_hsla();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (rect, colour) in &p.backgrounds {
                window.paint_quad(fill(*rect, *colour));
            }
            for (origin, line) in &p.lines {
                let _ = line.paint(*origin, p.cell_height, TextAlign::Left, None, window, cx);
            }
            match &p.cursor {
                Some(CursorLook::Outline(rect, colour)) => window.paint_quad(outline(*rect, *colour, BorderStyle::Solid)),
                Some(CursorLook::Bar(rect, colour)) => window.paint_quad(fill(*rect, *colour)),
                Some(CursorLook::Block) | None => {}
            }
            if let Some((rect, line)) = &p.marked {
                window.paint_quad(fill(*rect, background));
                let _ = line.paint(rect.origin, p.cell_height, TextAlign::Left, None, window, cx);
            }
        });

        let focus = self.session.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.session.clone()), cx);
        window.set_cursor_style(CursorStyle::IBeam, &p.hitbox);

        let (session, hitbox) = (self.session.clone(), p.hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                session.update(cx, |s, cx| s.mouse_down(event, window, cx));
                cx.stop_propagation();
            }
        });
        let session = self.session.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble {
                session.update(cx, |s, cx| s.mouse_moved(event, cx));
            }
        });
        let session = self.session.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble {
                session.update(cx, |s, cx| s.mouse_up(event, cx));
            }
        });
        let (session, hitbox) = (self.session.clone(), p.hitbox.clone());
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                session.update(cx, |s, cx| s.scrolled(event, cx));
            }
        });
    }
}
