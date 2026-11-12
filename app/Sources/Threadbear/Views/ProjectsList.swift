import AppKit
import SwiftUI
import ThreadbearCore

/// The sidebar's Projects view: a section per project, newest activity
/// first, with its open threads under it (a thread in two projects is
/// under both), then the threads in no project. A project's header opens
/// its page. A project's archived threads are folded away under its open
/// ones, a few at a time.
struct ProjectsList: View {
    @Environment(AppModel.self) private var app
    @State private var showArchived = false
    /// How many archived threads each unfolded project shows.
    @State private var archivedShown: [String: Int] = [:]
    /// Kept built while another view's up (SidebarView), and then drawn
    /// from the sections as they were when it was hidden, so a thread
    /// changing in the inbox doesn't build them again for a list nobody
    /// sees. Shown again, it reads them afresh.
    var shown = true
    @State private var frozen: ProjectSections?

    var body: some View {
        let built = !shown ? frozen ?? app.projectSections : app.projectSections
        let archived = app.projects.filter(\.archived)
        // With the projectDecisions experiment on, each project is one row,
        // folded until its chevron opens it (ProjectFolds); otherwise every
        // project's threads are listed under its heading, as before.
        let folding = app.projectDecisionsOn
        let open = app.openProjectRows
        let shows = { (id: String) in !folding || open.contains(id) }
        // Each thread's first row that's drawn, for the arrow keys to scroll
        // to: one in a folded project isn't there to scroll to (Codex's review).
        var firstRow: [String: String] = [:]
        // As drawn: each project, then its parts under it, a part only while
        // its parent is shown open.
        let drawn = built.topLevel.flatMap { top in [top] + (shows(top.id) ? built.parts[top.id] ?? [] : []) }
        for section in drawn where shows(section.id) {
            for thread in section.threads where firstRow[thread.id] == nil {
                firstRow[thread.id] = SectionRow.id(section: section.id, thread: thread.id)
            }
        }
        for thread in built.unfiled where shows(ProjectFolds.unfiledId) && firstRow[thread.id] == nil {
            firstRow[thread.id] = SectionRow.id(section: Self.unfiledId, thread: thread.id)
        }
        return VStack(spacing: 0) {
            ModeHeader(title: "Projects", count: built.topLevel.count)
            SidebarButtonRow(icon: "plus", title: "New project", tip: "Make a new project", busy: app.isBusy(BusyKey.newProject)) {
                app.newProject(with: nil)
            }
            .padding(.horizontal, SidebarMetrics.outerInset)
            ThreadList(order: folding ? ProjectFolds.order(built, open: open) : built.order,
                       layout: folding ? ProjectFolds.layout(built, open: open) : nil,
                       scrollID: { firstRow[$0] ?? $0 }) { select in
                if app.threadsLoaded {
                    ForEach(built.topLevel) { section in
                        // A parent's row counts its parts' threads and
                        // decisions too (docs/protocol.md, Projects, "Parts").
                        let all = built.threadsWithParts(section)
                        if folding {
                            ProjectFoldRow(id: section.id, title: section.project.title, project: section.project,
                                           count: all.count, decisions: ProjectDecisions.count(all))
                        } else {
                            ProjectHeader(project: section.project, count: section.threads.count)
                        }
                        if shows(section.id) {
                            sectionThreads(section, select: select)
                            // Its parts nest under it, indented, folding with it.
                            ForEach(built.parts[section.id] ?? []) { part in
                                if folding {
                                    ProjectFoldRow(id: part.id, title: part.project.title, project: part.project,
                                                   count: part.threads.count, decisions: ProjectDecisions.count(part.threads), isPart: true)
                                } else {
                                    ProjectHeader(project: part.project, count: part.threads.count, isPart: true)
                                }
                                if shows(part.id) {
                                    sectionThreads(part, select: select)
                                        .padding(.leading, Self.partIndent)
                                }
                            }
                        }
                    }
                    if !built.unfiled.isEmpty {
                        if folding {
                            ProjectFoldRow(id: ProjectFolds.unfiledId, title: "Not in a project", project: nil,
                                           count: built.unfiled.count, decisions: ProjectDecisions.count(built.unfiled))
                        } else {
                            SectionHeading(title: "Not in a project", icon: "square.dashed")
                        }
                        if !folding || open.contains(ProjectFolds.unfiledId) {
                            ForEach(built.unfiled.map { SectionRow(section: Self.unfiledId, thread: $0) }) { row in
                                ThreadRowView(thread: row.thread, mode: .projects) { select(row.thread.id) }
                            }
                        }
                    }
                    if built.sections.isEmpty && built.unfiled.isEmpty {
                        EmptyNote(text: "No projects yet.")
                    }
                    if !archived.isEmpty {
                        ArchivedProjects(projects: archived, shown: $showArchived)
                    }
                }
            }
        }
        .onChange(of: shown) { _, now in frozen = now ? nil : app.projectSections }
    }

