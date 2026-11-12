import XCTest
@testable import Threadbear

/// A save from the schedule editor sends the folder only when it changed
/// (security review, 11 Nov 2026, finding 89): it went with every save, and
/// the runner took it as a move, which approved the schedule's check again
/// as its scripts were then, though the editor never shows the check.
final class ScheduleEditorFolderTests: XCTestCase {
    private func opened(_ cwd: String) -> ScheduleEditorState {
        var s = ScheduleEditorState(scheduleId: "s1")
        s.cwd = cwd
        s.openedCwd = cwd
        return s
    }

    func testASaveThatKeepsTheFolderSendsNone() {
        XCTAssertNil(opened("/Users/m/dev/x").updateCwd)
        var s = opened("/Users/m/dev/x")
        s.cwd = "/Users/m/dev/x "
        XCTAssertNil(s.updateCwd)
    }

    func testAChangedFolderIsSent() {
        var s = opened("/Users/m/dev/x")
        s.cwd = "/Users/m/dev/y"
        XCTAssertEqual(s.updateCwd, "/Users/m/dev/y")
        // Another Mac's default folder, cleared: sent as empty.
        var away = opened("~/work")
        away.cwd = ""
        XCTAssertEqual(away.updateCwd, "")
    }
}
