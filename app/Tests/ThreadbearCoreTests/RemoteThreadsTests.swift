import Foundation
import XCTest
@testable import ThreadbearCore

/// Other runners' threads (docs/protocol.md, "Other runners' threads"): the
/// list and its rows as sidebar summaries.
final class RemoteThreadsTests: XCTestCase {
    private func decode(_ object: [String: Any]) throws -> RemoteThreads {
        try JSONDecoder().decode(RemoteThreads.self, from: Data(E.json(object).utf8))
    }

    /// Mike's runner order (backend API 58), the same one the phone sets,
    /// for Settings > Runners. An older runner has none: nil, not empty.
    func testCarriesTheRunnerOrder() throws {
        XCTAssertEqual(try decode(["runners": [], "threads": [], "order": ["r_m5", "r_bruce"]]).order, ["r_m5", "r_bruce"])
        XCTAssertNil(try decode(["runners": [], "threads": []]).order)
        XCTAssertNil(try decode(["runners": [], "threads": [], "order": "nonsense"]).order)
    }

    /// Daybreak 87: a compromised extra Mac's row under an id with shell or
    /// markdown in it never reaches the sidebar, so never Copy debug info.
    func testARowWhoseIdIsntAThreadIdsShapeIsLeftOut() throws {
        let ids = ["remote$(printf TB_ID_EXECUTED >&2)", "deadbeef) ![local](/tmp/private.png", "../x", "é", String(repeating: "a", count: 65), "0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e", "demo_upload"]
        let list = try decode(["runners": [], "threads": ids.map { ["runnerId": "r_bruce", "threadId": $0] }])
        XCTAssertEqual(list.threads.map(\.threadId), ["0cf430ac-f429-4b2e-9c1d-2f6a8b7c9d0e", "demo_upload"])
    }

    func testDecodesTolerantly() throws {
        let list = try decode([
            "runners": [
                // An older runner still sends the direct link's rpcUrl and
                // tokenFile: ignored like any other key this app doesn't know.
                ["id": "r_bruce", "name": "bruce", "icon": "macmini", "online": true, "lastSeenAt": "2026-10-29T10:00:00Z",
                 "rpcUrl": "wss://bruce.example.ts.net/rpc", "tokenFile": "/tmp/tb/secrets/runners/r_bruce.token", "extra": 1],
                // Nothing but an id: named by it, offline.
                ["id": "r_studio", "name": ""],
                // No id: dropped, not the whole list.
                ["name": "nobody"],
            ],
            "threads": [
                ["runnerId": "r_bruce", "threadId": "t1", "title": "Backup", "cwd": "/Users/bruce/misc", "accountId": "claude-main",
                 "provider": "claude", "model": "opus", "status": "running", "needsYou": false, "unread": true, "preview": "Going",
                 "createdAt": "2026-10-29T09:00:00Z", "lastActivityAt": "2026-10-29T09:30:00Z", "future": ["x": 1]],
                ["runnerId": "r_studio", "threadId": "t2", "status": "something-new"],
                ["runnerId": "r_bruce"],
            ],
        ])
        XCTAssertEqual(list.runners.map(\.id), ["r_bruce", "r_studio"])
        let bruce = try XCTUnwrap(list.runner("r_bruce"))
        XCTAssertEqual(bruce.name, "bruce")
        XCTAssertEqual(bruce.icon, "macmini")
        XCTAssertTrue(bruce.online)
        XCTAssertNotNil(bruce.lastSeenAt)
        let studio = try XCTUnwrap(list.runner("r_studio"))
        XCTAssertEqual(studio.name, "r_studio")
        XCTAssertFalse(studio.online)

        XCTAssertEqual(list.threads.map(\.threadId), ["t1", "t2"])
        XCTAssertEqual(list.threads[0].status, .running)
        XCTAssertEqual(list.threads[0].model, "opus")
        XCTAssertEqual(list.threads[1].status, .idle, "a status this app doesn't know reads as idle")
        XCTAssertEqual(list.threads[1].provider, "claude")
        XCTAssertEqual(list.threads[1].lastActivityAt, list.threads[1].createdAt)

        XCTAssertEqual(try decode([:]), RemoteThreads(), "an empty answer is none")
    }

