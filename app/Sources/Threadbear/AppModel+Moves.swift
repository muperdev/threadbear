import Foundation
import ThreadbearCore

/// Moving a thread to another runner (docs/protocol.md, "Moving a thread to
/// another runner"; the `moveToRunner` experiment). Every move is asked of
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

    /// The menu shows: the experiment's on here and there's somewhere to go.
    var canMoveThreads: Bool { experiments.contains("moveToRunner") && client != nil }

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

    /// The thread's old runner let it go during a move. It's on its way to
    /// the other runner, which lists it in a moment: keep its row, its
    /// place and the selection, as that runner's from now on, and let its
    /// transcript open again from there. Any state short of failing counts,
    /// as a move between two other runners only ever shows as starting
    /// here. False when it isn't a move, and the thread should go.
    func movedAway(_ threadId: String) -> Bool {
        guard var thread = threads[threadId], let moving = thread.moving, !moving.ended else { return false }
        let selfId = runners.first(where: \.isSelf)?.id
        thread.runnerId = moving.to == selfId ? nil : moving.to
        thread.moving = nil
        if thread.runnerId != nil { madeRemote.insert(threadId) }
        liveRemote.remove(threadId)
        if let session = sessions.removeValue(forKey: threadId) { session.unsubscribe() }
        threads[threadId] = thread
        return true
    }
}
