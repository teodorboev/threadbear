import Foundation
import Testing
@testable import ThreadbearKit

/// The phone's "6 min later · Mine the repo finished" divider, as the Mac
/// draws it, from rows (Mike, 3 Nov).
@Suite struct TurnGapTests {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)
    func m(_ seq: Int, _ role: PhoneMessage.Role, _ text: String, at: Double, origin: String? = nil, summary: Bool = false, turn: String?, wokeBy: [WokeTask]? = nil) -> PhoneMessage {
        PhoneMessage(seq: seq, role: role, text: text, origin: origin, toolKind: role == .tool ? "command" : nil, summary: summary, turn: turn, wokeBy: wokeBy, at: t0.addingTimeInterval(at))
    }

    /// The Let's Build It thread: "still running", six minutes, "it's back",
    /// half a minute, then the last turn's reply.
    var research: [PhoneMessage] {
        [
            m(1, .user, "Technical Convex video", at: 0, turn: "t1"),
            m(2, .tool, "Mine the repo", at: 5, turn: "t1"),
            m(3, .assistant, "The two researchers are still running.", at: 83, turn: "t1"),
            m(4, .assistant, "The repo research is back.", at: 83 + 351, turn: "t2", wokeBy: [WokeTask(kind: "agent", description: "Mine LBI repo for video stories"), WokeTask(kind: "shell", description: "Scrape")]),
            m(5, .assistant, "Both reports are in.", at: 83 + 351 + 45, turn: "t3", wokeBy: [WokeTask(kind: "agent", description: "Prior art")]),
            m(6, .tool, "Render the cards", at: 500, turn: "t3"),
            m(7, .tool, "threadbear: end_turn", at: 560, turn: "t3"),
            m(8, .assistant, "We'd got further than I said.", at: 570, turn: "t3"),
        ]
    }

    func labels(_ items: [ThreadItem]) -> [String] {
        items.map { item in
            switch item {
            case let .message(message): "m\(message.seq)"
            case .tools: "tools"
            case .worked: "worked"
            case let .gap(gap): gap.label
            case let .note(note): "note\(note.seq)"
            }
        }
    }

    @Test func aFinishedStretchMarksEachWaitBetweenReplies() {
        #expect(labels(Turns.fold(research, busy: false)) == [
            "m1", "worked", "m3", "6 min later · Mine LBI repo for video stories finished", "m4", "45s later · Prior art finished", "m8",
        ])
    }

    @Test func aStretchStillGoingMarksTheWaitsInItsRows() {
        let rows = Array(research.prefix(4))
        #expect(labels(Turns.fold(rows, busy: true)) == ["m1", "tools", "m3", "6 min later · Mine LBI repo for video stories finished", "m4"])
    }

    @Test func rowsFromBeforeTurnsHaveNoDivider() {
        let old = research.map { var r = $0; r.turn = nil; r.wokeBy = nil; return r }
        #expect(!Turns.fold(old, busy: false).contains { if case .gap = $0 { true } else { false } })
    }

    @Test func aTurnARunnerMessageStartedHasNoDivider() {
        let rows = [
            m(1, .user, "Fix it", at: 0, turn: "t1"),
            m(2, .assistant, "Merged; checking after the restart.", at: 10, turn: "t1"),
            m(3, .user, "The app has restarted.", at: 300, origin: "runner", turn: "t2"),
            m(4, .assistant, "It's live.", at: 320, turn: "t2"),
        ]
        #expect(!Turns.fold(rows, busy: false).contains { if case .gap = $0 { true } else { false } })
    }

    @Test func aRowsWokeByDecodes() throws {
        let json = #"{"seq":4,"role":"assistant","text":"x","turn":"b0a73e31","wokeBy":[{"kind":"agent","description":"Mine the repo"}],"at":"2026-11-03T06:23:14.000Z"}"#
        let row = try JSONDecoder().decode(PhoneMessage.self, from: Data(json.utf8))
        #expect(row.wokeBy == [WokeTask(kind: "agent", description: "Mine the repo")])
    }
}
