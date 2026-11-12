// The transcript's ```mermaid and ```html blocks drawn into pictures, as the
// Swift app's RichRenderer draws them with offscreen web views, in two sealed
// pages made the first time one's needed and kept (so a thread without any
// costs nothing):
//
// - Diagrams: the app's own page (threadbear_core::rich_block::diagram_page),
//   whose policy lets none of its scripts run, with Pele (pele.run, bundled,
//   MIT) and the draw script run beside it in a world of their own, as the
//   Swift app's content world. The diagram's source goes in as a JSON string,
//   never markup; Pele's SVG has no scripts or handlers, and links and
//   pictures are off.
// - Pages: the agent's HTML behind the same policy, with JavaScript off in
//   that page altogether, measured and pictured through DevTools.
//
// Both in each look, at two pixels a point (the box scales them), and both
// sealed by sealed.rs: no network, no navigation, nothing kept.
use super::offscreen::{base64_encode, picture, Offscreen, Options};
use serde_json::{json, Value as Json};
use std::cell::RefCell;
use std::rc::Rc;
use threadbear_core::rich_block as rich;

/// Pele, the diagram library the Swift app bundles (its notice is
/// pele-LICENSE.txt beside it), built into the app the same way.
const PELE: &str = include_str!("../../../../app/Sources/Threadbear/Resources/Rich/pele.js");

/// A page is laid out this wide, in points; the box scales the picture to
/// the reading column (RichRenderer.pageWidth).
pub const PAGE_WIDTH: f64 = 720.;
/// The tallest a page is drawn; past it the box fades out.
pub const PAGE_MAX_HEIGHT: f64 = 1600.;
/// At most about a million points in a picture (four million pixels at two
/// a point): more than the box ever shows (RichRenderer.snapshotPoints).
const PICTURE_POINTS: f64 = 1_000_000.;

/// A block drawn: its size in points at the zoom it was drawn for, its
/// picture in each look, and the page its full-size window shows.
pub struct Drawn {
    pub width: f32,
    pub height: f32,
    /// A page taller than is drawn: the box shows its top and fades out.
    pub clipped: bool,
    pub dark: Vec<u8>,
    pub light: Vec<u8>,
    pub window_page: String,
}

struct DiagramView {
    page: Offscreen,
    /// The world Pele and the draw script run in, apart from the page's.
    world: i64,
}

thread_local! {
    static DIAGRAMS: RefCell<Option<Rc<DiagramView>>> = const { RefCell::new(None) };
    static PAGES: RefCell<Option<Rc<Offscreen>>> = const { RefCell::new(None) };
}

/// Drops both views, for a drawing that took too long (a stuck page is
/// never waited on: the next block gets new ones).
pub fn reset() {
    DIAGRAMS.with(|d| d.borrow_mut().take());
    PAGES.with(|p| p.borrow_mut().take());
}

/// One of the app's own pages as an address, its bytes in the address
/// itself, so there's nothing for it to be served from.
pub fn data_url(html: &str) -> String {
    format!("data:text/html;charset=utf-8;base64,{}", base64_encode(html.as_bytes()))
}

async fn call(page: &Offscreen, method: &str, params: Json) -> Result<Json, String> {
    page.devtools(method, params).await.map_err(|_| "its page went away".to_owned())?
}

/// Transparent where the page draws nothing, so a diagram sits on the box's
/// own fill (the Swift views don't draw a background either).
async fn see_through(page: &Offscreen) -> Result<(), String> {
    call(page, "Emulation.setDefaultBackgroundColorOverride", json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } })).await.map(|_| ())
}

async fn diagram_view(scale: f32) -> Result<Rc<DiagramView>, String> {
    if let Some(view) = DIAGRAMS.with(|d| d.borrow().clone()).filter(|v| !v.page.stopped()) {
        return Ok(view);
    }
    // Room for a wide diagram to lay out at its own size; a picture bigger
    // than the view is taken beyond it.
    let (page, loaded) = Offscreen::open(&data_url(&rich::diagram_page()), Options { javascript: true, width: 1200, height: 900, scale })?;
    loaded.await.map_err(|_| "the diagram page went away".to_owned())??;
    see_through(&page).await?;
    let tree = call(&page, "Page.getFrameTree", json!({})).await?;
    let frame = tree["frameTree"]["frame"]["id"].as_str().ok_or("the diagram page has no frame")?.to_owned();
    let world = call(&page, "Page.createIsolatedWorld", json!({ "frameId": frame, "worldName": "threadbear-rich", "grantUniveralAccess": false })).await?;
    let world = world["executionContextId"].as_i64().ok_or("the diagram page's world wasn't made")?;
    let loaded = call(&page, "Runtime.evaluate", json!({ "expression": PELE, "contextId": world, "returnByValue": false, "silent": true })).await?;
    if loaded.get("exceptionDetails").is_some() {
        return Err("the diagram library didn't load".into());
    }
    let view = Rc::new(DiagramView { page, world });
    DIAGRAMS.with(|d| *d.borrow_mut() = Some(view.clone()));
    Ok(view)
}

