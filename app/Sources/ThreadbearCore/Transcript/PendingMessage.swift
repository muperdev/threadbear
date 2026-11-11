import Foundation

/// A message Mike has sent from this app that the runner hasn't shown in
/// the thread yet. It sits at the end of the transcript as his bubble,
/// dimmed, so a click on Send always shows something at once, however long
/// a busy runner takes to answer (on a Mac loaded with renders and backups
/// its event loop stalled for 5 to 35 seconds). It goes once the thread's
/// own `user.message` for it arrives, or when the send fails or the
/// connection drops (the draft goes back in the box then).
public struct PendingMessage: Sendable, Hashable, Identifiable {
    /// Made up by the app; the runner never sees it.
    public var id: String
    public var text: String
    public var images: [ImageRef]
    public var answers: [AskAnswer]
    /// The thread's last seq when it was sent. Only a `user.message` after
    /// it can be this one: an older "yes" in the history isn't.
    public var afterSeq: Int
    /// The runner has taken it but not shown it: it's waiting behind the
    /// running turn (docs/protocol.md, `threads.send`), and its
    /// `user.message` comes when its own turn starts.
    public var queued: Bool
    /// The turn it will start, when Mike queued it (Cmd+Return) and the
    /// runner said so: `threads.unqueue` takes it back by this, for the
    /// pencil and the remove button. Nil for one an older runner queued.
    public var queuedTurnId: String?
    /// Held in the app until a restarting runner is back: sent while it
    /// was down (or turned away as it went), it goes once the new one
    /// connects, and says so under the bubble (Mike, 9 Nov).
    public var waitingForRunner: Bool

    public init(id: String = UUID().uuidString, text: String, images: [ImageRef] = [], answers: [AskAnswer] = [], afterSeq: Int, queued: Bool = false, waitingForRunner: Bool = false) {
        self.id = id
        self.text = text
        self.images = images
        self.answers = answers
        self.afterSeq = afterSeq
        self.queued = queued
        self.waitingForRunner = waitingForRunner
    }

    /// Whether `event` is the runner showing this message. Matched on the
    /// words (and how many pictures, as the runner copies them to the
    /// thread's own folder under new paths): its `user.message` usually
    /// arrives before the reply to `threads.send` does, so there's no turn
    /// id to go on yet. Messages from the runner, a schedule, the phone or
    /// an edit are never ours.
    public func isLanded(by event: Event) -> Bool {
        guard event.seq > afterSeq,
              case .userMessage(let text, let images, let origin, _, let editOf, _) = event.payload,
              origin == nil, editOf == nil else { return false }
        return text == self.text && images.count == self.images.count
    }

    /// How the bubble shows it.
    public var row: TranscriptRow {
        let message = UserMessage(text: text, images: images, answers: answers, editable: queued && queuedTurnId != nil, pending: waitingForRunner ? .waitingForRunner : queued ? .queued : .sending)
        return TranscriptRow(id: "p:" + id, seq: afterSeq, content: .user(message))
    }

    /// Takes out the messages that `events` show, each event standing for
    /// one message (two sends of "yes" need two). Returns whether any went.
    @discardableResult
    public static func remove(landedIn events: some Sequence<Event>, from list: inout [PendingMessage]) -> Bool {
        guard !list.isEmpty else { return false }
        var removed = false
        for event in events {
            guard case .userMessage = event.payload else { continue }
            if let index = list.firstIndex(where: { $0.isLanded(by: event) }) {
                list.remove(at: index)
                removed = true
                if list.isEmpty { break }
            }
        }
        return removed
    }
}
