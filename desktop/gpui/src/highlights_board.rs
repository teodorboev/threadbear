//! A Notion board beside the thread. The project page's cards open Notion;
//! these mention a card in the message box (Swift's HighlightsBoard).
use crate::highlights::{icon, MenuAction, MenuItem, Palette, SidePane, OUTER_INSET};
use crate::style::{self, px};
use crate::tips::Tip as _;
use gpui_kit::base::{Scrollbar, ScrollbarMode, TestSupportExt as _};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::collections::HashSet;
use threadbear_core::board;
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::projects::{self, Task, TaskKind, Tasks};

#[derive(Default)]
pub(crate) struct BoardState {
    unfolded: HashSet<String>,
    all: HashSet<String>,
    scroll: ScrollHandle,
    window: Option<u64>,
}

impl SidePane {
    pub(crate) fn task_board(&mut self, project: &str, tasks: &Tasks, thread: &ThreadSummary, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let columns = tasks.board(&thread.id);
        if columns.is_empty() { return div().into_any_element() }
        let key = format!("{}/{project}", thread.id);
        let state = self.boards.entry(key.clone()).or_default();
        state.window = Some(crate::thread::window_key(window));
        let folded = columns.iter().filter(|c| c.column.kind == TaskKind::Done && !state.unfolded.contains(&c.column.name)).count();
        let available = self.width / style::zoomed(1.) - OUTER_INSET * 2.;
        let width = board::column_width(available, columns.len() - folded, folded);
        let scroll = state.scroll.clone();
        let unfolded = state.unfolded.clone();
        let all = state.all.clone();
        let mut row = h_flex().items_start().gap(px(6.)).px(px(OUTER_INSET)).pb(px(12.));
        for (index, column) in columns.iter().enumerate() {
            let name = column.column.name.clone();
            let count = column.cards.len();
            let done = column.column.kind == TaskKind::Done;
            let color = crate::project_page::task_color(column.column, cx);
            let dot = || div().size(px(6.)).rounded_full().flex_none().bg(color);
            if done && !unfolded.contains(&name) {
                let (key, name) = (key.clone(), name.clone());
                row = row.child(v_flex().id(SharedString::from(format!("board-fold-{project}-{index}"))).w(px(28.)).flex_none().items_center().gap(px(6.)).py(px(8.)).rounded(px(8.)).bg(p.card).hover(move |s| s.bg(p.hover)).cursor_pointer()
                    .tip(format!("Show {count} in {name}"))
                    .test_support().role(Role::Button).aria_label(SharedString::from(format!("Show {count} in {name}")))
                    .child(dot()).child(div().max_w(px(24.)).truncate().text_size(px(9.)).text_color(p.secondary).child(name.clone())).child(div().text_size(px(style::size::SMALL)).text_color(p.faint).child(count.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| { this.boards.entry(key.clone()).or_default().unfolded.insert(name.clone()); cx.notify(); })));
                continue;
            }
            let header = h_flex().gap(px(5.)).items_center().px(px(3.)).child(dot()).child(div().flex_1().min_w_0().truncate().font_semibold().child(name.clone())).child(div().text_color(p.faint).child(count.to_string()));
            let (fold_key, fold_name) = (key.clone(), name.clone());
            let header = div().id(SharedString::from(format!("board-heading-{project}-{index}"))).text_size(px(style::size::SMALL)).line_height(px(style::natural_line(style::size::SMALL))).text_color(p.secondary).child(header).test_support()
                .when(done, |d| d.cursor_pointer().tip(format!("Fold {name}")).role(Role::Button).aria_label(SharedString::from(format!("Fold {name}"))).on_click(cx.listener(move |this, _, _, cx| { this.boards.entry(fold_key.clone()).or_default().unfolded.remove(&fold_name); cx.notify(); })));
            let mut col = v_flex().w(px(width)).flex_none().gap(px(5.)).p(px(6.)).rounded(px(8.)).bg(p.card).child(header);
            let shown = if all.contains(&name) { count } else { count.min(4) };
            for task in column.cards.iter().take(shown) {
                col = col.child(self.board_card(task, done, project, thread, p, cx));
            }
            if count > 4 {
                let expanded = all.contains(&name);
                let (key, name) = (key.clone(), name.clone());
                let words = if expanded { "Show fewer".to_owned() } else { format!("{} more", count - 4) };
                col = col.child(div().id(SharedString::from(format!("board-more-{project}-{index}"))).px(px(4.)).text_size(px(style::size::SMALL)).text_color(p.faint).cursor_pointer().tip(if expanded { "Show the first 4 cards".to_owned() } else { format!("Show every card in {name}") }).test_support().role(Role::Button).aria_label(SharedString::from(words.clone())).child(words)
                    .on_click(cx.listener(move |this, _, _, cx| { let all = &mut this.boards.entry(key.clone()).or_default().all; if !all.remove(&name) { all.insert(name.clone()); } cx.notify(); })));
            }
            row = row.child(col);
        }
        let total = width * (columns.len() - folded) as f32 + folded as f32 * 28. + columns.len().saturating_sub(1) as f32 * 6.;
        let overflow = total > available + 1.;
        // The scroll viewport measures its immediate child. An unconstrained
        // flex row would fit the viewport while its columns overflowed it,
        // leaving max_offset at zero and the far columns unreachable.
        let row = row.w(px(total + OUTER_INSET * 2.)).flex_none();
        let mut view = v_flex().w_full().min_w_0().pt(px(4.)).child(div().relative().w_full().child(div().id(SharedString::from(format!("board-scroll-{key}"))).w_full().overflow_x_scroll().restrict_scroll_to_axis().track_scroll(&scroll).child(row))
            .when(overflow, |d| d.child(Scrollbar::horizontal(&scroll).mode(ScrollbarMode::Always))));
        if overflow {
            let mut arrows = h_flex().px(px(OUTER_INSET)).gap(px(6.)).items_center();
            for (direction, symbol, tip) in [(-1, "chevron.left", "Previous board column"), (1, "chevron.right", "Next board column")] {
                let scroll = scroll.clone();
                let at = -f32::from(scroll.offset().x) / style::zoomed(1.);
                let enabled = if direction < 0 { at > 0.5 } else { at < total - available - 0.5 };
                arrows = arrows.child(div().id(SharedString::from(format!("board-page-{key}-{direction}"))).size(px(22.)).flex().items_center().justify_center().rounded(px(4.)).tip(tip).test_support().role(Role::Button).aria_label(tip)
                    .when(enabled, |d| d.hover(move |s| s.bg(p.hover)).cursor_pointer())
                    .when(!enabled, |d| d.opacity(0.35).cursor_default())
                    .child(icon(symbol, 10., p.secondary)).when(enabled, |d| d.on_click(cx.listener(move |_, _, _, cx| {
                        let at = -f32::from(scroll.offset().x);
                        let maximum = f32::from(scroll.max_offset().x);
                        let to = board::page(at, direction, style::zoomed(width + 6.), maximum);
                        scroll.set_offset(point(gpui_kit::px(-to), gpui_kit::px(0.)));
                        cx.notify();
                    }))));
            }
            view = view.child(arrows);
        }
        view.into_any_element()
    }

