import AppKit
import BearDock
import Observation
import ThreadbearCore

// The runner connection: connecting, the notification pump, and applying what it sends.
// Split out of AppModel.swift (4 Nov 2026) as a plain move.
extension AppModel {
    // MARK: Connection

    func start() {
        guard connectTask == nil else { return }
        connectTask = Task { await connectLoop() }
    }

    /// Connects, loads, pumps notifications until the socket drops, then
    /// tries again with backoff.
    private func connectLoop() async {
        while !Task.isCancelled {
            // While retrying, the banner keeps saying what went wrong
            // instead of flickering to "Connecting" on every attempt.
            if connection == .connected { connection = .connecting }
            // Every attempt's client is closed when it's done with, so its
            // URLSession (which holds the transport) is let go. Before, each
            // failed attempt leaked one: about 360 an hour with the runner off.
            var attempt: RPCClient?
            do {
                let endpoint = try RunnerConfig.endpoint(dataDir: dataDir)
                let client = RPCClient(transport: WebSocketTransport(url: endpoint.url, token: endpoint.token))
                attempt = client
                try await client.start()
                let build = BuildInfo.read(bundle: Bundle.main.bundleURL).flatMap { $0.appTree.isEmpty ? nil : (commit: $0.commit, short: $0.short, tree: $0.appTree) }
                let hello = try await client.hello(version: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev", build: build)
                self.client = client
                runnerVersion = hello.runnerVersion
                accounts = hello.accounts
                defaults = hello.defaults
                connection = .connected
                // A step 4 runner puts its CLIs in hello; an older one has
                // none, and `clis.list` tells which.
                clis = hello.clis
                clisSupported = hello.clis.isEmpty ? nil : true
                runnerUpdates = hello.updates
                experiments = hello.experiments
                Providers.update(hello.providers)
                // An older runner sends no pools; the app then works as it
                // did before them.
                pools = hello.pools ?? []
                poolsSupported = hello.pools != nil
                if hello.clis.isEmpty { loadClis() }
                // Maybe a newer runner than last time.
                highlightsUnsupported = false
                lastDownReason = nil
                AppLog.write("connection", "connected to runner \(hello.runnerVersion) at \(endpoint.url.absoluteString)")
                // Whatever restart there was is over; a new pending one
                // comes back with hello's notice and runner.status.
                updateRestart { $0.connected() }
                refreshRunnerStatus()
                clearConnectionError()
                backoff.reset()
                // Archived threads come too: the Archived section shows how
                // many there are, the search finds them, and they open as
                // fast as live ones.
                let list = try await client.listThreads(include: [.inbox, .snoozed, .archived])
                var fresh: [String: ThreadSummary] = [:]
                for thread in list { fresh[thread.id] = thread }
                // Other runners' threads aren't this runner's to list.
                // Nor are threads still on their way here or there (movedAway):
                // only this list or the destination's row says they've arrived.
                for (id, thread) in threads where (thread.isRemote || madeRemote.contains(id)) && fresh[id] == nil { fresh[id] = thread }
                // Everything this list has is this Mac's, one that came here
                // from another runner too (a move done while the app was away,
                // or before its old runner let it go): `arrivedHere` stops
                // didSet keeping the old runner's id on it.
                let arrived = Set(list.map(\.id))
                madeRemote.subtract(arrived)
                arrivedHere = arrived
                threads = fresh
                arrivedHere = []
                // A thread deleted while the app wasn't connected never
                // reached forget(), so its Terminal shell would run on.
                Terminals.shared.stopAll(except: list.map(\.id))
                threadsLoaded = true
                onThreadsLoaded?()
                threadCountMayHaveChanged()
                servePanelRequest()
                loadSchedules()
                loadRunners()
                loadRemoteThreads()
                loadProjects()
                loadConnectors()
                loadSettings()
                loadStandingInstructions()
                for session in sessions.values where session.state == .loaded {
                    await session.subscribe()
                }
                for session in sessions.values where session.state != .loaded {
                    session.open(width: transcript.width > 0 ? transcript.width : transcript.currentTableWidth)
                }
                if let ref = subagentSelection {
                    let session = subagentSession(for: ref)
                    if session.state == .loaded { session.refreshSubagent() } else { session.open(width: transcript.width > 0 ? transcript.width : transcript.currentTableWidth) }
                }
                await pump(client)
                client.close()
                // reconnectNow() may already have started a new loop with a
                // new client; only the loop that owns the client clears it.
                guard self.client === client else { return }
                self.client = nil
                for session in sessions.values { session.connectionLost() }
                connection = .lost
                updateRestart { $0.disconnected(now: Date()) }
                AppLog.write("connection", "lost the connection to the runner")
            } catch RPCError.unauthorized {
                attempt?.close()
                guard !Task.isCancelled else { return }
                client = nil
                connection = .unauthorized
                AppLog.write("connection", "the runner didn't accept the token")
            } catch {
                attempt?.close()
                guard !Task.isCancelled else { return }
                client = nil
                connection = .runnerDown(error.localizedDescription)
                updateRestart { $0.disconnected(now: Date()) }
                if lastDownReason != error.localizedDescription {
                    lastDownReason = error.localizedDescription
                    AppLog.write("connection", "can't reach the runner: \(error.localizedDescription)")
                }
            }
            var delay = backoff.next()
            // A restart is over in a few seconds; backing off to 4 s and
            // more left the window saying "Reconnecting" well after the
            // runner was back.
            if restart.isRestarting(now: Date()) { delay = min(delay, 0.5) }
            try? await Task.sleep(for: .seconds(delay))
        }
    }

    private func pump(_ client: RPCClient) async {
        for await notification in client.notifications {
            switch notification {
            case .threadUpdated(let thread):
                receive(thread)
            case .threadEvent(let threadId, let event):
                sessions[threadId]?.receive(event)
                if case .reviewUpdated(let review) = event.payload { receiveReview(review, threadId: threadId) }
                if case .buttonUpdated(let button) = event.payload { ReplyButtons.shared.receive(button, threadId: threadId) }
            case .threadDeleted(let threadId):
                // A thread moving away stays as it is until where it's going
                // lists it, which that runner's row may already have done.
                if movedAway(threadId) { applyRemoteIndex() } else { forget(threadId) }
            case .accountsUpdated(let list):
                // The runner learned something (its model lists, a
                // default it saw a turn run on, new usage): replaces
                // hello's list.
                accounts = list
            case .poolsUpdated(let list):
                pools = list
                poolsSupported = true
            case .clisUpdated(let list):
                clis = list
                clisSupported = true
            case .schedulesUpdated(let list):
                schedules = list
                schedulesSupported = true
            case .remoteThreadsUpdated(let remote):
                receive(remote: remote)
            case .projectsUpdated(let list):
                receive(projects: list)
            case .librarianUpdated(let status):
                librarian = status
                projectsSupported = true
            case .connectorsUpdated(let status):
                connectors = status
            case .settingsUpdated(let settings):
                receive(settings)
            case .standingInstructions(let instructions):
                standingInstructions = instructions
            case .runnerNotice(let level, let message):
                notice(level: level, message)
            case .runnerUpdates(let updates):
                runnerUpdates = updates
            case .other:
                break
            }
        }
    }

    /// A restart notice moves the quiet banner along; other warnings and
    /// errors go in the error banner.
    private func notice(level: String, _ message: String) {
        AppLog.write("notice", "\(level): \(message)")
        if restart(notice: message) {
            refreshRunnerStatus()
        } else if level != "info" {
            errorMessage = message
        }
    }

    #if DEBUG
    /// Tests drive AppModel over a fake transport.
    func useClientForTesting(_ client: RPCClient) {
        self.client = client
        connection = .connected
    }

    /// Tests feed it what the runner would: a notice, a status, a drop.
    func noticeForTesting(level: String = "info", _ message: String) { notice(level: level, message) }
    func receiveForTesting(_ thread: ThreadSummary) { receive(thread) }
    func restartStatusForTesting(_ pending: PendingRestart?) { updateRestart { $0.status(pending, now: Date()) } }
    func connectionLostForTesting() {
        connection = .lost
        updateRestart { $0.disconnected(now: Date()) }
    }
    #endif

    /// Try again straight away (after starting the runner, say).
    func reconnectNow() {
        connection = .connecting
        backoff.reset()
        connectTask?.cancel()
        connectTask = nil
        client?.close()
        start()
    }

    func startRunner() {
        whileBusy(BusyKey.startRunner) { [self] in
            let result = await RunnerLauncher.start()
            if let failure = result { errorMessage = failure }
            try? await Task.sleep(for: .milliseconds(600))
            reconnectNow()
        }
    }

    func receive(_ thread: ThreadSummary) {
        if heldWhileSliding[thread.id] != nil {
            heldWhileSliding[thread.id] = thread
            return
        }
        // Archived somewhere else: slide its row off first, then let it go.
        // Not one archived here, which is already sliding (`archive`).
        if thread.isArchived, threads[thread.id]?.isArchived == false, !slidingOff.contains(thread.id) {
            heldWhileSliding[thread.id] = thread
            slideOff(thread.id)
            Task {
                try? await Task.sleep(for: Self.slideTime)
                if let held = heldWhileSliding.removeValue(forKey: thread.id) { apply(held) }
                await endSlide(thread.id)
            }
            return
        }
        apply(thread)
    }

    private func apply(_ incoming: ThreadSummary) {
        let previous = threads[incoming.id]
        var incoming = incoming
        let movingHere = previous?.isRemote == true && previous?.moving.map { !$0.ended && $0.to == runners.first(where: \.isSelf)?.id } == true
        if madeRemote.contains(incoming.id) || movingHere {
            // Waiting on its move: no summary says where it is (a relayed one
            // and this Mac's own look the same here), so it stays locked with
            // its move, and a move here is only taken as arrived from this
            // Mac's own list (Daybreak 113), whether or not its old runner
            // has let it go yet.
            if incoming.moving == nil { incoming.moving = previous?.moving }
            if movingHere { confirmArrival(incoming.id) }
        }
        let thread = keepingAskedMove(incoming, over: previous)
        threads[thread.id] = thread
        threadCountMayHaveChanged()
        // The open transcript's "3 sub-agents working in the background"
        // line follows the summary, not the events.
        if previous?.background != thread.background { sessions[thread.id]?.setBackground(thread.background) }
        if previous?.looseEnds != thread.looseEnds { sessions[thread.id]?.setLooseEnds(thread.looseEnds) }
        // The open sub-agent did something: read its transcript again.
        if let ref = subagentSelection, ref.threadId == thread.id,
           previous?.subagents.first(where: { $0.id == ref.subagentId }) != thread.subagents.first(where: { $0.id == ref.subagentId }) {
            subagentSessions[ref.key]?.refreshSubagent()
        }
        // A thread a pending restart waits for may just have finished.
        if restart.isPending, previous?.status != thread.status { refreshRunnerStatus() }
        checkQueued(thread.id)
        let becameNeedy = thread.needsYou && previous?.needsYou != true
        let showing = isLooking && isShowing(thread.id)
        // Not for another runner's threads: notifications are low-key,
        // and that Mac's app says so there.
        if becameNeedy && !showing && notificationsEnabled && !thread.isRemote {
            Notifier.post(thread: thread)
        }
        // The ding for a finished thread (Settings > General >
        // Notifications), wherever you are looking. Not another runner's,
        // as with notifications, nor a scripted app's.
        if notificationsEnabled, !thread.isRemote, let previous {
            DingPlayer.threadChanged(from: previous.status, to: thread.status)
        }
        if showing && thread.lastSeq > thread.lastSeenSeq && thread.status != .waiting {
            scheduleMarkSeen(thread.id)
        }
        if thread.panelRequest != nil, thread.id == selection { servePanelRequest() }
    }

    /// A thread deleted on the runner (by an agent's delete_thread, or `tb
    /// delete`) disappears from every list straight away.
    func forget(_ threadId: String) {
        let panel = WindowDock.threadPanel(threadId)
        if dock.layout.contains(panel) { dock.close(panel) }
        threads[threadId] = nil
        Terminals.shared.stop(threadId: threadId)
        drafts[threadId] = nil
        contextUsage[threadId] = nil
        if let session = sessions.removeValue(forKey: threadId) { session.unsubscribe() }
        if selection == threadId { selection = nil }
        if subagentSelection?.threadId == threadId { subagentSelection = nil }
        for key in subagentSessions.keys where key.hasPrefix(threadId + "/") { subagentSessions[key] = nil }
    }
}
