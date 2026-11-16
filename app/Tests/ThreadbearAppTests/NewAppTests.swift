import XCTest
@testable import Threadbear

/// The brief the switch thread starts with (NewApp.swift): it has to run the
/// script from main, never the working tree, build before asking, and only
/// switch once the person says so.
final class NewAppTests: XCTestCase {
    func testTheBriefRunsMainsScriptAndAsksBeforeSwitching() {
        let brief = NewAppOffer.brief(checkout: "/Users/someone/dev/threadbear")
        XCTAssertTrue(brief.contains("git -C \"/Users/someone/dev/threadbear\" show refs/heads/main:desktop/scripts/switch-from-swift.sh"))
        XCTAssertTrue(brief.contains("--no-open"), "the build runs first without switching")
        XCTAssertTrue(brief.contains("Switch now") && brief.contains("Later"), "it asks with those answers")
        XCTAssertTrue(brief.contains("Don't switch before I answer"))
        XCTAssertFalse(brief.contains("—"), "no em dashes in what it says")
        // Tried as a stranger on Tinker Desk (15 Nov 2026): a first build took
        // 31 minutes and about 4 GB, and the script stops with exit 6 when
        // there isn't room.
        XCTAssertTrue(brief.contains("20 to 40 minutes") && !brief.contains("10 minutes"))
        XCTAssertTrue(brief.contains("exit 6") && brief.contains("Exit 4"))
    }
}