    fn board_card(&self, task: &Task, done: bool, project: &str, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mine = task.is_worked_by(&thread.id);
        let other = (!mine).then(|| self.app.upgrade()).flatten().and_then(|app| {
            let app = app.read(cx);
            task.working_thread(app.threads.keys().map(String::as_str)).and_then(|id| app.threads.get(id)).cloned()
        });
        let group = SharedString::from(format!("board-card-{project}-{}", task.id));
        let mut meta = h_flex().min_w_0().gap(px(5.)).text_size(px(10.5)).line_height(px(style::natural_line(10.5))).text_color(p.faint)
            .when(!task.who.is_empty(), |d| d.child(div().min_w_0().truncate().child(task.who.join(", "))))
            .when_some(projects::task_due(task), |d, due| d.child(div().truncate().child(due))).child(div().flex_1());
        if mine { meta = meta.child(icon("bubble.left.fill", 10., p.secondary)); }
        if let Some(other) = &other {
            let id = other.id.clone();
            meta = meta.child(h_flex().id(SharedString::from(format!("board-thread-{}", task.id))).gap(px(3.)).items_center().cursor_pointer().tip(format!("Open {}", other.title)).test_support().role(Role::Button).aria_label(SharedString::from(format!("Open {}", other.title)))
                .when(other.busy(), |d| d.child(div().size(px(5.)).rounded_full().bg(p.accent)))
                .child(icon("bubble.left", 10., p.secondary))
                .on_click(cx.listener(move |this, _, window, cx| { cx.stop_propagation(); if let Some(app) = this.app.upgrade() { app.update(cx, |app, cx| app.open_thread_link(&id, window, cx)); } })));
        }
        let mut body = v_flex().gap(px(3.)).px(px(8.)).py(px(6.)).child(div().text_size(px(style::size::SMALL)).line_height(px(style::natural_line(style::size::SMALL))).text_color(if done { p.secondary } else { p.fg }).line_clamp(2).overflow_hidden().text_ellipsis().child(task.title.clone())).child(meta);
        // This sibling button handles its own click before the card, so opening
        // Notion never also changes the draft.
        let url = task.url.clone();
        body = body.child(div().id(SharedString::from(format!("board-notion-{}", task.id))).absolute().top(px(3.)).right(px(3.)).invisible().group_hover(group.clone(), |s| s.visible()).rounded(px(4.)).bg(p.hover).p(px(2.)).cursor_pointer().tip("Open in Notion").test_support().role(Role::Button).aria_label("Open in Notion")
            .child(icon("arrow.up.forward.square", 11., p.secondary)).on_click(cx.listener(move |_, _, _, cx| { cx.stop_propagation(); crate::files::open_web(&url, cx); })));
        let (mention, mention_thread) = (task.clone(), thread.id.clone());
        let (menu_task, menu_other, menu_thread) = (task.clone(), other.map(|t| t.id.clone()), thread.id.clone());
        div().id(group.clone()).group(group).relative().rounded(px(6.)).border_1().border_color(p.card_border).bg(cx.theme().muted).hover(move |s| s.bg(p.hover)).cursor_pointer().tip("Mention this card in your message")
            .test_support().role(Role::Button).aria_label(SharedString::from(format!("Board card: {}", task.title)))
            .child(body)
            .on_click(cx.listener(move |this, _, window, cx| this.mention_board_card(&mention, &mention_thread, window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                // A card's menu owns this click; the enclosing dock panel has
                // a different menu for its unused background.
                cx.stop_propagation();
                let mut items = vec![MenuItem::action("Mention in Message", "text.bubble", MenuAction::MentionCard { task: menu_task.clone(), thread_id: menu_thread.clone() }), MenuItem::action("Open in Notion", "arrow.up.forward.square", MenuAction::OpenUrl { url: menu_task.url.clone() })];
                if let Some(id) = &menu_other { items.push(MenuItem::action("Open the Thread Working on It", "bubble.left", MenuAction::OpenThread { id: id.clone() })); }
                items.push(MenuItem::action("Copy Link", "link", MenuAction::Copy { text: menu_task.url.clone() }));
                this.open_menu(items, ev.position, window, cx);
            })).into_any_element()
    }

