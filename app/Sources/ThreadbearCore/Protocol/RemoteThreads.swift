import Foundation

/// Other runners' threads (docs/protocol.md, "Other runners' threads"):
/// what `remoteThreads.list` answers and `remoteThreads.updated` sends. The
/// runner reads them from the backend; the app lists them beside its own
/// runner's and reaches them through it and Convex. Runners never talk to
/// each other.
public struct RemoteThreads: Sendable, Hashable, Decodable {
    public var runners: [RemoteRunner]
    public var threads: [RemoteThread]
    /// Mike's runner order (backend API 58): every runner's id, this one's
    /// too, the same order the phone sets. Nil from an older runner or
    /// backend, which has none to give.
    public var order: [String]?

    public init(runners: [RemoteRunner] = [], threads: [RemoteThread] = [], order: [String]? = nil) {
        self.runners = runners
        self.threads = threads
        self.order = order
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        runners = c.lossyArray(RemoteRunner.self, forKey: .runners)
        // A row whose id isn't a thread id's shape is left out: an extra Mac
        // may be compromised, and ids go into shell lines (ThreadLink.isThreadId).
        threads = c.lossyArray(RemoteThread.self, forKey: .threads).filter { ThreadLink.isThreadId($0.threadId) }
        order = try? c.decodeIfPresent([String].self, forKey: .order)
    }

    enum CodingKeys: String, CodingKey { case runners, threads, order }

    public func runner(_ id: String) -> RemoteRunner? { runners.first { $0.id == id } }

    /// The threads as sidebar summaries, each marked with its runner.
    public var summaries: [ThreadSummary] { threads.map(\.summary) }
}

/// Another runner, as the backend knows it. An older runner also sent
/// `rpcUrl` and `tokenFile`, for the direct link the app no longer has:
/// they're left undecoded.
public struct RemoteRunner: Sendable, Hashable, Decodable, Identifiable {
    public var id: String
    /// "bruce": its Mac's hostname by default.
    public var name: String
    /// An SF Symbol for its Mac ("macmini"), as `RunnerInfo.icon`.
    public var icon: String?
    /// Heard from in the last 3 minutes.
    public var online: Bool
    public var lastSeenAt: Date?

    public init(id: String, name: String, icon: String? = nil, online: Bool = true, lastSeenAt: Date? = nil) {
        self.id = id
        self.name = name
        self.icon = icon
        self.online = online
        self.lastSeenAt = lastSeenAt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        func text(_ key: CodingKeys) -> String? { ((try? c.decodeIfPresent(String.self, forKey: key)) ?? nil).flatMap { $0.isEmpty ? nil : $0 } }
        name = text(.name) ?? id
        icon = text(.icon)
        online = ((try? c.decodeIfPresent(Bool.self, forKey: .online)) ?? nil) ?? false
        lastSeenAt = text(.lastSeenAt).flatMap(ISO8601.parse)
    }

    enum CodingKeys: String, CodingKey { case id, name, icon, online, lastSeenAt }
}

/// One row of another runner's sidebar table: what a sidebar row shows,
/// not the whole summary that runner keeps.
public struct RemoteThread: Sendable, Hashable, Decodable {
    public var runnerId: String
    public var threadId: String
    public var title: String
    public var cwd: String
    public var accountId: String
    public var provider: String
    public var model: String?
    public var status: ThreadStatus
    public var needsYou: Bool
    public var unread: Bool
    public var preview: String
    public var createdAt: Date
    public var lastActivityAt: Date
    public var snoozedUntil: Date?
    public var archivedAt: Date?
    public var starredAt: Date?
    /// Who started it, when it wasn't Mike: the row's byline.
    public var startedBy: ThreadStartedBy?
    /// What Mike last said to it (backend API 32): the row's "You: ...".
    public var asked: String?
    /// The librarian's search words (backend API 74), for the sidebar's search.
    public var keywords: String?
    /// A move the main Mac's runner knows of (docs/protocol.md, "Moving a
    /// thread to another runner"), put on the row by it, never by the backend.
    public var moving: ThreadMoving?

