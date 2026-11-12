import XCTest
@testable import ThreadbearCore

/// Parts of a project (docs/protocol.md, Projects, "Parts"): a part nests
/// under its parent in the sidebar and inside its parent's card.
final class ProjectPartsTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_000_000)

    private func thread(_ id: String, _ project: String, minutesAgo: Double, asks: Bool = false) -> ThreadSummary {
        ThreadSummary(id: id, title: id, needsYou: asks, lastActivityAt: now.addingTimeInterval(-minutesAgo * 60),
                      projects: [ThreadProject(id: project, title: project)], needs: asks ? "you" : nil,
                      asks: asks ? [Ask(action: "Ship it?", options: ["Yes", "No"])] : [], asksSeq: asks ? 3 : nil)
    }

    private func build() -> ProjectSections {
        let projects = [
            Project(id: "tb", title: "Threadbear", updatedAt: now.addingTimeInterval(-86_400)),
            Project(id: "port", title: "GPUI port", updatedAt: now.addingTimeInterval(-86_400), parentId: "tb"),
            Project(id: "android", title: "Android app", updatedAt: now.addingTimeInterval(-86_400), parentId: "tb"),
            Project(id: "pool", title: "Pool", updatedAt: now.addingTimeInterval(-86_400)),
            Project(id: "orphan", title: "Orphan", updatedAt: now.addingTimeInterval(-86_400), parentId: "gone"),
        ]
        let threads = [
            thread("t-pool", "pool", minutesAgo: 5),
            thread("t-port", "port", minutesAgo: 1, asks: true),
            thread("t-tb", "tb", minutesAgo: 30),
            thread("t-android", "android", minutesAgo: 40, asks: true),
        ]
        return ProjectSections.build(projects: projects, threads: threads, now: now)
    }

    func testPartsNestUnderTheirParentWhichGoesByTheirActivity() {
        let sections = build()
        XCTAssertEqual(sections.topLevel.map(\.id), ["tb", "pool", "orphan"], "Threadbear goes first, by its part's newest thread; a part whose parent's gone stands alone")
        XCTAssertEqual(sections.parts["tb"]?.map(\.id), ["android", "port"], "parts in title order")
        XCTAssertEqual(sections.sections.count, 5, "every project's section is still there to find by id")
        XCTAssertEqual(sections.threadsWithParts(sections.topLevel[0]).map(\.id), ["t-port", "t-tb", "t-android"])
        XCTAssertEqual(sections.order, ["t-tb", "t-android", "t-port", "t-pool"], "the arrow keys go through a parent, then its parts")
    }

    func testFoldedRowsShowAPartUnderItsOpenParent() {
        let sections = build()
        XCTAssertEqual(ProjectFolds.layout(sections, open: []), ["p:tb", "p:pool", "p:orphan"], "a part folds away with its parent")
        XCTAssertEqual(ProjectFolds.layout(sections, open: ["tb"]), ["p:tb", "tb/t-tb", "p:android", "p:port", "p:pool", "p:orphan"])
        XCTAssertEqual(ProjectFolds.order(sections, open: ["tb", "port"]), ["t-tb", "t-port"])
        XCTAssertEqual(ProjectFolds.order(sections, open: ["port"]), [], "a part's threads only show with its parent open")
    }

    func testAPartIsInsideItsParentsCardWhichCountsItsDecisions() {
        let cards = ProjectCard.build(build(), decisions: true)
        XCTAssertEqual(cards.map(\.id), ["tb", "pool", "orphan"])
        XCTAssertEqual(cards[0].parts.map(\.title), ["Android app", "GPUI port"])
        XCTAssertEqual(cards[0].status.decisions, 2, "both parts' decisions")
        XCTAssertEqual(cards[0].status.open, 3)
        XCTAssertEqual(ProjectCard.matching(cards, "GPUI").map(\.id), ["tb"], "a part's name finds its parent's card")
    }

    func testACycleOrThreeLevelsHidesNothing() {
        let projects = [
            Project(id: "a", title: "A", updatedAt: now, parentId: "b"),
            Project(id: "b", title: "B", updatedAt: now, parentId: "a"),
            Project(id: "top", title: "Top", updatedAt: now),
            Project(id: "mid", title: "Mid", updatedAt: now, parentId: "top"),
            Project(id: "low", title: "Low", updatedAt: now, parentId: "mid"),
        ]
        let sections = ProjectSections.build(projects: projects, threads: [], now: now)
        XCTAssertEqual(Set(sections.topLevel.map(\.id)), ["a", "b", "top", "low"])
        XCTAssertEqual(sections.parts["top"]?.map(\.id), ["mid"])
    }

    func testAnUpdateMakesItAPartOrItsOwnAgain() {
        let project = Project(id: "port", title: "GPUI port")
        XCTAssertEqual(ProjectUpdate(parentId: "tb").applied(to: project).parentId, "tb")
        XCTAssertEqual(ProjectUpdate(parentId: "tb").params["parentId"], .string("tb"))
        var part = project
        part.parentId = "tb"
        XCTAssertNil(ProjectUpdate(parentId: "").applied(to: part).parentId)
        XCTAssertEqual(ProjectUpdate(title: "GPUI").applied(to: part).parentId, "tb", "left out, it stays a part")
        let decoded = try? JSONDecoder().decode(Project.self, from: Data(#"{"id":"p","title":"P","parentId":"tb"}"#.utf8))
        XCTAssertEqual(decoded?.parentId, "tb")
    }
}
