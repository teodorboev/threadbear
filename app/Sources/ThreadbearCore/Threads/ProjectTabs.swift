import Foundation

/// A project's page in two tabs (the projectDecisions experiment): its
/// decisions, and everything else about it. Mike, 12 Nov 2026: "tabbed at
/// the top so you can swap between coordinator and project info".
public enum ProjectTab: String, Sendable, CaseIterable {
    case decisions, project

    public var title: String {
        switch self {
        case .decisions: "Decisions"
        case .project: "Project"
        }
    }
}

public enum ProjectTabs {
    /// The tab a page opens on when none has been picked for it: Decisions
    /// when something's waiting, else Project.
    public static func opening(decisions: Int) -> ProjectTab {
        decisions > 0 ? .decisions : .project
    }

    /// The tab showing: the one last picked for this project, else the one
    /// it opened on this visit. Never worked out again from the decisions
    /// while it's open, so the page doesn't jump under the pointer as they
    /// come and go.
    public static func current(picked: ProjectTab?, openedOn: ProjectTab?, decisions: Int) -> ProjectTab {
        picked ?? openedOn ?? opening(decisions: decisions)
    }

    /// The tab beside `tab`, for ⇧⌘[ and ⇧⌘]: they stop at the ends rather
    /// than wrapping, as there are only two.
    public static func step(_ tab: ProjectTab, by delta: Int) -> ProjectTab {
        let all = ProjectTab.allCases
        let at = all.firstIndex(of: tab) ?? 0
        return all[min(max(at + delta, 0), all.count - 1)]
    }
}