    public init(runnerId: String, threadId: String, title: String = "", cwd: String = "", accountId: String = "", provider: String = "claude",
                model: String? = nil, status: ThreadStatus = .idle, needsYou: Bool = false, unread: Bool = false, preview: String = "",
                createdAt: Date = Date(), lastActivityAt: Date = Date(), snoozedUntil: Date? = nil, archivedAt: Date? = nil, startedBy: ThreadStartedBy? = nil) {
        self.runnerId = runnerId
        self.threadId = threadId
        self.title = title
        self.cwd = cwd
        self.accountId = accountId
        self.provider = provider
        self.model = model
        self.status = status
        self.needsYou = needsYou
        self.unread = unread
        self.preview = preview
        self.createdAt = createdAt
        self.lastActivityAt = lastActivityAt
        self.snoozedUntil = snoozedUntil
        self.archivedAt = archivedAt
        self.startedBy = startedBy
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        runnerId = try c.decode(String.self, forKey: .runnerId)
        threadId = try c.decode(String.self, forKey: .threadId)
        func text(_ key: CodingKeys) -> String? { (try? c.decodeIfPresent(String.self, forKey: key)) ?? nil }
        func flag(_ key: CodingKeys) -> Bool { ((try? c.decodeIfPresent(Bool.self, forKey: key)) ?? nil) ?? false }
        title = text(.title) ?? ""
        cwd = text(.cwd) ?? ""
        accountId = text(.accountId) ?? ""
        provider = text(.provider).flatMap { $0.isEmpty ? nil : $0 } ?? "claude"
        model = text(.model).flatMap { $0.isEmpty ? nil : $0 }
        status = ((try? c.decodeIfPresent(ThreadStatus.self, forKey: .status)) ?? nil) ?? .idle
        needsYou = flag(.needsYou)
        unread = flag(.unread)
        preview = text(.preview) ?? ""
        createdAt = text(.createdAt).flatMap(ISO8601.parse) ?? Date(timeIntervalSince1970: 0)
        lastActivityAt = text(.lastActivityAt).flatMap(ISO8601.parse) ?? createdAt
        snoozedUntil = text(.snoozedUntil).flatMap(ISO8601.parse)
        archivedAt = text(.archivedAt).flatMap(ISO8601.parse)
        starredAt = text(.starredAt).flatMap(ISO8601.parse)
        startedBy = (try? c.decodeIfPresent(ThreadStartedBy.self, forKey: .startedBy)) ?? nil
        asked = text(.asked).flatMap { $0.isEmpty ? nil : $0 }
        keywords = text(.keywords).flatMap { $0.isEmpty ? nil : $0 }
        moving = (try? c.decodeIfPresent(ThreadMoving.self, forKey: .moving)) ?? nil
    }

    enum CodingKeys: String, CodingKey {
        case runnerId, threadId, title, cwd, accountId, provider, model, status, needsYou, unread, preview
        case createdAt, lastActivityAt, snoozedUntil, archivedAt, starredAt, startedBy, asked, keywords, moving
    }

    /// As a summary the sidebar's rows and sections take like a local one.
    /// The table has no seqs, so an unread one gets 1 and 0, which is all
    /// the rows look at; opening the thread fetches the real summary from
    /// its runner.
    public var summary: ThreadSummary {
        var summary = ThreadSummary(
            id: threadId, title: title, cwd: cwd, accountId: accountId, provider: provider, model: model,
            status: status, needsYou: needsYou, createdAt: createdAt, updatedAt: lastActivityAt, lastActivityAt: lastActivityAt,
            snoozedUntil: snoozedUntil, archivedAt: archivedAt, starredAt: starredAt, preview: preview,
            lastSeq: unread ? 1 : 0, lastSeenSeq: 0
        )
        // Hibernating until that runner restarts is kept as a date in the
        // year 9999 (`threads.update` with "restart").
        if let snoozedUntil, snoozedUntil > Self.restartFarOff { summary.snoozedUntilRestart = true }
        summary.runnerId = runnerId
        summary.startedBy = startedBy
        summary.asked = asked
        summary.keywords = keywords
        summary.moving = moving
        return summary
    }

    static let restartFarOff = Date(timeIntervalSince1970: 200_000_000_000)
}

/// Why a new thread can't go to another runner, in words for the banner.
public struct RemoteUnreachable: Error, Sendable, Equatable, LocalizedError {
    public enum Reason: Sendable, Equatable {
        /// Its runner isn't in the list any more.
        case unknownRunner
    }

    public var runnerId: String
    public var runnerName: String
    public var online: Bool
    public var reason: Reason

    public init(runnerId: String, runnerName: String, online: Bool, reason: Reason) {
        self.runnerId = runnerId
        self.runnerName = runnerName
        self.online = online
        self.reason = reason
    }

    public var headline: String { "Can't reach \(runnerName) from this Mac." }

    public var detail: String {
        switch reason {
        case .unknownRunner: return "Its runner isn't listed any more."
        }
    }

    public var errorDescription: String? { detail }
}