    /// A project's open threads, then its archived ones folded away.
    @ViewBuilder
    private func sectionThreads(_ section: ProjectSection, select: @escaping (String) -> Void) -> some View {
        ForEach(section.threads.map { SectionRow(section: section.id, thread: $0) }) { row in
            ThreadRowView(thread: row.thread, mode: .projects, heading: section.project.id) { select(row.thread.id) }
        }
        if !section.archived.isEmpty {
            let shown = archivedShown[section.id] ?? 0
            ArchivedFold(count: section.archived.count, open: shown > 0) {
                archivedShown[section.id] = shown > 0 ? nil : Self.archivedStep
            }
            // Their own section ids: a row id is a section's and a thread's.
            ForEach(section.archived.prefix(shown).map { SectionRow(section: section.id + Self.archivedSuffix, thread: $0) }) { row in
                ThreadRowView(thread: row.thread, mode: .archive, heading: section.project.id) { select(row.thread.id) }
            }
            if shown > 0 && section.archived.count > shown {
                ShowMoreRow(more: min(Self.archivedStep, section.archived.count - shown)) {
                    archivedShown[section.id] = shown + Self.archivedStep
                }
            }
        }
    }

    /// How far a part sits in under its parent, with its threads.
    static var partIndent: CGFloat { zoomed(14) }
    static let unfiledId = "-"
    static let archivedSuffix = "/archived"
    /// Archived threads shown at a time.
    static let archivedStep = 5
}

/// Under a project's open threads: how many it has archived, a click
/// showing the newest of them.
private struct ArchivedFold: View {
    let count: Int
    let open: Bool
    let toggle: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: toggle) {
            HStack(spacing: zoomed(5)) {
                Text("\(count) archived").monospacedDigit()
                Image(systemName: open ? "chevron.down" : "chevron.right")
                    .font(.system(size: zoomed(8), weight: .semibold))
                Spacer(minLength: 0)
            }
            .font(.system(size: TypeScale.sidebarMeta))
            .foregroundStyle(hovering ? AnyShapeStyle(.secondary) : AnyShapeStyle(SidebarMetrics.faint))
            .padding(.leading, SidebarMetrics.leading)
            .padding(.trailing, SidebarMetrics.trailing)
            .padding(.vertical, zoomed(4))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .tip(open ? "Hide archived threads" : "Show archived threads")
    }
}

/// After the archived threads shown so far, when there are more.
private struct ShowMoreRow: View {
    let more: Int
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Text("Show \(more) more")
                .font(.system(size: TypeScale.sidebarMeta))
                .foregroundStyle(hovering ? AnyShapeStyle(.secondary) : AnyShapeStyle(SidebarMetrics.faint))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.leading, SidebarMetrics.leading)
                .padding(.vertical, zoomed(4))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .tip("Show more archived threads")
    }
}

/// A project's heading: its title, quiet like the inbox's headings but a
/// button, highlighted while its page is open.
private struct ProjectHeader: View {
    @Environment(AppModel.self) private var app
    @Environment(\.controlActiveState) private var activeState
    let project: Project
    /// Its open threads, listed under it.
    let count: Int
    /// A part, drawn indented under its parent.
    var isPart = false
    @State private var hovering = false

    private var isOpen: Bool { app.openProjectId == project.id }

