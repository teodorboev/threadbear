import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// A project's page tabs (projectDecisions experiment): the keys stop at the
/// ends, and only a change is kept as a pick.
@MainActor
final class ProjectTabsAppTests: XCTestCase {
    func testTheKeysStopAtTheEndsAndOnlyAChangeIsKept() {
        _ = NSApplication.shared
        let app = AppModel()
        app.experiments = ["projectDecisions"]
        app.projects = [Project(id: "p", title: "P")]
        app.openProjectPage("p")
        XCTAssertEqual(app.projectTab("p"), .project, "nothing waiting: it opens on Project")
        XCTAssertFalse(app.canStepProjectTab(by: 1), "Show Next Tab has nowhere to go")
        app.stepProjectTab(by: 1)
        app.pickProjectTab("p", .project)
        XCTAssertNil(app.projectTabsPicked["p"], "a press that changed nothing isn't kept")
        XCTAssertTrue(app.canStepProjectTab(by: -1))
        app.stepProjectTab(by: -1)
        XCTAssertEqual(app.projectTab("p"), .decisions)
        XCTAssertEqual(app.projectTabsPicked["p"], .decisions)
        app.experiments = []
        XCTAssertFalse(app.canStepProjectTab(by: 1), "no tabs with the experiment off")
    }
}
