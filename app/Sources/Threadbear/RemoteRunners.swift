import AppKit
import ThreadbearCore

/// Other runners' threads (docs/protocol.md, "Other runners' threads").
/// Their sidebar rows come from this Mac's runner, which reads them from
/// the backend, and sit in `threads` beside this Mac's own, marked with
/// `runnerId`, so the sidebar, the header and the composer take them as
/// they are. Every request about one goes through this Mac's runner,
/// which passes it on through Convex (docs/protocol.md, "Other runners'
/// threads through the backend"): the machines never connect to each other.
extension AppModel {
    // MARK: The list

    /// The list after hello; `remoteThreads.updated` replaces it from then on.
    func loadRemoteThreads() {
        guard let client else { return }
        Task {
            do {
                receive(remote: try await client.remoteThreads())
            } catch {
                AppLog.write("rpc", "remoteThreads.list failed: \(error.localizedDescription)")
            }
        }
    }

    func receive(remote: RemoteThreads) {
        remoteIndex = remote
        if remoteRunners != remote.runners { remoteRunners = remote.runners }
        if let pending = pendingRunnerOrder {
            if remote.order == pending { pendingRunnerOrder = nil }
        } else if runnerOrder != remote.order {
            runnerOrder = remote.order
        }
        applyRemoteIndex()
        applyDefaultRunner()
    }

    /// Puts the table's rows in `threads`, and lets go of rows that have
    /// gone from it (deleted there).
    func applyRemoteIndex() {
        var next = threads
        var listed: Set<String> = []
        for summary in remoteIndex.summaries {
            let waiting = madeRemote.contains(summary.id)
            if let existing = next[summary.id] {
                // Never in place of one of this Mac's own, unless it's
                // moving to the runner that lists it.
                if !existing.isRemote && !waiting { continue }
                // While it waits on its move only the runner it's going to
                // and the one it's leaving speak for it: an old Mac's
                // lingering row can't take it, and the one it's leaving
                // putting its row back keeps it that runner's, still locked
                // with its move (Daybreak 113).
                if waiting && summary.runnerId != existing.runnerId && summary.runnerId != existing.moving?.to { continue }
            }
            listed.insert(summary.id)
            madeRemote.remove(summary.id)
            next[summary.id] = keepingAskedMove(summary, over: next[summary.id])
        }
        let gone = next.values.filter { $0.isRemote && !listed.contains($0.id) && !madeRemote.contains($0.id) }.map(\.id)
        if next != threads { threads = next }
        // A thread that moved off another runner stays, as its new one's.
        let leaving = gone.filter { !movedAway($0) }
        if !leaving.isEmpty { forgetUnlessHere(leaving) }
        threadCountMayHaveChanged()
    }

    /// A thread read through Convex has the table's row for a summary,
    /// and that row has no card (thread-index.ts leaves the asks out, as
    /// they'd cost a write each). The card still comes, in the relay's
    /// events, so the transcript draws it, but picking an answer and
    /// sending it go by the summary's `needs`, `asks` and `asksSeq`
    /// (Decisions.isOpen), and without them every click on it did nothing
    /// (Mike, 6 Nov, on a Hetzner thread). So while it's open here they
    /// come from the card it's showing, in the relay's seqs like the card.
    func withRelayCard(_ summary: ThreadSummary) -> ThreadSummary {
        guard summary.isRemote else { return summary }
        return Decisions.withCard(summary, sessions[summary.id]?.openCard)
    }

    /// A relayed thread's card came or went (ThreadSession.syncOpenCard).
    /// Any summary stored over it gets the card too (`threads`' didSet).
    func relayCardChanged(_ threadId: String) {
        guard isRemote(threadId), let thread = threads[threadId] else { return }
        let next = withRelayCard(thread)
        if next != thread { threads[threadId] = next }
    }

    // MARK: The order

    /// Every runner as Settings > Runners lists them: in Mike's order, then
    /// any he hasn't placed, this Mac first and the rest as the runner has
    /// them (the phone's list does the same, convex/phone.ts orderedRunners).
    var orderedRunnerIds: [String] {
        let selfId = runners.first(where: \.isSelf)?.id
        let all = (selfId.map { [$0] } ?? []) + remoteRunners.map(\.id)
        let order = runnerOrder ?? []
        let rank = { (id: String) in order.firstIndex(of: id) ?? order.count }
        return all.enumerated().sorted { (rank($0.element), $0.offset) < (rank($1.element), $1.offset) }.map(\.element)
    }

