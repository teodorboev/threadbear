// The Patches list and the panels a patch gives a thread, a port of the Swift
// app's PatchesList, ListPanel and ImagePanel (PanelsView.swift), with action
// chips (ActionChips). A panel's rows or picture come from the runner
// (`panels.render`), asked for when it's shown and every so often while it is.
// A web panel's page is web_panels.rs's (`panel.web`): the side pane only
// draws one if it's asked to anyway, as a placeholder with Open in browser.
use crate::call;
use crate::highlights::{icon, palette, tipped, PaneRow, Palette, SidePane, OUTER_INSET};
use crate::style;
use gpui_kit::component::{h_flex, spinner::Spinner, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use threadbear_core::highlights::{self as core, patches as words, Action, Panel, PanelItem, PanelRender, PatchInfo};
use threadbear_core::inbox::ThreadSummary;

/// The list panel is asked for its rows this often while it's showing
/// (`ListPanel`), an image panel every 3 seconds, 5 while it has no picture.
const LIST_REFRESH: Duration = Duration::from_secs(15);

/// What patch panels hold between frames.
#[derive(Default)]
pub(crate) struct PanelsState {
    /// Each panel's last answer, by "<thread>/<panel>".
    pub renders: HashMap<String, PanelRender>,
    pictures: HashMap<String, Arc<Image>>,
    /// Panels being polled, so one timer each.
    polling: HashSet<String>,
    /// Patch versions whose Trust has been pressed (`trustingPatches`).
    pub trusting: HashSet<String>,
    /// Actions running now, by "<thread>/<action>" (`runningActions`).
    pub running: HashSet<String>,
}

use std::sync::Arc;

fn panel_key(thread_id: &str, panel_id: &str) -> String {
    format!("{thread_id}/{panel_id}")
}

fn trust_key(file: &str, hash: &str) -> String {
    format!("{file} {hash}")
}

/// A tool patch's panel as a card at the top of Highlights (`ToolCard`).
pub(crate) fn tool_card(panel: &Panel, thread_id: String, p: Palette, cx: &mut Context<SidePane>) -> impl IntoElement + use<> {
    let text = panel.summary.clone().or_else(|| panel.note.clone());
    let (card, hover) = (p.card, style::primary_opacity(cx, 0.08));
    let title = panel.title.clone();
    let panel_id = panel.id.clone();
    let symbol = core::panel_symbol(panel).to_owned();
    div().px(px(OUTER_INSET)).pt(px(10.)).child(
        tipped(
            div()
                .id(SharedString::from(format!("hl-tool-{}", panel.id)))
                .rounded(px(10.))
                .bg(card)
                .border_1()
                .border_color(p.card_border)
                .hover(move |s| s.bg(hover))
                .p(px(12.))
                .cursor_pointer()
                .child(
                    v_flex()
                        .gap(px(6.))
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .items_center()
                                .child(icon(&symbol, 13., p.secondary))
                                .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::LABEL)).font_semibold().child(title.clone()))
                                .child(icon("chevron.right", 10., p.faint)),
                        )
                        .when_some(text, |s, t| s.child(div().text_size(px(style::size::DETAIL)).line_height(px(16.)).text_color(p.secondary).child(t))),
                ),
            format!("Open {title}"),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose_section(&thread_id.clone(), &panel_id.clone(), cx))),
    )
}

impl SidePane {
    /// Asks for a panel's rows or picture now and then while it's showing.
    pub(crate) fn refresh_panel(&mut self, thread_id: &str, panel_id: &str, cx: &mut Context<Self>) {
        let key = panel_key(thread_id, panel_id);
        self.fetch_panel(key.clone(), thread_id.to_owned(), panel_id.to_owned(), cx);
        if !self.panels.polling.insert(key.clone()) {
            return;
        }
        let (thread, panel) = (thread_id.to_owned(), panel_id.to_owned());
        cx.spawn(async move |this, cx| loop {
            let image = this
                .update(cx, |t, cx| {
                    let showing = t.thread(cx).is_some_and(|th| th.id == thread) && t.chosen.get(&thread) == Some(&panel);
                    if !showing {
                        t.panels.polling.remove(&key);
                    }
                    (showing, t.panels.renders.get(&key).map(|r| r.image.is_some()).unwrap_or(false))
                })
                .ok();
            let Some((showing, has_image)) = image else { return };
            if !showing {
                return;
            }
            // As ImagePanel's loop: 3 s with a picture, 5 without; a list 15.
            let kind_is_list = this.update(cx, |t, cx| t.panel_is_list(&panel, cx)).unwrap_or(false);
            let delay = if kind_is_list { LIST_REFRESH } else if has_image { Duration::from_secs(3) } else { Duration::from_secs(5) };
            cx.background_executor().timer(delay).await;
            if this.update(cx, |t, cx| t.fetch_panel(key.clone(), thread.clone(), panel.clone(), cx)).is_err() {
                return;
            }
        })
        .detach();
    }

