// The sidebar's Projects view, a port of the Swift app's ProjectsList: a
// heading per project, newest activity first, with its open threads under it
// (a thread in two projects is under both) and its archived ones folded away a
// few at a time, its parts nested under it; then the threads in no project,
// and the archived projects folded away at the foot. A heading opens the
// project's page. With the projectDecisions experiment each project is one
// row instead, folded until its chevron opens it, saying how many decisions
// wait in it (`ProjectFoldRow`).
//
// A thread's row is the inbox's own (sidebar.rs), leaving out the project
// whose heading it's under, as the Swift row's `heading` does.
use crate::app::Threadbear;
use crate::home_page::mode_header;
use crate::projects::project_icon;
use crate::sidebar::{icon, palette, tipped, Palette, CORNER, LEADING, OUTER_INSET, TRAILING};
use crate::{now_ms, style};
// `tipped!` (sidebar.rs) needs TestSupportExt where it's used.
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::projects::{self as rules, Project, Section, Sections, UNFILED_ID};
use threadbear_core::sidebar::RowLook;

/// How far a part sits in under its parent, with its threads.
const PART_INDENT: f32 = 14.;
/// Archived threads shown at a time.
const ARCHIVED_STEP: usize = 5;

/// One line of the list, worked out before anything's drawn: the sections
/// borrow the app's lists, and drawing a row needs the app.
enum Item {
    Heading { project: Project, count: usize, part: bool },
    Fold { id: String, title: String, project: Option<Project>, count: usize, decisions: usize, part: bool },
    Unfiled,
    /// `group` keeps a thread in two projects two rows.
    Row { thread: ThreadSummary, section: &'static str, group: String, part: bool },
    ArchivedFold { project: String, count: usize, open: bool, part: bool },
    ShowMore { project: String, more: usize, part: bool },
    Empty,
}

/// The thread as its row under `heading` shows it: that project left out of
/// its tile and its place line, as the heading says it already.
fn under(t: &ThreadSummary, heading: &str) -> ThreadSummary {
    let mut t = t.clone();
    if let Some(projects) = t.projects.as_mut() {
        projects.retain(|m| m.id != heading);
        if projects.is_empty() {
            t.projects = None;
        }
    }
    t
}

impl Threadbear {
    /// What the list draws, top to bottom.
    fn projects_list_items(&self, folding: bool) -> Vec<Item> {
        let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
        let open = &self.projects.open_rows;
        let shows = |id: &str| !folding || open.contains(id);
        let mut items = Vec::new();
        for top in sections.top_level() {
            // A parent's row counts its parts' threads and decisions too.
            let all = sections.threads_with_parts(top);
            if folding {
                items.push(Item::Fold { id: top.id().to_owned(), title: top.project.title.clone(), project: Some(top.project.clone()), count: all.len(), decisions: rules::decision_count(&all), part: false });
            } else {
                items.push(Item::Heading { project: top.project.clone(), count: top.threads.len(), part: false });
            }
            if !shows(top.id()) {
                continue;
            }
            self.push_section_rows(&mut items, top, false);
            // Its parts nest under it, indented, folding with it.
            for part in sections.parts_of(top.id()) {
                if folding {
                    items.push(Item::Fold { id: part.id().to_owned(), title: part.project.title.clone(), project: Some(part.project.clone()), count: part.threads.len(), decisions: rules::decision_count(&part.threads), part: true });
                } else {
                    items.push(Item::Heading { project: part.project.clone(), count: part.threads.len(), part: true });
                }
                if shows(part.id()) {
                    self.push_section_rows(&mut items, part, true);
                }
            }
        }
        if !sections.unfiled.is_empty() {
            if folding {
                items.push(Item::Fold { id: UNFILED_ID.to_owned(), title: "Not in a project".to_owned(), project: None, count: sections.unfiled.len(), decisions: rules::decision_count(&sections.unfiled), part: false });
            } else {
                items.push(Item::Unfiled);
            }
            if shows(UNFILED_ID) {
                items.extend(sections.unfiled.iter().map(|t| Item::Row { thread: (*t).clone(), section: "inbox", group: UNFILED_ID.to_owned(), part: false }));
            }
        }
        if sections.sections.is_empty() && sections.unfiled.is_empty() {
            items.push(Item::Empty);
        }
        items
    }

