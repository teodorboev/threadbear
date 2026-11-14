import XCTest
@testable import Threadbear

/// Daybreak 112: a folder's .app claiming another app's bundle id is never
/// started by a fixed button or menu, and drawing its icon never asks
/// NSWorkspace (which can register it with Launch Services).
@MainActor
final class SignedAppTests: XCTestCase {
    private var folder: URL!

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("signed-app-\(UUID().uuidString)")
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: folder)
    }

    /// An unsigned Impostor.app with Chrome's id, naming `icon`, with a blue
    /// icns in its Resources unless `withIcon` is false.
    private func impostor(icon: String = "Multi", withIcon: Bool = true) throws -> URL {
        let app = folder.appendingPathComponent("Impostor-\(UUID().uuidString).app")
        let resources = app.appendingPathComponent("Contents/Resources")
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: app.appendingPathComponent("Contents/MacOS"), withIntermediateDirectories: true)
        let plist = """
        <?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.google.Chrome</string>\
        <key>CFBundleExecutable</key><string>run</string><key>CFBundlePackageType</key><string>APPL</string>\
        <key>CFBundleIconFile</key><string>\(icon)</string></dict></plist>
        """
        try Data(plist.utf8).write(to: app.appendingPathComponent("Contents/Info.plist"))
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: app.appendingPathComponent("Contents/MacOS/run"))
        if withIcon { try FileManager.default.copyItem(at: Self.multiIcns, to: resources.appendingPathComponent("Multi.icns")) }
        return app
    }

    private static var multiIcns: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../../desktop/gpui/testdata/icons/Multi.icns").standardized
    }

    func testAnUnsignedBundleWithChromesIdFailsTheChromeCheck() throws {
        XCTAssertNil(SignedApp.checked(try impostor(), SignedApp.googleChrome))
        XCTAssertNil(SignedApp.checked(try impostor(), SignedApp.appleSimulator))
        let chrome = URL(fileURLWithPath: "/Applications/Google Chrome.app")
        if FileManager.default.fileExists(atPath: chrome.path) {
            XCTAssertNotNil(SignedApp.checked(chrome, SignedApp.googleChrome), "Google's own Chrome was refused")
        }
    }

    func testASignedCopyOutsideTheAppFoldersIsRefused() throws {
        let calculator = URL(fileURLWithPath: "/System/Applications/Calculator.app")
        let requirement = #"anchor apple and identifier "com.apple.calculator""#
        XCTAssertNotNil(SignedApp.checked(calculator, requirement))
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let copy = folder.appendingPathComponent("Calculator.app")
        try FileManager.default.copyItem(at: calculator, to: copy)
        XCTAssertNil(SignedApp.checked(copy, requirement), "a signed copy in a temp folder")
    }

    func testAnApplicationsIconIsItsOwnIcnsFile() throws {
        XCTAssertNotNil(PatchImage.applicationIcon(try impostor().path))
        XCTAssertNotNil(PatchImage.applicationIcon(try impostor(icon: "Multi.icns").path))
    }

    func testAnApplicationsIconNeverLeavesItsBundle() throws {
        XCTAssertNil(PatchImage.applicationIcon(try impostor(icon: "../../../Multi").path))
        XCTAssertNil(PatchImage.applicationIcon(try impostor(withIcon: false).path))
        let linked = try impostor(withIcon: false)
        try FileManager.default.createSymbolicLink(at: linked.appendingPathComponent("Contents/Resources/Multi.icns"), withDestinationURL: Self.multiIcns)
        XCTAssertNil(PatchImage.applicationIcon(linked.path), "a link to an icns outside the bundle")
        let fifo = try impostor()
        let plist = fifo.appendingPathComponent("Contents/Info.plist")
        try FileManager.default.removeItem(at: plist)
        XCTAssertEqual(mkfifo(plist.path, 0o600), 0)
        XCTAssertNil(PatchImage.applicationIcon(fifo.path), "a FIFO for Info.plist, read without hanging")
    }
}