    fn panel_is_list(&self, panel_id: &str, cx: &App) -> bool {
        self.thread(cx).is_some_and(|t| t.panels().iter().any(|p| p.id == panel_id && p.kind == "list"))
    }

    fn fetch_panel(&mut self, key: String, thread_id: String, panel_id: String, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client.clone(), "panels.render", json!({ "threadId": thread_id, "panelId": panel_id })).await;
            let render = match res {
                Ok(v) => PanelRender::decode(&v),
                Err(_) => PanelRender { note: Some("Not connected to the runner.".to_owned()), ..Default::default() },
            };
            // The picture comes from the runner, so another Mac's works too.
            let picture = match render.image.as_ref() {
                Some(image) => {
                    let read = call(client, "files.read", json!({ "path": image.path })).await.ok().as_ref().and_then(threadbear_core::folders::FileRead::decode);
                    read.map(|r| r.data)
                }
                None => None,
            };
            this.update(cx, |t, cx| {
                match picture {
                    Some(bytes) => {
                        let format = render.image.as_ref().map(|i| i.media_type.clone()).unwrap_or_default();
                        let format = if format.contains("jpeg") { ImageFormat::Jpeg } else { ImageFormat::Png };
                        t.panels.pictures.insert(key.clone(), Arc::new(Image::from_bytes(format, bytes)));
                    }
                    None if render.image.is_none() => {
                        t.panels.pictures.remove(&key);
                    }
                    None => {}
                }
                t.panels.renders.insert(key, render);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A patch-given panel's section: a list, a picture, or (for now) a page's placeholder.
    pub(crate) fn render_panel(&mut self, thread: &ThreadSummary, id: &str, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(panel) = thread.panels().into_iter().find(|panel| panel.id == id) else {
            return crate::files::pane_note(&format!("This thread has no {id} panel."), p).into_any_element();
        };
        let key = panel_key(&thread.id, id);
        match panel.kind.as_str() {
            "list" => match self.panels.renders.get(&key) {
                None => {
                    self.refresh_panel(&thread.id, id, cx);
                    crate::files::pane_note("Loading…", p).into_any_element()
                }
                Some(render) => match render.items.clone().filter(|i| !i.is_empty()) {
                    Some(items) => self.render_list_panel(thread, items, p, cx),
                    None => {
                        let note = render.note.clone().unwrap_or_else(|| "Nothing in this list.".to_owned());
                        crate::files::pane_note(&note, p).into_any_element()
                    }
                },
            },
            "image" => {
                let picture = self.panels.pictures.get(&key).cloned();
                if self.panels.renders.get(&key).is_none() {
                    self.refresh_panel(&thread.id, id, cx);
                }
                match picture {
                    Some(image) => div().size_full().flex().items_center().justify_center().p(px(16.)).child(img(ImageSource::Image(image)).size_full().object_fit(ObjectFit::Contain)).into_any_element(),
                    None => {
                        let note = self.panels.renders.get(&key).and_then(|r| r.note.clone()).unwrap_or_else(|| "Taking a screenshot…".to_owned());
                        crate::files::pane_note(&note, p).into_any_element()
                    }
                }
            }
            // A web panel: web_panels.rs draws its page (shell.rs hands the
            // section to it), so this is only a fallback.
            _ => self.render_web_placeholder(&panel, p, cx),
        }
    }

    fn render_web_placeholder(&mut self, panel: &Panel, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let url = panel.url.clone().unwrap_or_default();
        let bg = style::button_fill(cx);
        let edge = style::button_edge(cx);
        let label = "Open in browser";
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .p(px(24.))
            .child(div().text_center().text_size(px(style::size::LABEL)).text_color(p.secondary).child(format!("{} is a web page. It opens in your browser for now.", panel.title)))
            .child(
                tipped(div().id("hl-web-open").px(px(12.)).py(px(5.)).rounded(px(6.)).bg(bg).border_1().border_color(edge).cursor_pointer().child(div().text_size(px(style::size::LABEL)).text_color(p.fg).child(label)), format!("Open {url}"))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| crate::files::open_web(&url.clone(), cx))),
            )
            .into_any_element()
    }

    fn render_list_panel(&mut self, thread: &ThreadSummary, items: Vec<PanelItem>, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let reserves = items.iter().any(|i| i.symbol().is_some());
        let headings = PanelItem::headings(&items);
        let mut list = v_flex().py(px(8.));
        for (index, item) in items.iter().enumerate() {
            if let Some(heading) = headings[index].clone() {
                list = list.child(self.heading(crate::highlights::Heading { key: format!("panel-{index}"), title: heading, icon: None, count: None, folded: false, foldable: false, accessory: None }, p, cx));
            }
            list = list.child(self.list_row(item, index, reserves, thread, p, cx));
        }
        div().id("list-panel").size_full().overflow_y_scroll().child(list).into_any_element()
    }

