import XCTest
@testable import ThreadbearCore

/// ```bash blocks get a play icon that types the command into the Terminal.
final class ShellBlockTests: XCTestCase {
    func testOnlyShellFencesCount() {
        for language in ["sh", "bash", "zsh", "shell", "Bash title=x"] {
            XCTAssertEqual(ShellBlock.from(language: language, code: "ls\n")?.command, "ls", language)
        }
        for language in ["python", "bashful", "console", "", nil] as [String?] {
            XCTAssertNil(ShellBlock.from(language: language, code: "ls"), language ?? "nil")
        }
        XCTAssertNil(ShellBlock.from(language: "bash", code: " \n\n"))
    }

    func testPromptsComeOffOnlyWhenEveryLineHasOne() {
        XCTAssertEqual(ShellBlock.from(language: "sh", code: "$ a\n\n$ b\n")?.command, "a\n\nb")
        XCTAssertEqual(ShellBlock.from(language: "sh", code: "$ a\nb")?.command, "$ a\nb")
        XCTAssertEqual(ShellBlock.from(language: "sh", code: "$HOME/x")?.command, "$HOME/x")
    }

    // Each of these could make what runs differ from what's on screen, or
    // run a line without Return being pressed.
    func testRefusesWhatCantBeTypedSafely() {
        let bad = [
            "echo hi\u{1b}[201~\nrm -rf ~", // ends a bracketed paste early
            "echo\u{07}", "a\rb", "a\u{7f}b", "a\u{9b}201~",
            "rm\u{200b} x", // zero width
            "echo \u{202e}cod", // right-to-left override
            "a\u{2028}b",
        ]
        for code in bad { XCTAssertNil(ShellBlock.from(language: "bash", code: code), code.debugDescription) }
        XCTAssertNotNil(ShellBlock.from(language: "bash", code: "echo 'héllo 🐻'\n\tls"))
        XCTAssertNil(ShellBlock.from(language: "bash", code: String(repeating: "a", count: ShellBlock.maxSource + 1)))
    }

    func testMayHold() {
        XCTAssertTrue(ShellBlock.mayHold("x\n```bash\nls\n```"))
        XCTAssertFalse(ShellBlock.mayHold("no fence, just bash"))
        XCTAssertFalse(ShellBlock.mayHold("```python\nx\n```"))
    }

    private func step(_ command: String = "ls", front: Bool = true, bracketed: Bool = true, starting: Bool = false, line: TerminalPut.Line = .empty) -> TerminalPut.Step {
        TerminalPut.step(for: ShellBlock(command: command), shellInFront: front, bracketedPaste: bracketed, starting: starting, line: line)
    }

    func testTypesOnAnEmptyPrompt() {
        XCTAssertEqual(step(), .type)
        XCTAssertEqual(step("a\nb"), .type)
    }

    func testNeverTypesIntoARunningProgram() {
        // vim turns bracketed paste on too, so that alone isn't enough.
        XCTAssertEqual(step(front: false, bracketed: true), .busy)
    }

    // `read -k 11 answer; eval "$answer"` owns the terminal from inside the
    // shell, so eleven typed characters would have run with no Return.
    func testNeedsTheShellToBeEditingALine() {
        XCTAssertEqual(step(front: true, bracketed: false), .busy)
        XCTAssertEqual(step("a\nb", front: true, bracketed: false), .busy)
    }

    func testWaitsForAShellThatsStarting() {
        XCTAssertEqual(step(bracketed: false, starting: true), .wait)
        XCTAssertEqual(step(front: false, starting: true), .wait)
        XCTAssertEqual(step(starting: true), .type)
    }

    func testNeverAddsToWhatSomeoneTyped() {
        XCTAssertEqual(step(line: .edited), .busy)
    }

    func testADifferentBlockIsCopiedWhileOneWaitsAtThePrompt() {
        XCTAssertEqual(step("ls", line: .put("npm test")), .occupied)
    }

    func testASecondClickDoesntTypeItAgain() {
        XCTAssertEqual(step("npm test", line: .put("npm test")), .alreadyThere)
    }

