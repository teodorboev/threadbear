import Foundation
import XCTest
@testable import ThreadbearCore

/// The drafts' file (DraftStore) is only ever its owner's, from the first
/// byte (Daybreak, 11 Nov, findings 88 and 94; `private_file.rs` in the GPUI
/// app has the same tests).
final class PrivateFileTests: XCTestCase {
    private var folder: URL!

    override func setUpWithError() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("tb-private-file-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o755])
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: folder)
    }

    private func mode(_ url: URL) throws -> Int {
        (try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int ?? -1) & 0o777
    }

    func testWrittenWholeAndOnlyForItsOwner() throws {
        let url = folder.appendingPathComponent("drafts.json")
        try PrivateFile.write(Data("secret".utf8), to: url)
        XCTAssertEqual(try Data(contentsOf: url), Data("secret".utf8))
        XCTAssertEqual(try mode(url), 0o600)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: url.path)
        try PrivateFile.write(Data("two".utf8), to: url)
        XCTAssertEqual(try mode(url), 0o600, "replaced, not written into")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: folder.path), ["drafts.json"], "nothing left behind")
    }

    /// A symlink at the file's own name is replaced, never written through.
    func testASymlinkAtTheNameIsReplacedNotFollowed() throws {
        let url = folder.appendingPathComponent("drafts.json")
        let theirs = folder.appendingPathComponent("theirs")
        try FileManager.default.createSymbolicLink(at: url, withDestinationURL: theirs)
        try PrivateFile.write(Data("secret".utf8), to: url)
        XCTAssertFalse(FileManager.default.fileExists(atPath: theirs.path), "the symlink was followed")
        XCTAssertEqual(try Data(contentsOf: url), Data("secret".utf8))
        XCTAssertEqual(try mode(url), 0o600)
    }

    func testANewFolderIsTheOwnersOnly() throws {
        let url = folder.appendingPathComponent("a/drafts.json")
        try PrivateFile.write(Data("x".utf8), to: url)
        XCTAssertEqual(try mode(folder.appendingPathComponent("a")), 0o700)
    }

    /// Another account opening each file in the folder as it appears never
    /// finds one it could read.
    func testNoFileIsEverReadableByAnyoneElse() throws {
        let url = folder.appendingPathComponent("drafts.json")
        let path = folder.path
        let stop = ManagedFlag()
        let caught = ManagedCount()
        let watcher = Thread {
            while !stop.value {
                for name in (try? FileManager.default.contentsOfDirectory(atPath: path)) ?? [] {
                    var info = stat()
                    if lstat(path + "/" + name, &info) == 0, info.st_mode & 0o077 != 0 { caught.add() }
                }
            }
        }
        watcher.start()
        for i in 0..<500 { try PrivateFile.write(Data("secret \(i)".utf8), to: url) }
        stop.value = true
        Thread.sleep(forTimeInterval: 0.05)
        XCTAssertEqual(caught.value, 0)
    }
}

private final class ManagedFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var _value = false
    var value: Bool {
        get { lock.withLock { _value } }
        set { lock.withLock { _value = newValue } }
    }
}

private final class ManagedCount: @unchecked Sendable {
    private let lock = NSLock()
    private var _value = 0
    var value: Int { lock.withLock { _value } }
    func add() { lock.withLock { _value += 1 } }
}
