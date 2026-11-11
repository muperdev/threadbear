import Foundation

/// How much an inbox thread wants Mike, lowest first, and whether it counts
/// in the badge: the one rule the Mac's sidebar and Dock badge
/// (ThreadSections) and the phone's inbox and badge (InboxSections) both
/// read. The phone's ThreadbearKit links to this file, as it does to
/// SlashCommands.swift. The website keeps a copy (`urgency` in
/// web/src/sections.ts), and all three run the cases in
/// app/Tests/Fixtures/inbox-urgency-cases.json, so they can't drift apart.
///
/// 0: stuck on his answer (a question, an approval, a design review).
/// 1: finished, wanting him, with a failed or stopped turn.
/// 2: finished, wanting him.
/// 3: read, but its last turn failed or was stopped, so it's not done.
/// 4: done.
///
/// Unread isn't part of it: a turn that ended saying it needs "nothing"
/// from him leaves `needsYou` false though the thread stays unread, and
/// that thread is done. The phone and website used to rank it with the
/// ones wanting him and count it in the badge while the Mac filed it as
/// done; Mike chose the Mac's way for all three (9 Nov).
public enum InboxUrgency {
    public static func of(answerWaiting: Bool, needsYou: Bool, broken: Bool) -> Int {
        if answerWaiting { return 0 }
        if needsYou { return broken ? 1 : 2 }
        return broken ? 3 : 4
    }

    /// Whether a thread this urgent counts in the badge (when it's in the
    /// inbox, or starred and awake).
    public static func counts(_ urgency: Int) -> Bool { urgency <= 2 }
}