    func testCommandsThatDifferOnlyInSpacesAreDifferent() {
        XCTAssertEqual(step(#"echo "a b""#, line: .put(#"echo "ab""#)), .occupied)
    }

    private func after(_ key: String?, _ line: TerminalPut.Line = .put("ls"), control: Bool = false, option: Bool = false, shift: Bool = false, command: Bool = false) -> TerminalPut.Line {
        TerminalPut.line(line, afterKey: key, control: control, option: option, shift: shift, command: command)
    }

    func testKeysTrackWhatsOnThePrompt() {
        XCTAssertEqual(after("\r"), .empty)
        XCTAssertEqual(after("\r", .edited), .empty)
        XCTAssertEqual(after("\u{3}"), .empty, "the keypad's Enter")
        XCTAssertEqual(after("c", .edited, control: true), .empty)
        XCTAssertEqual(after("a", .empty), .edited)
        XCTAssertEqual(after("a", control: true), .edited, "Ctrl-A moves the cursor")
        XCTAssertEqual(after("\r", shift: true), .edited)
        XCTAssertEqual(after("\u{F700}"), .edited, "the up arrow brings back a command")
        XCTAssertEqual(after("j", command: true), .put("ls"), "a shortcut isn't input")
        XCTAssertEqual(after("c"), .edited, "a plain c is just a c")
    }

    func testAPasteThatWouldRunLinesIsAskedAbout() {
        XCTAssertFalse(TerminalPut.pasteRuns("echo one"))
        XCTAssertTrue(TerminalPut.pasteRuns("touch /tmp/x\n"))
        XCTAssertTrue(TerminalPut.pasteRuns("a\rb"))
        XCTAssertTrue(TerminalPut.pasteRuns("echo $(touch /tmp/x)\t"), "a Tab is the Tab key without brackets")
        XCTAssertTrue(TerminalPut.pasteQuestion("ls\t").detail.contains("Tab"))
        XCTAssertTrue(TerminalPut.pasteQuestion("ls\n").detail.contains("Return"))
        XCTAssertEqual(TerminalPut.pasteQuestion("a\nb\n").title, "Paste 2 lines?")
        XCTAssertEqual(TerminalPut.pasteQuestion("ls\n").title, "Paste this line?")
    }

    func testAPasteIsSentAsTheTerminalWould() {
        XCTAssertEqual(TerminalPut.pasteBytes("a\nb", bracketed: false), Array("a\rb".utf8))
        XCTAssertEqual(TerminalPut.pasteBytes("ls\r\n", bracketed: true), Array("\u{1b}[200~ls\r\u{1b}[201~".utf8))
        XCTAssertEqual(TerminalPut.pasteBytes("echo\u{1b}[201~\nrm -rf ~", bracketed: true), Array("\u{1b}[200~echo[201~\rrm -rf ~\u{1b}[201~".utf8))
        // With no brackets a control character would be a key pressed.
        XCTAssertEqual(TerminalPut.pasteBytes("ls\u{3}\u{1}\u{1b}:q!\u{15}\u{7f}\u{9b}\tx", bracketed: false), Array("ls:q!\tx".utf8))
    }

    /// Daybreak's finding 84: inside the brackets a ^\ (or ^C, ^Z) is still
    /// a signal to the pty, which flushes the opening bracket, so the line
    /// break after it ran the command. Every control but Tab and Return
    /// goes, C0, DEL and C1 alike (the GPUI app's tests/paste_pty.rs plays it
    /// against a real bash).
    func testABracketedPasteHasNoControlCharactersInIt() {
        XCTAssertEqual(TerminalPut.pasteBytes("\u{1c}printf \"TB_%s_RAN\\n\" PASTE\n", bracketed: true),
                       Array("\u{1b}[200~printf \"TB_%s_RAN\\n\" PASTE\r\u{1b}[201~".utf8))
        let every = String(String.UnicodeScalarView((0..<0x20).compactMap(Unicode.Scalar.init) + (0x7f..<0xa0).compactMap(Unicode.Scalar.init)))
        XCTAssertEqual(TerminalPut.pasteBytes("a\(every)b", bracketed: true), Array("\u{1b}[200~a\t\r\rb\u{1b}[201~".utf8))
        XCTAssertEqual(TerminalPut.pasteBytes("a\(every)b", bracketed: false), Array("a\t\r\rb".utf8))
        XCTAssertEqual(TerminalPut.pasteBytes("caf\u{e9} \u{65e5}\u{672c} \u{1f43b}", bracketed: true),
                       Array("\u{1b}[200~caf\u{e9} \u{65e5}\u{672c} \u{1f43b}\u{1b}[201~".utf8))
    }

    func testASnippetWithALineBreakIsAPasteNotKeys() {
        XCTAssertTrue(TerminalPut.isBulkText("touch /tmp/x\n"))
        XCTAssertTrue(TerminalPut.isBulkText("ls\u{1}"))
        XCTAssertFalse(TerminalPut.isBulkText("日本語"), "an input method's words are typed")
        XCTAssertFalse(TerminalPut.isBulkText("\r"))
        XCTAssertFalse(TerminalPut.isBulkText("\r\n"))
        XCTAssertFalse(TerminalPut.isBulkText("é"))
    }

    func testTheShellIsKnownByItsProgram() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("tb-shell-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let real = dir.appendingPathComponent("zsh")
        try Data().write(to: real)
        let started = try XCTUnwrap(TerminalPut.realPath(real.path))
        XCTAssertTrue(TerminalPut.isTheShell(started: started, running: started))
        let link = dir.appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: real)
        XCTAssertTrue(TerminalPut.isTheShell(started: started, running: link.path), "a link to it is it")
        XCTAssertFalse(TerminalPut.isTheShell(started: started, running: "/usr/bin/ssh"))
    }
}
