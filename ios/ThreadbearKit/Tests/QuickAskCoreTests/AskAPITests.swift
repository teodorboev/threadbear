import Foundation
import Testing
@testable import QuickAskCore

struct AskAPITests {
    private let perth = TimeZone(identifier: "Australia/Perth")!
    private let now = Date(timeIntervalSince1970: 1_792_735_560) // 2026-10-23T06:06:00Z

    private func json(_ data: Data) throws -> [String: Any] {
        try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    @Test func bodyMatchesTheContract() throws {
        let history = [Exchange(question: "What did I do yesterday?", answer: "You worked on Pebble.")]
        let body = AskAPI.body(question: "  And the day before?  ", history: history, timeZone: perth, now: now)
        let object = try json(try AskAPI.encode(body))

        #expect(object["question"] as? String == "And the day before?")
        #expect(object["timezone"] as? String == "Australia/Perth")
        #expect(object["clientTime"] as? String == "2026-10-23T14:06:00+08:00")
        let sent = try #require(object["history"] as? [[String: String]])
        #expect(sent == [["question": "What did I do yesterday?", "answer": "You worked on Pebble."]])
        #expect(Set(object.keys) == ["question", "history", "timezone", "clientTime"])
    }

    @Test func historyIsLeftOutWhenEmpty() throws {
        let body = AskAPI.body(question: "Hello there", history: [], timeZone: perth, now: now)
        let object = try json(try AskAPI.encode(body))
        #expect(object["history"] == nil)
    }

    @Test func historyKeepsTheLastThreeOldestFirstAndSkipsEmptyAnswers() {
        let history = (1...5).map { Exchange(question: "Q\($0)", answer: $0 == 4 ? " " : "A\($0)") }
        let body = AskAPI.body(question: "Q6", history: history, timeZone: perth, now: now)
        #expect(body.history?.map(\.question) == ["Q2", "Q3", "Q5"])
    }

    @Test func clientTimeUsesTheGivenZone() {
        #expect(AskAPI.isoTime(now, in: TimeZone(identifier: "UTC")!) == "2026-10-23T06:06:00Z")
        #expect(AskAPI.isoTime(now, in: TimeZone(identifier: "Europe/London")!) == "2026-10-23T07:06:00+01:00")
    }

    @Test func askRequestHeaders() throws {
        let body = AskAPI.body(question: "Hi there", history: [], timeZone: perth, now: now)
        let request = try AskAPI.askRequest(baseURL: URL(string: "https://happy-bear-123.convex.site")!,
                                            token: "aa_0123456789abcdef_secret", body: body)
        #expect(request.url?.absoluteString == "https://happy-bear-123.convex.site/api/quick/ask")
        #expect(request.httpMethod == "POST")
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer aa_0123456789abcdef_secret")
        #expect(request.value(forHTTPHeaderField: "Content-Type") == "application/json")
        #expect(request.value(forHTTPHeaderField: "Accept") == "application/x-ndjson")
        #expect(request.timeoutInterval == 75)
        #expect(try AskBody.decode(request.httpBody) == body)
    }

    @Test func pingRequest() {
        let request = AskAPI.pingRequest(baseURL: URL(string: "http://127.0.0.1:8787")!)
        #expect(request.url?.absoluteString == "http://127.0.0.1:8787/api/quick/ping")
        #expect(request.httpMethod == "GET")
        #expect(request.value(forHTTPHeaderField: "Authorization") == nil)
    }

    @Test func baseURLNormalising() {
        #expect(AskAPI.normalizeBaseURL(" https://calm-otter-456.convex.site/ ")?.absoluteString == "https://calm-otter-456.convex.site")
        #expect(AskAPI.normalizeBaseURL("calm-otter-456.convex.site")?.absoluteString == "https://calm-otter-456.convex.site")
        #expect(AskAPI.normalizeBaseURL("http://127.0.0.1:8787")?.absoluteString == "http://127.0.0.1:8787")
        #expect(AskAPI.normalizeBaseURL("") == nil)
        #expect(AskAPI.normalizeBaseURL("ftp://example.com") == nil)
    }

    @Test func tokenShape() {
        #expect(AskAPI.looksLikeToken("aa_0123456789abcdef_s3cr3t-Value"))
        #expect(!AskAPI.looksLikeToken("aa_0123_secret"))
        #expect(!AskAPI.looksLikeToken("Bearer aa_0123456789abcdef_x"))
    }

    @Test func failuresInPlainWords() {
        let body = Data(#"{"error":"Slow down"}"#.utf8)
        #expect(AskFailure.from(status: 401, body: Data()) == .unauthorized)
        #expect(AskFailure.from(status: 429, body: body) == .rateLimited("Slow down"))
        #expect(AskFailure.from(status: 400, body: Data(#"{"error":"question is required"}"#.utf8)) == .badRequest("question is required"))
        #expect(AskFailure.from(status: 503, body: Data("<html>".utf8)) == .server(status: 503, message: nil))
        #expect(AskFailure.unauthorized.message.contains("Settings"))
        #expect(AskFailure.unauthorized.needsSettings)
        #expect(AskFailure.from(URLError(.notConnectedToInternet)) == .offline)
        #expect(AskFailure.from(URLError(.timedOut)) == .timedOut)
        #expect(AskFailure.from(URLError(.cannotFindHost)) == .unreachable)
    }
}

private extension AskBody {
    static func decode(_ data: Data?) throws -> AskBody {
        try JSONDecoder().decode(AskBody.self, from: try #require(data))
    }
}

struct QuickAskLinkTests {
    private func link(_ string: String) -> Result<QuickAskLink, QuickAskLink.Refusal> {
        QuickAskLink.parse(URL(string: string)!)
    }

    @Test func anHttpsLinkIsTaken() throws {
        let parsed = try link("threadbear://quickask?url=https%3A%2F%2Fmac-mini.tail1234.ts.net%3A9443&token=aa_0123456789abcdef_x").get()
        #expect(parsed.server == "https://mac-mini.tail1234.ts.net:9443")
        #expect(parsed.token == "aa_0123456789abcdef_x")
        #expect(parsed.hostName == "mac-mini.tail1234.ts.net:9443")
        // No scheme is https.
        #expect(try link("threadbear://quickask?url=mac-mini.example.ts.net&token=t").get().server == "mac-mini.example.ts.net")
    }

    @Test func plainHttpAndBrokenLinksAreRefused() {
        #expect(link("threadbear://quickask?url=http%3A%2F%2Fevil.example&token=t") == .failure(.unusable))
        #expect(link("threadbear://quickask?url=https%3A%2F%2Fok.example") == .failure(.unusable))
        #expect(link("threadbear://quickask?url=https%3A%2F%2Fok.example&token=") == .failure(.unusable))
        #expect(link("threadbear://pair?url=https%3A%2F%2Fok.example&token=t") == .failure(.notQuickAsk))
    }

    @Test func onlyAnotherHostCountsAsAChange() throws {
        let parsed = try link("threadbear://quickask?url=https%3A%2F%2Fmac-mini.example.ts.net%3A9443&token=t").get()
        // The first link counts as a change: it's asked about like any other.
        #expect(parsed.changesServer(from: nil))
        #expect(!parsed.changesServer(from: URL(string: "https://Mac-Mini.example.ts.net:9443/")!))
        #expect(parsed.changesServer(from: URL(string: "https://mac-mini.example.ts.net")!))
        #expect(parsed.changesServer(from: URL(string: "https://evil.example:9443")!))
    }
}