/// The scale a picture of `width` by `height` points is taken at, so it's
/// never more than PICTURE_POINTS.
fn capped(width: f64, height: f64) -> f64 {
    (PICTURE_POINTS / (width * height).max(1.)).sqrt().min(1.)
}

/// Draws a diagram in both looks, at `zoom` percent of the transcript's type.
pub async fn draw_diagram(source: &str, zoom: u32, scale: f32) -> Result<Drawn, String> {
    let view = diagram_view(scale).await?;
    let font_size = 14. * f64::from(zoom) / 100.;
    let mut looks = Vec::new();
    for dark in [true, false] {
        // Room to lay out at its own size before it's measured.
        view.page.resize(1200, 900);
        let answer = call(&view.page, "Runtime.evaluate", json!({ "expression": rich::draw_call(source, dark, font_size), "contextId": view.world, "returnByValue": true, "silent": true })).await?;
        if answer.get("exceptionDetails").is_some() {
            return Err("the diagram library stopped".into());
        }
        let value = &answer["result"]["value"];
        if let Some(error) = value["error"].as_str() {
            return Err(error.to_owned());
        }
        let (width, height) = (value["width"].as_f64().unwrap_or(0.), value["height"].as_f64().unwrap_or(0.));
        if width <= 0. || height <= 0. {
            return Err("the diagram came out empty".into());
        }
        let svg = value["svg"].as_str().unwrap_or_default().to_owned();
        let png = capture(&view.page, width, height, f64::from(scale) * capped(width, height)).await?;
        looks.push((width, height, svg, png));
    }
    let (light, dark) = (looks.pop().ok_or("no picture")?, looks.pop().ok_or("no picture")?);
    Ok(Drawn { width: dark.0 as f32, height: dark.1 as f32, clipped: false, window_page: rich::diagram_pane_page(&dark.2, &light.2, "Diagram"), dark: dark.3, light: light.3 })
}

async fn page_view(scale: f32) -> Result<Rc<Offscreen>, String> {
    if let Some(view) = PAGES.with(|p| p.borrow().clone()).filter(|v| !v.stopped()) {
        return Ok(view);
    }
    let (page, loaded) = Offscreen::open("about:blank", Options { javascript: false, width: PAGE_WIDTH as i32, height: 1, scale })?;
    loaded.await.map_err(|_| "the page went away".to_owned())??;
    see_through(&page).await?;
    let view = Rc::new(page);
    PAGES.with(|p| *p.borrow_mut() = Some(view.clone()));
    Ok(view)
}

/// The view made the picture's size, `width` by `height` points at `scale`
/// pixels each, painted that size, and pictured: a picture is only ever of
/// what's inside the view (a page measured in a view a point tall and
/// pictured beyond it came out blank).
async fn capture(view: &Offscreen, width: f64, height: f64, scale: f64) -> Result<Vec<u8>, String> {
    view.resize_and_paint(width.ceil() as i32, height.ceil() as i32, scale as f32).await.map_err(|_| "its page went away".to_owned())??;
    picture(view, width, height).await
}

/// Draws an agent's html in both looks. The page is laid out 720 points wide
/// at `zoom` percent: narrower in CSS pixels and that much bigger each, as
/// the Swift app's pageZoom does.
pub async fn draw_page(html: &str, zoom: u32, scale: f32) -> Result<Drawn, String> {
    let view = page_view(scale).await?;
    let zoom = f64::from(zoom.max(10)) / 100.;
    let (css_width, pixels) = (PAGE_WIDTH / zoom, f64::from(scale) * zoom);
    let page = rich::html_page(html);
    view.load(&data_url(&page)).await.map_err(|_| "the page went away".to_owned())??;
    let mut looks = Vec::new();
    for dark in [true, false] {
        call(&view, "Emulation.setEmulatedMedia", json!({ "features": [{ "name": "prefers-color-scheme", "value": if dark { "dark" } else { "light" } }] })).await?;
        // A view a point tall to measure in: the content's never measured
        // shorter than the view, so a tall one would make a short page that tall.
        view.resize_and_paint(css_width.ceil() as i32, 1, pixels as f32).await.map_err(|_| "the page went away".to_owned())??;
        let metrics = call(&view, "Page.getLayoutMetrics", json!({})).await?;
        let height = metrics["cssContentSize"]["height"].as_f64().or_else(|| metrics["contentSize"]["height"].as_f64()).unwrap_or(1.).max(1.);
        let shown = height.min(PAGE_MAX_HEIGHT / zoom).ceil();
        // A page is at most 720 by 1600 points, a picture well within budget.
        let png = capture(&view, css_width, shown, pixels).await?;
        looks.push(((shown * zoom) as f32, height > PAGE_MAX_HEIGHT / zoom, png));
    }
    // Let go of the agent's page until the next one.
    let _ = view.load("about:blank").await;
    let (light, dark) = (looks.pop().ok_or("no picture")?, looks.pop().ok_or("no picture")?);
    Ok(Drawn { width: PAGE_WIDTH as f32, height: dark.0.max(light.0), clipped: dark.1 || light.1, window_page: page, dark: dark.2, light: light.2 })
}
