import Foundation

/// Where the app connects and with what token. The Mac's Settings › Phone &
/// browser draws a QR code of `TBPAIR1_` and the base64url of
/// `url=…&code=<one-use code>&key=<runnerId>:<key>` (runner/src/devices.ts,
/// pairingText), which only the app's own scanner reads: it trades the code
/// for the phone's token (`claimed`). Quick Ask's lesson: never route a token
/// through the Mac clipboard.
///
/// It used to be a `threadbear://pair?url=…&token=…` link the Camera opened,
/// and any app can register that scheme, so another app could be handed the
/// 90-day token (Daybreak finding 127, 15 Nov 2026). PairingGate refuses such
/// a link from anywhere; only a Debug build's `-ThreadbearPair` takes one.
public struct Pairing: Sendable, Hashable {
    public var deploymentURL: URL
    /// Empty until a pairing code has been traded for it.
    public var token: String
    /// The one-use pairing code from the Mac's QR code, to trade for `token`.
    public var code: String?
    /// Runner id to secret-box public key (base64), from the link's `key`
    /// items: the Macs a secret may be sealed to (RunnerKeyPins). They come
    /// from the Mac that drew the QR code, not the mirror, which is why
    /// they can be trusted. Empty for a link from before they were added.
    /// Not saved with the pairing: they're kept as pins once it works.
    public var runnerKeys: [String: String]

    public init(deploymentURL: URL, token: String, code: String? = nil, runnerKeys: [String: String] = [:]) {
        self.deploymentURL = deploymentURL
        self.token = token
        self.code = code
        self.runnerKeys = runnerKeys
    }

    /// What starts the Mac's pairing text.
    public static let textPrefix = "TBPAIR1_"

    /// The Mac's pairing text, or an older Mac's `threadbear://pair` link.
    /// Nil for anything else. Where it came from is PairingGate's to judge.
    public static func parse(_ text: String, allowLocal: Bool = false) -> Pairing? {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix(textPrefix) {
            let body = text.dropFirst(textPrefix.count)
            guard !body.isEmpty, body.count <= 4096,
                  body.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") })
            else { return nil }
            var b64 = body.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
            b64 += String(repeating: "=", count: (4 - b64.count % 4) % 4)
            guard let data = Data(base64Encoded: b64), let query = String(data: data, encoding: .utf8) else { return nil }
            // Split by hand: URLComponents traps on a query that isn't
            // percent-encoded properly, and this is whatever the camera saw.
            let items: [URLQueryItem] = query.split(separator: "&").map { pair in
                let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
                let decode = { (s: Substring) in String(s).removingPercentEncoding ?? "" }
                return URLQueryItem(name: decode(parts[0]), value: parts.count > 1 ? decode(parts[1]) : nil)
            }
            guard let code = items.first(where: { $0.name == "code" })?.value, isCode(code),
                  // A code and a token both is no text the Mac makes.
                  !items.contains(where: { $0.name == "token" })
            else { return nil }
            return parse(items: items, credential: .code(code), allowLocal: allowLocal)
        }
        guard let url = URL(string: text) else { return nil }
        return parse(url, allowLocal: allowLocal)
    }

    /// `tbp_` and hex, as convex/tokens.ts makes them.
    static func isCode(_ code: String) -> Bool {
        code.hasPrefix("tbp_") && code.count > 4 && code.count <= 200
            && code.dropFirst(4).allSatisfy { $0.isASCII && $0.isHexDigit }
    }

    /// The token for a pairing code, from `tokens:claim` (backend API 80),
    /// which takes each code once: this pairing with its token and no code.
    /// One that has its token already is handed back as it is.
    public func claimed(with transport: any ConvexTransport) async throws -> Pairing {
        guard let code else { return self }
        let data = try await transport.mutation("tokens:claim", ["code": .string(code)])
        struct Claimed: Decodable { let token: String }
        guard let token = try? JSONDecoder().decode(Claimed.self, from: data).token, token.hasPrefix("tb_") else {
            throw TransportError.server("The server didn't give the phone a token for that code.")
        }
        return Pairing(deploymentURL: deploymentURL, token: token, runnerKeys: runnerKeys)
    }

    /// `<runnerId>:<base64 key>`. A runner id has no colon (it's hex), and
    /// base64 never does, so the last one splits them.
    static func runnerKeys(_ items: [URLQueryItem]) -> [String: String] {
        var keys: [String: String] = [:]
        for item in items where item.name == "key" {
            guard let raw = item.value, let colon = raw.lastIndex(of: ":") else { continue }
            let runnerId = String(raw[..<colon])
            // A "+" a link maker left unescaped reads as a space.
            let key = String(raw[raw.index(after: colon)...]).replacingOccurrences(of: " ", with: "+")
            guard !runnerId.isEmpty, SecretSeal.isUsableKey(key) else { continue }
            keys[runnerId] = key
        }
        return keys
    }

