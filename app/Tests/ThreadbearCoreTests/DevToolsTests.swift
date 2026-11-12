import XCTest
@testable import ThreadbearCore

/// A message the runner sends by itself after a restart (`origin: "runner"`).
final class RunnerMessageTests: XCTestCase {
    func testOriginIsReadAndCarriedToTheRow() {
        let event = E.event(1, "user.message", ["text": "Check your change.", "images": [], "origin": "runner"])
        XCTAssertEqual(event.payload, .userMessage(text: "Check your change.", images: [], origin: "runner"))
        let model = TranscriptModel(events: [event, E.user(2, "Thanks")])
        guard case .user(let runner) = model.items[0].content, case .user(let mike) = model.items[1].content else { return XCTFail() }
        XCTAssertTrue(runner.isFromRunner)
        XCTAssertFalse(mike.isFromRunner)
        // Still a user row: it starts the turn the runner asked for.
        XCTAssertEqual(model.items[0].kind, .user)
    }

    func testAnUnknownOriginIsNotTheRunner() {
        let event = E.event(1, "user.message", ["text": "hi", "images": [], "origin": "somethingNew"])
        guard case .user(let message) = TranscriptModel(events: [event]).items[0].content else { return XCTFail() }
        XCTAssertFalse(message.isFromRunner)
    }

    func testExportNamesTheRunnerNotYou() {
        let model = TranscriptModel(events: [E.event(1, "user.message", ["text": "Carry on.", "images": [], "origin": "runner"])])
        XCTAssertEqual(MarkdownExport.section(for: model.items[0]), "## Runner\n\nCarry on.")
        XCTAssertEqual(MarkdownExport.section(for: TranscriptModel(events: [E.user(1, "Hi")]).items[0]), "## You\n\nHi")
    }
}

final class RunnerRestartTests: XCTestCase {
    let now = Date(timeIntervalSince1970: 1_800_000_000)

    func testParsesTheRunnersNotices() {
        XCTAssertEqual(RunnerRestart.parse("The runner will restart when 2 threads finish."), .pending(2, onYou: 0))
        XCTAssertEqual(RunnerRestart.parse("The runner will restart when 1 thread finishes."), .pending(1, onYou: 0))
        XCTAssertEqual(RunnerRestart.parse("The runner will restart when 1 thread finishes and you answer 2."), .pending(3, onYou: 2))
        XCTAssertEqual(RunnerRestart.parse("The runner will restart when you answer 2 threads."), .pending(2, onYou: 2))
        XCTAssertEqual(RunnerRestart.parse("The runner is restarting now."), .restarting)
        XCTAssertEqual(RunnerRestart.parse("The pending runner restart was cancelled."), .cancelled)
        XCTAssertNil(RunnerRestart.parse("Release X kept crashing at startup, so the runner went back to Y."))
        XCTAssertNil(RunnerRestart.parse("claude isn't on PATH"))
    }

    func testPendingThenStatusThenRestartThenBack() {
        var restart = RunnerRestart()
        XCTAssertTrue(restart.notice("The runner will restart when 2 threads finish.", now: now))
        XCTAssertEqual(restart.pendingText, "The runner will restart when 2 threads finish.")
        // The notice has the count; runner.status names the threads.
        restart.status(PendingRestart(waitingFor: ["a", "b"]), now: now)
        XCTAssertEqual(restart.phase, .pending(count: 2, threadIds: ["a", "b"]))
        XCTAssertEqual(restart.waitingForText { $0 == "a" ? "Fix the build" : nil }, "Waiting for Fix the build, b")
        // One finishes.
        restart.status(PendingRestart(waitingFor: ["b"]), now: now)
        XCTAssertEqual(restart.pendingText, "The runner will restart when 1 thread finishes.")
        XCTAssertTrue(restart.notice("The runner is restarting now.", now: now))
        XCTAssertTrue(restart.isRestarting(now: now.addingTimeInterval(5)))
        XCTAssertNil(restart.pendingText)
        restart.disconnected(now: now.addingTimeInterval(1))
        XCTAssertEqual(restart.phase, .restarting(since: now), "losing the connection keeps when it started")
        restart.connected()
        XCTAssertEqual(restart.phase, .none)
    }

