import Foundation
import Testing
@testable import ThreadbearKit

/// Every pairing link waits for a yes, paired or not (Daybreak finding 5:
/// an expired token left the phone unpaired, and a link then replaced the
/// saved server, token and runner keys without asking).
@MainActor @Suite struct PairingRequestTests {
    static let macKey = "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
    static let otherKey = Data(repeating: 9, count: 32).base64EncodedString()
    static let old = Pairing(deploymentURL: URL(string: "https://happy-bear-123.convex.cloud")!, token: "tb_expired")

    /// The Mac's pairing text; `token` only tells two codes apart.
    static func link(host: String, token: String = "tb_new", key: String? = nil) -> String {
        text(host: host, code: "tbp_" + Data(token.utf8).map { String(format: "%02x", $0) }.joined(), key: key)
    }

    /// What the app keeps, as plain values, to see that asking and
    /// cancelling never touch them.
    final class Saved {
        var pairing: Pairing?
        var stores: [String: MemoryRunnerKeyStore] = [:]
        func keys(_ p: Pairing) -> any RunnerKeyStore {
            let host = p.deploymentURL.host()!
            if let s = stores[host] { return s }
            let s = MemoryRunnerKeyStore()
            stores[host] = s
            return s
        }
    }

    /// The finding's first test: the old token is expired (the phone shows
    /// the pair screen, `isPaired` is false), and a link from outside still
    /// asks before anything changes, naming the new server and the key change.
    @Test func aLinkAfterTheTokenExpiredIsAskedAbout() {
        let saved = Saved()
        saved.pairing = Self.old
        let oldStore = MemoryRunnerKeyStore(["mac": Self.macKey])
        saved.stores["happy-bear-123.convex.cloud"] = oldStore
        let gate = PairingGate()
        let url = Self.link(host: "evil-fox-9.convex.cloud", key: Self.otherKey)
        #expect(.asking == gate.ask(url, from: .scanned, allowLocal: false, current: saved.pairing, keys: saved.keys))
        let request = try! #require(gate.pending)
        #expect(request.title == "Pair with evil-fox-9.convex.cloud?")
        #expect(request.message.contains("from happy-bear-123.convex.cloud to evil-fox-9.convex.cloud"))
        #expect(request.addedKeys == 1)
        #expect(request.message.contains("adds 1 Mac"))
        // Asking saved nothing: the old pairing and its keys are as they were,
        // and the new server's store is still empty.
        #expect(saved.pairing == Self.old)
        #expect(oldStore.key(for: "mac") == Self.macKey)
        #expect(saved.stores["evil-fox-9.convex.cloud"]?.key(for: "mac") == nil)
    }

    /// The first pairing is asked about too.
    /// Two self-hosted backends on one Mac differ only by port: a link to
    /// the other one says it moves the phone, naming both with their ports.
    @Test func anotherPortIsAnotherServer() throws {
        let current = Pairing(deploymentURL: URL(string: "https://mac.tail1.ts.net:3210")!, token: "tb_old")
        let link = Self.link(host: "mac.tail1.ts.net:8443")
        let request = try #require(PairingRequest(text: link, allowLocal: false, current: current, keys: { _ in MemoryRunnerKeyStore() }))
        #expect(request.replacesHost == "mac.tail1.ts.net:3210")
        #expect(!request.samePairedServer)
        #expect(request.title == "Pair with mac.tail1.ts.net:8443?")
        // A cloud address has no port, so it's named as it always was.
        #expect(Pairing.serverName(URL(string: "https://happy-bear-123.convex.cloud")!) == "happy-bear-123.convex.cloud")
        // Nor does one saved with a port, or the simulator's local backend: their Keychain names stay as they were.
        #expect(Pairing.serverName(URL(string: "https://happy-bear-123.convex.cloud:443")!) == "happy-bear-123.convex.cloud")
        #expect(Pairing.serverName(URL(string: "http://127.0.0.1:3210")!) == "127.0.0.1")
    }

