import Foundation

/// Where the app connects and with what token. The runner side makes a
/// phone token (`tb phone-token`) and shows it as a QR code of a
/// `threadbear://pair?url=…&token=…&key=<runnerId>:<key>` link; the Camera
/// opens the app with it. Quick Ask's lesson: never route a token through
/// the Mac clipboard.
public struct Pairing: Sendable, Hashable {
    public var deploymentURL: URL
    public var token: String
    /// Runner id to secret-box public key (base64), from the link's `key`
    /// items: the Macs a secret may be sealed to (RunnerKeyPins). They come
    /// from the Mac that drew the QR code, not the mirror, which is why
    /// they can be trusted. Empty for a link from before they were added.
    /// Not saved with the pairing: they're kept as pins once it works.
    public var runnerKeys: [String: String]

    public init(deploymentURL: URL, token: String, runnerKeys: [String: String] = [:]) {
        self.deploymentURL = deploymentURL
        self.token = token
        self.runnerKeys = runnerKeys
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
              let raw = items.first(where: { $0.name == "url" })?.value,
              let deployment = URL(string: raw), let host = deployment.host(),
              let token = items.first(where: { $0.name == "token" })?.value, !token.isEmpty
        else { return nil }
        let keys = runnerKeys(items)
        if allowLocal, deployment.scheme == "http", host == "127.0.0.1" || host == "localhost" {
            return Pairing(deploymentURL: deployment, token: token, runnerKeys: keys)
        }
        guard deployment.scheme == "https" else { return nil }
        let name = host.lowercased()
        if isCloud(name) {
            // The client talks to .convex.cloud; a .site link works too.
            let cloud = name.hasSuffix(".convex.site")
                ? URL(string: "https://" + name.replacingOccurrences(of: ".convex.site", with: ".convex.cloud"))!
                : deployment
            return Pairing(deploymentURL: cloud, token: token, runnerKeys: keys)
        }
        guard let selfHosted = selfHostedBackend(deployment, host: name) else { return nil }
        return Pairing(deploymentURL: selfHosted, token: token, runnerKeys: keys)
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

    public var link: URL {
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
