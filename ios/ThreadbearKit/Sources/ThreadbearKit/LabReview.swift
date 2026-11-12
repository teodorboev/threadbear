import Foundation

/// Another lab's review of a turn's change (docs/protocol.md, "Review by
/// another lab"): the thread row's `review` (backend API 76), for the turn
/// its `cardTurn` names. Decoded leniently, field by field, so a ledger in a
/// shape this phone doesn't know never stops the thread loading.
public struct PhoneLabReview: Codable, Sendable, Hashable {
    public var reviewer: String
    public var independent: Bool
    public var rounds: Int
    public var threadId: String
    public var passed: Bool
    public var unstructured: Bool?
    public var unchecked: Bool?
    public var findings: [PhoneReviewFinding]

    public init(reviewer: String, independent: Bool = true, rounds: Int = 1, threadId: String = "", passed: Bool = false, unstructured: Bool? = nil, unchecked: Bool? = nil, findings: [PhoneReviewFinding]) {
        self.reviewer = reviewer
        self.independent = independent
        self.rounds = rounds
        self.threadId = threadId
        self.passed = passed
        self.unstructured = unstructured
        self.unchecked = unchecked
        self.findings = findings
    }

    private enum CodingKeys: String, CodingKey { case reviewer, independent, rounds, threadId, passed, unstructured, unchecked, findings }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        reviewer = (try? c.decode(String.self, forKey: .reviewer)) ?? "Another lab"
        independent = (try? c.decode(Bool.self, forKey: .independent)) ?? true
        // A Convex number.
        rounds = Int((try? c.decode(Double.self, forKey: .rounds)) ?? 1)
        threadId = (try? c.decode(String.self, forKey: .threadId)) ?? ""
        passed = (try? c.decode(Bool.self, forKey: .passed)) ?? false
        unstructured = try? c.decodeIfPresent(Bool.self, forKey: .unstructured)
        unchecked = try? c.decodeIfPresent(Bool.self, forKey: .unchecked)
        findings = ((try? c.decodeIfPresent(LossyList<PhoneReviewFinding>.self, forKey: .findings)) ?? nil)?.elements ?? []
    }
}

public struct PhoneReviewFinding: Codable, Sendable, Hashable, Identifiable {
    public var id: Int
    public var severity: String
    public var claim: String
    public var `where`: String?
    public var outcome: String?
    public var note: String?

    public init(id: Int, severity: String = "medium", claim: String, where: String? = nil, outcome: String? = nil, note: String? = nil) {
        self.id = id
        self.severity = severity
        self.claim = claim
        self.where = `where`
        self.outcome = outcome
        self.note = note
    }

    private enum CodingKeys: String, CodingKey { case id, severity, claim, `where`, outcome, note }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = Int(try c.decode(Double.self, forKey: .id))
        severity = (try? c.decode(String.self, forKey: .severity)) ?? "medium"
        claim = try c.decode(String.self, forKey: .claim)
        `where` = try? c.decodeIfPresent(String.self, forKey: .where)
        outcome = try? c.decodeIfPresent(String.self, forKey: .outcome)
        note = try? c.decodeIfPresent(String.self, forKey: .note)
    }
}

/// The review's words, as the Mac's row says them (desktop/core
/// LabReview::heading, rows/lab_review.rs) and the website's (lab-review.ts).
public enum LabReviewText {
    /// "Reviewed by Codex: all answered". The login ("Codex (codex-1)") is left off.
    public static func heading(_ r: PhoneLabReview) -> String {
        let name = r.reviewer.components(separatedBy: " (").first.flatMap { $0.isEmpty ? nil : $0 } ?? r.reviewer
        let who = r.independent ? "Reviewed by \(name)" : "Reviewed by a fresh \(name) session"
        let rounds = r.rounds > 1 ? ", \(r.rounds) rounds" : ""
        let open = r.findings.filter { $0.outcome == nil || $0.outcome == "left" }.count
        let result: String
        if r.unstructured == true {
            result = "see its reply"
        } else if r.unchecked == true {
            result = "last fixes not re-reviewed"
        } else if r.passed {
            result = r.findings.isEmpty ? "nothing found" : "passed"
        } else if open > 0 {
            result = "\(open) open"
        } else {
            result = "all answered"
        }
        return "\(who)\(rounds): \(result)"
    }

    public enum Outcome: Sendable { case fixed, disputed, left, none }

    public static func outcome(_ f: PhoneReviewFinding) -> (Outcome, String) {
        switch f.outcome {
        case "fixed": (.fixed, "Fixed")
        case "disputed": (.disputed, "Disputed")
        case "left": (.left, "Left for you")
        default: (.none, "No answer")
        }
    }

    /// The line under a finding: what was done, the agent's note, how serious, and where.
    public static func detail(_ f: PhoneReviewFinding) -> String {
        var line = outcome(f).1
        if let note = f.note, !note.isEmpty { line += ": \(note)" }
        if f.severity == "high" { line += " · serious" }
        if let place = f.where, !place.isEmpty { line += " · \(place)" }
        return line
    }

    /// The seq of the reply the review goes over: the last assistant message
    /// of the turn the row says it's from. Nil when that reply isn't loaded,
    /// or an older runner's row says no turn: not shown, rather than on the
    /// wrong reply.
    public static func replySeq(in messages: [PhoneMessage], cardTurn: String?) -> Int? {
        guard let cardTurn else { return nil }
        return messages.last { $0.role == .assistant && !$0.plan && $0.turn == cardTurn }?.seq
    }
}
