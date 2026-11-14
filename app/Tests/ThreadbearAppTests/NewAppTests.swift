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
    }
}
