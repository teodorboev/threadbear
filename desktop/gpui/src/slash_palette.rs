// The slash command palette over the composer: SlashPaletteView.swift, drawn in
// the window above the box (the Swift app gives it a panel window of its own;
// here it's a deferred layer on the same window, which is what keeps it clear
// of the transcript). Its rules are in threadbear_core::slash; the arrows, Tab,
// Return and Esc reach it from the box's key handling, so the cursor stays in
// the box and typing keeps narrowing the list (composer.rs, `palette_key`).
use crate::app::Threadbear;
use crate::composer::Composer;
use crate::prefs;
use crate::style::{self, size};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::json;
#[cfg(feature = "automation")]
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use threadbear_core::composer::effort_title;
use threadbear_core::logins::model_title_with;
use threadbear_core::slash::{self, PaletteKey, PaletteState, SlashCommand};
use crate::tips::Tip as _;

/// The commands each thread has offered (`AppModel.slashCommands`); the app
/// holds one. Asked for each time a thread's palette opens, as the runner
/// keeps each folder's list for ten minutes and new skills show up that way.
#[derive(Default)]
pub(crate) struct SlashState {
    pub(crate) commands: HashMap<String, Vec<SlashCommand>>,
    pub(crate) fetching: HashSet<String>,
}

impl Threadbear {
    /// `loadSlashCommands`: an older runner (-32601) or one that couldn't
    /// list them: the palette says there are none rather than spinning.
    pub(crate) fn load_slash_commands(&mut self, thread_id: String, cx: &mut Context<Self>) {
        if !self.slash.fetching.insert(thread_id.clone()) {
            return;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = crate::call(client, "threads.commands", json!({ "threadId": thread_id })).await;
            this.update(cx, |this, cx| {
                this.slash.fetching.remove(&thread_id);
                match res {
                    Ok(value) => {
                        this.slash.commands.insert(thread_id.clone(), slash::decode_commands(&value));
                    }
                    Err(_) => {
                        this.slash.commands.entry(thread_id.clone()).or_default();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Threadbear {
    /// `newThreadCommandsKey`: where a new thread's palette keeps its list,
    /// per runner, login and folder, so changing any of them in the box's
    /// chips asks again.
    pub(crate) fn new_thread_commands_key(&self) -> String {
        let logins = self.draft_logins();
        let (account, cwd) = self.with_draft(&logins, |d| (d.account_id(), d.cwd()));
        format!("new\0{}\0{account}\0{cwd}", self.new_thread.picks.runner.as_deref().unwrap_or(""))
    }

    /// `loadNewThreadSlashCommands`: the same list for the new-thread page's
    /// box, before there's a thread (`folders.commands`), for the login,
    /// folder and mode it would start with. Another runner's draft isn't
    /// asked yet, as in the Swift app: its palette lists none.
    pub(crate) fn load_new_thread_slash_commands(&mut self, cx: &mut Context<Self>) {
        let key = self.new_thread_commands_key();
        if self.slash.fetching.contains(&key) {
            return;
        }
        if self.new_thread.picks.runner.is_some() {
            self.slash.commands.entry(key).or_default();
            cx.notify();
            return;
        }
        let logins = self.draft_logins();
        let (account, cwd, permission) = self.with_draft(&logins, |d| (d.account_id(), d.cwd(), d.permission()));
        let mut params = json!({ "cwd": cwd, "permissionMode": permission });
        if !account.is_empty() {
            params["accountId"] = json!(account);
        }
        self.slash.fetching.insert(key.clone());
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = crate::call(client, "folders.commands", params).await;
            this.update(cx, |this, cx| {
                this.slash.fetching.remove(&key);
                match res {
                    Ok(value) => {
                        this.slash.commands.insert(key.clone(), slash::decode_commands(&value));
                    }
                    // An older runner (-32601): none, as for a thread.
                    Err(_) => {
                        this.slash.commands.entry(key.clone()).or_default();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// What a palette shows for what's typed: read each time it's drawn.
struct PaletteView {
    all: Vec<SlashCommand>,
    /// The runner hasn't answered yet for this thread.
    loading: bool,
    query: String,
    recent: Vec<String>,
    provider: String,
    /// `paletteCurrent`: what /model and /effort would change, as it is now.
    current: HashMap<String, String>,
    /// Its command files are this Mac's to open only on this Mac's threads.
    can_open_files: bool,
    home: String,
    /// Typed partway through a message: only the skills the agent can run
    /// there are listed, and a pick goes into the words (Insert), never sent.
    mid_message: bool,
}

impl Composer {
    /// `paletteTyped`: the command being typed as the message's last word,
    /// at its start or partway through it (skills named mid-message, 9 Nov
    /// 2026), in a new thread too (`folders.commands`), and not once Esc
    /// closed it at this text.
    pub(crate) fn palette_typed(&self, text: &str) -> Option<slash::Typed> {
        if self.palette.closed_at.as_deref() == Some(text) {
            return None;
        }
        slash::typed(text)
    }

    /// The palette while it's open. Partway through a message it only opens
    /// on a match, so a slash in other words ("the /tmp folder") doesn't get
    /// in the way.
    fn palette_open(&self, text: &str, cx: &App) -> Option<PaletteView> {
        let typed = self.palette_typed(text)?;
        let view = self.palette_view(&typed, cx);
        if view.mid_message && slash::matches(&view.all, &view.query, slash::Category::All, &[]).is_empty() {
            return None;
        }
        Some(view)
    }

    /// Where the palette's list is kept: the thread's id, or for a new
    /// thread its runner, login and folder's (`paletteListKey`).
    fn palette_list_key(&self, cx: &App) -> String {
        match (&self.thread_id, self.app.upgrade()) {
            (Some(id), _) => id.clone(),
            (None, Some(app)) => app.read(cx).new_thread_commands_key(),
            (None, None) => String::new(),
        }
    }

    /// Esc closed the palette at that text only: any change reopens it, even
    /// back to the same words later. And the list is asked for each time a
    /// command starts being typed, the thread's or the new thread's.
    pub(crate) fn palette_text_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        if self.palette.closed_at.as_deref().is_some_and(|c| c != text) {
            self.palette.closed_at = None;
        }
        if self.palette_typed(&text).is_none() {
            self.palette_asked = None;
            return;
        }
        let key = self.palette_list_key(cx);
        if self.palette_asked.as_deref() == Some(key.as_str()) {
            return;
        }
        self.palette_asked = Some(key);
        if let Some(app) = self.app.upgrade() {
            match self.thread_id.clone() {
                Some(id) => app.update(cx, |app, cx| app.load_slash_commands(id, cx)),
                None => app.update(cx, |app, cx| app.load_new_thread_slash_commands(cx)),
            }
        }
    }

    /// `ComposerView.paletteKey`: Arrows, Tab, Return and Esc while the
    /// palette is open. False lets the box have the key as usual.
    pub(crate) fn palette_key(&mut self, key: PaletteKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let text = self.input.read(cx).value().to_string();
        // Partway through a message, a closed palette leaves the keys alone.
        let Some(view) = self.palette_open(&text, cx) else { return false };
        let matches = slash::matches(&view.all, &view.query, self.palette.category, &view.recent);
        // Return with nothing to pick sends what's typed, as words.
        if key == PaletteKey::Enter && matches.is_empty() {
            return false;
        }
        if let Some((command, run)) = slash::handle_key(key, &matches, &mut self.palette, &text) {
            // A skill named partway through a message isn't sent on its own.
            self.palette_pick(command, run && !view.mid_message, window, cx);
        }
        // Keep the pick in sight as the arrows move it (the list scrolls).
        if matches!(key, PaletteKey::Up | PaletteKey::Down) {
            if let Some(index) = matches.iter().position(|c| Some(c.name.as_str()) == self.palette.selected.as_deref()) {
                self.palette_scroll.scroll_to_item(index);
            }
        }
        cx.notify();
        true
    }

    /// `ComposerView.pick`: puts the command in the box in place of what was
    /// typed for it, to send now, or with a space after it for its arguments
    /// (or, partway through a message, the words after it). Only the typed
    /// word is replaced, so the words and chips before it stay, as one undo.
    pub(crate) fn palette_pick(&mut self, command: SlashCommand, run: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, _| {
                app.new_thread.prefs.recent_slash = slash::used(&command.name, &app.new_thread.prefs.recent_slash);
                prefs::save(&app.new_thread.prefs);
            });
        }
        self.palette = PaletteState::default();
        let text = slash::fill(&command, !run);
        let current = self.input.read(cx).value().to_string();
        let start = slash::typed(&current).map_or(0, |t| t.start);
        let end = current.len();
        self.input.update(cx, |s, cx| {
            s.set_selected_range(start..end, cx);
            s.replace(text, window, cx);
            s.focus(window, cx);
            crate::web_panels::keyboard_to_app(window);
        });
        if run {
            self.send(window, cx);
        }
        cx.notify();
    }

    fn palette_view(&self, typed: &slash::Typed, cx: &App) -> PaletteView {
        let key = self.palette_list_key(cx);
        let app = self.app.upgrade();
        let (mut all, loading, recent) = app.as_ref().map_or((Vec::new(), false, Vec::new()), |app| {
            let t = app.read(cx);
            (t.slash.commands.get(&key).cloned().unwrap_or_default(), !t.slash.commands.contains_key(&key), t.new_thread.prefs.recent_slash.clone())
        });
        let mid_message = typed.mid_message();
        if mid_message {
            all = slash::mid_message(&all);
        }
        let query = typed.query.clone();
        let data = self.chip_data(cx);
        let provider = data.summary.as_ref().map(|t| t.provider.clone()).unwrap_or_default();
        let mut current = HashMap::new();
        if let Some(thread) = &data.summary {
            let account = data.logins.account(&thread.account_id);
            let model = thread.model.clone().or_else(|| thread.model_in_use.clone()).or_else(|| account.and_then(|a| a.default_model.clone()));
            if let Some(model) = model {
                let choices = account.map_or(&[][..], |a| a.models.as_slice());
                current.insert("model".to_owned(), model_title_with(Some(&model), choices) + if thread.model.is_none() { ", the default" } else { "" });
            }
            if let Some(level) = thread.effort.clone().or_else(|| thread.effort_in_use.clone()) {
                current.insert("effort".to_owned(), effort_title(&level) + if thread.effort.is_none() { ", the model's own" } else { "" });
            }
        }
        let can_open_files = data.away.is_none();
        PaletteView { all, loading, query, recent, provider, current, can_open_files, home: std::env::var("HOME").unwrap_or_default(), mid_message }
    }

    /// The palette's layer over the box, or None while it's closed.
    pub(crate) fn render_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = self.input.read(cx).value().to_string();
        // A new thread's login or folder changed in its chips while a
        // command was being typed: ask for that one's list (the Swift
        // palette's `.task(id: paletteListKey)`).
        if self.palette_typed(&text).is_some() && self.palette_asked.as_deref() != Some(self.palette_list_key(cx).as_str()) {
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                me.update(cx, |this, cx| this.palette_text_changed(cx)).ok();
            });
        }
        let view = self.palette_open(&text, cx)?;
        let matches = slash::matches(&view.all, &view.query, self.palette.category, &view.recent);
        let picked = slash::picked(&matches, &self.palette).cloned();
        let categories = self.render_palette_categories(&view, cx);
        let list = self.render_palette_list(&view, &matches, picked.as_ref(), cx);
        let detail = self.render_palette_detail(&view, picked.as_ref(), cx);
        // Its size: what it was dragged to, kept within the window, else as
        // wide as the box and about half the window tall (slash_resize.rs).
        // In points, as kept; the window's are zoomed.
        let zoom = style::zoomed(1.);
        let viewport = window.viewport_size();
        let layout = crate::slash_resize::kept(self, cx);
        let auto_height = slash::palette_height(f64::from(viewport.height / zoom), 300., 560.);
        let (width, height) = crate::slash_resize::size(layout, f64::from(self.card_width.get() / zoom), size(viewport.width / zoom, viewport.height / zoom), auto_height);
        // No taller than the room over the box, so it doesn't run off the
        // top of the window where the box sits high up (the new-thread page).
        let room = f64::from(self.box_top.get() / zoom) - 8. - 8.;
        let height = if room > 0. { height.min(room.max(160.)) } else { height };
        // Each column as wide as its longest entry needs, or as it was
        // dragged to, the description the rest; measured over every command,
        // not just the matches, so the columns hold still while typing
        // (SlashPalette.columnWidths).
        let categories_natural = layout.categories.unwrap_or_else(|| categories_natural_width(&view));
        let list_natural = layout.list.unwrap_or_else(|| list_natural_width(&view.all));
        let (categories_width, list_width) = slash::column_widths(width, categories_natural, list_natural, layout.list.is_some(), 200., 120., 110.);
        crate::slash_resize::drawn_as((width, height), (categories_width, list_width));
        let hairline = style::hairline(cx);
        let composer = cx.entity().downgrade();
        let menu_composer = composer.clone();
        let panel = h_flex()
            .id("slash-palette")
            .relative()
            .size_full()
            .items_start()
            .rounded(px(crate::slash_resize::RADIUS))
            .overflow_hidden()
            .bg(cx.theme().background)
            .border_1()
            .border_color(hairline)
            .shadow_lg()
            // Keep clicks out of the transcript under it.
            .occlude()
            .child(div().flex_none().w(px(categories_width as f32)).h_full().child(categories))
            .child(crate::slash_resize::column_rule(0, categories_width, list_width + 200., width, cx))
            .child(div().flex_none().w(px(list_width as f32)).h_full().child(list))
            .child(crate::slash_resize::column_rule(1, list_width, categories_width + 200., width, cx))
            .child(div().flex_1().min_w_0().h_full().child(detail))
            // Its edges drag it bigger; a right-click anywhere on it puts it back.
            .children(crate::slash_resize::edges((width, height), cx))
            .context_menu(move |menu, _, _| crate::slash_resize::menu(menu_composer.clone(), menu));
        // Centred over the box, kept inside the window.
        let left = crate::slash_resize::left_offset(width as f32 * zoom, f32::from(viewport.width));
        Some(
            deferred(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_full()
                    .mb(px(8.))
                    .h(px(height as f32))
                    .child(crate::slash_resize::measure_box())
                    .child(div().absolute().top_0().bottom_0().left(gpui_kit::px(left)).w(px(width as f32)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(panel))
                    .children(crate::slash_resize::drag_watch(composer)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The categories down the left, with their counts.
    fn render_palette_categories(&mut self, view: &PaletteView, cx: &mut Context<Self>) -> impl IntoElement {
        let counts = slash::counts(&view.all, &view.query, &view.recent);
        let mut column = v_flex().p(px(8.)).gap(px(2.));
        for (category, count) in counts {
            let on = self.palette.category == category;
            let tip: SharedString = category.tip(&view.provider).into();
            let title = category.title(&view.provider);
            let (fg, muted) = (cx.theme().foreground, cx.theme().muted_foreground);
            let hover = style::primary_opacity(cx, 0.05);
            column = column.child(
                h_flex()
                    .id(SharedString::from(format!("slash-cat-{title}")))
                    .test_support()
                    .aria_label(tip.clone())
                    .justify_between()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .text_size(px(size::LABEL + 1.))
                    .when(on, |s| s.bg(style::primary_opacity(cx, 0.1)))
                    .when(!on, |s| s.hover(move |s| s.bg(hover)))
                    .text_color(if on { fg } else { muted })
                    .tip(tip.clone())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.palette.category = category;
                        this.palette.selected = None;
                        this.focus(window, cx);
                        crate::web_panels::keyboard_to_app(window);
                        cx.notify();
                    }))
                    .child(div().truncate().child(title))
                    .child(div().flex_none().text_color(muted).child(count.to_string())),
            );
        }
        column
    }

    /// The matches, or why there are none.
    fn render_palette_list(&mut self, view: &PaletteView, matches: &[SlashCommand], picked: Option<&SlashCommand>, cx: &mut Context<Self>) -> AnyElement {
        if view.loading {
            return palette_placeholder("Getting this thread's commands…".to_owned(), cx).into_any_element();
        }
        if matches.is_empty() {
            let note = if view.query.is_empty() { "Nothing here yet.".to_owned() } else { format!("No command matches /{}. Send it and it goes to the agent as words.", view.query) };
            return palette_placeholder(note, cx).into_any_element();
        }
        let selected_name = picked.map(|c| c.name.clone());
        let mut rows = v_flex().w_full();
        for command in matches {
            let on = selected_name.as_deref() == Some(command.name.as_str());
            rows = rows.child(self.render_palette_row(command, on, view.mid_message, cx));
        }
        div()
            .id("slash-list")
            .size_full()
            .py(px(6.))
            .overflow_y_scroll()
            .track_scroll(&self.palette_scroll)
            .child(rows)
            .into_any_element()
    }

    fn render_palette_row(&mut self, command: &SlashCommand, on: bool, mid_message: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let name = command.name.clone();
        let title = format!("/{name}");
        let tip: SharedString = if mid_message { format!("/{name}: double-click to put it in the message") } else { format!("/{name}: double-click to run it") }.into();
        let (fg, accent, hover) = (cx.theme().foreground, style::accent(cx).opacity(0.75), style::primary_opacity(cx, 0.05));
        let picked = command.clone();
        div()
            .id(SharedString::from(format!("slash-row-{name}")))
            .test_support()
            .aria_label(tip.clone())
            .w_full()
            .px(px(14.))
            .py(px(6.))
            .cursor_pointer()
            .text_size(px(size::LABEL + 1.))
            .truncate()
            .when(on, |s| s.bg(accent).text_color(gpui_kit::white()))
            .when(!on, |s| s.text_color(fg).hover(move |s| s.bg(hover)))
            .tip(tip.clone())
            // Picked on the first click, at once; a second click on the
            // picked row runs it (read from the click itself).
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 && this.palette.selected.as_deref() == Some(picked.name.as_str()) {
                    // Partway through a message it goes into the words.
                    this.palette_pick(picked.clone(), !mid_message, window, cx);
                } else {
                    this.palette.selected = Some(picked.name.clone());
                    this.focus(window, cx);
                    crate::web_panels::keyboard_to_app(window);
                    cx.notify();
                }
            }))
            .child(title)
    }

    /// The picked command in full; the words scroll, the buttons stay.
    fn render_palette_detail(&mut self, view: &PaletteView, picked: Option<&SlashCommand>, cx: &mut Context<Self>) -> AnyElement {
        let Some(command) = picked else { return div().into_any_element() };
        let name = command.name.clone();
        let mut words = v_flex().gap(px(10.)).p(px(16.)).child(div().text_size(px(size::CARD_TITLE + 3.)).font_weight(FontWeight::SEMIBOLD).child(format!("/{name}")));
        if let Some(text) = command.description.as_deref().filter(|d| !d.is_empty()) {
            words = words.child(div().text_size(px(size::LABEL + 1.)).text_color(style::secondary_text(cx)).child(slash::plain_description(text)));
        }
        words = words.child(div().text_size(px(size::CODE)).font_family(cx.theme().mono_font_family.clone()).px(px(10.)).py(px(8.)).w_full().rounded(px(7.)).bg(style::primary_opacity(cx, 0.06)).child(command.usage()));
        if let Some(now) = view.current.get(&name) {
            words = words.child(div().text_size(px(size::LABEL + 1.)).child(format!("Now: {now}")));
        }
        if let Some(aliases) = command.aliases.as_ref().filter(|a| !a.is_empty()) {
            words = words.child(div().text_size(px(size::LABEL)).text_color(style::secondary_text(cx)).child(format!("Also {}", aliases.iter().map(|a| format!("/{a}")).collect::<Vec<_>>().join(", "))));
        }
        let origin = v_flex().gap(px(3.)).child(div().text_size(px(size::LABEL)).text_color(style::tertiary_label(cx)).child(slash::origin(command, &view.provider))).when_some(command.path.clone(), |s, path| {
            s.child(div().text_size(px(size::LABEL - 1.)).font_family(cx.theme().mono_font_family.clone()).text_color(style::tertiary_label(cx)).child(slash::short_path(&path, &view.home)))
        });
        words = words.child(origin);
        let scroller = div().id("slash-detail").flex_1().min_h_0().w_full().overflow_y_scroll().child(words);
        let buttons = self.render_palette_buttons(view, command, cx);
        v_flex().size_full().child(scroller).child(buttons).into_any_element()
    }

    /// Run, Add details and Open file along the bottom.
    fn render_palette_buttons(&mut self, view: &PaletteView, command: &SlashCommand, cx: &mut Context<Self>) -> impl IntoElement {
        let (run, fill) = (command.clone(), command.clone());
        let mut row = h_flex().flex_none().gap(px(8.)).px(px(16.)).py(px(12.));
        if view.mid_message {
            row = row.child(palette_button("Insert", format!("Put /{} in the message, for the agent to run (Return or Tab)", command.name), cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.palette_pick(fill.clone(), false, window, cx))));
        } else {
            row = row
            .child(palette_button("Run", format!("Send /{} now (Return)", command.name), cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.palette_pick(run.clone(), true, window, cx))))
            .child(palette_button("Add details", format!("Put /{} in the box to type more after it (Tab)", command.name), cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.palette_pick(fill.clone(), false, window, cx))));
        }
        if let Some(path) = command.path.clone().filter(|_| view.can_open_files) {
            let tip = format!("Open {} beside the thread", slash::short_path(&path, &view.home));
            row = row.child(palette_button("Open file", tip, cx, cx.listener(move |this, _: &ClickEvent, _, cx| this.palette_open_file(path.clone(), cx))));
        }
        row
    }

    /// Open file: the skill in the pane beside the thread, read through the
    /// runner, as the Swift app previews it. Its path is the catalog's, which
    /// a cloned repo chooses (`Inspect.app#help/SKILL.md` beside an
    /// `Inspect.app`, Daybreak 9 Nov 2026), so it's never handed to the
    /// system as it is: with no thread to show it beside (the new-thread
    /// page), it goes through the open gate, which opens the real file and
    /// asks before anything that would run. Another runner's thread has no
    /// button at all.
    fn palette_open_file(&mut self, path: String, cx: &mut Context<Self>) {
        let data = self.chip_data(cx);
        if data.away.is_none() {
            let thread = data.summary.filter(|_| self.thread_id.is_some());
            match (self.app.upgrade(), thread) {
                (Some(app), Some(thread)) => {
                    let side = app.read(cx).side.clone();
                    side.update(cx, |side, cx| side.preview_path(&path, None, &thread, cx));
                }
                _ => crate::open_gate::open_file(&path, crate::open_gate::Origin::ThisMac, cx),
            }
        }
        self.palette.closed_at = Some(self.input.read(cx).value().to_string());
        cx.notify();
    }

    /// `dump`'s palette (`AppDump.PaletteInfo`), while it's open.
    #[cfg(feature = "automation")]
    pub(crate) fn palette_dump(&self, cx: &App) -> Value {
        let text = self.input.read(cx).value().to_string();
        let Some(view) = self.palette_open(&text, cx) else { return Value::Null };
        let query = view.query.clone();
        let matches = slash::matches(&view.all, &query, self.palette.category, &view.recent);
        let picked = slash::picked(&matches, &self.palette).cloned();
        let categories: Vec<Value> = slash::counts(&view.all, &query, &view.recent).into_iter().map(|(c, n)| json!({ "title": c.title(&view.provider), "count": n })).collect();
        let note = if view.loading { Some("Getting this thread's commands…".to_owned()) } else if matches.is_empty() { Some(if query.is_empty() { "Nothing here yet.".to_owned() } else { format!("No command matches /{query}. Send it and it goes to the agent as words.") }) } else { None };
        let detail = picked.as_ref().map(|c| {
            let mut buttons = if view.mid_message { vec!["Insert"] } else { vec!["Run", "Add details"] };
            if c.path.is_some() && view.can_open_files {
                buttons.push("Open file");
            }
            json!({
                "title": format!("/{}", c.name),
                "usage": c.usage(),
                "description": c.description.clone().filter(|d| !d.is_empty()),
                "now": view.current.get(&c.name),
                "also": c.aliases.as_ref().filter(|a| !a.is_empty()).map(|a| format!("Also {}", a.iter().map(|x| format!("/{x}")).collect::<Vec<_>>().join(", "))),
                "origin": slash::origin(c, &view.provider),
                "path": c.path.as_ref().map(|p| slash::short_path(p, &view.home)),
                "buttons": buttons,
            })
        });
        json!({
            "query": query,
            "category": self.palette.category.title(&view.provider),
            "categories": categories,
            "commands": matches.iter().map(|c| format!("/{}", c.name)).collect::<Vec<_>>(),
            "picked": picked.map(|c| format!("/{}", c.name)),
            "detail": detail,
            "note": note,
            // As last drawn, in points (slash_resize.rs): its width and
            // height, and its categories' and list's widths.
            "size": crate::slash_resize::drawn().map(|d| [d[0], d[1]]),
            "columns": crate::slash_resize::drawn().map(|d| [d[2], d[3]]),
        })
    }
}

fn palette_placeholder(text: String, cx: &App) -> impl IntoElement {
    div().p(px(16.)).size_full().text_size(px(size::LABEL + 1.)).text_color(style::secondary_text(cx)).child(text)
}

fn palette_button(title: &'static str, tip: String, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let tip: SharedString = tip.into();
    let shown = tip.clone();
    div()
        .id(SharedString::from(format!("slash-button-{title}")))
        .test_support()
        .aria_label(tip)
        .px(px(10.))
        .py(px(4.))
        .rounded(px(6.))
        .text_size(px(size::LABEL))
        .bg(style::primary_opacity(cx, 0.08))
        .hover(|s| s.bg(style::primary_opacity(cx, 0.14)))
        .cursor_pointer()
        .child(title)
        .tip(shown.clone())
        .on_click(on_click)
}

/// About a pixel per em at the label size: a measure of the longest entries
/// the columns' widths are worked out from (`SlashPaletteView.listWidth` and
/// `categoriesWidth` measure the real text; these are close enough, since the
/// rows truncate when a column is narrower).
fn text_width(text: &str) -> f64 {
    text.chars().count() as f64 * 7.3
}

/// `SlashPaletteView.listWidth`.
fn list_natural_width(commands: &[SlashCommand]) -> f64 {
    commands.iter().map(|c| text_width(&format!("/{}", c.name))).fold(0., f64::max) + 14. * 2. + 14.
}

/// `SlashPaletteView.categoriesWidth`: the longest title beside its count.
fn categories_natural_width(view: &PaletteView) -> f64 {
    let longest = slash::counts(&view.all, "", &view.recent).iter().map(|(c, n)| text_width(&c.title(&view.provider)) + text_width(&n.to_string())).fold(0., f64::max);
    longest + 10. * 2. + 8. * 2. + 14.
}
