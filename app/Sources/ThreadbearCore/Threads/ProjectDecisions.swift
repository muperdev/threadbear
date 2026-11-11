import Foundation

/// Decisions on the Projects page (the `projectDecisions` experiment,
/// runner/src/experiments/projectDecisions.md): every open card of asks from
/// a project's threads, gathered at the top of its page, so Mike can answer
/// them in a row. Nothing new comes from the runner: each thread's summary
/// already carries its asks, and an answer is the same `threads.send` the
/// card in the thread makes. Each question stays its own thread's.
///
/// Mike picked this (direction A) on 10 Nov 2026 after trying Ghostex's
/// coordinator, whose replies lost what they answered because a manager
/// agent relayed them (~/dev/me/misc/ghostex-notes/coordinator-directions.md).
public enum ProjectDecisions {
    /// The thread is waiting on Mike with a card of asks he hasn't finished:
    /// the last turn said it needs him, nothing is running, and some ask is
    /// unticked. A thread that needs him without asks (a plain reply) isn't a
    /// decision; it's in the inbox as today.
    public static func isWaiting(_ thread: ThreadSummary) -> Bool {
        guard thread.waitingOnMike, !thread.isArchived, thread.asksSeq != nil else { return false }
        return thread.asks.contains { !$0.ticked }
    }

    /// How many of its asks are still open.
    public static func openCount(_ thread: ThreadSummary) -> Int {
        isWaiting(thread) ? thread.asks.filter { !$0.ticked }.count : 0
    }

    /// The project's threads with decisions, newest activity first. Takes a
    /// section's threads, which already leave out archived and hibernating
    /// ones, as the inbox does.
    public static func threads(_ threads: [ThreadSummary]) -> [ThreadSummary] {
        threads.filter(isWaiting).sorted {
            $0.lastActivityAt != $1.lastActivityAt ? $0.lastActivityAt > $1.lastActivityAt : $0.id < $1.id
        }
    }

    /// Open asks across these threads.
    public static func count(_ threads: [ThreadSummary]) -> Int {
        threads.reduce(0) { $0 + openCount($1) }
    }

    /// "1 decision", "3 decisions".
    public static func label(_ count: Int) -> String {
        count == 1 ? "1 decision" : "\(count) decisions"
    }

    /// The section's line beside its heading: "4 decisions from 3 threads",
    /// or just the count when each thread has one.
    public static func heading(decisions: Int, threads: Int) -> String {
        decisions == threads ? label(decisions) : label(decisions) + " from " + (threads == 1 ? "1 thread" : "\(threads) threads")
    }

    /// One line on what the thread is doing, from what the runner already
    /// keeps: the librarian's sentence on what it's for (the first line of
    /// `recap`, there with the recapBox experiment on), else the last turn's
    /// headline or first line (`preview`). Nil when there's neither.
    public static func recapLine(_ thread: ThreadSummary) -> String? {
        if let recap = thread.recap {
            let first = recap.split(whereSeparator: \.isNewline).first.map(String.init)?.trimmingCharacters(in: .whitespaces) ?? ""
            // A note from before purposes has only its bullets.
            if !first.isEmpty, !first.hasPrefix("- ") { return first }
        }
        let preview = thread.preview.split(whereSeparator: \.isNewline).first.map(String.init)?.trimmingCharacters(in: .whitespaces) ?? ""
        return preview.isEmpty ? nil : preview
    }

    /// Whether an answer on the page is still on its way: one was sent for
    /// this card (`sentFor`, the asksSeq it answered) and the thread hasn't
    /// taken it yet. Once the thread starts its turn it isn't waiting, and
    /// the card drops off the list.
    public static func isSending(_ thread: ThreadSummary, sentFor seq: Int?) -> Bool {
        guard let seq else { return false }
        return thread.asksSeq == seq && isWaiting(thread)
    }
}
