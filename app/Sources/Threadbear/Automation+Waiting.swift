#if DEBUG
import AppKit
import BearDock
import ThreadbearCore

// waitFor, and the helpers the commands share.
// Split out of Automation.swift (4 Nov 2026) as a plain move. Debug builds only,
// like the rest of Automation.
extension Automation {
    // MARK: waitFor

    /// Waits until every condition given holds (or, with `"not": true`,
    /// until they don't), checking every 50 ms, for up to `timeout` seconds
    /// (default 10).
    static func waitFor(_ c: [String: Any], app: AppModel) async -> String {
        let conditions = WaitCondition(c)
        guard !conditions.isEmpty else { return "error nothing to wait for (row, kind, status, text, banner, connection, loaded, ready)" }
        let started = Date()
        let timeout = number(c["timeout"]) ?? 10
        while true {
            if conditions.holds(app) {
                return "ok \(Int(Date().timeIntervalSince(started) * 1000)) ms"
            }
            if Date().timeIntervalSince(started) >= timeout {
                return "error timed out after \(timeout.formatted()) s waiting for \(conditions.description)"
            }
            try? await Task.sleep(for: .milliseconds(50))
        }
    }

    struct WaitCondition: CustomStringConvertible {
        var row: String?
        var kind: String?
        var status: String?
        var thread: String?
        var text: String?
        var banner: String?
        var connection: String?
        var loaded: Bool?
        /// Connected, with the first thread list in (what `dev-app.sh
        /// start` waits for).
        var ready: Bool?
        var negate = false

        init(_ c: [String: Any]) {
            row = c["row"] as? String
            kind = c["kind"] as? String
            status = c["status"] as? String
            thread = c["thread"] as? String
            text = c["text"] as? String
            banner = c["banner"] as? String
            connection = c["connection"] as? String
            loaded = c["loaded"] as? Bool
            ready = c["ready"] as? Bool
            negate = c["not"] as? Bool ?? false
        }

        var isEmpty: Bool { [row, kind, status, text, banner, connection].allSatisfy { $0 == nil } && loaded == nil && ready == nil }

        var description: String {
            var parts: [String] = []
            if let row { parts.append("row \(row)") }
            if let kind { parts.append("a \(kind) row") }
            if let status { parts.append("status \(status)" + (thread.map { " on \($0)" } ?? "")) }
            if let text { parts.append("text \"\(text)\"") }
            if let banner { parts.append(banner.isEmpty ? "no banner" : "banner \"\(banner)\"") }
            if let connection { parts.append("connection \(connection)") }
            if let loaded { parts.append(loaded ? "the thread loaded" : "no thread loaded") }
            if let ready { parts.append(ready ? "the thread list" : "no thread list") }
            return (negate ? "not " : "") + parts.joined(separator: " and ")
        }

        @MainActor
        func holds(_ app: AppModel) -> Bool {
            allHold(app) != negate
        }

        @MainActor
        private func allHold(_ app: AppModel) -> Bool {
            let session = app.transcript.session
            let rows = session.map { s in (0..<s.displayCount).map { s.displayRow($0) } } ?? []
            if let row, !rows.contains(where: { $0.id.hasPrefix(row) }) { return false }
            if let kind, !rows.contains(where: { $0.kind.rawValue == kind }) { return false }
            if let status {
                let id = thread.flatMap { prefix in app.threads.keys.sorted().first { $0.hasPrefix(prefix) } } ?? app.selection
                guard let id, app.threads[id]?.status.rawValue == status else { return false }
            }
            if let text {
                // Items too, so text folded in a closed group counts.
                let items = session?.model.items ?? []
                guard (rows + items).contains(where: { RowDebug.text($0).contains(text) }) else { return false }
            }
            if let banner {
                let shown = app.banner?.text ?? ""
                if banner.isEmpty ? !shown.isEmpty : !shown.contains(banner) { return false }
            }
            if let connection, AppDump.connectionName(app.connection) != connection { return false }
            if let loaded {
                let isLoaded = session != nil && session?.state == .loaded && session?.threadId == app.selection
                if isLoaded != loaded { return false }
            }
            if let ready, (app.connection == .connected && app.threadsLoaded) != ready { return false }
            return true
        }
    }

    // MARK: Helpers

    static func status(_ app: AppModel) -> String {
        var rows: [String] = []
        if let session = app.transcript.session {
            rows = (0..<session.displayCount).map { session.displayRow($0).id }
        }
        let threads = app.threads.values.sorted { $0.lastActivityAt > $1.lastActivityAt }.map {
            ["id": $0.id, "status": $0.status.rawValue, "needsYou": $0.needsYou, "archived": $0.isArchived,
             "snoozed": $0.isSnoozed(at: Date()), "title": $0.title] as [String: Any]
        }
        let object: [String: Any] = [
            "connection": String(describing: app.connection),
            "selection": app.selection ?? "",
            "mode": String(describing: app.sidebarMode),
            "error": app.errorMessage ?? "",
            "banner": app.banner?.text ?? "",
            "drafts": app.drafts.mapValues { ["text": $0.text, "images": $0.images.map(\.path)] },
            "rows": rows,
            "threads": threads,
        ]
        return json(object)
    }