    /// Convex cloud deployments, or a self-hosted backend at any https
    /// address (docs/self-hosting.md): on the tailnet
    /// (`https://<mac>.<tailnet>.ts.net:<port>`), or an address of the
    /// user's own in front of it (setup --url, 14 Nov 2026), as however the
    /// phone gets there is theirs to choose. `allowLocal` (Debug builds, for
    /// the simulator) also takes the local dev backend on this Mac,
    /// `http://127.0.0.1:3210`. Whichever it is, PairingRequest asks before
    /// it's used, naming the server.
    ///
    /// Before 14 Nov only Convex cloud and ts.net hosts were taken. That was
    /// never what kept a stranger's link out: anyone can make a Convex
    /// deployment or a tailnet. The question, and its "only if you just
    /// scanned the code on your own Mac", is.
    public static func parse(_ url: URL, allowLocal: Bool = false) -> Pairing? {
        guard url.scheme == "threadbear", url.host() == "pair",
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
              let token = items.first(where: { $0.name == "token" })?.value, !token.isEmpty
        else { return nil }
        return parse(items: items, credential: .token(token), allowLocal: allowLocal)
    }

    enum Credential { case token(String), code(String) }

    static func parse(items: [URLQueryItem], credential: Credential, allowLocal: Bool) -> Pairing? {
        guard let raw = items.first(where: { $0.name == "url" })?.value,
              let deployment = URL(string: raw), let host = deployment.host()
        else { return nil }
        let keys = runnerKeys(items)
        func made(_ url: URL) -> Pairing {
            switch credential {
            case .token(let token): Pairing(deploymentURL: url, token: token, runnerKeys: keys)
            case .code(let code): Pairing(deploymentURL: url, token: "", code: code, runnerKeys: keys)
            }
        }
        if allowLocal, deployment.scheme == "http", host == "127.0.0.1" || host == "localhost" {
            return made(deployment)
        }
        guard deployment.scheme == "https" else { return nil }
        let name = host.lowercased()
        if isCloud(name) {
            // The client talks to .convex.cloud; a .site link works too.
            let cloud = name.hasSuffix(".convex.site")
                ? URL(string: "https://" + name.replacingOccurrences(of: ".convex.site", with: ".convex.cloud"))!
                : deployment
            return made(cloud)
        }
        guard let selfHosted = selfHostedBackend(deployment, host: name) else { return nil }
        return made(selfHosted)
    }

    static func isCloud(_ host: String) -> Bool {
        host.hasSuffix(".convex.cloud") || host.hasSuffix(".convex.site")
    }

    /// A self-hosted backend: https, any host but this phone's own loopback
    /// (a link to it would send the phone to itself), any port, and nothing
    /// else in the URL (no login, path, query or fragment), so the client
    /// gets just its origin.
    static func selfHostedBackend(_ url: URL, host: String) -> URL? {
        guard url.scheme == "https", !host.isEmpty, !isLoopback(host),
              url.user() == nil, url.password() == nil, url.query() == nil, url.fragment() == nil,
              url.path().isEmpty || url.path() == "/"
        else { return nil }
        var c = URLComponents()
        c.scheme = "https"
        c.host = host
        c.port = url.port
        return c.url
    }

    static func isLoopback(_ host: String) -> Bool {
        let h = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]")).lowercased()
        return h == "localhost" || h == "::1" || h.hasPrefix("127.") || h.hasSuffix(".localhost")
    }

    /// A server's name for asking, showing and keeping things under. A
    /// self-hosted backend is its host and port, as two on one Mac (or behind
    /// one proxy) can differ only by port. Convex cloud and this Mac's dev
    /// backend are their host alone, exactly as every pairing was keyed
    /// before (a saved cloud URL with a port, too), so an update never loses
    /// a token or a pinned key.
    public static func serverName(_ url: URL) -> String {
        guard let host = url.host() else { return url.absoluteString }
        let name = host.lowercased()
        guard !isCloud(name), !isLoopback(name), let port = url.port else { return host }
        return "\(host):\(port)"
    }

    /// The old token link, for tests and the debug launch argument only:
    /// nothing the app shows or sends makes one (Daybreak 127).
    var link: URL {
        var c = URLComponents()
        c.scheme = "threadbear"
        c.host = "pair"
        c.queryItems = [.init(name: "url", value: deploymentURL.absoluteString), .init(name: "token", value: token)]
            + runnerKeys.sorted { $0.key < $1.key }.map { .init(name: "key", value: "\($0.key):\($0.value)") }
        // URLComponents leaves "+" as is, which a query reads as a space.
        c.percentEncodedQuery = c.percentEncodedQuery?.replacingOccurrences(of: "+", with: "%2B")
        return c.url!
    }
}