    /// `runners.list` in the same order, for the Runners panel.
    var orderedRunners: [RunnerInfo] {
        let order = runnerOrder ?? []
        // Unplaced ones after, this Mac first, as runners.list has them.
        let rank = { (r: RunnerInfo) in order.firstIndex(of: r.id) ?? order.count + (r.isSelf ? 0 : 1) }
        return runners.enumerated().sorted { (rank($0.element), $0.offset) < (rank($1.element), $1.offset) }.map(\.element)
    }

    /// The order can be changed once the backend has one (API 58) and this
    /// Mac's own runner is known: without it a move would save an order
    /// that leaves this Mac out.
    var canOrderRunners: Bool {
        runnerOrder != nil && !remoteRunners.isEmpty && runners.contains(where: \.isSelf)
    }

    /// A drop in a list of only some runners (the panel lists runners.list's):
    /// they swap places among themselves, and the rest of the order stays.
    func setRunnerOrder(part ids: [String]) {
        let full = orderedRunnerIds + ids.filter { !orderedRunnerIds.contains($0) }
        setRunnerOrder(ReorderDrag.merged(ids, into: full))
    }

    /// One place up or down, for VoiceOver's actions on a runner's row (a
    /// drag saves the whole order itself).
    func moveRunner(_ id: String, by offset: Int) {
        let ids = orderedRunnerIds
        guard let from = ids.firstIndex(of: id), ids.indices.contains(from + offset) else { return }
        setRunnerOrder(ReorderDrag.moving(ids, from: from, to: from + offset))
    }

    /// Saves the order through this Mac's runner, here at once.
    func setRunnerOrder(_ ids: [String]) {
        guard let client else { return }
        pendingRunnerOrder = ids
        runnerOrder = ids
        Task {
            do {
                try await client.setRunnerOrder(ids)
                // Saved. Normally the list comes back in it at once; if
                // something else (the phone) moved it again before that, the
                // backend's order wins after a moment rather than this one
                // staying on screen for good.
                try? await Task.sleep(for: .seconds(5))
                if pendingRunnerOrder == ids {
                    pendingRunnerOrder = nil
                    runnerOrder = remoteIndex.order
                }
            } catch {
                if pendingRunnerOrder == ids {
                    pendingRunnerOrder = nil
                    runnerOrder = remoteIndex.order
                }
                errorMessage = "Couldn't save the runner order. \(error.localizedDescription)"
            }
        }
    }

    func isRemote(_ threadId: String) -> Bool { threads[threadId]?.isRemote == true }

    /// The logins a thread's pickers offer: its runner's. This Mac's for its
    /// own threads; another runner's once Convex has given them
    /// (`loadRelayLogins`), nil until then (the composer shows just that Mac's name). A new
    /// thread's are those of the runner it's for (`newThreadLogins`).
    func logins(for thread: ThreadSummary?) -> RunnerLogins? {
        guard let thread else { return newThreadLogins }
        guard let runnerId = thread.runnerId else { return RunnerLogins(accounts: accounts, pools: pools, defaults: defaults) }
        return remoteLogins[runnerId]
    }

    /// The runner one of another runner's threads lives on, if it's listed.
    func remoteRunner(of thread: ThreadSummary) -> RemoteRunner? {
        guard let id = thread.runnerId else { return nil }
        return remoteRunners.first { $0.id == id }
    }

    /// "bruce", for another runner's thread; nil for this Mac's own.
    func runnerName(of thread: ThreadSummary) -> String? {
        guard thread.isRemote else { return nil }
        return remoteRunner(of: thread)?.name ?? "another Mac"
    }

    /// The runner's name on a sidebar row ("M5", "bruce"), this Mac's own
    /// threads included: Mike moves threads between machines and asked to
    /// see at a glance where each one is (9 Nov). Nil with only this Mac,
    /// where every row would just repeat the same name.
    func runnerLabel(of thread: ThreadSummary) -> String? {
        if thread.isRemote { return runnerName(of: thread) }
        guard !remoteRunners.isEmpty else { return nil }
        return runners.first(where: \.isSelf)?.name
    }