    /// Threads waiting on Mike's answer won't finish by themselves, so the
    /// line says he's what it waits for (the runner's own sentences).
    func testThreadsWaitingOnYouAreCountedApart() {
        var restart = RunnerRestart()
        XCTAssertTrue(restart.notice("The runner will restart when 1 thread finishes and you answer 2.", now: now))
        XCTAssertEqual(restart.pendingText, "The runner will restart when 1 thread finishes and you answer 2.")
        restart.status(PendingRestart(waitingFor: ["a", "b", "c"]), now: now)
        XCTAssertEqual(restart.pendingText { $0 != "a" }, "The runner will restart when 1 thread finishes and you answer 2.")
        // Mike answers one; it's running again until it finishes.
        XCTAssertEqual(restart.pendingText { $0 == "c" }, "The runner will restart when 2 threads finish and you answer 1.")
        restart.status(PendingRestart(waitingFor: ["c"]), now: now)
        XCTAssertEqual(restart.pendingText { $0 == "c" }, "The runner will restart when you answer 1 thread.")
        XCTAssertEqual(RunnerRestart.text(waiting: 2, onYou: 2), "The runner will restart when you answer 2 threads.")
        XCTAssertEqual(RunnerRestart.text(waiting: 3, onYou: 0), "The runner will restart when 3 threads finish.")
    }

    func testOtherNoticesChangeNothing() {
        var restart = RunnerRestart()
        XCTAssertFalse(restart.notice("Release X kept crashing at startup, so the runner went back to Y.", now: now))
        XCTAssertEqual(restart.phase, .none)
    }

    func testLosingTheConnectionWhilePendingMeansItBegan() {
        // The "restarting now" notice can go down with the socket.
        var restart = RunnerRestart(phase: .pending(count: 1, threadIds: ["a"]))
        restart.disconnected(now: now)
        XCTAssertTrue(restart.isRestarting(now: now))
        // A plain drop with nothing pending is just a drop.
        var quiet = RunnerRestart()
        quiet.disconnected(now: now)
        XCTAssertEqual(quiet.phase, .none)
    }

    func testARunnerThatDoesntComeBackIsDownNotRestarting() {
        let restart = RunnerRestart(phase: .restarting(since: now))
        XCTAssertTrue(restart.isRestarting(now: now.addingTimeInterval(RunnerRestart.patience - 1)))
        XCTAssertFalse(restart.isRestarting(now: now.addingTimeInterval(RunnerRestart.patience + 1)))
    }

    func testCancelAndAnEmptyStatusClearAPendingRestart() {
        var restart = RunnerRestart(phase: .pending(count: 2, threadIds: []))
        restart.notice("The pending runner restart was cancelled.", now: now)
        XCTAssertEqual(restart.phase, .none)
        restart = RunnerRestart(phase: .pending(count: 2, threadIds: []))
        restart.status(nil, now: now)
        XCTAssertEqual(restart.phase, .none)
    }

    func testAStatusSentJustBeforeTheShutdownCantUndoIt() {
        var restart = RunnerRestart(phase: .restarting(since: now))
        restart.status(nil, now: now.addingTimeInterval(1))
        XCTAssertEqual(restart.phase, .restarting(since: now))
        restart.status(PendingRestart(waitingFor: ["a"]), now: now.addingTimeInterval(1))
        XCTAssertEqual(restart.phase, .restarting(since: now))
    }

    func testStatusWithNobodyToWaitForOrNowIsUnderWay() {
        var restart = RunnerRestart()
        restart.status(PendingRestart(when: "idle", waitingFor: []), now: now)
        XCTAssertTrue(restart.isRestarting(now: now))
        var now2 = RunnerRestart()
        now2.status(PendingRestart(when: "now", waitingFor: ["a"]), now: now)
        XCTAssertTrue(now2.isRestarting(now: now))
    }

