import Foundation
import Testing
@testable import ThreadbearKit

/// Another lab's review on the phone, worded as the Mac's row and the
/// website say it, and never in the way of the thread loading.
struct LabReviewTests {
    let ledger = PhoneLabReview(reviewer: "Codex (codex-1)", findings: [
        PhoneReviewFinding(id: 1, severity: "high", claim: "Drops the session on refresh.", where: "src/app.ts:3", outcome: "fixed"),
        PhoneReviewFinding(id: 2, severity: "low", claim: "Logs the token.", outcome: "disputed", note: "It logs a hash."),
    ])

    @Test func headingSaysWhoAndHowItCameOut() {
        #expect(LabReviewText.heading(ledger) == "Reviewed by Codex: all answered")
        var open = ledger
        open.rounds = 2
        open.findings[0].outcome = nil
        #expect(LabReviewText.heading(open) == "Reviewed by Codex, 2 rounds: 1 open")
        #expect(LabReviewText.heading(PhoneLabReview(reviewer: "Codex", passed: true, findings: [])) == "Reviewed by Codex: nothing found")
    }

    @Test func findingLineSaysWhatWasDone() {
        #expect(LabReviewText.detail(ledger.findings[0]) == "Fixed · serious · src/app.ts:3")
        #expect(LabReviewText.detail(ledger.findings[1]) == "Disputed: It logs a hash.")
    }

    @Test func decodesFromTheRowAndShrugsOffOddShapes() throws {
        let json = #"""
        {"reviewer":"Codex (codex-1)","independent":true,"rounds":1,"threadId":"r1","passed":false,
         "findings":[{"id":1,"round":1,"severity":"high","claim":"Drops the session.","outcome":"fixed"},{"id":"bad"},{"id":2,"round":1,"severity":"low","claim":"Logs the token.","later":true}],
         "somethingNew":42}
        """#
        let r = try JSONDecoder().decode(PhoneLabReview.self, from: Data(json.utf8))
        #expect(r.findings.map(\.claim) == ["Drops the session.", "Logs the token."])
        let odd = try JSONDecoder().decode(PhoneLabReview.self, from: Data(#"{"rounds":"two"}"#.utf8))
        #expect(odd.findings.isEmpty)
    }
}

extension LabReviewTests {
    @Test func goesOverItsOwnTurnsReplyOnly() {
        let at = Date(timeIntervalSince1970: 0)
        let messages = [
            PhoneMessage(seq: 1, role: .user, text: "Fix it", turn: "aaaa1111", at: at),
            PhoneMessage(seq: 3, role: .assistant, text: "On it.", turn: "aaaa1111", at: at),
            PhoneMessage(seq: 5, role: .assistant, text: "Fixed it.", turn: "aaaa1111", at: at),
            PhoneMessage(seq: 7, role: .user, text: "Again", turn: "bbbb2222", at: at),
        ]
        #expect(LabReviewText.replySeq(in: messages, cardTurn: "aaaa1111") == 5)
        #expect(LabReviewText.replySeq(in: messages, cardTurn: "bbbb2222") == nil)
        #expect(LabReviewText.replySeq(in: messages, cardTurn: nil) == nil)
    }
}
