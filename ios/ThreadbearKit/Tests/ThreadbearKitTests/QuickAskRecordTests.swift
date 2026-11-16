import Foundation
import Synchronization
import Testing
@testable import ThreadbearKit

@Suite struct QuickAskRecordTests {
    @Test func clipsToWhatTheMutationTakes() {
        let sources = (0..<12).map { QuickAskRecord.Source(title: "t\($0)", url: "https://x/\($0)") } + [.init(title: "none", url: "")]
        let ask = QuickAskRecord(question: "  " + String(repeating: "q", count: 2100), answer: String(repeating: "a", count: 20100) + "\n",
                                 sources: sources, askedAt: .now)
        #expect(ask.question.count == 2000)
        #expect(ask.answer.count == 20000)
        #expect(ask.sources.count == 10)
        #expect(ask.isWorthKeeping)
        #expect(!QuickAskRecord(question: "Why?", answer: "  ", sources: [], askedAt: .now).isWorthKeeping)
    }

    @Test func sendsPhoneQuickAsk() async throws {
        let transport = RecordingTransport()
        let backend = ConvexBackend(transport: transport, token: "tb_x")
        let at = Date(timeIntervalSince1970: 1_800_000_000)
        let id = try await backend.quickAsk(.init(question: "Which drive?", answer: "CannMedia.",
                                                  sources: [.init(title: "Mac mini move", url: "https://a/b")], askedAt: at))
        #expect(id == "cmd_1")
        let (name, args) = try #require(transport.calls.withLock { $0.first })
        #expect(name == "phone:quickAsk")
        #expect(args["token"] == .string("tb_x"))
        #expect(args["question"] == .string("Which drive?"))
        #expect(args["answer"] == .string("CannMedia."))
        #expect(args["askedAt"] == .number(1_800_000_000_000))
        #expect(args["sources"] == .array([.object(["title": .string("Mac mini move"), "url": .string("https://a/b")])]))
    }

    @Test func fakeMakesAThreadWithTheExchange() async throws {
        let backend = FakeBackend(turnDelay: .milliseconds(20))
        let commandId = try await backend.quickAsk(.init(question: "Which drive?", answer: "CannMedia.", sources: [], askedAt: .now))
        var commands = backend.command(commandId).makeAsyncIterator()
        let status = try #require(try await commands.next() ?? nil)
        let threadId = try #require(status.threadId)
        var threads = backend.thread(threadId).makeAsyncIterator()
        let detail = try #require(try await threads.next() ?? nil)
        #expect(detail.messages.map(\.text) == ["Which drive?", "CannMedia."])
    }
}

final class RecordingTransport: ConvexTransport {
    let calls = Mutex<[(String, [String: JSONValue])]>([])
    func mutation(_ name: String, _ args: [String: JSONValue]) async throws -> Data {
        calls.withLock { $0.append((name, args)) }
        return Data("\"cmd_1\"".utf8)
    }
    func query(_ name: String, _ args: [String: JSONValue]) async throws -> Data { Data("null".utf8) }
    func subscribe(_ name: String, _ args: [String: JSONValue]) -> AsyncThrowingStream<Data, Error> { AsyncThrowingStream { $0.finish() } }
    func connectionStates() -> AsyncStream<Bool> { AsyncStream { $0.yield(true) } }
}