    func testRowsBecomeSummariesMarkedWithTheirRunner() throws {
        let created = Date(timeIntervalSince1970: 1_800_000_000)
        let unread = RemoteThread(runnerId: "r_bruce", threadId: "t1", title: "Backup", cwd: "/Users/bruce/misc", model: "opus",
                                  needsYou: true, unread: true, preview: "Done", createdAt: created, lastActivityAt: created.addingTimeInterval(60))
        let summary = unread.summary
        XCTAssertEqual(summary.id, "t1")
        XCTAssertEqual(summary.runnerId, "r_bruce")
        XCTAssertTrue(summary.isRemote)
        XCTAssertEqual(summary.title, "Backup")
        XCTAssertEqual(summary.model, "opus")
        XCTAssertEqual(summary.lastActivityAt, created.addingTimeInterval(60))
        XCTAssertGreaterThan(summary.lastSeq, summary.lastSeenSeq, "unread shows as unread")
        XCTAssertTrue(ThreadRowLook(summary).unread)

        let read = RemoteThread(runnerId: "r_bruce", threadId: "t2").summary
        XCTAssertEqual(read.lastSeq, read.lastSeenSeq)

        let untilRestart = RemoteThread(runnerId: "r_bruce", threadId: "t3", snoozedUntil: ISO8601.parse("9999-12-31T00:00:00.000Z")).summary
        XCTAssertTrue(untilRestart.snoozedUntilRestart)
        let untilMonday = RemoteThread(runnerId: "r_bruce", threadId: "t4", snoozedUntil: Date().addingTimeInterval(86_400)).summary
        XCTAssertFalse(untilMonday.snoozedUntilRestart)

        // Never on the wire: a runner's summaries are all its own.
        let encoded = try JSONSerialization.jsonObject(with: JSONEncoder().encode(summary)) as? [String: Any]
        XCTAssertNil(encoded?["runnerId"])
        XCTAssertNil(try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(summary)).runnerId)
    }

    func testRemoteThreadsSortIntoSectionsLikeLocalOnes() {
        let now = Date()
        let local = ThreadSummary(id: "local", title: "Here", status: .idle, needsYou: true, lastActivityAt: now.addingTimeInterval(-600))
        let list = RemoteThreads(runners: [RemoteRunner(id: "r", name: "bruce")], threads: [
            RemoteThread(runnerId: "r", threadId: "needy", needsYou: true, unread: true, lastActivityAt: now.addingTimeInterval(-60)),
            RemoteThread(runnerId: "r", threadId: "busy", status: .running, lastActivityAt: now),
            RemoteThread(runnerId: "r", threadId: "idle", lastActivityAt: now.addingTimeInterval(-7200)),
            RemoteThread(runnerId: "r", threadId: "away", snoozedUntil: now.addingTimeInterval(3600)),
            RemoteThread(runnerId: "r", threadId: "gone", archivedAt: now),
        ])
        let sections = ThreadSections.build([local] + list.summaries, now: now)
        XCTAssertEqual(sections.inbox.map(\.id), ["needy", "local", "idle"], "needing Mike first, newest first within that")
        XCTAssertEqual(sections.running.map(\.id), ["busy"])
        XCTAssertEqual(sections.hibernating.map(\.id), ["away"])
        XCTAssertEqual(sections.archived.map(\.id), ["gone"])
    }

    func testListFallsBackToNoneOnAnOlderRunner() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in
            [E.json(["jsonrpc": "2.0", "id": request["id"]!, "error": ["code": -32601, "message": "Unknown method remoteThreads.list"]])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        let list = try await client.remoteThreads()
        XCTAssertEqual(list, RemoteThreads())
        XCTAssertEqual(transport.sent.first?["method"] as? String, "remoteThreads.list")
    }

    func testListAndUpdatedNotification() async throws {
        let transport = FakeTransport()
        // As text, so the responder closure can hold it.
        let answer = E.json([
            "runners": [["id": "r_bruce", "name": "bruce", "online": true]],
            "threads": [["runnerId": "r_bruce", "threadId": "t1", "title": "Backup"]],
        ])
        transport.onSend = { request in ["{\"jsonrpc\":\"2.0\",\"id\":\(request["id"] as! Int),\"result\":\(answer)}"] }
        let client = RPCClient(transport: transport)
        try await client.start()
        let list = try await client.remoteThreads()
        XCTAssertEqual(list.threads.first?.title, "Backup")

        var iterator = client.notifications.makeAsyncIterator()
        transport.push("{\"jsonrpc\":\"2.0\",\"method\":\"remoteThreads.updated\",\"params\":\(answer)}")
        guard case .remoteThreadsUpdated(let updated)? = await iterator.next() else { return XCTFail("no remoteThreads.updated") }
        XCTAssertEqual(updated, list)
    }

    func testUnreachableSaysWhy() {
        let gone = RemoteUnreachable(runnerId: "r_bruce", runnerName: "bruce", online: false, reason: .unknownRunner)
        XCTAssertEqual(gone.headline, "Can't reach bruce from this Mac.")
        XCTAssertEqual(gone.localizedDescription, "Its runner isn't listed any more.")
    }

    func testAnotherRunnersThreadCarriesItsBylineToTheRow() throws {
        let json = #"{"runnerId":"r","threadId":"t","title":"t","cwd":"/x/misc","status":"idle","startedBy":{"kind":"tool","label":"Sentry","why":"2 events"}}"#
        let summary = try JSONDecoder().decode(RemoteThread.self, from: Data(json.utf8)).summary
        XCTAssertEqual(ThreadByline.line(summary), .init(symbol: ThreadByline.toolSymbol, text: "misc", tip: "Started by Sentry (2 events)"))
        XCTAssertNil(RemoteThread(runnerId: "r", threadId: "t2").summary.startedBy)
    }

    func testAnotherRunnersThreadNamesItsPlaceWithWhatMikeAskedAsItsTip() throws {
        let json = #"{"runnerId":"r","threadId":"t","title":"t","cwd":"/x/misc","status":"idle","preview":"Backup done","asked":"try the GPU route"}"#
        let summary = try JSONDecoder().decode(RemoteThread.self, from: Data(json.utf8)).summary
        XCTAssertEqual(ProjectMark.place(summary), "misc")
        XCTAssertEqual(ProjectMark.lineTip(summary), "You: try the GPU route")
    }

    func testAToolResultSaysWhereItsOutputCanBeAskedFor() throws {
        let json = #"{"itemId":"relay-5","ok":true,"text":"The output is kept on bruce.","truncated":false,"images":[],"heldBack":{"seq":5}}"#
        let held = try JSONDecoder().decode(ToolResult.self, from: Data(json.utf8))
        XCTAssertEqual(held.heldBackSeq, 5)
        let plain = try JSONDecoder().decode(ToolResult.self, from: Data(#"{"itemId":"i","ok":true,"text":"x","truncated":false,"images":[]}"#.utf8))
        XCTAssertNil(plain.heldBackSeq)
    }
}
