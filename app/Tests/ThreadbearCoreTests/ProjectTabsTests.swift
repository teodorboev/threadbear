import XCTest
@testable import ThreadbearCore

/// A project's page in tabs (the projectDecisions experiment).
final class ProjectTabsTests: XCTestCase {
    func testItOpensOnDecisionsOnlyWhenSomethingsWaiting() {
        XCTAssertEqual(ProjectTabs.opening(decisions: 2), .decisions)
        XCTAssertEqual(ProjectTabs.opening(decisions: 0), .project)
    }

    func testAPickedTabWinsAndAnOpenPageDoesntJump() {
        XCTAssertEqual(ProjectTabs.current(picked: .project, openedOn: .decisions, decisions: 3), .project, "the last one picked")
        XCTAssertEqual(ProjectTabs.current(picked: nil, openedOn: .decisions, decisions: 0), .decisions, "decisions emptying while it's open doesn't move it")
        XCTAssertEqual(ProjectTabs.current(picked: nil, openedOn: .project, decisions: 4), .project, "nor do new ones arriving")
        XCTAssertEqual(ProjectTabs.current(picked: nil, openedOn: nil, decisions: 1), .decisions)
    }

    func testTheKeysStepAndStopAtTheEnds() {
        XCTAssertEqual(ProjectTabs.step(.decisions, by: 1), .project)
        XCTAssertEqual(ProjectTabs.step(.project, by: 1), .project)
        XCTAssertEqual(ProjectTabs.step(.project, by: -1), .decisions)
        XCTAssertEqual(ProjectTabs.step(.decisions, by: -1), .decisions)
    }
}
