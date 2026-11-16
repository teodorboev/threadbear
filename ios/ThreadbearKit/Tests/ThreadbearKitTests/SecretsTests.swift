import CryptoKit
import Foundation
import Testing
@testable import ThreadbearKit

@Suite struct SecretsTests {
    // The vector from the runner's side (runner/src/secrets.ts), so the two
    // seal and open the same bytes.
    static let recipientPrivate = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="
    static let recipientPublic = "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
    static let ephemeralPrivate = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI="

    @Test func sealMatchesTheRunnersVector() throws {
        let ephemeral = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: Data(base64Encoded: Self.ephemeralPrivate)!)
        let sealed = try SecretSeal.seal("hunter2 \u{2713}", toKey: Self.recipientPublic, approvalId: "approval-123",
                                         ephemeral: ephemeral, nonce: Data(base64Encoded: "AwMDAwMDAwMDAwMD")!)
        #expect(sealed.epk == "zo060cy2M+x7cMF4FKXHbs0CloUFDTRHRboFhw5YfVk=")
        #expect(sealed.nonce == "AwMDAwMDAwMDAwMD")
        #expect(sealed.box == "818iYL0/xbv1amlCprX8NVHr59l6QxFyCAWu")
    }

    @Test func theRunnersKeyOpensIt() throws {
        let sealed = try SecretSeal.seal("s3cret", toKey: Self.recipientPublic, approvalId: "a1")
        let recipient = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: Data(base64Encoded: Self.recipientPrivate)!)
        #expect(recipient.publicKey.rawRepresentation.base64EncodedString() == Self.recipientPublic)
        let epk = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: Data(base64Encoded: sealed.epk)!)
        func open(approvalId: String) throws -> String {
            let key = try recipient.sharedSecretFromKeyAgreement(with: epk).hkdfDerivedSymmetricKey(
                using: SHA256.self, salt: epk.rawRepresentation + recipient.publicKey.rawRepresentation,
                sharedInfo: Data("threadbear secret v1 \(approvalId)".utf8), outputByteCount: 32)
            let box = Data(base64Encoded: sealed.box)!
            let sealedBox = try AES.GCM.SealedBox(nonce: AES.GCM.Nonce(data: Data(base64Encoded: sealed.nonce)!),
                                                  ciphertext: box.dropLast(16), tag: box.suffix(16))
            return String(decoding: try AES.GCM.open(sealedBox, using: key), as: UTF8.self)
        }
        #expect(try open(approvalId: "a1") == "s3cret")
        // Bound to its card: another approval id can't open it.
        #expect(throws: (any Error).self) { try open(approvalId: "a2") }
    }

    @Test func eachSealIsFresh() throws {
        let a = try SecretSeal.seal("same", toKey: Self.recipientPublic, approvalId: "a1")
        let b = try SecretSeal.seal("same", toKey: Self.recipientPublic, approvalId: "a1")
        #expect(a.epk != b.epk)
        #expect(a.nonce != b.nonce)
        #expect(Data(base64Encoded: a.nonce)?.count == 12)
    }

    @Test func aBadKeyIsRefused() {
        #expect(throws: SecretSealError.badKey) { try SecretSeal.seal("x", toKey: "not base64!", approvalId: "a") }
        #expect(throws: SecretSealError.badKey) { try SecretSeal.seal("x", toKey: Data(count: 16).base64EncodedString(), approvalId: "a") }
        #expect(!SecretSeal.isUsableKey("AAAA"))
        #expect(SecretSeal.isUsableKey(Self.recipientPublic))
    }

    @Test func thePairedKeyIsTrustedAndAChangeIsRefused() {
        let store = MemoryRunnerKeyStore()
        let other = Data(repeating: 9, count: 32).base64EncodedString()
        RunnerKeyPins.pinFromPairing(["mac": Self.recipientPublic], in: store)
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "mac", threadId: "t1", in: store) == .trusted)
        #expect(store.runner(forThread: "t1") == "mac")
        #expect(RunnerKeyPins.check(other, runnerId: "mac", threadId: "t1", in: store) == .changed)
        // The refused key never replaces the kept one.
        #expect(store.key(for: "mac") == Self.recipientPublic)
    }

    /// The re-scan's case: a thread the phone has never seen, put by the
    /// backend on a runner id it made up, with its own key. Refused, and
    /// nothing about it is kept, so a second look is refused too.
    @Test func aRunnerTheMirrorMadeUpIsRefused() {
        let store = MemoryRunnerKeyStore()
        RunnerKeyPins.pinFromPairing(["mac": Self.recipientPublic], in: store)
        let evil = Data(repeating: 7, count: 32).base64EncodedString()
        #expect(RunnerKeyPins.check(evil, runnerId: "fresh", threadId: "never-seen", in: store) == .unknown)
        #expect(store.key(for: "fresh") == nil)
        #expect(store.runner(forThread: "never-seen") == nil)
        #expect(RunnerKeyPins.check(evil, runnerId: "fresh", threadId: "never-seen", in: store) == .unknown)
        // A fresh install with nothing paired yet trusts nobody.
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "mac", threadId: "t1", in: MemoryRunnerKeyStore()) == .unknown)
    }

    /// The backend moving a thread the phone knows onto another runner:
    /// refused, even one whose key is kept, and nothing is kept for it.
    @Test func aThreadMovedToAnotherRunnerIsRefused() {
        let store = MemoryRunnerKeyStore()
        let miniKey = Data(repeating: 5, count: 32).base64EncodedString()
        RunnerKeyPins.pinFromPairing(["mac": Self.recipientPublic, "mac-mini": miniKey], in: store)
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "mac", threadId: "t1", in: store) == .trusted)
        #expect(RunnerKeyPins.check(miniKey, runnerId: "mac-mini", threadId: "t1", in: store) == .moved)
        #expect(store.runner(forThread: "t1") == "mac")
        // Back on its own runner, it's fine again.
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "mac", threadId: "t1", in: store) == .trusted)
    }

    /// A thread seen in a list is pinned then, so a move is caught even on
    /// its first secret card.
    @Test func threadsSeenInAListArePinned() {
        let store = MemoryRunnerKeyStore()
        let miniKey = Data(repeating: 5, count: 32).base64EncodedString()
        RunnerKeyPins.pinFromPairing(["mac": Self.recipientPublic, "mac-mini": miniKey], in: store)
        RunnerKeyPins.noteThreads([("t1", "mac"), ("t2", "mac-mini"), ("t3", "")], in: store)
        #expect(store.runner(forThread: "t1") == "mac")
        #expect(store.runner(forThread: "t3") == nil)
        #expect(RunnerKeyPins.check(miniKey, runnerId: "mac-mini", threadId: "t1", in: store) == .moved)
        // A list that later says otherwise doesn't move the pin.
        RunnerKeyPins.noteThreads([("t1", "mac-mini")], in: store)
        #expect(store.runner(forThread: "t1") == "mac")
        #expect(RunnerKeyPins.check(miniKey, runnerId: "mac-mini", threadId: "t2", in: store) == .trusted)
    }

    /// A phone pinned before this rule (Mike's, with two Macs)
    /// keeps working: the keys it kept are trusted, new threads on them too.
    @Test func keysKeptBeforeTheRuleStillWork() {
        let miniKey = Data(repeating: 5, count: 32).base64EncodedString()
        let store = MemoryRunnerKeyStore(["laptop": Self.recipientPublic, "mac-mini": miniKey], threads: ["old": "laptop"])
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "laptop", threadId: "old", in: store) == .trusted)
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "laptop", threadId: "new", in: store) == .trusted)
        #expect(RunnerKeyPins.check(miniKey, runnerId: "mac-mini", threadId: "other", in: store) == .trusted)
    }

    /// Pairing again replaces a runner's kept key (a Mac whose key was
    /// deleted on purpose), adds new runners, and skips keys it can't use.
    @Test func pairingAgainReplacesAndAdds() {
        let store = MemoryRunnerKeyStore(["mac": Data(repeating: 9, count: 32).base64EncodedString()])
        let miniKey = Data(repeating: 5, count: 32).base64EncodedString()
        RunnerKeyPins.pinFromPairing(["mac": Self.recipientPublic, "mac-mini": miniKey, "bad": "AAAA", "": miniKey], in: store)
        #expect(store.key(for: "mac") == Self.recipientPublic)
        #expect(store.key(for: "mac-mini") == miniKey)
        #expect(store.key(for: "bad") == nil)
        #expect(store.key(for: "") == nil)
    }

    /// A fresh install's first secret: the paired Mac's key comes in the
    /// pairing link, so its card can be answered straight away.
    @Test func theFirstSecretOnAFreshInstallWorks() throws {
        let link = try #require(URL(string: "threadbear://pair?url=https://happy-bear-123.convex.cloud&token=tb_abc&key=0a1b2c3d4e5f:"
            + Self.recipientPublic.replacingOccurrences(of: "+", with: "%2B").replacingOccurrences(of: "/", with: "%2F").replacingOccurrences(of: "=", with: "%3D")))
        let pairing = try #require(Pairing.parse(link))
        let store = MemoryRunnerKeyStore()
        RunnerKeyPins.pinFromPairing(pairing.runnerKeys, in: store)
        #expect(RunnerKeyPins.check(Self.recipientPublic, runnerId: "0a1b2c3d4e5f", threadId: "t1", in: store) == .trusted)
    }

    @Test func secretCardsDecode() throws {
        let json = #"{"approvalId":"s1","kind":"secret","label":"Mac administrator password","reason":"To install it","key":"\#(Self.recipientPublic)"}"#
        let pending = try JSONDecoder().decode(PendingApproval.self, from: Data(json.utf8))
        #expect(pending == .secret(id: "s1", label: "Mac administrator password", reason: "To install it", placeholder: nil, key: Self.recipientPublic))
        // And back, as the fake and the tests make rows.
        #expect(try JSONDecoder().decode(PendingApproval.self, from: JSONEncoder().encode(pending)) == pending)

        let noKey = try JSONDecoder().decode(PendingApproval.self, from: Data(#"{"approvalId":"s2","kind":"secret","label":"x","reason":"y"}"#.utf8))
        #expect(noKey == .other(id: "s2", kind: "secret"))
        // A kind from a newer runner waits for the Mac rather than showing Allow and Deny.
        let newer = try JSONDecoder().decode(PendingApproval.self, from: Data(#"{"approvalId":"n","kind":"somethingNew"}"#.utf8))
        #expect(newer == .other(id: "n", kind: "somethingNew"))
        #expect(newer.id == "n")
    }

    @Test func theFakeTakesASecret() async throws {
        let backend = FakeBackend(turnDelay: .zero, secret: true)
        var stream = backend.thread("secret").makeAsyncIterator()
        guard case .secret? = try await stream.next()??.thread.pendingApproval else {
            Issue.record("no secret card")
            return
        }
        let sealed = try SecretSeal.seal("x", toKey: FakeBackend.sampleSecretKey, approvalId: "sec1")
        _ = try await backend.secret(threadId: "secret", approvalId: "sec1", answer: .sealed(sealed))
        #expect(try await stream.next()??.thread.pendingApproval == nil)
    }
}
