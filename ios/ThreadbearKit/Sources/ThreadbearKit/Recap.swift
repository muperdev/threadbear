import Foundation

/// The recap box: what a thread that has sat for a while is for and where
/// it stands. The same rule as the Mac's Recap.swift, the website's
/// view-logic.ts and the runner's recapShowing (docs/protocol.md, "The
/// recap box"), so a change goes in all four.
public enum Recap {
    /// 12 hours: AWAY_MS in runner.ts.
    public static let quietAfter: TimeInterval = 12 * 3600

    public static func isDue(_ thread: PhoneThread, dismissedAt: Double?, now: Date = Date()) -> Bool {
        guard let recap = thread.recap, !recap.isEmpty, thread.status == .idle, thread.archivedAt == nil else { return false }
        guard now.timeIntervalSince(thread.lastActivityAt) >= quietAfter else { return false }
        return dismissedAt != thread.lastActivityAt.timeIntervalSince1970
    }

    /// The sentence on what it's for, if the note has one, and the bullets.
    public static func parts(_ recap: String) -> (purpose: String?, points: [String]) {
        var purpose: String?
        var points: [String] = []
        for raw in recap.split(separator: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("- ") {
                points.append(String(line.dropFirst(2)))
            } else if purpose == nil, points.isEmpty, !line.isEmpty {
                purpose = line
            } else if !line.isEmpty {
                points.append(line)
            }
        }
        return (purpose, points)
    }

    public static func lastWorkedOn(_ date: Date, now: Date = Date()) -> String {
        let hours = Int((now.timeIntervalSince(date) / 3600).rounded())
        return "Last worked on " + (hours < 48 ? "\(hours) hours ago" : "\(Int((Double(hours) / 24).rounded())) days ago")
    }
}