    func testRunnerStatusDecodesLeniently() throws {
        let full = """
        {"version":"0.1.0+20261027T101500Z-abc1234","release":"20261027T101500Z-abc1234","pid":12,"running":["a"],
         "background":[],"pendingRestart":{"when":"idle","requestedAt":"2026-10-27T10:00:00Z","waitingFor":["a","b"],"resumeThreadId":"c"}}
        """
        let status = try JSONDecoder().decode(RunnerStatus.self, from: Data(full.utf8))
        XCTAssertEqual(status.release, "20261027T101500Z-abc1234")
        XCTAssertEqual(status.pendingRestart?.waitingFor, ["a", "b"])
        XCTAssertEqual(status.pendingRestart?.resumeThreadId, "c")
        let sparse = try JSONDecoder().decode(RunnerStatus.self, from: Data(#"{"running":"oops"}"#.utf8))
        XCTAssertEqual(sparse.running, [])
        XCTAssertNil(sparse.pendingRestart)
    }
}

/// "Copy debug info" and the automation dump.
final class RowDebugTests: XCTestCase {
    /// Two turns: work that folds into a group (with an edit and its diff),
    /// an approval asked and answered, a plan updated twice, and a second
    /// turn still running.
    let events: [Event] = [
        E.user(1, "Fix it"),
        E.event(2, "turn.started", ["turnId": "t1"]),
        E.event(3, "plan.updated", ["entries": [["text": "Look", "status": "in_progress"]]]),
        E.event(4, "reasoning", ["itemId": "r1", "text": "Looking"]),
        E.call(5, "c1", name: "Edit", kind: "edit", title: "Edit a.swift", input: ["file_path": "/p/a.swift", "old_string": "a", "new_string": "b"]),
        E.event(6, "approval.requested", ["approvalId": "ap1", "itemId": "c1", "name": "Edit", "title": "Edit a.swift", "input": [:], "options": ["allow", "deny"]]),
        E.event(7, "approval.resolved", ["approvalId": "ap1", "decision": "allow"]),
        E.result(8, "c1", text: "ok"),
        E.event(9, "edit.diff", ["itemId": "c1", "path": "/p/a.swift", "hunks": [["oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-a", "+b"]]]]),
        E.event(10, "plan.updated", ["entries": [["text": "Look", "status": "completed"]]]),
        E.text(11, "m1", "Fixed."),
        E.done(12),
        E.event(13, "system.note", ["text": "Runner restarted: a to b"], turn: nil),
        E.event(14, "user.message", ["text": "Check it.", "images": [], "origin": "runner"], turn: "t2"),
        E.event(15, "turn.started", ["turnId": "t2"], turn: "t2"),
        E.result(16, "c2", text: "shot", images: [["path": "/tmp/a.png", "mediaType": "image/png"]]),
    ]

    func testSeqsForEachKindOfRow() {
        let model = TranscriptModel(events: events)
        XCTAssertEqual(model.sourceSeqs(of: "u:1"), [1])
        // A tool: its call, result and diff.
        XCTAssertEqual(model.sourceSeqs(of: "t:c1"), [5, 8, 9])
        XCTAssertEqual(model.sourceSeqs(of: "ap:ap1"), [6, 7])
        // Every update of a turn's plan goes to its first row.
        XCTAssertEqual(model.sourceSeqs(of: "p:3"), [3, 10])
        XCTAssertEqual(model.sourceSeqs(of: "a:m1"), [11])
        XCTAssertEqual(model.sourceSeqs(of: "f:t1"), [12])
        XCTAssertEqual(model.sourceSeqs(of: "n:13"), [13])
        XCTAssertEqual(model.sourceSeqs(of: "i:c2"), [16])
        // The working line: the turn's start.
        XCTAssertEqual(model.sourceSeqs(of: "w:t2"), [15])
        XCTAssertEqual(model.sourceSeqs(of: "nope"), [])
    }

    func testGroupAndChangesCardTakeTheirMembersSeqs() throws {
        let model = TranscriptModel(events: events)
        let group = try XCTUnwrap(model.rows.first { $0.kind == .group })
        guard case .group(let members) = group.content else { return XCTFail() }
        // A finished turn's one line of work: its plan too, and the
        // runner's note after it.
        XCTAssertEqual(members.memberIds, ["p:3", "r:r1", "t:c1", "ap:ap1", "n:13"])
        XCTAssertEqual(model.sourceSeqs(of: group.id), [3, 4, 5, 6, 7, 8, 9, 10, 13])
        XCTAssertEqual(model.sourceSeqs(of: "cf:t1"), [5, 8, 9])
    }

    /// Daybreak 87: the see line run by a real shell, against a `tb` that
    /// prints its arguments. Another runner's id arrives as one argument,
    /// and nothing in it runs.
    func testTheSeeLinesIdIsOneShellWordWhateverItHolds() throws {
        let model = TranscriptModel(events: events)
        let row = try XCTUnwrap(model.item(id: "t:c1"))
        for id in ["remote$(printf TB_ID_EXECUTED >&2)", "x'; printf TB_ID_EXECUTED >&2; '", "x`printf TB_ID_EXECUTED >&2`", "x\nprintf TB_ID_EXECUTED >&2"] {
            let info = RowDebug.info(threadId: id, row: row, seqs: [5], runnerVersion: "1", source: .tb("tb"))
            let line = try XCTUnwrap(info.components(separatedBy: "\nsee: ").last).trimmingCharacters(in: .newlines)
            let shell = Process()
            shell.executableURL = URL(fileURLWithPath: "/bin/sh")
            shell.arguments = ["-c", "tb() { for a in \"$@\"; do printf '<%s>' \"$a\"; done; }; " + line]
            let (out, err) = (Pipe(), Pipe())
            shell.standardOutput = out
            shell.standardError = err
            try shell.run()
            shell.waitUntilExit()
            let stdout = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            let stderr = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            XCTAssertFalse(stderr.contains("TB_ID_EXECUTED"), line)
            XCTAssertEqual(stdout, "<events><\(id)><--from><5><--to><5>", line)
        }
        XCTAssertEqual(RowDebug.shellWord("th1"), "th1")
    }

    func testInfoReadsAsKeyValueLines() throws {
        let model = TranscriptModel(events: events)
        let row = try XCTUnwrap(model.item(id: "t:c1"))
        XCTAssertEqual(RowDebug.info(threadId: "th1", row: row, seqs: model.sourceSeqs(of: row.id), runnerVersion: "0.1.0+source"), """
        thread: th1
        row: t:c1
        kind: diff
        turn: t1
        events: 5, 8-9
        runner: 0.1.0+source
        see: tb events th1 --from 5 --to 9

        """)
        // A dev runner's own tb, and the mock's fixture: a bare tb from a
        // thread would ask the live runner.
        XCTAssertTrue(RowDebug.info(threadId: "th1", row: row, seqs: [5, 8, 9], runnerVersion: "0.1.0+source", source: .tb("/tmp/dev/tb"))
            .hasSuffix("see: /tmp/dev/tb events th1 --from 5 --to 9\n"))
        XCTAssertTrue(RowDebug.info(threadId: "th1", row: row, seqs: [5, 8, 9], runnerVersion: "0.0.0-mock", source: .fixture)
            .hasSuffix("see: lines 5-9 of events.ndjson in the thread's fixture folder (line N is seq N)\n"))
        XCTAssertTrue(RowDebug.info(threadId: "th1", row: row, seqs: [7], runnerVersion: "0.0.0-mock", source: .fixture).contains("see: line 7 of events.ndjson"))
        let note = try XCTUnwrap(model.item(id: "n:13"))
        XCTAssertEqual(RowDebug.info(threadId: "th1", row: note, seqs: [], runnerVersion: ""), """
        thread: th1
        row: n:13
        kind: note
        events: none loaded
        runner: unknown

        """)
    }

    func testSeqRanges() {
        XCTAssertEqual(RowDebug.seqRanges([3, 7, 8, 9, 12]), "3, 7-9, 12")
        XCTAssertEqual(RowDebug.seqRanges([5, 4, 4]), "4-5")
        XCTAssertEqual(RowDebug.seqRanges([]), "")
    }

    func testExcerptIsOneShortLine() throws {
        let model = TranscriptModel(events: events + [E.text(17, "m2", "First line\n\n  second   line " + String(repeating: "x", count: 200))])
        let long = RowDebug.excerpt(try XCTUnwrap(model.item(id: "a:m2")), limit: 30)
        XCTAssertEqual(long, "First line second line xxxxxx…")
        XCTAssertEqual(long.count, 30)
        XCTAssertEqual(RowDebug.excerpt(try XCTUnwrap(model.item(id: "ap:ap1"))), "Edit a.swift (allow)")
        XCTAssertEqual(RowDebug.excerpt(try XCTUnwrap(model.item(id: "i:c2"))), "1 image")
        XCTAssertEqual(RowDebug.excerpt(try XCTUnwrap(model.item(id: "f:t1"))), "4s, $0.12")
        XCTAssertEqual(RowDebug.excerpt(try XCTUnwrap(model.row(id: "w:t2"))), "Working")
        XCTAssertEqual(RowDebug.excerpt(try XCTUnwrap(model.item(id: "u:14"))), "(sent by the runner) Check it.")
    }
}