    static func number(_ value: Any?) -> Double? {
        (value as? NSNumber)?.doubleValue ?? (value as? String).flatMap(Double.init)
    }

    /// One line of JSON (a string becomes a quoted, escaped string).
    static func json(_ value: Any) -> String {
        let data = (try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys, .fragmentsAllowed, .withoutEscapingSlashes])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    /// The composer's text view, not a decision card's box that shares
    /// its class.
    static func findComposer(in view: NSView?) -> ComposerTextView? {
        guard let view else { return nil }
        if let match = view as? ComposerTextView, !match.inAnswerBox { return match }
        for sub in view.subviews {
            if let found = findComposer(in: sub) { return found }
        }
        return nil
    }

    static func find<T: NSView>(_ type: T.Type, in view: NSView?) -> T? {
        guard let view else { return nil }
        if let match = view as? T { return match }
        for sub in view.subviews {
            if let found = find(type, in: sub) { return found }
        }
        return nil
    }

    /// Drives the same session as the comparison controls. Replies still
    /// go through the dev/mock runner, never the installed runner.
    static func designReview(_ c: [String: Any], app: AppModel) async -> String {
        guard let threadId = app.selection else { return "error open a thread first" }
        let snapshot = app.session(for: threadId).model.items.compactMap { row -> DesignReview? in
            if case .review(let review) = row.content { return review }
            return nil
        }.first { c["id"] as? String == nil || $0.id == c["id"] as? String }
        guard let snapshot else { return "error no review in this thread" }
        let key = threadId + "/" + snapshot.id
        if c["close"] as? Bool == true {
            app.reviewWindows[key]?.close()
            return "ok closed"
        }
        // {"open": false}: only reads one that's open, as after a click on its card.
        if c["open"] as? Bool == false, app.reviewWindows[key] == nil { return "error that review isn't open" }
        app.openReview(threadId: threadId, review: snapshot)
        guard let controller = app.reviewWindows[key] else { return "error couldn't open review" }
        let session = controller.session
        await session.refresh()
        if let mode = c["mode"] as? String, ReviewMode(rawValue: mode) == .all { session.showAll() }
        if let mode = c["mode"] as? String, ReviewMode(rawValue: mode) == .single { session.enlarge(session.focused) }
        if let show = c["show"] as? Int { session.enlarge(show, shot: c["shot"] as? Int ?? 0) }
        if let actual = c["actual"] as? Bool { session.zoom(actual ? .actual : .fit) }
        if let peek = c["peek"] as? Bool { session.peeking = peek && session.canPeek }
        if let notes = c["notes"] as? [String: String] { for (id, text) in notes { session.setNote(text, for: id) } }
        if let pick = c["pick"] as? String { session.pick(pick) }
        if let picks = c["pick"] as? [String] { for id in picks { session.pick(id) } }
        if c["none"] as? Bool == true { session.toggleNone() }
        if let reply = c["reply"] as? String { session.setReply(reply) }
        if c["save"] as? Bool == true { await session.saveNow() }
        if c["submit"] as? Bool == true { await session.submit() }
        if c["cancel"] as? Bool == true { await session.cancelReview() }
        if let browse = c["browse"] as? Int { session.browse(browse) }
        controller.window?.tuckBehind()
        try? await Task.sleep(for: .milliseconds(250))
        return "ok " + json([
            "id": session.review.id, "window": controller.window?.windowNumber ?? 0,
            "status": session.review.status, "revision": session.review.revision,
            "mode": session.mode.rawValue, "actual": session.zoomed,
            "pickedId": session.draft.pickedId ?? "", "pickedIds": session.draft.pickedIds, "notes": session.draft.notes,
            "reply": session.draft.reply ?? "", "none": session.draft.none, "shot": session.shot, "peeking": session.peeking,
            "dirty": session.dirty, "error": session.error ?? "",
            // What the window shows, for the specs (features/review.feature).
            "focused": session.focusedOption?.id ?? "", "sendLabel": session.sendLabel, "editable": session.editable,
            "minimap": find(ReviewImageScroll.self, in: controller.window?.contentView).map { !$0.minimap.isHidden } ?? false,
            // What the stage draws, and what Open live would open.
            "stage": session.mode == .all ? "all" : session.shownShot?.video != nil ? "player" : session.shownShot?.page != nil ? "page" : "picture",
            "live": session.mode == .single ? (session.shownOption.flatMap { LinkPolicy.web($0.url) }?.absoluteString ?? "") : "",
        ] as [String: Any])
    }

    static func all<T: NSView>(_ type: T.Type, in view: NSView?) -> [T] {
        guard let view else { return [] }
        var out: [T] = []
        if let match = view as? T { out.append(match) }
        for sub in view.subviews { out += all(type, in: sub) }
        return out
    }

    static func log(_ line: String) {
        guard let logURL, let handle = try? FileHandle(forWritingTo: logURL) else { return }
        handle.seekToEndOfFile()
        handle.write(Data((line + "\n").utf8))
        try? handle.close()
    }
}
#endif