    /// A row with a link or a file opens it the way a highlight does.
    fn list_row(&mut self, item: &PanelItem, index: usize, reserves: bool, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let color = match item.tone_kind() {
            Some("attention") => Some(p.needs_you),
            Some("failed") => Some(p.failed),
            _ => None,
        };
        // A patch's detail is usually a sentence, so it gets two lines and keeps its start.
        let mut row = PaneRow::new(SharedString::from(format!("panel-row-{index}")), item.title.clone())
            .icon_opt(item.symbol())
            .tinted(color)
            .reserves_icon(reserves)
            .trailing(item.trailing.clone())
            .trailing_symbol(if item.tone_kind() == Some("done") && item.icon.is_some() { Some("checkmark") } else { None })
            .detail(item.detail.clone())
            .detail_lines(2);
        let (path, url, title) = (item.path.clone(), item.url.clone(), item.title.clone());
        let thread2 = thread.clone();
        if path.is_some() || url.is_some() {
            let (path2, url2, title2) = (path.clone(), url.clone(), title.clone());
            let thread3 = thread2.clone();
            row = row.on_click(move |this, _, cx| {
                if let Some(path) = &path2 {
                    this.preview_path(path, Some(title2.clone()), &thread3, cx);
                } else if let Some(url) = &url2 {
                    this.follow_link(url, cx);
                }
            });
            let (path3, url3, title3) = (path, url, title);
            row = row.on_menu(move |this, pos, window, cx| {
                let items = if let Some(path) = &path3 {
                    SidePane::file_menu(path, &title3, &thread2)
                } else if let Some(url) = &url3 {
                    SidePane::link_menu(url)
                } else {
                    vec![]
                };
                if !items.is_empty() {
                    this.open_menu(items, pos, window, cx);
                }
            });
        }
        row.render(p, cx).into_any_element()
    }

    /// The Patches section (`PatchesList`).
    pub(crate) fn render_patches(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let patches = thread.patches();
        // On a project's page they're all the project's (projects.rs,
        // `pane_thread`): one list, and what they are (PanelsView's `project`).
        let project_page = self.app.upgrade().is_some_and(|app| app.read(cx).projects.open.is_some());
        let project_title = |id: &str| self.projects.get(id).map(|p| p.title.clone());
        let groups = if project_page { vec![words::PatchGroup { project_id: None, title: None, patches: patches.clone() }] } else { words::group(thread, &patches, project_title) };
        let (actions, panels, hooks) = (thread.actions(), thread.panels(), thread.hooks());
        let mut list = v_flex().p(px(16.)).gap(px(14.));
        let show_headings = groups.len() > 1;
        for group in groups {
            if show_headings {
                list = list.child(
                    div()
                        .pb(px(4.))
                        .border_b_1()
                        .border_color(p.hairline)
                        .text_size(px(style::size::DETAIL))
                        .font_semibold()
                        .text_color(p.secondary)
                        .child(group.title.clone().unwrap_or_else(|| "This thread's folder".to_owned())),
                );
            }
            for patch in group.patches {
                let gives = words::gives(&patch.id, &actions, &panels, &hooks);
                list = list.child(self.patch_row(&patch, gives, thread, p, cx));
            }
        }
        let footer = if project_page {
            "These come from the project's folders, and every thread in it gets them. Ask the agent for another."
        } else {
            "A patch is a file in .threadbear/patches (in the thread's folder, a parent folder, a folder of one of its projects, or ~) whose default export lists actions, panels and hooks. Ask the agent for one."
        };
        list = list.child(div().pt(px(6.)).text_size(px(style::size::SMALL)).text_color(p.faint).child(footer));
        div().id("patches-list").size_full().overflow_y_scroll().child(list).into_any_element()
    }

