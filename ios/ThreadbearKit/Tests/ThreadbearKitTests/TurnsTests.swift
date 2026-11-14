import Foundation
import Testing
@testable import ThreadbearKit

@Suite struct TurnsTests {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)
    func m(_ seq: Int, _ role: PhoneMessage.Role, _ text: String, at: Double, origin: String? = nil, summary: Bool = false, turn: String? = nil) -> PhoneMessage {
        PhoneMessage(seq: seq, role: role, text: text, origin: origin, toolKind: role == .tool ? "command" : nil, summary: summary, turn: turn, at: t0.addingTimeInterval(at))
    }

    /// Work carried on after end_turn's bullets: the shape of a real thread
    /// (30402425, 1 Nov) that crashed the phone each time it was opened, as
    /// the range from the last step to the summary ran backwards.
    @Test func workAfterTheSummaryShowsTheLastThingSaidAndDoesntCrash() {
        let messages = [
            m(1, .user, "Set up the API", at: 0),
            m(2, .tool, "Make the keys", at: 5),
            m(3, .tool, "threadbear: end_turn", at: 10),
            m(4, .assistant, "- Keys made\n- Install the app next", at: 11, summary: true),
            m(5, .tool, "Check for the app on the phone", at: 20),
            m(6, .assistant, "Installing it myself.", at: 30),
            m(7, .tool, "Tap Get", at: 40),
            m(8, .tool, "threadbear: end_turn", at: 50),
            m(9, .assistant, "It's installed, sign in and tell me.", at: 60),
        ]
        let items = Turns.fold(messages, busy: false)
        guard case let .message(reply) = items.last else { Issue.record("expected the reply"); return }
        #expect(reply.seq == 9)
        #expect(Turns.replyIndexes(messages) == [8])
    }

    @Test func workAfterTheSummaryWithNothingSaidKeepsTheSummary() {
        let body = [
            m(1, .assistant, "- Done", at: 0, summary: true),
            m(2, .tool, "One more check", at: 5),
        ]
        #expect(Turns.replyIndexes(body) == [0])
    }

    @Test func aFinishedTurnFoldsItsWorkAndShowsTheSummary() {
        let messages = [
            m(1, .user, "Fix it", at: 0),
            m(2, .assistant, "Looking.", at: 5),
            m(3, .tool, "Run the tests", at: 10),
            m(4, .assistant, "Done.", at: 120),
            m(5, .tool, "threadbear: end_turn", at: 132),
            m(6, .assistant, "- Fixed it\n- Tests pass", at: 133, summary: true),
        ]
        let items = Turns.fold(messages, busy: false)
        #expect(items.count == 3)
        guard case let .worked(w) = items[1] else { Issue.record("expected the work folded"); return }
        #expect(w.title == "Worked for 2m 13s")
        guard case let .message(reply) = items[2] else { Issue.record("expected the reply"); return }
        #expect(reply.seq == 6)
        #expect(Set(items.map(\.id)).count == items.count)
    }

    @Test func aLongerMessageAfterTheLastStepBeatsTheBullets() {
        let messages = [
            m(1, .user, "Why?", at: 0),
            m(2, .tool, "Read x.ts", at: 1),
            m(3, .assistant, "Because the cache is keyed by folder, so two threads in one folder share a run and the second never starts its own.", at: 30),
            m(4, .tool, "threadbear: end_turn", at: 31),
            m(5, .assistant, "- Explained it", at: 31, summary: true),
        ]
        let items = Turns.fold(messages, busy: false)
        guard case let .message(reply) = items.last else { Issue.record("expected a reply"); return }
        #expect(reply.seq == 3)
    }

    @Test func theTurnStillRunningIsNotFoldedAndRunnerMessagesJoinTheTurnBefore() {
        let messages = [
            m(1, .user, "Go", at: 0),
            m(2, .tool, "Build", at: 1),
            m(3, .assistant, "Built.", at: 2),
            m(4, .user, "The app restarted", at: 3, origin: "runner"),
            m(5, .tool, "Check it", at: 4),
            m(6, .assistant, "Checked.", at: 9),
            m(7, .user, "Next", at: 20),
            m(8, .tool, "Run", at: 21),
        ]
        let items = Turns.fold(messages, busy: true)
        // Go, its work and the runner's in one line, each turn's reply, then
        // the live turn as it comes. As on the Mac, the runner's turn doesn't
        // take the reply before it away.
        guard case let .worked(w) = items[1] else { Issue.record("expected the first turn folded"); return }
        #expect(w.title == "Worked for 9s")
        #expect(replies(items) == [1, 3, 6, 7])
        #expect(items.count == 6)
        guard case .tools = items[5] else { Issue.record("expected the live turn unfolded"); return }
    }

    /// The seqs of the messages left on show, Mike's included.
    func replies(_ items: [ThreadItem]) -> [Int] {
        items.compactMap { if case let .message(m) = $0 { m.seq } else { nil } }
    }

    // Thread 9bc89060, 1 Nov 2026: a background wait timed out after the reply,
    // the CLI woke the agent, and its "still waiting on you" replaced a picture
    // Mike had asked for. A wake-up has no message to mark where it starts, so
    // the rows say which turn they're from.
    @Test func aWakeUpAfterAReplyLeavesTheReplyOnShow() {
        let messages = [
            m(157, .user, "Generate an image", at: 0, turn: "a"),
            m(160, .tool, "Draw it", at: 16, turn: "a"),
            m(164, .tool, "threadbear: end_turn", at: 20, turn: "a"),
            m(166, .assistant, "Here's a test bear", at: 23, turn: "a"),
            m(170, .assistant, "That wait hit its limit", at: 56, turn: "b"),
            m(171, .tool, "threadbear: end_turn", at: 58, turn: "b"),
            m(173, .assistant, "Still waiting on you.", at: 60, turn: "b"),
        ]
        let items = Turns.fold(messages, busy: false)
        #expect(replies(items) == [157, 166, 173])
        guard case let .worked(w) = items[1] else { Issue.record("expected the work folded"); return }
        #expect(w.items.contains { if case let .message(m) = $0 { m.seq == 170 } else { false } }, "the wake-up's first line is work")
    }

    @Test func aWakeUpsSummaryDoesntTakeTheReplysPlace() {
        let wake = [
            m(1, .user, "Fix it", at: 0, turn: "a"),
            m(2, .tool, "Build", at: 1, turn: "a"),
            m(3, .assistant, "- Fixed it", at: 2, summary: true, turn: "a"),
            m(4, .assistant, "- Nothing changed", at: 9, summary: true, turn: "b"),
        ]
        #expect(replies(Turns.fold(wake, busy: false)) == [1, 3, 4])
        // The runner's own turn does: its end_turn is told to cover the stretch.
        let restart = [
            m(1, .user, "Fix it", at: 0, turn: "a"),
            m(2, .tool, "Build", at: 1, turn: "a"),
            m(3, .assistant, "- Fixed it", at: 2, summary: true, turn: "a"),
            m(4, .user, "The app restarted", at: 3, origin: "runner", turn: "c"),
            m(5, .assistant, "- Fixed and checked", at: 9, summary: true, turn: "c"),
        ]
        #expect(replies(Turns.fold(restart, busy: false)) == [1, 5])
    }

    // Rows written before the backend took `turn` read as they always did.
    @Test func rowsWithoutATurnAreOneTurn() {
        let messages = [
            m(1, .user, "Go", at: 0),
            m(2, .assistant, "One.", at: 1),
            m(3, .assistant, "Two.", at: 2),
        ]
        #expect(replies(Turns.fold(messages, busy: false)) == [1, 3])
    }

    @Test func aQuickAnswerHasNothingToFold() {
        let items = Turns.fold([m(1, .user, "Hi", at: 0), m(2, .assistant, "Hello", at: 2)], busy: false)
        #expect(items.count == 2)
    }

    @Test func answersAreWrittenAsTheMacWritesThem() {
        let asks = [
            PhoneAsk(action: "Merge it?", why: "Tests pass.", options: ["Merge", "Not yet"]),
            PhoneAsk(action: "Add the key", why: "In Settings.", reply: "done"),
        ]
        #expect(asks[1].paragraph == "Add the key. In Settings.")
        #expect(AskAnswer.message(asks: asks, picks: [1: 0], extra: "thanks") == "2. Add the key\n   done\n\nthanks")
        #expect(AskAnswer.message(asks: [asks[0]], picks: [0: 1]) == "Merge it?\nNot yet")
        #expect(AskAnswer.answers(asks: asks, picks: [0: 5]).isEmpty)
        #expect(AskAnswer.followUpMessage(["Check the nightly"]) == "Check the nightly")
        #expect(AskAnswer.followUpMessage(["Check the nightly", "Refresh the cache"], extra: " quick \n") == "- Check the nightly\n- Refresh the cache\n\nquick")
    }

    @Test func threadRowsAndMessagesDecodeTheCard() throws {
        let row = #"""
        {"threadId":"t","runnerId":"r","title":"T","cwd":"/w","accountId":"a","provider":"claude","status":"idle","needsYou":true,"unread":true,"preview":"p","lastActivityAt":"2026-10-30T00:00:00.000Z","view":"inbox",
         "asks":[{"action":"Merge it?","why":"Tests pass.","options":["Merge","Not yet"],"suggested":0.0,"links":[{"title":"Open PR","url":"https://x"}]}],"asksSeq":12.0,"next":["Ship it"]}
        """#
        let thread = try JSONDecoder().decode(PhoneThread.self, from: Data(row.utf8))
        #expect(thread.asks?.first?.suggestedIndex == 0)
        #expect(thread.asksSeq == 12)
        #expect(thread.next == ["Ship it"])
        let msg = try JSONDecoder().decode(PhoneMessage.self, from: Data(#"{"seq":3.0,"role":"assistant","text":"- a","summary":true,"at":"2026-10-30T00:00:00.000Z"}"#.utf8))
        #expect(msg.summary)
    }
}

/// A thread whose login ran out and moved to another: the runner's line
/// about it (role `note`, backend API 79) shows where it happened and never
/// folds into the work, as on the Mac (13 Nov). The same cases as
/// web/src/turns.test.ts.
@Suite struct LoginNoteTests {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)
    let moved = "Moved to mike@example.org: mike@example.net hit its 5-hour limit, back at 11:30 am."
    func m(_ seq: Int, _ role: PhoneMessage.Role, _ text: String, at: Double, origin: String? = nil, turn: String? = nil) -> PhoneMessage {
        PhoneMessage(seq: seq, role: role, text: text, origin: origin, toolKind: role == .tool ? "command" : nil, turn: turn, at: t0.addingTimeInterval(at))
    }

    func shown(_ items: [ThreadItem]) -> [String] {
        items.map { item in
            switch item {
            case let .message(m): "\(m.role.rawValue):\(m.text)"
            case let .note(m): "note:\(m.text)"
            case .tools: "tools"
            case .worked: "worked"
            case .gap: "gap"
            }
        }
    }

    @Test func aNoteStaysOutOfTheWorkBetweenWhatCameBeforeAndTheReplyAfter() {
        let rows = [
            m(1, .user, "land it", at: 0),
            m(2, .tool, "Ran tb land", at: 5),
            m(3, .assistant, "You've hit your 5-hour limit", at: 60, turn: "aaaa1111"),
            m(4, .note, moved, at: 61),
            m(5, .user, "Carry on from where you left off.", at: 62, origin: "runner", turn: "bbbb2222"),
            m(6, .tool, "Ran git push", at: 70, turn: "bbbb2222"),
            m(7, .assistant, "Landed.", at: 90, turn: "bbbb2222"),
        ]
        let without = shown(Turns.fold(rows.filter { $0.role != .note }, busy: false))
        let with = shown(Turns.fold(rows, busy: false))
        #expect(with.filter { !$0.hasPrefix("note:") } == without)
        #expect(with.last == "assistant:Landed.")
        #expect(with.dropLast().last == "note:\(moved)")
    }

    /// The login ran out before the turn did anything, so the folded work
    /// is all from after the move: the note still goes under the limit's
    /// words (from Codex's review).
    @Test func aLimitHitBeforeAnyWorkKeepsTheNoteUnderIt() {
        let rows = [
            m(1, .user, "fix it", at: 0),
            m(2, .assistant, "You've hit your limit", at: 1, turn: "aaaa1111"),
            m(3, .note, moved, at: 2),
            m(4, .user, "Carry on.", at: 3, origin: "runner", turn: "bbbb2222"),
            m(5, .tool, "Edited a file", at: 4, turn: "bbbb2222"),
            m(6, .assistant, "Fixed.", at: 9, turn: "bbbb2222"),
        ]
        #expect(shown(Turns.fold(rows, busy: false)) == ["user:fix it", "worked", "assistant:You've hit your limit", "note:\(moved)", "assistant:Fixed."])
    }

    @Test func aThreadWaitingForRoomEndsOnItsNote() {
        let wait = "Every login in Claude is out of room. Carrying on at 11:30 am."
        let rows = [m(1, .user, "land it", at: 0), m(2, .tool, "Ran tb land", at: 5), m(3, .assistant, "You've hit your 5-hour limit", at: 60), m(4, .note, wait, at: 61)]
        #expect(shown(Turns.fold(rows, busy: false)).last == "note:\(wait)")
        // The done card and the reply's buttons go by the last thing said,
        // which isn't the note.
        #expect(Turns.lastShown(rows)?.seq == 3)
    }

    @Test func aRunningTurnShowsItInPlace() {
        let rows = [m(1, .user, "go", at: 0), m(2, .tool, "Read a", at: 1), m(3, .note, moved, at: 2), m(4, .assistant, "So far", at: 3)]
        #expect(shown(Turns.fold(rows, busy: true)) == ["user:go", "tools", "note:\(moved)", "assistant:So far"])
    }

    @Test func aPageWithANoteDecodes() throws {
        let json = #"{"messages":[{"seq":1,"role":"user","text":"Fix it","at":1762650000000},{"seq":2,"role":"note","text":"Moved to a: b hit its 5-hour limit.","at":1762650001000}],"hasMore":false}"#
        let page = try JSONDecoder().decode(MessagePage.self, from: Data(json.utf8))
        #expect(page.messages.map(\.role) == [.user, .note])
    }
}