    /// A project's open threads, then its archived ones folded away.
    fn push_section_rows(&self, items: &mut Vec<Item>, section: &Section, part: bool) {
        let id = section.id();
        items.extend(section.threads.iter().map(|t| Item::Row { thread: under(t, id), section: "inbox", group: id.to_owned(), part }));
        if section.archived.is_empty() {
            return;
        }
        let shown = self.projects.archived_shown.get(id).copied().unwrap_or(0);
        items.push(Item::ArchivedFold { project: id.to_owned(), count: section.archived.len(), open: shown > 0, part });
        // Their own group: a row is a section's and a thread's.
        items.extend(section.archived.iter().take(shown).map(|t| Item::Row { thread: under(t, id), section: "archived", group: format!("{id}/archived"), part }));
        if shown > 0 && section.archived.len() > shown {
            items.push(Item::ShowMore { project: id.to_owned(), more: ARCHIVED_STEP.min(section.archived.len() - shown), part });
        }
    }

    /// `ProjectsList`: the view's name and its count, New project, then the
    /// list.
    pub(crate) fn render_projects_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let now = now_ms();
        let clock = threadbear_core::sidebar::local_now();
        let folding = self.project_decisions_on();
        let top_count = Sections::build(&self.projects.list, self.threads.values(), now).top_level().len();
        let mut list = v_flex().px(px(OUTER_INSET)).pb(px(32.));
        if self.sidebar.loaded {
            for (n, item) in self.projects_list_items(folding).into_iter().enumerate() {
                let el = match item {
                    Item::Heading { project, count, part } => self.render_project_heading(&project, count, part, &p, cx),
                    Item::Fold { id, title, project, count, decisions, part } => self.render_project_fold(&id, &title, project.as_ref(), count, decisions, part, &p, cx),
                    Item::Unfiled => self.section_heading("projects-unfiled", "Not in a project", "square.dashed", 0, false, false, None, &p, |_, _| {}, cx),
                    Item::Row { thread, section, group, part } => {
                        let row = self.render_row(&thread, section, None, now, &clock, &p, cx);
                        // Its own id per group, so the same thread under two
                        // projects is two rows to the window.
                        div().id(ElementId::Name(format!("projects-row-{group}-{n}").into())).when(part, |d| d.pl(px(PART_INDENT))).child(row).into_any_element()
                    }
                    Item::ArchivedFold { project, count, open, part } => self.render_archived_fold(&project, count, open, part, &p, cx),
                    Item::ShowMore { project, more, part } => self.render_archived_more(&project, more, part, &p, cx),
                    Item::Empty => div().pl(px(LEADING)).pt(px(10.)).text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(style::natural_line(style::size::SIDEBAR_PREVIEW))).text_color(p.secondary).child("No projects yet.").into_any_element(),
                };
                list = list.child(el);
            }
            let archived: Vec<Project> = self.projects.list.iter().filter(|p| p.archived).cloned().collect();
            if !archived.is_empty() {
                list = list.child(self.render_archived_projects(&archived, &p, cx));
            }
        }
        let side = p.side;
        v_flex()
            .relative()
            .size_full()
            .bg(p.side)
            .child(mode_header("Projects", Some(top_count), &p))
            .child(div().px(px(OUTER_INSET)).child(self.render_new_project_row(&p, cx)))
            .child(
                // As the inbox's list: no scrollbar, fading out at the foot.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(div().id("projects-list").size_full().overflow_y_scroll().track_scroll(&self.projects.list_scroll).child(list))
                    .child(div().absolute().left_0().right_0().bottom_0().h(px(34.)).bg(linear_gradient(180., linear_color_stop(side.opacity(0.), 0.), linear_color_stop(side, 0.88)))),
            )
            .into_any_element()
    }

    /// `SidebarButtonRow` for New project: a spinner for its plus, and no
    /// more clicks, until the runner has made it.
    fn render_new_project_row(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.projects.busy.contains("new");
        let (hover, secondary, fg) = (p.hover, p.secondary, p.fg);
        let mark: AnyElement = if busy {
            Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(secondary).with_size(px(12.)).into_any_element()
        } else {
            icon("plus", style::size::SIDEBAR_BUTTON, secondary).into_any_element()
        };
        tipped!(h_flex().gap(px(8.)).pl(px(LEADING - 2.)).pr(px(TRAILING)).py(px(7.)).rounded(px(CORNER)).when(!busy, |s| s.hover(move |s| s.bg(hover))), "new-project-row", "Make a new project")
            .when(!busy, |s| s.on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.new_project(None, window, cx))))
            .child(div().w(px(16.)).flex().justify_center().child(mark))
            .child(div().text_size(px(style::size::SIDEBAR_BUTTON)).line_height(px(17.)).text_color(fg).child("New project"))
            .into_any_element()
    }

    /// Notes the pointer over one of the view's own controls.
    fn hover_listener(&self, key: String, cx: &mut Context<Self>) -> Box<dyn Fn(&bool, &mut Window, &mut App) + 'static> {
        Box::new(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.projects.hovered = Some(key.clone());
            } else if this.projects.hovered.as_deref() == Some(key.as_str()) {
                this.projects.hovered = None;
            }
            cx.notify();
        }))
    }

    /// `ProjectHeader`: its icon and title, quiet like the inbox's headings
    /// but a button, highlighted while its page is open; the count of its
    /// threads at the right, or a chevron under the pointer.
    fn render_project_heading(&mut self, project: &Project, count: usize, part: bool, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let id = project.id.clone();
        let key = format!("heading:{id}");
        let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
        let open = self.projects.open.as_deref() == Some(id.as_str());
        let title_color = if open || hovering { p.fg } else { p.secondary };
        let trailing: Option<AnyElement> = if hovering {
            Some(icon("chevron.right", 10., p.faint).into_any_element())
        } else if count > 0 {
            Some(div().text_size(px(style::size::SIDEBAR_META)).line_height(px(14.)).text_color(p.faint).child(count.to_string()).into_any_element())
        } else {
            None
        };
        let (open_id, menu_id) = (id.clone(), id.clone());
        let heading = tipped!(
            h_flex().w_full().gap(px(6.)).pl(px(LEADING)).pr(px(TRAILING)).py(px(6.)).rounded(px(CORNER)).when(open, |s| s.bg(p.selected)).when(!open && hovering, |s| s.bg(p.hover)),
            ElementId::Name(format!("project-heading-{id}").into()),
            format!("Open {}", project.title)
        )
        .on_hover(self.hover_listener(key, cx))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_project_page(&open_id, cx)))
        .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| this.open_project_menu(&menu_id, ev.position, window, cx)))
        .child(project_icon(Some(project), "", 15., cx))
        .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::SIDEBAR_HEADING)).line_height(px(16.)).font_semibold().text_color(title_color).child(project.title.clone()))
        .children(trailing);
        // The same room above each group as the inbox's headings.
        div().pt(px(12.)).pb(px(2.)).when(part, |d| d.pl(px(PART_INDENT))).child(heading).into_any_element()
    }

    /// `ProjectFoldRow` (the projectDecisions experiment): the chevron shows or
    /// hides its threads under it, the rest of the row opens its page. On the
    /// right, its decisions in the needs-you orange when it has any, else a
    /// quiet count of its threads. Not in a project folds the same way, and
    /// having no page, its whole row folds.
    #[allow(clippy::too_many_arguments)]
    fn render_project_fold(&mut self, id: &str, title: &str, project: Option<&Project>, count: usize, decisions: usize, part: bool, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.projects.open_rows.contains(id);
        let key = format!("fold:{id}");
        let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
        let is_page = project.is_some_and(|pr| self.projects.open.as_deref() == Some(pr.id.as_str()));
        let chevron_color = if hovering { p.secondary } else { p.faint };
        let toggle_id = id.to_owned();
        // 20 by 22 with 6 over and under, as the Swift chevron's frame, which
        // makes the row 34 tall.
        let chevron = tipped!(div().flex_none().w(px(20.)).py(px(6.)), ElementId::Name(format!("fold-chevron-{id}").into()), if open { "Hide its threads" } else { "Show its threads" })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_project_row(&toggle_id, cx)))
            .child(div().h(px(22.)).flex().items_center().justify_center().child(icon(if open { "chevron.down" } else { "chevron.right" }, 9., chevron_color)));
        let mark: AnyElement = match project {
            Some(pr) => project_icon(Some(pr), "", 16., cx),
            None => div().size(px(16.)).flex().items_center().justify_center().child(icon("square.dashed", 12., p.secondary)).into_any_element(),
        };
        let trailing: Option<AnyElement> = if decisions > 0 {
            Some(div().flex_none().text_size(px(style::size::SIDEBAR_META)).line_height(px(14.)).font_medium().text_color(style::needs_you(cx)).child(rules::decision_label(decisions)).into_any_element())
        } else if count > 0 {
            Some(div().flex_none().text_size(px(style::size::SIDEBAR_META)).line_height(px(14.)).text_color(p.faint).child(count.to_string()).into_any_element())
        } else {
            None
        };
        let tip = match project {
            None if open => "Hide the threads in no project".to_owned(),
            None => "Show the threads in no project".to_owned(),
            Some(pr) if decisions > 0 => format!("Open {} to answer its {}", pr.title, rules::decision_label(decisions)),
            Some(pr) => format!("Open {}", pr.title),
        };
        let (click_id, page_id) = (id.to_owned(), project.map(|pr| pr.id.clone()));
        let main = tipped!(h_flex().flex_1().min_w_0().gap(px(7.)).pr(px(TRAILING)).py(px(6.)), ElementId::Name(format!("fold-row-{id}").into()), tip)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| match &page_id {
                Some(page) => this.open_project_page(page, cx),
                None => this.toggle_project_row(&click_id, cx),
            }))
            .child(mark)
            .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::SIDEBAR_TITLE)).line_height(px(17.)).font_semibold().text_color(p.fg).child(title.to_owned()))
            .children(trailing);
        let menu_id = project.map(|pr| pr.id.clone());
        let row = h_flex()
            .id(ElementId::Name(format!("fold-{id}").into()))
            .w_full()
            .items_stretch()
            .rounded(px(CORNER))
            .when(is_page, |s| s.bg(p.selected))
            .when(!is_page && hovering, |s| s.bg(p.hover))
            .on_hover(self.hover_listener(key, cx))
            .when_some(menu_id, |s, menu_id| s.on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| this.open_project_menu(&menu_id, ev.position, window, cx))))
            .child(chevron)
            .child(main);
        div().pt(px(2.)).when(part, |d| d.pl(px(PART_INDENT))).child(row).into_any_element()
    }

    /// Opens or folds a project's row, remembered between launches.
    pub(crate) fn toggle_project_row(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.projects.open_rows.remove(id) {
            self.projects.open_rows.insert(id.to_owned());
        }
        let mut rows: Vec<String> = self.projects.open_rows.iter().cloned().collect();
        rows.sort();
        self.settings.saved.projects_open_rows = Some(rows);
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// `ArchivedFold`: under a project's open threads, how many it has
    /// archived, a click showing the newest few.
    fn render_archived_fold(&mut self, project: &str, count: usize, open: bool, part: bool, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let key = format!("archived:{project}");
        let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
        let color = if hovering { p.secondary } else { p.faint };
        let id = project.to_owned();
        tipped!(h_flex().gap(px(5.)).pl(px(LEADING)).pr(px(TRAILING)).py(px(4.)).when(part, |s| s.ml(px(PART_INDENT))), ElementId::Name(format!("archived-fold-{project}").into()), if open { "Hide archived threads" } else { "Show archived threads" })
            .on_hover(self.hover_listener(key, cx))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if this.projects.archived_shown.remove(&id).is_none() {
                    this.projects.archived_shown.insert(id.clone(), ARCHIVED_STEP);
                }
                cx.notify();
            }))
            .text_size(px(style::size::SIDEBAR_META))
            .line_height(px(14.))
            .text_color(color)
            .child(format!("{count} archived"))
            .child(icon(if open { "chevron.down" } else { "chevron.right" }, 8., color))
            .into_any_element()
    }

    /// `ShowMoreRow`: after the archived threads shown so far.
    fn render_archived_more(&mut self, project: &str, more: usize, part: bool, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let key = format!("more:{project}");
        let hovering = self.projects.hovered.as_deref() == Some(key.as_str());
        let id = project.to_owned();
        tipped!(div().pl(px(LEADING)).py(px(4.)).when(part, |s| s.ml(px(PART_INDENT))), ElementId::Name(format!("archived-more-{project}").into()), "Show more archived threads")
            .on_hover(self.hover_listener(key, cx))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                *this.projects.archived_shown.entry(id.clone()).or_insert(0) += ARCHIVED_STEP;
                cx.notify();
            }))
            .text_size(px(style::size::SIDEBAR_META))
            .line_height(px(14.))
            .text_color(if hovering { p.secondary } else { p.faint })
            .child(format!("Show {more} more"))
            .into_any_element()
    }

    /// `ArchivedProjects`: folded away at the foot; each opens its page.
    fn render_archived_projects(&mut self, projects: &[Project], p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.projects.archived_open;
        let secondary = p.secondary;
        let fold = tipped!(h_flex().gap(px(5.)).pl(px(LEADING)).pr(px(TRAILING)).pt(px(18.)).pb(px(6.)), "archived-projects", if open { "Hide archived projects" } else { "Show archived projects" })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.projects.archived_open = !this.projects.archived_open;
                cx.notify();
            }))
            .text_size(px(style::size::SIDEBAR_HEADING))
            .line_height(px(16.))
            .font_semibold()
            .text_color(secondary)
            .child("Archived projects")
            .child(icon(if open { "chevron.down" } else { "chevron.right" }, 9., secondary))
            .child(div().flex_1())
            .child(projects.len().to_string());
        let mut column = v_flex().child(fold);
        if open {
            for project in projects {
                let active = self.projects.open.as_deref() == Some(project.id.as_str());
                let (hover, selected, fg) = (p.hover, p.selected, p.fg);
                let id = project.id.clone();
                column = column.child(
                    tipped!(h_flex().gap(px(8.)).pl(px(LEADING - 2.)).pr(px(TRAILING)).py(px(7.)).rounded(px(CORNER)).when(active, |s| s.bg(selected)).when(!active, |s| s.hover(move |s| s.bg(hover))), ElementId::Name(format!("archived-project-{id}").into()), format!("Open {}", project.title))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_project_page(&id, cx)))
                        .child(div().w(px(16.)).flex().justify_center().child(icon("archivebox", style::size::SIDEBAR_BUTTON, secondary)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::SIDEBAR_BUTTON)).line_height(px(17.)).text_color(fg).child(project.title.clone())),
                );
            }
        }
        column.into_any_element()
    }

    /// The Projects view in the dump, as the Swift app's has it: a section
    /// per project (parts too) in the sidebar's order, then Not in a project.
    /// With the projectDecisions experiment a folded one lists no threads and
    /// says how many it has.
    #[cfg(feature = "automation")]
    pub(crate) fn projects_sidebar_dump(&self) -> Value {
        let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
        let folding = self.project_decisions_on();
        let thread = |t: &ThreadSummary| {
            let look = RowLook::of(t, None);
            json!({
                "id": t.id, "title": t.title, "status": t.status, "needsYou": t.needs_you, "unread": look.unread,
                "hovered": self.sidebar.hover_override.as_deref() == Some(t.id.as_str()), "place": t.place(), "bold": look.emphasised,
            })
        };
        let section = |id: &str, name: &str, threads: &[&ThreadSummary]| {
            let folded = folding && !self.projects.open_rows.contains(id);
            json!({
                "name": name,
                "threads": if folded { vec![] } else { threads.iter().map(|t| thread(t)).collect::<Vec<_>>() },
                "collapsed": folding.then_some(folded),
                "count": folding.then_some(threads.len()),
            })
        };
        let mut out: Vec<Value> = sections.sections.iter().map(|s| section(s.id(), &s.project.title, &s.threads)).collect();
        out.push(section(UNFILED_ID, "Not in a project", &sections.unfiled));
        Value::Array(out)
    }
}

/// `expand` and `collapse` with `projectRow`: a project's folded row (the
/// projectDecisions experiment), by title or id, as its chevron does; "-" or
/// "Not in a project" for the threads in none.
#[cfg(feature = "automation")]
pub(crate) fn fold_automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    let key = cmd["projectRow"].as_str().unwrap_or("");
    let id = if key == "Not in a project" || key == UNFILED_ID {
        UNFILED_ID.to_owned()
    } else {
        app.read(cx).projects.list.iter().find(|p| p.id == key || p.title == key).map(|p| p.id.clone()).ok_or_else(|| format!("no project {key}"))?
    };
    let open = cmd["cmd"] == "expand";
    app.update(cx, |t, cx| {
        if open != t.projects.open_rows.contains(&id) {
            t.toggle_project_row(&id, cx);
        }
    });
    Ok(json!("ok"))
}