    var body: some View {
        Button { app.openProjectPage(project.id) } label: {
            HStack(spacing: zoomed(6)) {
                ProjectIconView(project: project, size: 15)
                .accessibilityHidden(true)
                Text(project.title)
                    .font(.system(size: TypeScale.sidebarHeading, weight: .semibold))
                    .foregroundStyle(isOpen || hovering ? .primary : .secondary)
                    .lineLimit(1)
                Spacer(minLength: zoomed(6))
                if hovering {
                    Image(systemName: "chevron.right")
                        .font(.system(size: zoomed(10), weight: .semibold))
                        .foregroundStyle(SidebarMetrics.faint)
                } else if count > 0 {
                    Text("\(count)")
                        .font(.system(size: TypeScale.sidebarMeta))
                        .foregroundStyle(SidebarMetrics.faint)
                        .monospacedDigit()
                }
            }
            .padding(.leading, SidebarMetrics.leading)
            .padding(.trailing, SidebarMetrics.trailing)
            .padding(.vertical, zoomed(6))
            .background(background, in: RoundedRectangle(cornerRadius: SidebarMetrics.corner))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .padding(.leading, isPart ? ProjectsList.partIndent : 0)
        // The same room above each group as the inbox's headings.
        .padding(.top, zoomed(12))
        .padding(.bottom, zoomed(2))
        .tip("Open \(project.title)")
        .iconContextMenu { ProjectMenu(project: project) }
        .accessibilityAddTraits(.isHeader)
    }

    private var background: Color {
        if isOpen { return Color.primary.opacity(activeState == .key ? 0.08 : 0.06) }
        return hovering ? SidebarMetrics.hover : .clear
    }
}

/// A project as one row in the folded Projects view (the projectDecisions
/// experiment): the chevron shows or hides its threads under it, the rest of
/// the row opens its page. On the right, its decisions in the needs-you
/// orange when it has any, as they're what its page opens on, else a quiet
/// count of its threads. Not in a project folds the same way, and having no
/// page, its row folds too.
private struct ProjectFoldRow: View {
    @Environment(AppModel.self) private var app
    @Environment(\.controlActiveState) private var activeState
    /// The project's id, or ProjectFolds.unfiledId.
    let id: String
    let title: String
    /// Nil for Not in a project.
    let project: Project?
    let count: Int
    let decisions: Int
    /// A part, drawn indented under its parent's row.
    var isPart = false
    @State private var hovering = false

    private var open: Bool { app.openProjectRows.contains(id) }
    private var isPage: Bool { project.map { app.openProjectId == $0.id } ?? false }

