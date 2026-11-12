// A project's page, in the main panel in place of a thread (a port of the
// Swift app's ProjectPage): its icon and title, who made it and when, New
// Thread; then its parts, goals, state, task board, notes, folders, threads
// and the patches its folders give them. The title is edited in place; goals,
// state and notes with Edit, in a box with Save and Cancel. Archive, parts,
// merge and delete are in the bar's ... menu.
use crate::app::Threadbear;
use crate::projects::{project_icon, ProjectAction};
use crate::sidebar::{MenuAction, MenuRow};
use crate::style::{self, px};
use crate::{icons, now_ms};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use threadbear_core::highlights::patches::{gives, source_name};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::new_thread::abbreviate_home;
use threadbear_core::projects::{self as rules, Project, Section, Sections, Task, TaskColumn, TaskKind, Tasks, TextField};
use threadbear_core::sidebar::local_now;

/// The page's column (`ProjectPage`'s maxWidth).
const PAGE_WIDTH: f32 = 720.;

impl Threadbear {
    /// The open project's page, or nothing if it's gone.
    pub(crate) fn render_project_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.open_project().cloned() else { return div().into_any_element() };
        let threads: Vec<ThreadSummary> = rules::threads_in(&project.id, self.threads.values()).into_iter().cloned().collect();
        let header = self.render_page_header(&project, threads.len(), window, cx);
        let mut column = v_flex().w_full().gap(px(30.)).child(header);
        // The projectDecisions experiment: its threads' open cards of asks
        // first, as that's what's waiting on Mike (project_decisions.rs).
        if let Some(decisions) = self.render_decisions_section(&project, window, cx) {
            column = column.child(decisions);
        }
        if let Some(parts) = self.render_parts_section(&project, cx) {
            column = column.child(parts);
        }
        column = column
            .child(self.render_text_section(&project, TextField::Goals, window, cx))
            .child(self.render_text_section(&project, TextField::State, window, cx))
            .child(self.render_tasks_section(&project, cx))
            .child(self.render_text_section(&project, TextField::Notes, window, cx))
            .child(self.render_folders_section(&project, cx))
            .child(self.render_threads_section(&project, &threads, cx))
            .child(self.render_patches_section(&project, &threads, cx));
        div()
            .id(SharedString::from(format!("project-page-{}", project.id)))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.projects.page_scroll)
            .child(h_flex().w_full().justify_center().child(v_flex().w_full().max_w(px(PAGE_WIDTH + 64.)).px(px(32.)).pt(px(28.)).pb(px(48.)).child(column)))
            .into_any_element()
    }

    /// The icon (a menu to choose another), the title, who made it and
    /// whose part it is, and New Thread.
    fn render_page_header(&mut self, project: &Project, threads: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let faint = style::faint_text(cx);
        let title = self.project_title_field(project, window, cx);
        let made = rules::made_by(project, threads, &local_now());
        let mut byline = h_flex().gap(px(6.)).items_center().text_size(px(style::size::LABEL)).line_height(px(16.)).child(div().text_color(faint).child(made));
        // A part says whose, and goes up to it.
        if let Some(parent) = project.parent_id.as_deref().and_then(|p| self.project(p)).cloned() {
            let link = style::link_text(cx);
            byline = byline.child(div().text_color(faint).child("·")).child(
                crate::highlights::tipped(div().id("project-parent").cursor_pointer().text_color(link).hover(|s| s.underline()), format!("Open {}", parent.title))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_project_page(&parent.id, cx)))
                    .child(format!("Part of {}", parent.title)),
            );
        }
        let id = project.id.clone();
        // As tall as the Swift field (27), so who made it sits where it does,
        // and 2 up: AppKit's field puts its text that much nearer its top.
        let title_box = div()
            .w_full()
            .mt(px(-2.))
            .text_size(px(24.))
            .line_height(px(27.))
            .font_semibold()
            // Escape puts it back as it was.
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    this.revert_project_title(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(crate::highlights::tipped(div().id("project-title"), "The project's name").child(Input::new(&title).appearance(false).pl(px(0.)).h(px(27.)).text_size(px(24.)).font_semibold()));
        h_flex()
            .w_full()
            .items_center()
            .gap(px(14.))
            .child(self.render_page_icon(project, cx))
            .child(v_flex().flex_1().min_w_0().gap(px(4.)).child(title_box).child(byline))
            .child(large_push_button("project-new-thread", "square.and.pencil", "New Thread", "Start a thread in this project, in its folder, with its goals from the first message", cx, cx.listener(move |this, _: &ClickEvent, _, cx| this.new_thread_in_project(&id, cx))))
            .into_any_element()
    }

    /// `IconButton`: the project's icon, big, with a menu to choose another or
    /// go back to the librarian's. Its tooltip says where it came from; it
    /// spins while a new one's on its way.
    fn render_page_icon(&mut self, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let id = project.id.clone();
        let busy = self.projects.busy.contains(&format!("icon:{id}"));
        let tip = rules::icon_help(project, &crate::files::home_dir());
        let mut rows = vec![MenuRow::item("Choose a Picture…", "photo", MenuAction::Project(ProjectAction::ChooseIcon(id.clone())))];
        if project.icon.as_ref().is_some_and(|i| i.is_chosen()) {
            rows.push(MenuRow::item("Use the Librarian's Icon", "books.vertical", MenuAction::Project(ProjectAction::AutomaticIcon(id.clone()))));
        }
        if project.icon.as_ref().is_some_and(|i| i.from.is_some()) {
            rows.push(MenuRow::separator());
            rows.push(MenuRow::item("Show Original in Finder", "folder", MenuAction::Project(ProjectAction::RevealIcon(id.clone()))));
        }
        let face: AnyElement = if busy {
            div().size(px(44.)).flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(style::secondary_text(cx)).with_size(px(16.))).into_any_element()
        } else {
            project_icon(Some(project), "", 44., cx)
        };
        crate::highlights::tipped(div().id("project-icon").flex_none().cursor_pointer().hover(|s| s.opacity(0.8)), tip)
            .when(!busy, |s| {
                s.on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    let at = ev.position();
                    this.open_menu(None, rows.clone(), point(at.x, at.y + px(14.)), false, window, cx);
                }))
            })
            .child(face)
            .into_any_element()
    }

    /// `PartsSection`: a parent's parts, each with its state's first line and
    /// its decisions (with the projectDecisions experiment) or how many
    /// threads it has. A row opens the part's page.
    fn render_parts_section(&mut self, project: &Project, cx: &mut Context<Self>) -> Option<AnyElement> {
        let decisions_on = self.project_decisions_on();
        let parts: Vec<(Project, usize, usize)> = {
            let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
            sections.parts_of(&project.id).iter().map(|s: &&Section| (s.project.clone(), s.threads.len(), if decisions_on { rules::decision_count(&s.threads) } else { 0 })).collect()
        };
        if parts.is_empty() {
            return None;
        }
        let mut section = v_flex().w_full().child(page_heading("Parts", Some(parts.len().to_string()), None, cx));
        for (part, threads, decisions) in parts {
            let key = format!("part:{}", part.id);
            let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
            let id = part.id.clone();
            let right: AnyElement = if decisions > 0 {
                div().text_color(style::needs_you(cx)).child(rules::decision_label(decisions)).into_any_element()
            } else {
                div().text_color(style::faint_text(cx)).child(rules::thread_count_words(threads)).into_any_element()
            };
            section = section.child(
                crate::highlights::tipped(
                    h_flex().id(SharedString::from(format!("part-{}", part.id))).w_full().items_center().gap(px(10.)).px(px(8.)).mx(px(-8.)).py(px(7.)).rounded(px(6.)).cursor_pointer().when(hovering, |s| s.bg(style::sidebar_hover(cx))),
                    format!("Open {}", part.title),
                )
                .on_hover(self.page_hover(key, cx))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_project_page(&id, cx)))
                .child(project_icon(Some(&part), "", 20., cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(div().truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).font_semibold().child(part.title.clone()))
                        .when_some(rules::part_line(&part), |s, line| s.child(div().truncate().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(style::secondary_text(cx)).child(line))),
                )
                .child(div().flex_none().text_size(px(style::size::DETAIL)).line_height(px(15.)).font_medium().child(right)),
            );
        }
        Some(section.into_any_element())
    }

    /// Notes the pointer over one of the page's rows.
    fn page_hover(&self, key: String, cx: &mut Context<Self>) -> Box<dyn Fn(&bool, &mut Window, &mut App) + 'static> {
        Box::new(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.projects.hovered = Some(key.clone());
            } else if this.projects.hovered.as_deref() == Some(key.as_str()) {
                this.projects.hovered = None;
            }
            cx.notify();
        }))
    }

    /// `TextSection`: goals, state or notes, with Edit; while editing, the
    /// box with Cancel and Save. Empty, it says what it's for, and a double
    /// click starts editing.
    fn render_text_section(&mut self, project: &Project, field: TextField, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.projects.editing.as_ref().filter(|e| e.project == project.id && e.field == field).map(|e| e.input.clone());
        let key = format!("text:{}", field.title());
        let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
        let detail = field.detail(project, &local_now());
        let id = project.id.clone();
        let edit = editing.is_none().then(|| {
            let id = id.clone();
            quiet_button(SharedString::from(format!("edit-{}", field.title())), "Edit", &format!("Edit the {} (or double-click it)", field.title().to_lowercase()), hovering, cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.start_editing_project(&id, field, window, cx)))
        });
        let mut section = v_flex().w_full().id(SharedString::from(format!("section-{}", field.title()))).on_hover(self.page_hover(key, cx)).child(page_heading(field.title(), detail, edit, cx));
        let text = field.text(project).to_owned();
        if let Some(input) = editing {
            let min = if field == TextField::Notes { 220. } else { 90. };
            let _ = window;
            section = section.child(
                v_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_end()
                    .child(
                        div()
                            .w_full()
                            .min_h(px(min))
                            .px(px(6.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .bg(style::primary_opacity(cx, 0.035))
                            .border(gpui_kit::px(0.5))
                            .border_color(cx.theme().border)
                            // Escape cancels, as the Swift box's Cancel shortcut does.
                            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                                if ev.keystroke.key == "escape" {
                                    this.finish_editing_project(false, window, cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .child(crate::composer::bare_textarea(Textarea::new(&input)).text_size(px(style::size::LABEL)).line_height(px(19.))),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(small_push_button("edit-cancel", "Cancel", "Put it back as it was (Esc)", cx, cx.listener(|this, _: &ClickEvent, window, cx| this.finish_editing_project(false, window, cx))))
                            .child(small_push_button("edit-save", "Save", "Save (⌘Return)", cx, cx.listener(|this, _: &ClickEvent, window, cx| this.finish_editing_project(true, window, cx)))),
                    ),
            );
        } else if text.trim().is_empty() {
            section = section.child(
                div()
                    .id(SharedString::from(format!("empty-{}", field.title())))
                    .text_size(px(style::size::LABEL))
                    .line_height(px(16.))
                    .text_color(style::faint_text(cx))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        if ev.click_count() == 2 {
                            this.start_editing_project(&id, field, window, cx);
                        }
                    }))
                    .child(field.empty()),
            );
        } else {
            // Bold, code and links, with the line breaks kept, through the
            // open gate like every link in the app.
            let base = project.folders.first().cloned().unwrap_or_default();
            let view = crate::open_gate::reading_markdown_view(SharedString::from(format!("project-{}-{}", project.id, field.title())), keep_breaks(&text), crate::open_gate::Origin::ThisMac, &base).style(page_text_style(cx));
            // 19 a line (13 pt and the Swift text's 3 between lines), with
            // the first line's glyphs at its top as SwiftUI puts them, so a
            // point and a half up.
            section = section.child(div().w_full().mt(px(-1.5)).text_size(px(style::size::LABEL)).line_height(px(19.)).child(view));
        }
        section.into_any_element()
    }

    /// `TasksSection`: the board read from the Notion page, a column per Status
    /// option. Read-only: a card opens in Notion, where it's moved. Opening
    /// the page asks the runner to read it again if it's over a minute old.
    fn render_tasks_section(&mut self, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let id = project.id.clone();
        let detail = project.tasks.as_ref().and_then(|t| rules::tasks_detail(t, &local_now()));
        let trailing: AnyElement = match &project.notion {
            Some(notion) => {
                let busy = self.projects.busy.contains(&format!("tasks:{id}"));
                let open_url = project.tasks.as_ref().map(|t| t.url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| notion.clone());
                let refresh_id = id.clone();
                let refresh: AnyElement = if busy {
                    div().size(px(16.)).flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(style::secondary_text(cx)).with_size(px(11.))).into_any_element()
                } else {
                    icon_button("tasks-refresh", "arrow.clockwise", "Read the board from Notion again", cx, cx.listener(move |this, _: &ClickEvent, _, cx| this.refresh_project_tasks(&refresh_id, true, cx)))
                };
                h_flex()
                    .gap(px(10.))
                    .child(refresh)
                    .child(icon_button("tasks-open", "arrow.up.forward.square", "Open the board in Notion, where cards are moved", cx, move |_, _, cx| {
                        crate::open_gate::open_web(&open_url, cx);
                    }))
                    .into_any_element()
            }
            None => quiet_button("tasks-add", "Add Notion page…", "Show a task board from a Notion page here", true, cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.link_project_notion(&id, window, cx))),
        };
        let mut section = v_flex().w_full().child(page_heading("Tasks", detail, Some(trailing), cx));
        let faint = style::faint_text(cx);
        let note = |text: String, color: Hsla| div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(color).child(text);
        section = match (&project.notion, &project.tasks) {
            (None, _) => section.child(note("None yet. A Notion page with a Tasks database shows here as a board.".into(), faint)),
            (Some(_), Some(tasks)) if !tasks.columns.is_empty() => {
                let section = match &tasks.error {
                    Some(error) => section.child(div().pb(px(8.)).text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(style::needs_you(cx)).child(error.clone())),
                    None => section,
                };
                section.child(self.render_task_board(tasks, cx))
            }
            (Some(_), Some(Tasks { error: Some(error), .. })) => section.child(note(error.clone(), style::needs_you(cx))),
            (Some(_), tasks) => section.child(note(if tasks.as_ref().is_some_and(|t| !t.loading) { "No cards yet.".into() } else { "Reading the board from Notion…".into() }, faint)),
        };
        section.into_any_element()
    }

    /// `TaskBoard`: the columns side by side, each as wide as the others. A
    /// Done column shows its newest few; the rest are a click away in Notion.
    fn render_task_board(&mut self, tasks: &Tasks, cx: &mut Context<Self>) -> AnyElement {
        let mut board = h_flex().w_full().items_start().gap(px(8.));
        for (n, column) in tasks.columns.iter().enumerate() {
            let items = tasks.items_in(column);
            let done = column.kind == TaskKind::Done;
            let shown: Vec<&Task> = if done { items.iter().take(rules::DONE_SHOWN).copied().collect() } else { items.clone() };
            let mut col = v_flex()
                .flex_1()
                .min_w_0()
                .min_h(px(80.))
                .gap(px(6.))
                .p(px(6.))
                .rounded(px(8.))
                .bg(style::card_background(cx))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .px(px(4.))
                        .pb(px(2.))
                        .items_center()
                        .child(div().flex_none().size(px(6.)).rounded_full().bg(task_color(column, cx)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::DETAIL)).line_height(px(15.)).font_semibold().text_color(style::secondary_text(cx)).child(column.name.clone()))
                        .child(div().flex_none().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(style::faint_text(cx)).child(items.len().to_string())),
                );
            for (i, task) in shown.iter().enumerate() {
                col = col.child(self.render_task_card(task, done, n * 1000 + i, cx));
            }
            if shown.len() < items.len() {
                let url = tasks.url.clone();
                col = col.child(
                    crate::highlights::tipped(div().id(("tasks-more", n)).px(px(4.)).cursor_pointer().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(style::faint_text(cx)).hover(|s| s.underline()), "See them in Notion")
                        .on_click(move |_, _, cx| {
                            crate::open_gate::open_web(&url, cx);
                        })
                        .child(format!("{} more", items.len() - shown.len())),
                );
            }
            board = board.child(col);
        }
        board.into_any_element()
    }

    /// `TaskCard`: its title, then who, when and the thread working on it. A
    /// click opens it in Notion; the thread's chip opens the thread.
    fn render_task_card(&mut self, task: &Task, done: bool, n: usize, cx: &mut Context<Self>) -> AnyElement {
        let faint = style::faint_text(cx);
        let thread = task.thread.as_deref().map(str::to_lowercase).filter(|t| t.chars().count() >= 6).and_then(|t| self.threads.get(&t).or_else(|| self.threads.values().find(|x| x.id.starts_with(&t))).cloned());
        let mut meta = h_flex().flex_wrap().gap(px(6.)).items_center().text_size(px(style::size::SMALL)).line_height(px(14.)).text_color(faint);
        let has_meta = !task.who.is_empty() || task.due.is_some() || thread.is_some();
        if !task.who.is_empty() {
            meta = meta.child(div().truncate().child(task.who.join(", ")));
        }
        if let Some(due) = rules::task_due(task) {
            meta = meta.child(div().whitespace_nowrap().child(due));
        }
        if let Some(thread) = thread {
            let id = thread.id.clone();
            let busy = thread.busy();
            let accent = style::control_accent(cx);
            meta = meta.child(
                crate::highlights::tipped(h_flex().id(("task-thread", n)).gap(px(3.)).items_center().px(px(4.)).py(px(1.)).rounded(px(4.)).bg(style::primary_opacity(cx, 0.07)).cursor_pointer(), format!("Open the thread working on it: {}", thread.title))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_thread_from_page(id.clone(), window, cx);
                    }))
                    .when(busy, |s| s.child(div().size(px(5.)).rounded_full().bg(accent)))
                    .child(icons::symbol("bubble.left").with_size(px(10.)).text_color(faint)),
            );
        }
        let url = task.url.clone();
        let (hover, code) = (style::hover_fill(cx), style::code_background(cx));
        crate::highlights::tipped(
            v_flex().id(("task", n)).w_full().gap(px(4.)).px(px(8.)).py(px(7.)).rounded(px(6.)).bg(code).hover(move |s| s.bg(hover)).border(gpui_kit::px(0.5)).border_color(style::card_border(cx)).cursor_pointer(),
            "Open in Notion",
        )
        .on_click(move |_, _, cx| {
            crate::open_gate::open_web(&url, cx);
        })
        .child(div().text_size(px(style::size::DETAIL)).line_height(px(15.)).line_clamp(4).text_color(if done { style::secondary_text(cx) } else { cx.theme().foreground }).child(task.title.clone()))
        .when(has_meta, |s| s.child(meta))
        .into_any_element()
    }

    /// `FoldersSection`: patches load from each of its folders. A folder shows
    /// in Finder; the minus, under the pointer, takes it out.
    fn render_folders_section(&mut self, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let id = project.id.clone();
        let add = quiet_button("folders-add", "Add folder…", "Add a folder: the project's threads get the patches in it", true, cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.add_project_folder(&id, window, cx)));
        let mut section = v_flex().w_full().gap(px(2.)).child(page_heading("Folders", None, Some(add), cx));
        let home = crate::files::home_dir();
        for (n, folder) in project.folders.iter().enumerate() {
            let key = format!("folder:{folder}");
            let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
            let (show, remove, id) = (folder.clone(), folder.clone(), project.id.clone());
            section = section.child(
                h_flex()
                    .id(("project-folder", n))
                    .w_full()
                    .gap(px(8.))
                    .items_center()
                    .py(px(3.))
                    .on_hover(self.page_hover(key, cx))
                    .child(div().w(px(16.)).flex().justify_center().child(icons::symbol("folder").with_size(px(12.)).text_color(style::secondary_text(cx))))
                    .child(
                        crate::highlights::tipped(div().id(("folder-link", n)).min_w_0().truncate().cursor_pointer().text_size(px(style::size::LABEL)).line_height(px(16.)).hover(|s| s.underline()), "Show in Finder")
                            .on_click(move |_, _, cx| crate::open_gate::reveal(&show, crate::open_gate::Origin::ThisMac, cx))
                            .child(abbreviate_home(folder, &home)),
                    )
                    .child(div().flex_1())
                    .child(div().when(!hovering, |d| d.invisible()).child(icon_button(SharedString::from(format!("folder-remove-{n}")), "minus.circle", "Take this folder out of the project", cx, cx.listener(move |this, _: &ClickEvent, _, cx| this.remove_project_folder(&id, &remove, cx))))),
            );
        }
        if project.folders.is_empty() {
            section = section.child(div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(style::faint_text(cx)).child("None yet. A thread in the project gets the patches in each of its folders."));
        }
        section.into_any_element()
    }

    /// `ThreadsSection`: its threads, put away ones too, newest first. A row
    /// opens its thread; its menu keeps it in the project or takes it out.
    fn render_threads_section(&mut self, project: &Project, threads: &[ThreadSummary], cx: &mut Context<Self>) -> AnyElement {
        let id = project.id.clone();
        let new = quiet_button("threads-new", "New thread…", "Start a thread in this project", true, cx, cx.listener(move |this, _: &ClickEvent, _, cx| this.new_thread_in_project(&id, cx)));
        let mut section = v_flex().w_full().child(page_heading("Threads", (!threads.is_empty()).then(|| threads.len().to_string()), Some(new), cx));
        let clock = local_now();
        let (faint, secondary) = (style::faint_text(cx), style::secondary_text(cx));
        for t in threads {
            let key = format!("thread:{}", t.id);
            let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
            let membership = t.projects.iter().flatten().find(|m| m.id == project.id).cloned();
            let tip = membership.as_ref().map(rules::put_here_by).unwrap_or("");
            let (open_id, menu_thread) = (t.id.clone(), t.id.clone());
            let (project_id, project_title, pinned) = (project.id.clone(), project.title.clone(), membership.as_ref().is_some_and(|m| m.pinned));
            section = section.child(
                crate::highlights::tipped(
                    h_flex().id(SharedString::from(format!("project-thread-{}", t.id))).w_full().items_baseline().gap(px(8.)).px(px(8.)).mx(px(-8.)).py(px(6.)).rounded(px(6.)).cursor_pointer().when(hovering, |s| s.bg(style::sidebar_hover(cx))),
                    tip,
                )
                .on_hover(self.page_hover(key, cx))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_thread_from_page(open_id.clone(), window, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        let thread = menu_thread.clone();
                        let mut rows = vec![MenuRow::item("Open", "arrow.right.circle", MenuAction::Project(ProjectAction::OpenThread(thread.clone())))];
                        if !pinned {
                            rows.push(MenuRow::item(format!("Keep it in {project_title}"), "pin", MenuAction::ThreadProject { thread: thread.clone(), add: Some(project_id.clone()), remove: None }));
                        }
                        rows.push(MenuRow::item(format!("Take it out of {project_title}"), "minus.circle", MenuAction::ThreadProject { thread: thread.clone(), add: None, remove: Some(project_id.clone()) }));
                        this.open_menu(Some(thread), rows, ev.position, false, window, cx);
                    }),
                )
                .child(div().flex_shrink(1.).min_w_0().truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(if t.is_archived() { secondary } else { cx.theme().foreground }).child(if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() }))
                .child(div().flex_shrink(1.).min_w_0().truncate().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(faint).child(threadbear_core::inbox::folder_name(&t.cwd)))
                .child(div().flex_1())
                .child(div().flex_none().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(faint).child(rules::thread_meta(t, &clock))),
            );
        }
        if threads.is_empty() {
            section = section.child(div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(faint).child("No threads in it yet. Start one with New Thread, add one from a thread's ... menu, or the librarian puts threads here as they work in its folders."));
        }
        section.into_any_element()
    }

    /// `PatchesSection`: what its folders give its threads, as their summaries
    /// say, each patch once, and where each came from.
    fn render_patches_section(&mut self, project: &Project, threads: &[ThreadSummary], cx: &mut Context<Self>) -> AnyElement {
        let refs: Vec<&ThreadSummary> = threads.iter().collect();
        let found = rules::patches_across(project, &refs);
        let mut section = v_flex().w_full().gap(px(12.)).child(page_heading("Patches", (!found.is_empty()).then(|| found.len().to_string()), None, cx));
        let home = crate::files::home_dir();
        let (faint, secondary) = (style::faint_text(cx), style::secondary_text(cx));
        for (n, (patch, thread)) in found.iter().enumerate() {
            let mut block = v_flex().gap(px(3.)).child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(icons::symbol("square.dashed").with_size(px(11.)).text_color(secondary))
                    .child(div().text_size(px(style::size::LABEL)).line_height(px(16.)).font_medium().child(patch.id.clone()))
                    .child(div().text_size(px(style::size::SMALL)).line_height(px(14.)).text_color(faint).child(source_name(&patch.source))),
            );
            if let Some(error) = &patch.error {
                block = block.child(div().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(style::failed(cx)).child(error.clone()));
            } else {
                let says = gives(&patch.id, &thread.actions(), &thread.panels(), &thread.hooks());
                if !says.is_empty() {
                    block = block.child(div().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(secondary).child(says));
                }
            }
            if patch.file.is_none() && thread.is_remote() {
                // Another Mac's folder: nothing here to show it in.
                if let Some(folder) = &patch.folder {
                    block = block.child(div().text_size(px(style::size::SMALL)).line_height(px(14.)).text_color(faint).child(abbreviate_home(folder, &home)));
                }
            } else if let Some(from) = patch.file.clone().or_else(|| patch.folder.clone()) {
                let file = patch.file.clone();
                let (thread_cwd, remote) = (thread.cwd.clone(), thread.is_remote());
                let shown = abbreviate_home(&from, &home);
                block = block.child(
                    crate::highlights::tipped(div().id(("patch-from", n)).cursor_pointer().text_size(px(style::size::SMALL)).line_height(px(14.)).text_color(faint).hover(|s| s.underline()), if file.is_none() { "Show the folder in Finder" } else { "Open the file" })
                        .on_click(move |_, _, cx| {
                            let origin = if remote { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
                            match &file {
                                Some(file) => crate::open_gate::open_link(&threadbear_core::open_policy::file_url(file), origin, &thread_cwd, cx),
                                None => crate::open_gate::reveal(&from, origin, cx),
                            }
                        })
                        .child(shown),
                );
            }
            section = section.child(block);
        }
        if found.is_empty() {
            section = section.child(div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(faint).child("None from its folders yet. A patch in .threadbear/patches in one of them shows here, and on every thread in the project."));
        }
        section.into_any_element()
    }
}

