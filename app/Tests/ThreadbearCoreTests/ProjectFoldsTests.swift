import XCTest
@testable import ThreadbearCore

/// The folded Projects view (projectDecisions experiment): one row a
/// project, its threads only while it's open.
final class ProjectFoldsTests: XCTestCase {
    func testOnlyOpenProjectsShowTheirThreads() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let thread = { (id: String, projects: [String], minutes: Double) in
            ThreadSummary(id: id, title: id, lastActivityAt: now.addingTimeInterval(-minutes * 60),
                          projects: projects.map { ThreadProject(id: $0, title: $0) })
        }
        let sections = ProjectSections.build(
            projects: [Project(id: "a", title: "A", updatedAt: now), Project(id: "b", title: "B", updatedAt: now)],
            threads: [thread("t1", ["a", "b"], 1), thread("t2", ["b"], 2), thread("loose", [], 3)], now: now)
        XCTAssertEqual(ProjectFolds.order(sections, open: []), [], "folded by default: nothing to arrow through")
        XCTAssertEqual(ProjectFolds.layout(sections, open: []), ["p:a", "p:b", "p:-"], "a row a project, and one for the rest")
        XCTAssertEqual(ProjectFolds.order(sections, open: ["b", "a"]), ["t1", "t2"], "a thread in two open projects comes once")
        XCTAssertEqual(ProjectFolds.layout(sections, open: ["b"]), ["p:a", "p:b", "b/t1", "b/t2", "p:-"])
        XCTAssertEqual(ProjectFolds.order(sections, open: ["-"]), ["loose"], "Not in a project opens the same way")
    }
}
