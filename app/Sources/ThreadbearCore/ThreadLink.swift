import Foundation

/// `threadbear://thread/<id>`: a link that brings a thread up in the Mac
/// app. BrowserBear's "Open thread" pill on an agent's Chrome tab opens one
/// (with `open`), so the thread shows in the app rather than on the website.
public enum ThreadLink {
    public static let scheme = "threadbear"

    public static func url(threadId: String) -> URL? {
        URL(string: "\(scheme)://thread/\(threadId)")
    }

    /// The thread a link points at, or nil for any other link (the phone's
    /// `threadbear://pair` among them).
    public static func threadId(from url: URL) -> String? {
        guard url.scheme?.lowercased() == scheme, url.host?.lowercased() == "thread" else { return nil }
        let id = url.pathComponents.dropFirst().first ?? ""
        // Thread ids are UUIDs and the like: nothing that could be a path.
        guard !id.isEmpty, id.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }) else { return nil }
        return id
    }

    /// Whether `id` is shaped like a thread id: 1 to 64 ASCII letters,
    /// digits, `-` and `_` (a runner's are UUIDs). Another runner's ids come
    /// through the backend, which took any string a runner wrote, and an id
    /// goes into a shell line (Copy debug info), so one from outside this
    /// runner is checked with this first (Daybreak 87, 11 Nov 2026). The
    /// backend (`isThreadId`), the runner and the GPUI app check the same.
    public static func isThreadId(_ id: String) -> Bool {
        (1...64).contains(id.utf8.count) && id.utf8.allSatisfy { c in
            (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) || c == 0x2D || c == 0x5F
        }
    }
}