impl Threadbear {
    /// `ProjectHeaderTitle`, in the main panel's bar over a project's page:
    /// Projects (which goes back to them), then its icon and title, and
    /// Archived after.
    pub(crate) fn render_project_header_title(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let project = self.open_project()?.clone();
        let (faint, secondary) = (style::faint_text(cx), style::secondary_text(cx));
        let fg = cx.theme().foreground;
        Some(
            h_flex()
                .min_w_0()
                // Where the thread's title line sits (header.rs).
                .pl(px(7.))
                .relative()
                .top(px(1.))
                .gap(px(5.))
                .items_center()
                .text_size(px(style::size::HEADER))
                .child(
                    crate::highlights::tipped(div().id("header-projects").flex_none().cursor_pointer().text_color(secondary).hover(move |s| s.text_color(fg)), "Show the Projects view")
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_mode(crate::shell::SidebarMode::Projects, cx)))
                        .child("Projects"),
                )
                .child(div().flex_none().text_color(faint).child("/"))
                .child(project_icon(Some(&project), "", 14., cx))
                .child(div().min_w_0().truncate().font_semibold().child(project.title.clone()))
                .when(project.archived, |s| s.child(div().flex_none().text_color(faint).child("·  Archived")))
                .into_any_element(),
        )
    }

    /// The bar's ... over a project's page: the project's menu (`ProjectMenu`).
    pub(crate) fn render_project_more_button(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.open_project()?.id.clone();
        let secondary = style::secondary_text(cx);
        let hover = style::sidebar_hover(cx);
        Some(
            crate::highlights::tipped(div().id("header-more").w(px(26.)).h(px(26.)).flex().items_center().justify_center().rounded(px(6.)).hover(move |s| s.bg(hover)), "More")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    let at = ev.position();
                    this.open_project_menu(&id, point(at.x, at.y + px(14.)), window, cx);
                }))
                .child(icons::symbol("ellipsis").with_size(px(style::size::LABEL)).text_color(secondary))
                .into_any_element(),
        )
    }
}

