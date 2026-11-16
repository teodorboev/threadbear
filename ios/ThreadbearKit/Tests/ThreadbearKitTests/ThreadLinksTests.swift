import Foundation
import Testing
@testable import ThreadbearKit

@Suite struct ThreadLinksTests {
    private func message(_ seq: Int, _ role: PhoneMessage.Role, _ text: String, summary: Bool = false) -> PhoneMessage {
        PhoneMessage(seq: seq, role: role, text: text, summary: summary, at: Date(timeIntervalSince1970: Double(seq) * 60))
    }

    /// Two finished turns, each folded.
    private var items: [ThreadItem] {
        Turns.fold([
            message(1, .user, "First"),
            message(2, .tool, "Run the tests"),
            message(3, .assistant, "All done.", summary: true),
            message(4, .user, "Second"),
            message(5, .tool, "Run the tests"),
            message(6, .assistant, "Now the risky part."),
            message(7, .tool, "Commit, land, retire the old simulator"),
            message(8, .tool, "Edited ~/dev/a.swift"),
            message(9, .assistant, "- [Landed](work:Commit, land)\n- [Tested](work:run the tests)", summary: true),
        ], busy: false)
    }

    @Test func escapesTheDestinationsAgentsWrite() {
        // Mike's screenshot, 1 Nov: this came through as raw text on the phone.
        let written = "and that's [landed on main](work:Commit, land, retire the old shared simulator) as `ec865ff`."
        let escaped = ThreadLinks.escapeDestinations(written)
        #expect(escaped == "and that's [landed on main](work:Commit,%20land,%20retire%20the%20old%20shared%20simulator) as `ec865ff`.")

        let parsed = try! AttributedString(markdown: escaped, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))
        #expect(String(parsed.characters) == "and that's landed on main as ec865ff.")
        let link = parsed.runs.compactMap(\.link).first
        #expect(link.flatMap(ThreadLinks.target) == .step(quote: "Commit, land, retire the old shared simulator"))
    }

    @Test func leavesEverythingElseAlone() {
        let web = "[x](https://a.b/c) and [y](/a/b.md)"
        #expect(ThreadLinks.escapeDestinations(web) == web)
        #expect(ThreadLinks.escapeDestinations("[done](work:npm%20test)") == "[done](work:npm%20test)")
        let span = "Write `[a](work:npm test)` like that, [b](work:npm test)"
        #expect(ThreadLinks.escapeDestinations(span) == "Write `[a](work:npm test)` like that, [b](work:npm%20test)")
    }

    @Test func aQuoteCanHaveBracketsInIt() throws {
        let escaped = ThreadLinks.escapeDestinations("[a](work:npm test (unit)) next")
        let parsed = try AttributedString(markdown: escaped, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))
        #expect(String(parsed.characters) == "a next")
        #expect(parsed.runs.compactMap(\.link).first.flatMap(ThreadLinks.target) == .step(quote: "npm test (unit)"))
    }

    @Test func readsEachKindOfLink() {
        #expect(ThreadLinks.target(of: URL(string: "thread:a1b2c3d4")!) == .thread("a1b2c3d4"))
        #expect(ThreadLinks.target(of: URL(string: "review:abc")!) == .review)
        #expect(ThreadLinks.target(of: URL(string: "https://a.b")!) == nil)
    }

    private func worked(_ i: Int) -> WorkedTurn {
        guard case let .worked(turn) = items[i] else { fatalError("item \(i) isn't a fold") }
        return turn
    }

    @Test func aWorkLinkFindsItsStepInItsOwnTurn() {
        let second = worked(4), first = worked(1)
        #expect(ThreadLinks.find(quote: "commit,  LAND", from: 9, in: items) == .init(turn: second, step: 7))
        // In both turns: the second's, as the link's in the second.
        #expect(ThreadLinks.find(quote: "Run the tests", from: 9, in: items) == .init(turn: second, step: 5))
        #expect(ThreadLinks.find(quote: "Run the tests", from: 3, in: items) == .init(turn: first, step: 2))
        // The card of asks has no message of its own, so the latest turn.
        #expect(ThreadLinks.find(quote: "Run the tests", from: nil, in: items) == .init(turn: second, step: 5))
        // A line the agent wrote, and a path as it wrote it.
        #expect(ThreadLinks.find(quote: "risky part", from: 9, in: items) == .init(turn: second, step: 6))
        #expect(ThreadLinks.find(quote: "/Users/alex/dev/a.swift", from: 9, in: items) == .init(turn: second, step: 8))
    }

    @Test func wordsFoundNowhereStillOpenTheWorking() {
        #expect(ThreadLinks.find(quote: "npm run nothing", from: 9, in: items) == .init(turn: worked(4), step: nil))
    }

    @Test func aLinkDoesNotFindItself() {
        // Its own words are in the summary, which isn't a step.
        #expect(ThreadLinks.find(quote: "Landed", from: 9, in: items) == .init(turn: worked(4), step: nil))
    }

    @Test func aTurnStillGoingShowsItsLinesSoFar() {
        let going = Turns.fold([message(1, .user, "Go"), message(2, .tool, "Run the tests"), message(3, .assistant, "[Ran](work:run the tests)")], busy: true)
        #expect(ThreadLinks.find(quote: "run the tests", from: 3, in: going)?.step == 2)
        #expect(ThreadLinks.find(quote: "run the tests", from: 3, in: going)?.turn.items == [.tools([message(2, .tool, "Run the tests")])])
        #expect(ThreadLinks.find(quote: "nothing", from: 3, in: going)?.step == nil)
    }

    @Test func noWorkingNoSheet() {
        let quick = Turns.fold([message(1, .user, "Hi"), message(2, .assistant, "[Hello](work:nothing)")], busy: false)
        #expect(ThreadLinks.find(quote: "nothing", from: 2, in: quick) == nil)
    }
}
