import Foundation
import Testing
@testable import QuickAskCore

struct SourcesHeaderTests {
    private func base64url(_ json: String) -> String {
        Data(json.utf8).base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    @Test func decodesTheContractShape() {
        let header = base64url("""
        [{"title":"Pebble pricing","threadId":"t1","date":"2026-10-22","url":"https://example.com/t/1"},
         {"title":"Recipe Box native","threadId":"t2","date":"2026-10-21","url":"https://example.com/t/2"}]
        """)
        #expect(SourcesHeader.decode(header) == [
            AnswerSource(title: "Pebble pricing", threadId: "t1", date: "2026-10-22", url: "https://example.com/t/1"),
            AnswerSource(title: "Recipe Box native", threadId: "t2", date: "2026-10-21", url: "https://example.com/t/2"),
        ])
    }

    @Test func handlesUnicodeAndUrlSafeCharacters() {
        // "ü" and "?" make the standard base64 contain + or /, which base64url swaps for - and _
        let sources = [AnswerSource(title: "Grüße? über >>> ~~~", threadId: "x", date: "2026-01-02", url: "https://example.com/?a=1&b=2")]
        let header = SourcesHeader.encode(sources)
        #expect(!header.contains("=") && !header.contains("+") && !header.contains("/"))
        #expect(SourcesHeader.decode(header) == sources)
    }

    @Test func allPaddingLengthsDecode() {
        for title in ["a", "ab", "abc", "abcd"] {
            let sources = [AnswerSource(title: title, threadId: "t", date: "2026-10-23", url: "https://e.com")]
            #expect(SourcesHeader.decode(SourcesHeader.encode(sources)) == sources)
        }
    }

    @Test func emptyMissingAndMalformed() {
        #expect(SourcesHeader.decode(nil) == [])
        #expect(SourcesHeader.decode("") == [])
        #expect(SourcesHeader.decode(base64url("[]")) == [])
        #expect(SourcesHeader.decode("not base64!!") == [])
        #expect(SourcesHeader.decode(base64url("{\"title\":\"not an array\"}")) == [])
    }

    @Test func dropsEntriesWithoutTitleOrUrlAndKeepsAtMostThree() {
        let header = base64url("""
        [{"title":"","url":"https://e.com/0"},{"title":"No URL"},{"title":"One","url":"https://e.com/1"},
         {"title":"Two","url":"https://e.com/2","threadId":"t2"},{"title":"Three","url":"https://e.com/3"},
         {"title":"Four","url":"https://e.com/4"}]
        """)
        #expect(SourcesHeader.decode(header).map(\.title) == ["One", "Two", "Three"])
    }

    @Test func displayDate() {
        let source = AnswerSource(title: "t", threadId: "", date: "2026-10-23", url: "https://e.com")
        #expect(source.displayDate.contains("2026"))
        #expect(AnswerSource(title: "t", threadId: "", date: "yesterday", url: "https://e.com").displayDate == "yesterday")
    }
}

struct ServerTimingTests {
    @Test func parsesTheContractHeader() {
        #expect(ServerTiming.parse("auth;dur=12, retrieve;dur=340.5") == [
            ServerTimingEntry(name: "auth", durationMs: 12),
            ServerTimingEntry(name: "retrieve", durationMs: 340.5),
        ])
    }

    @Test func toleratesDescriptionsQuotesAndMissingDurations() {
        #expect(ServerTiming.parse("cache;desc=\"Cache Read\";dur=\"23.2\",total, ;dur=4") == [
            ServerTimingEntry(name: "cache", durationMs: 23.2),
            ServerTimingEntry(name: "total", durationMs: nil),
        ])
        #expect(ServerTiming.parse(nil) == [])
    }

    @Test func dropsCloudflaresEntries() {
        // What prod's header looks like once Cloudflare has added its own
        let header = "auth;dur=123, retrieve;dur=451, model;dur=980, cfExtPri, cfL4;desc=\"?proto=TCP&rtt=21\", cfEdge;dur=4,cfOrigin;dur=1500"
        #expect(ServerTiming.ours(ServerTiming.parse(header)) == [
            ServerTimingEntry(name: "auth", durationMs: 123),
            ServerTimingEntry(name: "retrieve", durationMs: 451),
            ServerTimingEntry(name: "model", durationMs: 980),
        ])
    }

    @Test func onlyCloudflaresStyleOfNameCounts() {
        #expect(ServerTiming.isCloudflare("cfExtPri"))
        #expect(ServerTiming.isCloudflare("cfL4"))
        #expect(ServerTiming.isCloudflare("cf4"))
        #expect(!ServerTiming.isCloudflare("cf"))
        #expect(!ServerTiming.isCloudflare("cfg"))
        #expect(!ServerTiming.isCloudflare("conf"))
        #expect(!ServerTiming.isCloudflare("retrieve"))
    }
}