    /// The SF Symbol for the runner a thread lives on, as the schedule
    /// editor draws runners.
    func runnerIcon(of thread: ThreadSummary) -> String { RunnerIcon.symbol(remoteRunner(of: thread)?.icon) }

    /// "On bruce", and whether it's offline, for a remote row's tooltip.
    func runnerPlace(of thread: ThreadSummary) -> String? {
        guard let name = runnerName(of: thread) else { return nil }
        return remoteRunner(of: thread)?.online == false ? "On \(name), offline" : "On \(name)"
    }

    // MARK: Routing

    /// A tool's output on another runner's thread, from Convex: once for each tool, as
    /// this Mac's runner keeps it, and it arrives as events on the thread.
    func fetchHeldBody(threadId: String, seq: Int) {
        guard isRemote(threadId), let client else { return }
        Task {
            _ = try? await client.call("threads.heldBody", .object(["threadId": .string(threadId), "seq": .number(Double(seq))]), as: JSONValue.self)
        }
    }

    /// Where a thread's requests go: always this Mac's runner, which takes
    /// another runner's threads through Convex. Nil while it's not connected.
    func client(for threadId: String?) -> RPCClient? { client }

    /// The same, throwing when this Mac's runner isn't connected. Another
    /// runner's thread also has its logins asked for, for the pickers.
    func openClient(for threadId: String) async throws -> RPCClient {
        guard let client else { throw RPCError.disconnected(nil) }
        if let runnerId = threads[threadId]?.runnerId, remoteLogins[runnerId] == nil { loadRelayLogins(runnerId) }
        return client
    }

    /// What the banner says when a send can't go.
    func notConnectedText(_ threadId: String?, _ what: String) -> String {
        if let thread = threadId.flatMap({ threads[$0] }), let name = runnerName(of: thread) {
            return "Not connected to \(name), so \(what). It's still in the box."
        }
        return "Not connected to the runner, so \(what). It's still in the box."
    }

    /// For a transcript of another runner's thread that's still loaded but
    /// no longer subscribed: one that can't reach this Mac's runner says so
    /// where the transcript was. This Mac's runner subscribes it again once
    /// it's back (AppModel+Connection).
    func reconnectRemote(for threadId: String) {
        Task {
            do {
                _ = try await openClient(for: threadId)
            } catch {
                loadedSession(threadId)?.markUnreachable(error)
            }
        }
    }

    // MARK: New threads on another runner

    /// "Runs on" in the new-thread composer: nil for this Mac. Another
    /// runner's logins are asked for now, so they fill the pickers before
    /// Mike sends. The folder, login and pool picked so far were this
    /// Mac's, so they go back to that runner's defaults (and this Mac's
    /// when he comes back to it).
    func chooseNewThreadRunner(_ runnerId: String?, byMike: Bool = true) {
        if byMike { newThreadRunnerPicked = true }
        guard runnerId != newThreadRunnerId else { return }
        newThreadRunnerId = runnerId
        newThreadRunnerProblem = nil
        newThreadCwd = nil
        // Projects are this Mac's: another Mac's thread starts outside one,
        // so the In chip goes rather than promising what won't happen.
        if runnerId != nil { newThreadProjectId = nil }
        newThreadAccount = nil
        newThreadPool = nil
        newThreadProvider = nil
        guard let runnerId else { return }
        loadRelayLogins(runnerId)
    }

    /// Makes `runnerId` (nil: this Mac) the default, and the runner the
    /// draft in front of Mike starts on.
    func setDefaultRunner(_ runnerId: String?) {
        defaultRunnerId = runnerId
        newThreadRunnerPicked = false
        applyDefaultRunner()
    }

    /// The default runner, if it can take a new thread now (listed and
    /// online), else this Mac. Nothing happens once Mike
    /// has picked one for this draft himself.
    func applyDefaultRunner() {
        guard !newThreadRunnerPicked else { return }
        chooseNewThreadRunner(defaultRunnerReady ? defaultRunnerId : nil, byMike: false)
    }