    pub(crate) fn mention_board_card(&self, task: &Task, thread_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                // Keep the card's thread even if a menu is left open while
                // another pane becomes selected.
                if !app.threads.contains_key(thread_id) { return }
                let composer = app.composer_for(thread_id, window, cx);
                composer.update(cx, |c, cx| c.mention_card(task, cx));
                app.drafts_changed(cx);
            });
        }
    }

    /// Page navigation watches in capture, before native scrollers. Yield a
    /// gesture starting in a live board to its horizontal and vertical axes.
    pub(crate) fn board_scroll_at(&self, thread: &ThreadSummary, at: Point<Pixels>, window: &Window) -> bool {
        // A preview or explanation replaces the board without erasing its
        // last layout bounds. Only the visible Highlights body owns gestures.
        if self.previews.get(&thread.id).is_some_and(|pv| pv.from == crate::highlights::HIGHLIGHTS)
            || self.explaining_over(&thread.id) == Some(crate::highlights::HIGHLIGHTS)
        { return false }
        // A tall board may extend under the dock bar after the pane scrolls.
        // Its layout box alone includes content that is clipped out there.
        if !self.scroll.bounds().contains(&at) { return false }
        let owner = crate::thread::window_key(window);
        threadbear_core::highlights::project_rows(thread, |id| self.projects.get(id).cloned()).iter().any(|row| {
            !self.is_folded(&threadbear_core::highlights::fold::project(&row.project.id))
                && row.project.open_task_count() > 0
                && self.boards.get(&format!("{}/{}", thread.id, row.project.id)).is_some_and(|state| state.window == Some(owner) && state.scroll.bounds().contains(&at))
        })
    }
}