/// `PageHeading`: a section's title, a detail beside it and what acts on the
/// section at the right, over a hairline.
pub(crate) fn page_heading(title: &str, detail: Option<String>, trailing: Option<AnyElement>, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .pb(px(8.))
        .child(
            h_flex()
                .w_full()
                .items_baseline()
                .gap(px(8.))
                .pb(px(6.))
                .border_b(gpui_kit::px(0.5))
                .border_color(style::hairline(cx))
                .child(div().flex_none().text_size(px(style::size::LABEL)).line_height(px(16.)).font_semibold().text_color(style::secondary_text(cx)).child(title.to_owned()))
                .when_some(detail, |s, d| s.child(div().flex_none().text_size(px(style::size::SMALL)).line_height(px(14.)).text_color(style::faint_text(cx)).child(d)))
                .child(div().flex_1())
                .children(trailing),
        )
        .into_any_element()
}

/// A borderless button in words (SwiftUI `.borderless` at 12 pt): the
/// secondary colour, as AppKit draws one (#999 on the dark page), faded
/// unless the section's under the pointer.
fn quiet_button(id: impl Into<SharedString>, label: &str, tip: &str, lit: bool, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    let color = style::secondary_text(cx);
    crate::highlights::tipped(div().id(ElementId::Name(id.into())).flex_none().cursor_pointer().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(color).opacity(if lit { 1. } else { 0.6 }).hover(|s| s.opacity(1.)), tip.to_owned())
        .on_click(on_click)
        .child(label.to_owned())
        .into_any_element()
}

