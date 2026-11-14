import Foundation
import ThreadbearCore

/// Moving a thread to another runner (docs/protocol.md, "Moving a thread to
/// another runner"). Every move is asked of
/// this Mac's own runner, which holds the owner key, wherever the thread is.
extension AppModel {
    /// What the Move sheet is for: which thread, to which runner.
    struct MoveRequest: Identifiable, Equatable {
        var threadId: String
        var runnerId: String
        var runnerName: String
        /// The target's answer when that's why the sheet is opening, so it
        /// opens filled in rather than asking all over again.
        var answer: MovePreview?
        var id: String { threadId + ">" + runnerId }
    }

    /// The menu shows once the runner's there (and has somewhere to go).
    var canMoveThreads: Bool { client != nil }

    /// Where a thread could go: every runner but the one it's on, in Mike's
    /// order, offline ones included so the menu can grey them.
    func moveTargets(for thread: ThreadSummary) -> [RunnerInfo] {
        let selfId = runners.first(where: \.isSelf)?.id
        let here = thread.runnerId ?? selfId
        return orderedRunners.filter { $0.id != here }
    }

    /// Move to ▸ a runner. It goes straight away when the target has a clean
    /// answer; the sheet is for a problem, or something that won't come.
    /// With no connection the sheet opens too, and waits for one.
    func openMove(_ threadId: String, to runner: RunnerInfo) {
        let request = MoveRequest(threadId: threadId, runnerId: runner.id, runnerName: runner.name)
        guard connection == .connected else {
            moveRequest = request
            return
        }
        go(request, cwd: nil, accountId: nil, askFirst: true)
    }

    /// The sheet's guess, or what's in the way. Read-only: nothing starts.
    /// nil when this Mac's runner can't be reached just now. That's no
    /// answer about the move: the runner drops the app for a few seconds at
    /// every update, so the sheet waits and asks again once it's back.
    func movePreview(_ request: MoveRequest, cwd: String? = nil, accountId: String? = nil) async -> MovePreview? {
        guard let client, connection == .connected else { return nil }
        do {
            return try await client.movePreview(threadId: request.threadId, runnerId: request.runnerId, cwd: cwd, accountId: accountId)
        } catch RPCError.disconnected {
            return nil
        } catch {
            return MovePreview(problems: [error.localizedDescription])
        }
    }

    /// Move pressed in the sheet.
    func startMove(_ request: MoveRequest, cwd: String?, accountId: String?) {
        go(request, cwd: cwd, accountId: accountId, askFirst: false)
    }

    /// The row says "Moving to …" at once, from the click, and goes back if
    /// the sheet turns out to be needed or the runner turns the move down.
    private func go(_ request: MoveRequest, cwd: String?, accountId: String?, askFirst: Bool) {
        let threadId = request.threadId
        let key = BusyKey.moveThread(threadId)
        guard let client, !isBusy(key) else { return }
        let before = threads[threadId]
        if var guess = before {
            guess.moving = ThreadMoving(moveId: "", to: request.runnerId, toName: request.runnerName, state: "asking")
            threads[threadId] = guess
        }
        let putBack = { [self] in
            if var now = threads[threadId], now.moving?.moveId == "" {
                now.moving = before?.moving
                threads[threadId] = now
            }
        }
        whileBusy(key) { [self] in
            if askFirst {
                let answer = await movePreview(request)
                if answer?.canGoAsIs != true {
                    putBack()
                    var showing = request
                    showing.answer = answer
                    moveRequest = showing
                    return
                }
            }
            do {
                try await client.moveThread(threadId: threadId, runnerId: request.runnerId, cwd: cwd, accountId: accountId)
            } catch {
                putBack()
                show(error)
            }
        }
    }

    /// A move still being asked for is only known here, so a summary landing
    /// meanwhile mustn't take "Moving to …" off the row.
    func keepingAskedMove(_ incoming: ThreadSummary, over previous: ThreadSummary?) -> ThreadSummary {
        guard incoming.moving == nil, previous?.moving?.moveId == "", isBusy(BusyKey.moveThread(incoming.id)) else { return incoming }
        var kept = incoming
        kept.moving = previous?.moving
        return kept
    }

    /// Cancel on a moving row, before the thread has gone.
    func cancelMove(_ threadId: String) {
        guard let client else { return }
        whileBusy(BusyKey.moveThread(threadId), note: "Cancelling the move…") { [self] in
            do {
                try await client.cancelMove(threadId: threadId)
            } catch { show(error) }
        }
    }

    /// The thread's old runner let it go during a move (its row left the
    /// index, or this Mac's runner deleted it). Its row stays, with its
    /// place and the selection, as it was: the old runner's, locked with
    /// its move, until where it's going lists it (this Mac's own runner for
    /// a move here, that runner's index row for one to another). A row gone
    /// from its old runner says nothing about where the thread is, and a
    /// secondary runner can take its row out and put it back at will
    /// (Daybreak 113, 13 Nov 2026): taken as arrived, the thread looked like
    /// this Mac's and took messages that went to the old runner. Any state
    /// short of failing counts, as a move between two other runners only
    /// ever shows as starting here. False when it isn't a move, and the
    /// thread should go.
    func movedAway(_ threadId: String) -> Bool {
        guard let thread = threads[threadId], let moving = thread.moving, !moving.ended, !moving.to.isEmpty else { return false }
        madeRemote.insert(threadId)
        if let session = sessions.removeValue(forKey: threadId) { session.unsubscribe() }
        return true
    }

    /// Other runners' threads gone from the index with no move showing: a
    /// move here can drop its label (claimed) before its old row goes, so
    /// one this Mac's runner now lists has arrived and stays, as this
    /// Mac's; the rest were deleted there and go.
    func forgetUnlessHere(_ ids: [String]) {
        guard let client else { ids.forEach(forget); return }
        Task { [self] in
            let list = (try? await client.listThreads(include: [.inbox, .snoozed, .archived])) ?? []
            for id in ids {
                guard threads[id]?.isRemote == true else { continue }
                if let here = list.first(where: { $0.id == id }) {
                    madeRemote.remove(id)
                    if let session = sessions.removeValue(forKey: id) { session.unsubscribe() }
                    arrivedHere = [id]
                    threads[id] = here
                    arrivedHere = []
                } else if !remoteIndex.summaries.contains(where: { $0.id == id }) {
                    forget(id)
                }
            }
        }
    }

    /// A thread on its way here from another runner: once this Mac's runner
    /// lists it, it's this Mac's (the list is the only word on that; a
    /// summary of it could be the old runner's, relayed).
    func confirmArrival(_ threadId: String) {
        guard let client else { return }
        Task { [self] in
            guard let list = try? await client.listThreads(include: [.inbox, .snoozed, .archived]),
                  let arrived = list.first(where: { $0.id == threadId }),
                  threads[threadId]?.isRemote == true else { return }
            madeRemote.remove(threadId)
            if let session = sessions.removeValue(forKey: threadId) { session.unsubscribe() }
            arrivedHere = [threadId]
            threads[threadId] = arrived
            arrivedHere = []
        }
    }
}
