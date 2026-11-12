import XCTest
@testable import ThreadbearCore

/// The divider marking the wait before a turn the agent started itself, as
/// a sub-agent finished (Mike, 3 Nov: "still running" and "it's back" read
/// as one breath in the Let's Build It thread).
final class TurnGapTests: XCTestCase {
    static func at(_ event: Event, _ time: String) -> Event {
        var copy = event
        copy.ts = "2026-11-03T" + time + "Z"
        return copy
    }

    static func woke(_ seq: Int, turn: String, _ time: String, _ tasks: [[String: Any]] = []) -> Event {
        at(E.event(seq, "turn.started", tasks.isEmpty ? [:] : ["wokeBy": tasks], turn: turn), time)
    }

    /// As it happened: Mike answers, the turn says the researchers are still
    /// running, one comes back six minutes later, the other half a minute
    /// after that, and the last turn ends with a summary and a reply.
    static let research: [Event] = [
        StretchTests.mike(1, "Technical Convex video", turn: "t1"),
        at(E.event(2, "turn.started", [:], turn: "t1"), "06:16:00"),
        E.call(3, "c1", name: "Agent", kind: "task", title: "Mine the repo").withTurn("t1"), E.result(4, "c1").withTurn("t1"),
        StretchTests.text(5, "m1", "The two researchers are still running.", turn: "t1"),
        at(E.done(6, turn: "t1"), "06:17:23"),
        woke(7, turn: "t2", "06:23:00", [["kind": "agent", "description": "Mine LBI repo for video stories"], ["kind": "shell", "description": "Scrape YouTube"]]),
        StretchTests.note(8, "I read origin/main").withTurn("t2"),
        StretchTests.text(9, "m2", "The repo research is back.", turn: "t2"),
        at(E.done(10, turn: "t2"), "06:23:14"),
        woke(11, turn: "t3", "06:23:47", [["kind": "agent", "description": "Prior art and demand research"]]),
        StretchTests.text(12, "m3", "Both reports are in.", turn: "t3"),
        E.call(13, "c2", input: ["command": "render"]).withTurn("t3"), E.result(14, "c2").withTurn("t3"),
    ] + StretchTests.endTurn(15, "e3", turn: "t3") + [
        StretchTests.final(17, "m4", "We'd actually got further than I said.", turn: "t3"),
        at(E.done(18, turn: "t3"), "06:25:36"),
    ]

    func testAFinishedStretchMarksEachWaitBetweenReplies() {
        let model = TranscriptModel(events: Self.research)
        XCTAssertEqual(model.rows.map(\.id), ["u:1", "g:t:c1", "a:m1", "gap:t2", "a:m2", "gap:t3", "a:m4", "f:t3"])
        guard case .gap(let first)? = model.row(id: "gap:t2")?.content else { return XCTFail() }
        XCTAssertEqual(first.label, "6 min later · Mine LBI repo for video stories finished")
        guard case .gap(let second)? = model.row(id: "gap:t3")?.content else { return XCTFail() }
        XCTAssertEqual(second.label, "33s later · Prior art and demand research finished")
    }

    /// Each event as it comes matches building from scratch, and the wait
    /// shows as soon as the woken turn starts.
    func testLiveTheDividerShowsAsTheWokenTurnStarts() {
        var model = TranscriptModel(events: Array(Self.research.prefix(6)))
        var table = model.rows.map(\.id)
        for event in Self.research.dropFirst(6) {
            WorkGroupTests.replay(model.apply([event]), onto: &table, model: model)
            XCTAssertEqual(model.rows.map(\.id), TranscriptModel(events: Array(model.events)).rows.map(\.id), "after seq \(event.seq)")
            if event.seq == 7 {
                XCTAssertTrue(model.rows.map(\.id).contains("gap:t2"), "\(model.rows.map(\.id))")
            }
        }
        XCTAssertEqual(table, model.rows.map(\.id))
    }