    /// The default is another runner that a new thread can go to now.
    var defaultRunnerReady: Bool {
        guard let id = defaultRunnerId, let runner = remoteRunners.first(where: { $0.id == id }) else { return false }
        return runner.online
    }

    /// The runner a new thread will be made on, when it isn't this Mac.
    var newThreadRunner: RemoteRunner? { newThreadRunnerId.flatMap { id in remoteRunners.first { $0.id == id } } }

    /// Folders a new thread on another runner might start in: its default
    /// and the folders of its threads this Mac lists, newest first. Paths
    /// on that Mac, so nothing here is checked.
    func remoteFolders(_ runnerId: String) -> [String] {
        var seen = Set<String>()
        let defaultFolder = remoteLogins[runnerId]?.defaults.cwd ?? ""
        let recent = threads.values.filter { $0.runnerId == runnerId }.sorted { $0.lastActivityAt > $1.lastActivityAt }.map(\.cwd)
        return ([defaultFolder] + recent).filter { !$0.isEmpty && seen.insert($0).inserted }
    }

    /// Makes the new thread on that runner, through this Mac's runner and
    /// Convex, with what the composer's chips say (that runner's logins, a folder on that
    /// Mac). It's listed as that runner's from the start, so its calls go
    /// there, and opened as a new thread here is. If the runner can't be
    /// reached the draft stays and the banner says why.
    func sendNewThread(on runnerId: String, text: String, draft: Draft) {
        let name = remoteRunners.first { $0.id == runnerId }?.name ?? "that Mac"
        let images = draft.normalized.images
        ActivityRecorder.shared.sent(threadId: nil, images: images.count, answers: 0)
        creatingThread = true
        Task {
            defer { creatingThread = false }
            do {
                guard remoteRunners.contains(where: { $0.id == runnerId }) else {
                    throw RemoteUnreachable(runnerId: runnerId, runnerName: name, online: false, reason: .unknownRunner)
                }
                // Through this Mac's runner, as a command for that one; it
                // uploads the pictures with it.
                var request = newThreadRequest(text: text, images: images)
                guard let client else { throw RPCError.disconnected(nil) }
                request.runnerId = runnerId
                var made = try await client.createThread(request)
                if drafts["new"] == draft { drafts["new"] = nil }
                // The runner, folder, a picked mode and picks were for
                // this thread; the next starts on the defaults again,
                // the default runner's included (applyDefaultRunner below).
                newThreadRunnerId = nil
                newThreadRunnerPicked = false
                newThreadRunnerProblem = nil
                newThreadCwd = nil
                newThreadPermissionPick = nil
                newThreadAccount = nil
                newThreadPool = nil
                newThreadProvider = nil
                made.runnerId = runnerId
                // Kept until the sidebar table lists it.
                madeRemote.insert(made.id)
                threads[made.id] = made
                showStartedThread(made.id)
                applyDefaultRunner()
            } catch let unreachable as RemoteUnreachable {
                // The draft never left the box.
                newThreadRunnerProblem = unreachable.detail
                errorMessage = unreachable.headline + " " + unreachable.detail + " It's still in the box."
            } catch {
                // Not this Mac's runner's banner: it's that one that failed.
                AppLog.write("rpc", "threads.create on \(name) failed: \(error.localizedDescription)")
                errorMessage = "Couldn't start it on \(name). \(error.localizedDescription)"
            }
        }
    }

    // MARK: Through Convex

    /// That runner's logins for the pickers, from Convex through this Mac's
    /// runner.
    func loadRelayLogins(_ runnerId: String) {
        guard let client else { return }
        Task {
            do {
                let logins = try await client.remoteLogins(runnerId: runnerId)
                remoteLogins[runnerId] = logins
                if newThreadRunnerId == runnerId { newThreadRunnerProblem = nil }
            } catch {
                AppLog.write("rpc", "remoteThreads.logins for \(remoteName(runnerId)) failed: \(error.localizedDescription)")
                if newThreadRunnerId == runnerId { newThreadRunnerProblem = error.localizedDescription }
            }
        }
    }

    // MARK: Names

    private func remoteName(_ runnerId: String) -> String { remoteRunners.first { $0.id == runnerId }?.name ?? runnerId }
}