    var body: some View {
        HStack(spacing: 0) {
            Button(action: toggle) {
                Image(systemName: "chevron.right")
                    .font(.system(size: zoomed(9), weight: .bold))
                    .foregroundStyle(hovering ? AnyShapeStyle(.secondary) : AnyShapeStyle(SidebarMetrics.faint))
                    .rotationEffect(.degrees(open ? 90 : 0))
                    .frame(width: zoomed(20), height: zoomed(22))
                    .padding(.vertical, zoomed(6))
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .tip(open ? "Hide its threads" : "Show its threads")
            .accessibilityLabel(open ? "Hide \(title)'s threads" : "Show \(title)'s threads")
            Button {
                if let project { app.openProjectPage(project.id) } else { toggle() }
            } label: {
                HStack(spacing: zoomed(7)) {
                    Group {
                        if let project {
                            ProjectIconView(project: project, size: 16)
                        } else {
                            Image(systemName: "square.dashed")
                                .font(.system(size: zoomed(12)))
                                .foregroundStyle(.secondary)
                                .frame(width: zoomed(16), height: zoomed(16))
                        }
                    }
                    .accessibilityHidden(true)
                    Text(title)
                        .font(.system(size: TypeScale.sidebarTitle, weight: .semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Spacer(minLength: zoomed(6))
                    if decisions > 0 {
                        Text(ProjectDecisions.label(decisions))
                            .font(.system(size: TypeScale.sidebarMeta, weight: .medium))
                            .foregroundStyle(Color(nsColor: Theme.needsYou))
                    } else if count > 0 {
                        Text("\(count)")
                            .font(.system(size: TypeScale.sidebarMeta))
                            .foregroundStyle(SidebarMetrics.faint)
                    }
                }
                .monospacedDigit()
                .lineLimit(1)
                // The row's padding is inside the two buttons, so every
                // point of its highlight is one or the other (Codex's
                // review: a click near its edge did nothing).
                .padding(.trailing, SidebarMetrics.trailing)
                .padding(.vertical, zoomed(6))
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .tip(tip)
        }
        .fixedSize(horizontal: false, vertical: true)
        .background(background, in: RoundedRectangle(cornerRadius: SidebarMetrics.corner))
        .padding(.leading, isPart ? ProjectsList.partIndent : 0)
        .onHover { hovering = $0 }
        .padding(.top, zoomed(2))
        .iconContextMenu { if let project { ProjectMenu(project: project) } }
        .accessibilityAddTraits(.isHeader)
    }

    /// Shown or hidden straight away, its threads sliding in or out with the
    /// list's own row animation.
    private func toggle() {
        withAnimation(Juice.rows) { app.toggleProjectRow(id) }
    }

    private var tip: String {
        guard let project else { return open ? "Hide the threads in no project" : "Show the threads in no project" }
        return decisions > 0 ? "Open \(project.title) to answer its \(ProjectDecisions.label(decisions))" : "Open \(project.title)"
    }

    private var background: Color {
        if isPage { return Color.primary.opacity(activeState == .key ? 0.08 : 0.06) }
        return hovering ? SidebarMetrics.hover : .clear
    }
}

/// Archived projects, folded away at the foot; each opens its page.
private struct ArchivedProjects: View {
    @Environment(AppModel.self) private var app
    let projects: [Project]
    @Binding var shown: Bool

    var body: some View {
        Button { shown.toggle() } label: {
            HStack(spacing: zoomed(5)) {
                Text("Archived projects")
                Image(systemName: shown ? "chevron.down" : "chevron.right")
                    .font(.system(size: zoomed(9), weight: .semibold))
                Spacer(minLength: 0)
                Text("\(projects.count)").monospacedDigit()
            }
            .font(.system(size: TypeScale.sidebarHeading, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.leading, SidebarMetrics.leading)
            .padding(.trailing, SidebarMetrics.trailing)
            .padding(.top, zoomed(18))
            .padding(.bottom, zoomed(6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .tip(shown ? "Hide archived projects" : "Show archived projects")
        if shown {
            ForEach(projects) { project in
                SidebarButtonRow(icon: "archivebox", title: project.title, tip: "Open \(project.title)", active: app.openProjectId == project.id) {
                    app.openProjectPage(project.id)
                }
            }
        }
    }
}

/// Archive, merge and delete: the project header's context menu and the
/// toolbar's ... menu while its page is open.
struct ProjectMenu: View {
    @Environment(AppModel.self) private var app
    let project: Project

    var body: some View {
        if app.openProjectId != project.id {
            Button("Open", systemImage: "arrow.right.circle") { app.openProjectPage(project.id) }
            Divider()
        }
        if project.archived {
            Button("Unarchive", systemImage: "tray.and.arrow.up") { app.setProjectArchived(project.id, false) }
        } else {
            Button("Archive", systemImage: "archivebox") { app.setProjectArchived(project.id, true) }
        }
        Button(project.notion == nil ? "Add Notion Page…" : "Notion Page…", systemImage: "checklist") { app.linkProjectNotion(project.id) }
        let others = app.projects.filter { $0.id != project.id && !$0.archived }
        // Parts (docs/protocol.md, Projects, "Parts"), one level: only a
        // project with no parts of its own can be one, and only of a
        // project that isn't a part itself. Shown at once (updateProject).
        if project.parentId != nil {
            Button("Make Its Own Project", systemImage: "square.on.square") {
                app.updateProject(project.id, ProjectUpdate(parentId: ""))
            }
        } else if !app.projects.contains(where: { $0.parentId == project.id }) {
            let parents = others.filter { $0.parentId == nil }.sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
            if !parents.isEmpty {
                IconMenu("Make a Part of", systemImage: "square.stack") {
                    ForEach(parents) { other in
                        Button(other.title, systemImage: "folder") { app.updateProject(project.id, ProjectUpdate(parentId: other.id)) }
                    }
                }
            }
        }
        if !others.isEmpty {
            IconMenu("Merge into", systemImage: "arrow.triangle.merge") {
                ForEach(others) { other in
                    Button(other.title, systemImage: "square.stack") { app.mergeProject(project.id, into: other.id) }
                }
            }
        }
        Divider()
        Button("Copy project id", systemImage: "number") { app.copyToPasteboard(project.id) }
        Divider()
        Button("Delete…", systemImage: "trash") { app.deleteProject(project.id) }
    }
}