    @Test func theFirstPairingIsAskedAbout() throws {
        let gate = PairingGate()
        let saved = Saved()
        #expect(.asking == gate.ask(Self.link(host: "happy-bear-123.convex.cloud", key: Self.macKey), from: .scanned, allowLocal: false, current: nil, keys: saved.keys))
        let request = try #require(gate.pending)
        #expect(request.message.hasPrefix("This link pairs the phone with the server at happy-bear-123.convex.cloud."))
        #expect(request.replacesHost == nil)
    }

    /// The finding's second test: Cancel keeps the old server, token and keys,
    /// and leaves nothing to pair with afterwards.
    @Test func cancelKeepsTheOldPairingAndKeys() throws {
        let saved = Saved()
        saved.pairing = Self.old
        let store = MemoryRunnerKeyStore(["mac": Self.macKey])
        saved.stores["happy-bear-123.convex.cloud"] = store
        let gate = PairingGate()
        // Same server, a new token and a different key for the same Mac.
        let url = Self.link(host: "happy-bear-123.convex.cloud", token: "tb_attacker", key: Self.otherKey)
        #expect(.asking == gate.ask(url, from: .scanned, allowLocal: false, current: saved.pairing, keys: saved.keys))
        #expect(gate.pending?.changedKeys == 1)
        #expect(gate.pending?.message.contains("new token for happy-bear-123.convex.cloud") == true)
        #expect(gate.pending?.message.contains("changes the key of 1 Mac") == true)
        let shown = try #require(gate.pending)
        gate.cancel()
        #expect(gate.pending == nil)
        var paired = false
        #expect(gate.accept(shown) { _ in paired = true } == nil)
        #expect(!paired)
        #expect(saved.pairing == Self.old)
        #expect(saved.pairing?.token == "tb_expired")
        #expect(store.key(for: "mac") == Self.macKey)
    }

    /// Codex's P1: tapping Pair dismisses the alert, whose binding cancels
    /// the gate straight after the button's action. The yes is taken in the
    /// action itself, so it still pairs, with the link that was shown, once.
    @Test func yesPairsEvenThoughTheAlertCancelsAfterIt() async throws {
        let gate = PairingGate()
        let url = Self.link(host: "happy-bear-123.convex.cloud")
        gate.ask(url, from: .scanned, allowLocal: false, current: nil, keys: Saved().keys)
        let shown = try #require(gate.pending)
        var paired: [String] = []
        let task = gate.accept(shown) { paired.append($0.text) }
        gate.cancel() // the alert going away
        await task?.value
        #expect(paired == [url])
        #expect(gate.accept(shown) { paired.append($0.text) } == nil)
        #expect(paired == [url])
    }

    /// Codex's P2: a second link while the first is on screen is turned
    /// away, never swapped in under the yes Mike gives the first.
    @Test func aSecondLinkWhileAskingIsTurnedAway() async throws {
        let gate = PairingGate()
        let first = Self.link(host: "happy-bear-123.convex.cloud")
        gate.ask(first, from: .scanned, allowLocal: false, current: nil, keys: Saved().keys)
        let shown = try #require(gate.pending)
        #expect(.busy == gate.ask(Self.link(host: "evil-fox-9.convex.cloud", key: Self.otherKey), from: .scanned, allowLocal: false, current: nil, keys: Saved().keys))
        #expect(gate.pending?.id == shown.id)
        var paired: [String] = []
        await gate.accept(shown) { paired.append($0.text) }?.value
        #expect(paired == [first])
    }