    /// A woken turn that shows nothing of its own (it only wrote work) has
    /// no divider of its own: the next one covers both waits.
    func testWaitsWithNothingShownBetweenAreOneDivider() {
        let events: [Event] = [
            StretchTests.mike(1, "Go", turn: "t1"),
            Self.at(E.event(2, "turn.started", [:], turn: "t1"), "06:00:00"),
            StretchTests.text(3, "m1", "Waiting on two jobs.", turn: "t1"),
            Self.at(E.done(4, turn: "t1"), "06:01:00"),
            Self.woke(5, turn: "t2", "06:03:00", [["kind": "shell", "description": "npm test"]]),
            E.call(6, "c1", input: ["command": "cat out.log"]).withTurn("t2"), E.result(7, "c1").withTurn("t2"),
        ] + StretchTests.endTurn(8, "e2", turn: "t2") + [
            Self.at(E.done(10, turn: "t2"), "06:03:10"),
            Self.woke(11, turn: "t3", "06:05:00", [["kind": "shell", "description": "npm run build"]]),
            StretchTests.text(12, "m3", "Both done.", turn: "t3"),
            Self.at(E.done(13, turn: "t3"), "06:05:05"),
        ]
        let model = TranscriptModel(events: events)
        let gaps = model.rows.filter { $0.kind == .gap }
        XCTAssertEqual(gaps.map(\.id), ["gap:t3"], "\(model.rows.map(\.id))")
        guard case .gap(let gap)? = gaps.first?.content else { return XCTFail() }
        XCTAssertEqual(gap.label, "4 min later · npm test and npm run build finished")
    }

    /// Mike's own message starts a turn: no divider, his bubble says it.
    func testATurnMikeStartedHasNoDivider() {
        let events: [Event] = [
            StretchTests.mike(1, "Hi", turn: "t1"), Self.at(E.event(2, "turn.started", [:], turn: "t1"), "06:00:00"),
            StretchTests.text(3, "m1", "Hello", turn: "t1"), Self.at(E.done(4, turn: "t1"), "06:00:05"),
            StretchTests.mike(5, "Again", turn: "t2"), Self.at(E.event(6, "turn.started", [:], turn: "t2"), "07:00:00"),
            StretchTests.text(7, "m2", "Hi again", turn: "t2"), Self.at(E.done(8, turn: "t2"), "07:00:05"),
        ]
        XCTAssertFalse(TranscriptModel(events: events).rows.contains { $0.kind == .gap })
    }

    func testLabels() {
        let t0 = Date(timeIntervalSince1970: 0)
        func gap(_ seconds: Double, _ tasks: [BackgroundTask] = []) -> String { TurnGap(from: t0, to: t0.addingTimeInterval(seconds), wokeBy: tasks).label }
        XCTAssertEqual(gap(0.5), "Moments later")
        XCTAssertEqual(gap(45), "45s later")
        XCTAssertEqual(gap(90 * 60), "1 h 30 min later")
        XCTAssertEqual(gap(2 * 3600), "2 h later")
        XCTAssertEqual(gap(26 * 3600), "A day later")
        XCTAssertEqual(gap(60, [BackgroundTask(kind: .agent, description: "")]), "1 min later · A sub-agent finished")
        XCTAssertEqual(gap(60, [BackgroundTask(kind: .agent, description: "A"), BackgroundTask(kind: .agent, description: "B")]), "1 min later · A and B finished")
        XCTAssertEqual(gap(60, [BackgroundTask(kind: .agent, description: "A"), BackgroundTask(kind: .agent, description: "B"), BackgroundTask(kind: .agent, description: "C")]), "1 min later · A and 2 more finished")
    }

    /// A command with a commit message in it shows only its first line.
    func testALabelIsOneLine() {
        let t0 = Date(timeIntervalSince1970: 0)
        func line(_ description: String) -> String { TurnGap(from: t0, to: t0.addingTimeInterval(180), wokeBy: [BackgroundTask(kind: .shell, description: description)]).labelLine }
        XCTAssertEqual(line("git commit -qm \"Fix it\n\nWhy: it broke\""), "3 min later · git commit -qm \"Fix it…")
        XCTAssertEqual(line("npm test"), "3 min later · npm test finished")
        XCTAssertEqual(line("npm test\n"), "3 min later · npm test finished")
    }
}
