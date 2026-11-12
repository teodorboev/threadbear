import Foundation

/// The sidebar's Projects view with the projectDecisions experiment on: one
/// row per project, folded by default, its threads under it only while it's
/// open (Mike, 11 Nov 2026: "one row per project which can be expanded and
/// contracted"). "Not in a project" folds the same way. Which are open is
/// kept by id across launches (AppModel.openProjectRows).
public enum ProjectFolds {
    /// "Not in a project"'s id among the open ones.
    public static let unfiledId = "-"

    /// Thread ids top to bottom as the folded list shows them, for the arrow
    /// keys: only open projects' threads, each thread once.
    /// A part's threads show when it and its parent are both open.
    public static func order(_ sections: ProjectSections, open: Set<String>) -> [String] {
        var seen = Set<String>()
        var shown: [ThreadSummary] = []
        for section in sections.topLevel where open.contains(section.id) {
            shown += section.threads
            for part in sections.parts[section.id] ?? [] where open.contains(part.id) { shown += part.threads }
        }
        shown += open.contains(unfiledId) ? sections.unfiled : []
        return shown.map(\.id).filter { seen.insert($0).inserted }
    }

    /// What's drawn, a row id each (project rows as "p:<id>", threads as
    /// "<section>/<thread>"), so the list animates a project opening or
    /// closing as rows coming and going.
    public static func layout(_ sections: ProjectSections, open: Set<String>) -> [String] {
        var rows: [String] = []
        for section in sections.topLevel {
            rows.append("p:" + section.id)
            guard open.contains(section.id) else { continue }
            rows += section.threads.map { section.id + "/" + $0.id }
            // Its parts nest under it, each folding as a project does.
            for part in sections.parts[section.id] ?? [] {
                rows.append("p:" + part.id)
                if open.contains(part.id) { rows += part.threads.map { part.id + "/" + $0.id } }
            }
        }
        if !sections.unfiled.isEmpty {
            rows.append("p:" + unfiledId)
            if open.contains(unfiledId) { rows += sections.unfiled.map { unfiledId + "/" + $0.id } }
        }
        return rows
    }
}