    fn patch_row(&mut self, patch: &PatchInfo, gives: String, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let symbol = patch.icon.clone().unwrap_or_else(|| "square.dashed".to_owned());
        let mut row = v_flex().gap(px(4.)).child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(icon(&symbol, 11., p.secondary))
                .child(div().text_size(px(style::size::LABEL)).font_weight(FontWeight::MEDIUM).child(patch.id.clone()))
                .child(div().text_size(px(style::size::SMALL)).text_color(p.faint).child(words::source_name(&patch.source).to_owned())),
        );
        if patch.untrusted.is_some() {
            let trusting = patch.file.as_deref().zip(patch.hash.as_deref()).is_some_and(|(f, h)| self.panels.trusting.contains(&trust_key(f, h)));
            let needs_you = p.needs_you;
            let tip = if trusting {
                "Trusting it. The runner loads its patches again, which takes a few seconds."
            } else if patch.source == "claude" {
                "Let this folder's Claude Code hooks, status line and MCP servers run on your Mac, and its environment settings (which can change where Claude sends your messages) and permission rules apply, from your next message. Open the files below to read them first."
            } else {
                "Load this patch and let its code run on your Mac from now on. Open the file below to read it first."
            };
            let trust_patch = patch.clone();
            let thread_id = thread.id.clone();
            let button = tipped(
                div().id(SharedString::from(format!("trust-{}", patch.id))).flex_none().px(px(8.)).py(px(3.)).rounded(px(5.)).bg(style::button_fill(cx)).border_1().border_color(style::button_edge(cx)).when(!trusting, |s| s.cursor_pointer()).child(div().text_size(px(style::size::DETAIL)).child("Trust")),
                tip,
            )
            .when(!trusting, |b| b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.trust_patches(vec![trust_patch.clone()], thread_id.clone(), cx))));
            row = row.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().text_size(px(style::size::DETAIL)).text_color(needs_you).child(words::waiting_text(patch)))
                    .child(button)
                    .when(trusting, |s| s.child(Spinner::new().with_size(px(11.)).color(p.secondary))),
            );
            for grant in patch.grants.iter().flatten() {
                row = row.child(div().text_size(px(style::size::SMALL)).text_color(p.secondary).line_clamp(2).child(grant.clone()));
            }
        } else if let Some(error) = &patch.error {
            row = row.child(div().text_size(px(style::size::DETAIL)).text_color(p.failed).child(error.clone()));
        } else {
            if !gives.is_empty() {
                row = row.child(div().text_size(px(style::size::DETAIL)).text_color(p.secondary).child(gives));
            }
            if let Some(error) = &patch.live_error {
                row = row.child(div().text_size(px(style::size::DETAIL)).text_color(p.failed).child(error.clone()));
            }
            // Why threads in Ask and Auto ask before every edit.
            if let Some(why) = &patch.edits_ask {
                row = row.child(div().text_size(px(style::size::DETAIL)).text_color(p.needs_you).child(why.clone()));
            }
        }
        if let Some(matched) = patch.matched.as_ref().filter(|m| !m.is_empty()) {
            let names: Vec<String> = matched.iter().map(|m| core::last_component(m).to_owned()).collect();
            row = row.child(div().text_size(px(style::size::SMALL)).text_color(p.secondary).truncate().child(format!("Here for {}", names.join(", "))));
        }
        if let Some(file) = &patch.file {
            row = row.child(self.patch_file_line(file, None, thread, p, cx));
            if patch.untrusted.is_some() {
                for helper in patch.helpers.iter().flatten() {
                    let prefix = if patch.source == "claude" { "Also trusts" } else { "Imports" };
                    row = row.child(self.patch_file_line(helper, Some(prefix), thread, p, cx));
                }
            }
        } else if let Some(folder) = patch.folder.as_ref().filter(|_| patch.project_id.is_some()) {
            row = row.child(div().text_size(px(style::size::SMALL)).text_color(p.faint).truncate().child(abbreviate_home(folder)));
        }
        row.into_any_element()
    }

    fn patch_file_line(&mut self, file: &str, prefix: Option<&str>, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let words = match prefix {
            Some(prefix) => format!("{prefix} {}", abbreviate_home(file)),
            None => abbreviate_home(file),
        };
        let (path, thread2) = (file.to_owned(), thread.clone());
        tipped(
            div().id(SharedString::from(format!("patch-file-{file}"))).w_full().truncate().text_size(px(style::size::SMALL)).text_color(p.faint).cursor_pointer().child(words),
            if prefix.is_some() { "Open this file" } else { "Open the file" },
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            // A file on this runner in its app; another Mac's in the pane.
            if thread2.is_remote() {
                this.preview_path(&path.clone(), None, &thread2, cx);
            } else {
                this.open_externally(&path.clone(), cx);
            }
        }))
        .into_any_element()
    }

    /// Trusts patch files waiting in the thread's folder (`trustPatches`). The
    /// button stays busy until the thread's update says it's trusted, or a
    /// few seconds later in case that never comes.
    pub(crate) fn trust_patches(&mut self, patches: Vec<PatchInfo>, thread_id: String, cx: &mut Context<Self>) {
        let files: Vec<(String, String)> = patches.iter().filter_map(|p| p.file.clone().zip(p.hash.clone())).filter(|(f, h)| !self.panels.trusting.contains(&trust_key(f, h))).collect();
        if files.is_empty() {
            return;
        }
        let keys: Vec<String> = files.iter().map(|(f, h)| trust_key(f, h)).collect();
        self.panels.trusting.extend(keys.iter().cloned());
        cx.notify();
        let client = self.client.clone();
        let list: Vec<Value> = files.iter().map(|(f, h)| json!({ "file": f, "hash": h })).collect();
        cx.spawn(async move |this, cx| {
            let res = call(client, "patches.trust", json!({ "threadId": thread_id, "files": list })).await;
            let failed = res.err().map(|e| e.message);
            if failed.is_none() {
                cx.background_executor().timer(Duration::from_secs(5)).await;
            }
            this.update(cx, |t, cx| {
                for key in keys {
                    t.panels.trusting.remove(&key);
                }
                if let Some(message) = failed {
                    t.note_error(format!("Couldn't trust that patch: {message}"), cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs an action chip (`runAction`): the runner sends a prompt action as
    /// a message or runs a command action; a url action opens its page here.
    pub(crate) fn run_action(&mut self, action: &Action, thread_id: String, cx: &mut Context<Self>) {
        if action.kind == "url" {
            match action.url.as_deref().filter(|u| core::is_web_page(u)) {
                Some(url) => crate::files::open_web(url, cx),
                None => self.note_error(format!("{} has no web page to open.", action.title), cx),
            }
            return;
        }
        let key = panel_key(&thread_id, &action.id);
        if !self.panels.running.insert(key.clone()) {
            return;
        }
        cx.notify();
        let (client, action_id, title) = (self.client.clone(), action.id.clone(), action.title.clone());
        cx.spawn(async move |this, cx| {
            let res = call(client, "actions.run", json!({ "threadId": thread_id, "actionId": action_id })).await;
            this.update(cx, |t, cx| {
                t.panels.running.remove(&key);
                if let Err(e) = res {
                    t.note_error(format!("{title} didn't run: {}", e.message), cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The thread's actions as shelves on the composer's top edge
    /// (`ActionChips`): a tab for each place the chips came from, led by its
    /// badge, the chips grouped by patch with a hairline between, as many as
    /// fit `room` and the rest under "+N". A press runs the action; a running
    /// one shows a spinner and stops taking clicks. Inside a row of pills over
    /// the box they read as part of writing a message, which Mike didn't
    /// want (30 Oct), and the GPUI app had them that way until 11 Nov.
    pub(crate) fn render_action_chips(&mut self, thread: &ThreadSummary, room: f32, project_icons: &HashMap<String, crate::sidebar::ProjectIcon>, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let actions = thread.actions();
        let patches = thread.patches();
        let waiting: Vec<PatchInfo> = patches.iter().filter(|p| p.untrusted.is_some()).cloned().collect();
        if actions.is_empty() && waiting.is_empty() {
            return None;
        }
        let p = palette(cx);
        let chip_size = px(style::size::CHIP);
        let measure = |text: &str, window: &Window| -> f32 {
            let font = window.text_style().font();
            let run = TextRun { len: text.len(), font, color: Hsla::default(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(SharedString::from(text.to_owned()), chip_size, &[run], None).width)
        };
        // Each piece's width, as ActionChipsFit adds them up.
        let z = style::zoomed;
        let chip_width = |a: &Action, window: &Window| z(8.) + z(13.) + z(5.) + measure(&a.title, window) + a.badge.map_or(0., |n| z(5.) + measure(&badge_text(n), window) + z(10.)) + z(8.);
        let tab = |inner: f32| z(6.) * 2. + inner;
        let label = trust_label(&waiting);
        let waiting_width = (!waiting.is_empty()).then(|| tab(z(8.) + z(11.) + z(5.) + measure(&label, window) + z(8.)));
        let more_width = |n: usize, window: &Window| tab(z(6.) + measure(&format!("+{n}"), window) + z(6.));
        let widths: HashMap<String, f32> = actions.iter().map(|a| (a.id.clone(), chip_width(a, window))).collect();
        let row_width = |shown: usize, window: &Window| -> f32 {
            let mut parts: Vec<f32> = waiting_width.into_iter().collect();
            for shelf in core::ActionShelf::shelves(&actions[..shown], &patches) {
                let mut items = vec![z(18.) + z(2.) * 2., 1. + z(2.) * 2.];
                for (index, group) in shelf.patches.iter().enumerate() {
                    if index > 0 {
                        items.push(1. + z(2.) * 2.);
                    }
                    items.extend(group.iter().map(|a| widths.get(&a.id).copied().unwrap_or(0.)));
                }
                parts.push(tab(items.iter().sum::<f32>() + z(2.) * (items.len() as f32 - 1.)));
            }
            if shown < actions.len() {
                parts.push(more_width(actions.len() - shown, window));
            }
            parts.iter().sum::<f32>() + z(6.) * (parts.len().max(1) as f32 - 1.)
        };
        // Half a point for rounding, as Swift's `shown` allows.
        let shown = (0..=actions.len()).rev().find(|n| row_width(*n, window) <= room + 0.5).unwrap_or(0);
        let (fill, edge) = (style::primary_opacity(cx, if cx.theme().is_dark() { 0.05 } else { 0.035 }), style::hairline(cx));
        let shelf_tab = move || {
            h_flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .px(px(6.))
                .py(px(4.))
                .rounded_tl(px(10.))
                .rounded_tr(px(10.))
                .bg(fill)
                .border_t(gpui_kit::px(0.5))
                .border_l(gpui_kit::px(0.5))
                .border_r(gpui_kit::px(0.5))
                .border_color(edge)
        };
        let hairline = move || div().flex_none().mx(px(2.)).w(gpui_kit::px(1.)).h(px(14.)).bg(p.card_border);
        let mut row = h_flex().items_end().gap(px(6.));
        if !waiting.is_empty() {
            let tip = trust_tip(&waiting);
            row = row.child(
                shelf_tab().child(
                    tipped(div().id("action-trust").flex().items_center().gap(px(5.)).px(px(8.)).py(px(4.)).rounded(px(6.)).cursor_pointer().hover(move |s| s.bg(p.hover)), tip)
                        .child(icon("lock.shield", 11., style::needs_you(cx)))
                        .child(div().text_size(chip_size).text_color(p.fg).child(label.clone()))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.show_patches(cx))),
                ),
            );
        }
        for (index, shelf) in core::ActionShelf::shelves(&actions[..shown], &patches).into_iter().enumerate() {
            let project_title = match &shelf.origin {
                core::PanelOrigin::Project(id) => thread.projects.iter().flatten().find(|p| &p.id == id).map(|p| p.title.clone()),
                _ => None,
            };
            let badge_tip = shelf.origin.tip("These buttons", project_title.as_deref());
            let badge_face: AnyElement = match &shelf.origin {
                core::PanelOrigin::Project(_) => match crate::patch_icons::project_mark(&shelf.origin, thread, project_icons) {
                    Some(mark) => project_badge_face(mark, p),
                    None => div().into_any_element(),
                },
                other => crate::patch_icons::glyph(other.symbol().unwrap_or("folder.fill"), 10., Some(p.secondary)),
            };
            let mut tab_el = shelf_tab()
                .child(tipped(div().id(("action-shelf-badge", index)).flex().flex_none().items_center().justify_center().size(px(18.)).mx(px(2.)), badge_tip).child(badge_face))
                .child(hairline());
            for (group_index, group) in shelf.patches.iter().enumerate() {
                if group_index > 0 {
                    tab_el = tab_el.child(hairline());
                }
                for action in group {
                    tab_el = tab_el.child(self.action_chip(action, thread, cx));
                }
            }
            row = row.child(tab_el);
        }
        if shown < actions.len() {
            let more: Vec<Action> = actions[shown..].to_vec();
            row = row.child(shelf_tab().child(self.more_chip(more, thread, cx)));
        }
        Some(row.into_any_element())
    }

    /// One chip on a shelf (`ActionChip`): the patch's own icon in colour, or
    /// its symbol, the title and its live count. The shelf is the chips'
    /// frame, so each is only a fill under the pointer.
    fn action_chip(&mut self, action: &Action, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let running = self.panels.running.contains(&panel_key(&thread.id, &action.id));
        let (action2, thread_id) = (action.clone(), thread.id.clone());
        let symbol = core::action_glyph(action).to_owned();
        tipped(
            div().id(SharedString::from(format!("action-{}", action.id))).flex().flex_none().items_center().gap(px(5.)).px(px(8.)).py(px(4.)).rounded(px(6.)).when(!running, |s| s.cursor_pointer().hover(move |s| s.bg(p.hover))),
            chip_tip(action),
        )
        .when(running, |s| s.child(Spinner::new().with_size(px(11.)).color(p.secondary)))
        .when(!running, |s| s.child(crate::patch_icons::patch_icon(action.icon_file.as_deref(), &symbol, 11., Some(p.secondary))))
        .child(div().text_size(px(style::size::CHIP)).text_color(p.fg).whitespace_nowrap().child(action.title.clone()))
        .when_some(action.badge, |s, n| s.child(div().px(px(5.)).rounded_full().bg(style::primary_opacity(cx, 0.08)).text_size(px(style::size::SMALL)).font_weight(FontWeight::SEMIBOLD).text_color(p.secondary).child(badge_text(n))))
        .when(!running, |s| s.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.run_action(&action2, thread_id.clone(), cx))))
        .into_any_element()
    }

    /// The trust chip's press: Patches, where each can be read and trusted.
    fn show_patches(&mut self, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else { return };
        // Afterwards: the app may be mid-update when the pane is drawn.
        cx.defer(move |cx| app.update(cx, |t, cx| t.show_pane(crate::shell::PATCHES, cx)));
    }

    /// "+N" on a shelf of its own (`ActionChipsMoreTab`): the chips that
    /// didn't fit, in a menu.
    fn more_chip(&mut self, more: Vec<Action>, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::popover::Popover;
        let tip = more.iter().map(|a| a.title.clone()).collect::<Vec<_>>().join(", ");
        let (pane, thread_id) = (cx.entity().downgrade(), thread.id.clone());
        let count = more.len();
        style::menu::popover(Popover::new("actions-more"), cx)
            .anchor(Anchor::BottomLeft)
            .trigger(MoreTrigger { count, tip })
            .content(move |_, _, cx| {
                let popover = cx.entity();
                // A menu, drawn as the app's are (style::menu).
                v_flex().min_w(gpui_kit::px(200.)).children(more.iter().enumerate().map(|(i, action)| {
                    let (pane, thread_id, action, popover) = (pane.clone(), thread_id.clone(), action.clone(), popover.clone());
                    style::menu::row(("action-more-row", i), Some(crate::icons::symbol("bolt")), cx)
                        .child(action.title.clone())
                        .on_click(move |_, window, cx| {
                            popover.update(cx, |s, cx| s.dismiss(window, cx));
                            pane.update(cx, |pane, cx| pane.run_action(&action, thread_id.clone(), cx)).ok();
                        })
                }))
            })
            .into_any_element()
    }

    /// Whether the chip row has any chips, so the host can leave it out.
    pub(crate) fn has_action_chips(thread: &ThreadSummary) -> bool {
        thread.actions.as_ref().is_some_and(|a| !a.is_empty()) || thread.patches().iter().any(|p| p.untrusted.is_some())
    }

    #[cfg(feature = "automation")]
    pub(crate) fn fill_patches_dump(&self, pane: &mut serde_json::Map<String, Value>, thread: &ThreadSummary) {
        let (actions, panels, hooks) = (thread.actions(), thread.panels(), thread.hooks());
        let rows: Vec<Value> = thread
            .patches()
            .iter()
            .map(|patch| {
                let trusting = patch.file.as_deref().zip(patch.hash.as_deref()).is_some_and(|(f, h)| self.panels.trusting.contains(&trust_key(f, h)));
                json!({
                    "id": patch.id,
                    "source": words::source_name(&patch.source),
                    "waiting": patch.untrusted.as_ref().map(|_| words::waiting_text(patch)),
                    "trusting": patch.untrusted.is_some().then_some(trusting),
                    "gives": (patch.untrusted.is_none() && patch.error.is_none()).then(|| words::gives(&patch.id, &actions, &panels, &hooks)),
                    "error": patch.error.clone().or(patch.live_error.clone()),
                    "file": patch.file,
                })
            })
            .collect();
        pane.insert("patches".into(), Value::Array(rows));
    }

    #[cfg(feature = "automation")]
    pub(crate) fn fill_panel_dump(&self, pane: &mut serde_json::Map<String, Value>, thread: &ThreadSummary, id: &str) {
        let Some(panel) = thread.panels().into_iter().find(|p| p.id == id) else { return };
        let key = panel_key(&thread.id, id);
        match panel.kind.as_str() {
            "list" => match self.panels.renders.get(&key) {
                None => {
                    pane.insert("empty".into(), json!("Loading…"));
                }
                Some(render) => match render.items.as_ref().filter(|i| !i.is_empty()) {
                    None => {
                        pane.insert("empty".into(), json!(render.note.clone().unwrap_or_else(|| "Nothing in this list.".to_owned())));
                    }
                    Some(items) => {
                        let headings = PanelItem::headings(items);
                        let mut sections: Vec<Value> = vec![];
                        for (index, item) in items.iter().enumerate() {
                            let new_section = headings[index].clone().or_else(|| sections.is_empty().then(String::new));
                            if let Some(heading) = new_section {
                                sections.push(json!({ "key": heading, "title": heading, "count": 0, "folded": false, "rows": [] }));
                            }
                            let last = sections.last_mut().expect("a section");
                            let row = json!({ "title": item.title, "trailing": item.trailing, "detail": item.detail, "symbol": item.icon.clone().filter(|i| !i.is_empty()).or_else(|| item.tone_kind().map(str::to_owned)) });
                            last["rows"].as_array_mut().expect("rows").push(row);
                            last["count"] = json!(last["count"].as_i64().unwrap_or(0) + 1);
                        }
                        pane.insert("list".into(), Value::Array(sections));
                    }
                },
            },
            "image" => {
                let shown = self.panels.renders.get(&key).and_then(|r| r.image.as_ref()).and_then(|i| match (i.width, i.height) {
                    (Some(w), Some(h)) => Some(format!("picture {w}x{h}")),
                    _ => None,
                });
                let note = self.panels.renders.get(&key).and_then(|r| r.note.clone());
                pane.insert("image".into(), json!(shown.or(note).unwrap_or_else(|| "Taking a screenshot…".to_owned())));
            }
            _ => {}
        }
    }
}

fn abbreviate_home(path: &str) -> String {
    let home = crate::files::home_dir();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// The `trust` automation command.
#[cfg(feature = "automation")]
pub(crate) fn trust_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    let id = cmd["id"].as_str();
    let waiting: Vec<PatchInfo> = thread.patches().into_iter().filter(|p| p.untrusted.is_some() && id.is_none_or(|i| p.id == i)).collect();
    if waiting.is_empty() {
        return Err("nothing waiting to be trusted".into());
    }
    let names: Vec<String> = waiting.iter().map(|p| p.id.clone()).collect();
    side.update(cx, |pane, cx| pane.trust_patches(waiting, thread.id.clone(), cx));
    Ok(json!(format!("ok {}", names.join(" "))))
}

/// The `action` automation command: runs it (Swift waits for a command to
/// finish; here a step reads the result from the dump's `actions`).
#[cfg(feature = "automation")]
pub(crate) fn action_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    let id = cmd["id"].as_str().unwrap_or("");
    let actions = thread.actions();
    let Some(action) = actions.into_iter().find(|a| a.id == id) else { return Err(format!("no action {id}; this thread has {}", thread.actions().iter().map(|a| a.id.clone()).collect::<Vec<_>>().join(", "))) };
    side.update(cx, |pane, cx| pane.run_action(&action, thread.id.clone(), cx));
    Ok(json!(format!("ok ran {id}")))
}

/// The `actions` part of the dump, the Swift app's shape.
#[cfg(feature = "automation")]
pub(crate) fn actions_dump(pane: &SidePane, thread: &ThreadSummary) -> Value {
    Value::Array(thread.actions().iter().map(|a| json!({ "id": a.id, "title": a.title, "kind": a.kind, "running": pane.panels.running.contains(&panel_key(&thread.id, &a.id)) })).collect())
}

/// A chip's live count as it reads: "99+" past 99.
fn badge_text(n: i64) -> String {
    if n > 99 { "99+".to_owned() } else { n.to_string() }
}

/// What a chip does, then where it's from (`ActionChip.tip`): "From the
/// checks patch, in threadbear", so a chip's origin is a hover away.
fn chip_tip(action: &Action) -> String {
    let what = action.note.clone().or_else(|| action.command.clone()).or_else(|| action.prompt.clone()).or_else(|| action.url.clone()).unwrap_or_else(|| action.title.clone());
    if action.patch.is_empty() {
        return what;
    }
    let folder = action.folder.as_deref().map(|f| format!(", in {}", threadbear_core::inbox::folder_name(f))).unwrap_or_default();
    format!("{what}\nFrom the {} patch{folder}", action.patch)
}

/// "1 patch to trust", or the folder's Claude Code hooks when they're what
/// waits (`WaitingPatchesChip.label`).
fn trust_label(waiting: &[PatchInfo]) -> String {
    let claude = waiting.iter().any(|p| p.source == "claude");
    let patches = waiting.len() - usize::from(claude);
    if patches == 0 {
        "Hooks to trust".to_owned()
    } else if claude {
        format!("{} to trust", waiting.len())
    } else if patches == 1 {
        "1 patch to trust".to_owned()
    } else {
        format!("{patches} patches to trust")
    }
}

fn trust_tip(waiting: &[PatchInfo]) -> String {
    if waiting.iter().any(|p| p.source == "claude") {
        "This folder has Claude Code hooks, environment settings or patch files that haven't been used, because you haven't trusted them yet. Each can run code on your Mac. Open Patches to read and trust them".to_owned()
    } else {
        "This folder has patch files that haven't run, because you haven't trusted them yet. A patch is code that runs on your Mac. Open Patches to read and trust them".to_owned()
    }
}

/// A shelf's project badge: the project's picture, its symbol, or its letter.
fn project_badge_face(mark: crate::patch_icons::ProjectMark, p: Palette) -> AnyElement {
    match mark.icon {
        Some(crate::sidebar::ProjectIcon { image_path: Some(path), .. }) => {
            let letter = mark.letter.clone();
            img(std::path::PathBuf::from(path))
                .size(px(14.))
                .rounded(px(3.))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || div().text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).text_color(p.secondary).child(letter.clone()).into_any_element())
                .into_any_element()
        }
        Some(crate::sidebar::ProjectIcon { symbol: Some(symbol), .. }) => crate::patch_icons::glyph(&symbol, 11., Some(p.secondary)),
        _ => div().text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).text_color(p.secondary).child(mark.letter).into_any_element(),
    }
}

/// "+N" (`ActionChipsMoreTab`'s label), as the popover's trigger.
#[derive(IntoElement)]
struct MoreTrigger {
    count: usize,
    tip: String,
}

impl gpui_kit::component::Selectable for MoreTrigger {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for MoreTrigger {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        tipped(div().id("actions-more-button").px(px(6.)).py(px(4.)).rounded(px(6.)).cursor_pointer().hover(move |s| s.bg(p.hover)), self.tip)
            .child(div().text_size(px(style::size::CHIP)).text_color(p.secondary).child(format!("+{}", self.count)))
    }
}