/// A borderless symbol button (the board's refresh and open, a folder's minus).
fn icon_button(id: impl Into<SharedString>, symbol: &str, tip: &str, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    let color = style::secondary_text(cx);
    let fg = cx.theme().foreground;
    crate::highlights::tipped(div().id(ElementId::Name(id.into())).flex_none().size(px(18.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().text_color(color).hover(move |s| s.text_color(fg)), tip.to_owned())
        .on_click(on_click)
        .child(icons::symbol(symbol).with_size(px(12.)))
        .into_any_element()
}

/// A small push button (`.controlSize(.small)`), for Save, Cancel and a
/// decision's Open thread.
pub(crate) fn small_push_button(id: impl Into<ElementId>, label: &'static str, tip: impl Into<SharedString>, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    let color = style::label_text(cx);
    let ink = if cx.theme().is_dark() { 1. } else { 0. };
    crate::highlights::tipped(
        div().id(id).flex_none().h(px(19.)).px(px(9.)).rounded(px(4.)).flex().items_center().text_size(px(11.)).line_height(px(14.)).text_color(color).bg(hsla(0., 0., ink, 0.08)).active(move |s| s.bg(hsla(0., 0., ink, 0.16))).on_click(on_click).child(label),
        tip,
    )
    .into_any_element()
}

/// A large push button with a symbol (`.controlSize(.large)`), as New Thread
/// is: a capsule 28 tall.
fn large_push_button(id: &'static str, symbol: &'static str, label: &'static str, tip: &'static str, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    let color = style::label_text(cx);
    let ink = if cx.theme().is_dark() { 1. } else { 0. };
    crate::highlights::tipped(
        div()
            .id(id)
            .flex_none()
            .h(px(28.))
            .px(px(14.))
            .rounded_full()
            .flex()
            .items_center()
            .gap(px(7.))
            .text_size(px(13.))
            .line_height(px(16.))
            .whitespace_nowrap()
            .text_color(color)
            .bg(hsla(0., 0., ink, 0.08))
            .active(move |s| s.bg(hsla(0., 0., ink, 0.16)))
            .on_click(on_click)
            .child(icons::symbol(symbol).with_size(px(13.)).text_color(color))
            .child(label),
        tip,
    )
    .into_any_element()
}

/// Notion's colour names (`TaskColors`), for a column's dot.
fn task_color(column: &TaskColumn, cx: &App) -> Hsla {
    let rgb = |r: f32, g: f32, b: f32| Hsla::from(Rgba { r, g, b, a: 1. });
    match column.color.as_deref() {
        Some("blue") => rgb(0.04, 0.52, 1.),
        Some("green") => rgb(0.2, 0.78, 0.35),
        Some("orange") => rgb(1., 0.62, 0.04),
        Some("yellow") => rgb(1., 0.84, 0.04),
        Some("red") => rgb(1., 0.27, 0.23),
        Some("purple") => rgb(0.75, 0.35, 0.95),
        Some("pink") => rgb(1., 0.22, 0.37),
        Some("brown") => rgb(0.67, 0.56, 0.41),
        _ => style::faint_text(cx),
    }
}

/// The Swift page reads only bold, code and links in these, with every line
/// kept as it was (`inlineOnlyPreservingWhitespace`): a heading or a list is
/// its plain line, and an empty line is an empty line. So it goes to the
/// markdown view as one paragraph of hard-broken lines, an empty one holding a
/// no-break space, and anything at a line's start that would make a block of
/// its own escaped.
fn keep_breaks(text: &str) -> String {
    text.lines().map(|line| if line.trim().is_empty() { "\u{a0}".to_owned() } else { escape_block_start(line) }).collect::<Vec<_>>().join("  \n")
}

/// `line` with a backslash before what would start a block: a heading, a
/// quote, a fence, a table or HTML, a list item ("- ", "1. "), or a rule. The
/// marker alone, so "*bold* text" and "`code`" stay what they are.
fn escape_block_start(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && matches!(trimmed[digits..].chars().next(), Some('.') | Some(')')) {
        return format!("{indent}{}\\{}", &trimmed[..digits], &trimmed[digits..]);
    }
    let Some(first) = trimmed.chars().next() else { return line.to_owned() };
    let rest = &trimmed[first.len_utf8()..];
    let block = matches!(first, '#' | '>' | '<' | '|')
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || (matches!(first, '-' | '+' | '*' | '=') && (rest.is_empty() || rest.starts_with(' ') || rest.chars().all(|c| c == first || c == ' ')));
    if block {
        format!("{indent}\\{trimmed}")
    } else {
        line.to_owned()
    }
}

/// The page's text at 13 pt, in the reading column's colours.
fn page_text_style(cx: &App) -> gpui_kit::base::TextViewStyle {
    crate::rows::reading_style(cx)
}