    /// A yes for a request that isn't the one waiting (the same link opened
    /// again makes a new request) pairs nothing.
    @Test func aYesForAnotherRequestPairsNothing() throws {
        let gate = PairingGate()
        let url = Self.link(host: "happy-bear-123.convex.cloud")
        let stale = try #require(PairingRequest(text: url, allowLocal: false, current: nil, keys: Saved().keys))
        gate.ask(url, from: .scanned, allowLocal: false, current: nil, keys: Saved().keys)
        var paired = false
        #expect(gate.accept(stale) { _ in paired = true } == nil)
        #expect(!paired)
        #expect(gate.pending != nil)
    }

    @Test func sameKeysSaySo() throws {
        let saved = Saved()
        saved.stores["happy-bear-123.convex.cloud"] = MemoryRunnerKeyStore(["mac": Self.macKey])
        let request = try #require(PairingRequest(text: Self.link(host: "happy-bear-123.convex.cloud", key: Self.macKey),
                                                  allowLocal: false, current: Self.old, keys: saved.keys))
        #expect(!request.changesKeys)
        #expect(request.message.contains("stay the same"))
    }

    @Test func notAPairingLinkIsntHeld() {
        let gate = PairingGate()
        #expect(.notPairing == gate.ask(URL(string: "threadbear://pair?url=http://evil.example.com&token=t")!, from: .scanned, allowLocal: false, current: nil, keys: Saved().keys))
        #expect(.notPairing == gate.ask(URL(string: "threadbear://quickask?url=https://evil.example.com&token=t")!, from: .scanned, allowLocal: false, current: nil, keys: Saved().keys))
        #expect(gate.pending == nil)
    }

    // Daybreak 127 (15 Nov 2026): any app can claim the threadbear scheme,
    // so a pairing link another app, a page or the Camera opened the app
    // with may have reached that app too. Never taken from there, with or
    // without a token in it, paired or not; the app's scanner still is.
    @Test func aPairingLinkOpenedFromOutsideIsRefusedNotAsked() {
        let saved = Saved()
        let gate = PairingGate()
        let legacy = Pairing(deploymentURL: URL(string: "https://happy-bear-123.convex.cloud")!, token: "tb_t", runnerKeys: ["mac": Self.macKey]).link
        let text = Self.text(host: "happy-bear-123.convex.cloud", key: Self.macKey)
        #expect(.opened == gate.ask(legacy, from: .opened, allowLocal: false, current: nil, keys: saved.keys))
        #expect(.opened == gate.ask(legacy, from: .opened, allowLocal: false, current: Self.old, keys: saved.keys))
        #expect(.opened == gate.ask(text, from: .opened, allowLocal: false, current: nil, keys: saved.keys))
        #expect(gate.pending == nil)
        #expect(saved.stores["happy-bear-123.convex.cloud"]?.key(for: "mac") == nil)
        // Something else opened from outside is just not a pairing.
        #expect(.notPairing == gate.ask(URL(string: "threadbear://open")!, from: .opened, allowLocal: false, current: nil, keys: saved.keys))
        // An older Mac's token link isn't taken from the scanner or the
        // clipboard either: the token may have reached another app already,
        // and a clipboard app could swap one in for the Mac's code.
        #expect(.oldLink == gate.ask(legacy, from: .scanned, allowLocal: false, current: nil, keys: saved.keys))
        #expect(.oldLink == gate.ask(legacy.absoluteString, from: .pasted, allowLocal: false, current: nil, keys: saved.keys))
        #expect(gate.pending == nil)
        // The Mac's code from the scanner or the clipboard is asked about.
        #expect(.asking == gate.ask(text, from: .scanned, allowLocal: false, current: nil, keys: saved.keys))
        gate.cancel()
        #expect(.asking == gate.ask(text, from: .pasted, allowLocal: false, current: nil, keys: saved.keys))
    }

    /// The Mac's pairing text (runner/src/devices.ts, pairingText).
    static func text(host: String, scheme: String = "https", code: String = "tbp_" + String(repeating: "ab", count: 24), key: String? = nil, extra: String = "") -> String {
        var q = "url=" + "\(scheme)://\(host)".addingPercentEncoding(withAllowedCharacters: .alphanumerics)! + "&code=" + code + extra
        if let key { q += "&key=mac%3A" + key.addingPercentEncoding(withAllowedCharacters: .alphanumerics)! }
        return Pairing.textPrefix + Data(q.utf8).base64EncodedString()
            .replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    @Test func theMacsPairingTextCarriesACodeAndIsAskedAbout() throws {
        let gate = PairingGate()
        let text = Self.text(host: "happy-bear-123.convex.cloud", key: Self.macKey)
        #expect(.asking == gate.ask(text, from: .scanned, allowLocal: false, current: nil, keys: Saved().keys))
        let request = try #require(gate.pending)
        #expect(request.pairing.code == "tbp_" + String(repeating: "ab", count: 24))
        #expect(request.pairing.token.isEmpty)
        #expect(request.pairing.runnerKeys == ["mac": Self.macKey])
        #expect(request.pairing.deploymentURL.absoluteString == "https://happy-bear-123.convex.cloud")
        // Not a link: nothing the system would hand to an app.
        #expect(URL(string: text)?.scheme == nil)
    }

    @Test func aBadPairingTextIsNotAPairing() {
        let host = "happy-bear-123.convex.cloud"
        for bad in [Self.text(host: host, code: "tb_abc"), Self.text(host: host, code: "tbp_"), Self.text(host: host, code: "tbp_zz"),
                    Self.text(host: host, extra: "&token=tb_x"), Pairing.textPrefix, Pairing.textPrefix + "!!", Pairing.textPrefix + "%%%",
                    Pairing.textPrefix + Data("url=%ZZ&code=tbp_ab".utf8).base64EncodedString(),
                    Pairing.textPrefix + String(repeating: "A", count: 5000), Self.text(host: "evil.example.com", scheme: "http")] {
            #expect(Pairing.parse(bad) == nil, "took \(bad)")
        }
    }

    final class ClaimTransport: ConvexTransport, @unchecked Sendable {
        var calls: [(String, [String: JSONValue])] = []
        var answer: Result<String, TransportError>
        init(_ answer: Result<String, TransportError>) { self.answer = answer }
        func mutation(_ name: String, _ args: [String: JSONValue]) async throws -> Data {
            calls.append((name, args))
            return Data(try answer.get().utf8)
        }
        func query(_ name: String, _ args: [String: JSONValue]) async throws -> Data { throw TransportError.server("no") }
        func subscribe(_ name: String, _ args: [String: JSONValue]) -> AsyncThrowingStream<Data, Error> { AsyncThrowingStream { $0.finish() } }
        func connectionStates() -> AsyncStream<Bool> { AsyncStream { $0.finish() } }
    }

    @Test func aCodeIsTradedForItsTokenOnce() async throws {
        let offered = try #require(Pairing.parse(Self.text(host: "happy-bear-123.convex.cloud", key: Self.macKey)))
        let transport = ClaimTransport(.success(#"{"token":"tb_real","expiresAt":"2027-02-13T00:00:00.000Z"}"#))
        let paired = try await offered.claimed(with: transport)
        #expect(paired.token == "tb_real")
        #expect(paired.code == nil)
        #expect(paired.runnerKeys == ["mac": Self.macKey])
        #expect(transport.calls.map(\.0) == ["tokens:claim"])
        #expect(transport.calls.first?.1["code"] == .string(offered.code!))
        // A pairing that has its token already isn't traded.
        _ = try await paired.claimed(with: transport)
        #expect(transport.calls.count == 1)
        // The server refusing (used, expired) or answering nonsense pairs nothing.
        await #expect(throws: TransportError.self) { try await offered.claimed(with: ClaimTransport(.failure(.server("That pairing code has expired or was already used.")))) }
        await #expect(throws: TransportError.self) { try await offered.claimed(with: ClaimTransport(.success(#"{"token":"nope"}"#))) }
    }
}
