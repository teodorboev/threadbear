import CryptoKit
import Foundation

/// The runner RPC's challenge auth (docs/protocol.md, "Auth"). The token
/// never goes over the socket: the runner sends a fresh nonce, the app
/// answers with an HMAC of it and a nonce of its own, and the runner answers
/// the app's nonce the same way. Whoever is holding the port while the
/// runner is down (another account on this Mac can bind it then) learns
/// nothing it can use, and the app hears nothing from it it would trust.
/// Matches runner/src/rpc-auth.ts.
public enum RunnerAuth {
    public static let context = "threadbear-rpc-auth-v1"
    /// auth.prove's id, apart from RPCClient's numbers.
    public static let requestId = "auth"

    public enum Role: String, Sendable { case client, runner }

    /// What went wrong proving the runner is the runner.
    public struct NotProved: Error, Equatable, LocalizedError {
        public var reason: String
        public var errorDescription: String? { reason }
    }

    /// HMAC-SHA256 keyed with the token's UTF-8 bytes, lowercase hex. Bound
    /// to the runner's main server ("primary", the only kind the app talks
    /// to) and the port it listens on, so a challenge something on this
    /// port passes on from a runner on another one (a draining runner)
    /// can't be answered.
    public static func proof(token: String, role: Role, runnerNonce: String, clientNonce: String, port: Int) -> String {
        let key = SymmetricKey(data: Data(token.utf8))
        let message = Data("\(context) \(role.rawValue)\n\(runnerNonce)\n\(clientNonce)\nprimary \(port)".utf8)
        return hex(HMAC<SHA256>.authenticationCode(for: message, using: key))
    }

    public static func isNonce(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }

    /// 32 random bytes, lowercase hex.
    public static func newNonce() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        var generator = SystemRandomNumberGenerator()
        for index in bytes.indices { bytes[index] = generator.next() }
        return hex(bytes)
    }

    /// The app's answer to the runner's first message: the auth.prove text
    /// to send and the proof the runner has to send back. Anything but a
    /// challenge throws: the app never falls back to sending the token, as
    /// an impostor could just pretend to be an old runner.
    ///
    /// `dialedPort` is the port of a loopback URL, what a relay on this Mac
    /// would have to change; for any other host (runner.json's `host`, maybe
    /// behind a proxy whose port isn't the runner's) it's nil and the
    /// runner's own word for its port is used.
    public static func answer(challenge message: Data, token: String, dialedPort: Int?, clientNonce: String = newNonce()) throws -> (request: String, expected: String) {
        guard let object = try? JSONSerialization.jsonObject(with: message) as? [String: Any],
              object["method"] as? String == "auth.challenge",
              let params = object["params"] as? [String: Any],
              let runnerNonce = params["nonce"] as? String, isNonce(runnerNonce),
              let port = dialedPort ?? (params["port"] as? Int), (1...65535).contains(port)
        else {
            throw NotProved(reason: "it didn't send an auth challenge, so it's not the runner, or it's a runner older than this app (it updates itself from main)")
        }
        let request: [String: Any] = [
            "jsonrpc": "2.0", "id": requestId, "method": "auth.prove",
            "params": ["nonce": clientNonce, "proof": proof(token: token, role: .client, runnerNonce: runnerNonce, clientNonce: clientNonce, port: port)],
        ]
        let text = String(decoding: try JSONSerialization.data(withJSONObject: request, options: [.sortedKeys]), as: UTF8.self)
        return (text, proof(token: token, role: .runner, runnerNonce: runnerNonce, clientNonce: clientNonce, port: port))
    }

    /// Checks the runner's answer to auth.prove. Throws `NotProved` for a
    /// wrong or missing proof.
    public static func check(reply message: Data, expected: String) throws {
        guard let object = try? JSONSerialization.jsonObject(with: message) as? [String: Any],
              object["id"] as? String == requestId
        else { throw NotProved(reason: "it sent something else before proving it's the runner") }
        guard let result = object["result"] as? [String: Any], let proof = result["proof"] as? String,
              constantTimeEqual(proof, expected)
        else { throw NotProved(reason: "its proof was wrong, so it isn't the runner") }
    }

    /// The port of a loopback URL (see `answer`), nil for any other host.
    public static func loopbackPort(_ url: URL) -> Int? {
        guard let host = url.host, ["127.0.0.1", "localhost", "::1"].contains(host) else { return nil }
        return url.port ?? (url.scheme == "wss" ? 443 : 80)
    }

    static func constantTimeEqual(_ a: String, _ b: String) -> Bool {
        let x = Array(a.utf8), y = Array(b.utf8)
        guard x.count == y.count else { return false }
        var diff: UInt8 = 0
        for index in x.indices { diff |= x[index] ^ y[index] }
        return diff == 0
    }

    private static func hex<S: Sequence>(_ bytes: S) -> String where S.Element == UInt8 {
        bytes.map { String(format: "%02x", $0) }.joined()
    }
}
