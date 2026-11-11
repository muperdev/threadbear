import Foundation
import ThreadbearCore

/// What's in each composer, kept in `<data dir>/app-drafts.json` so it
/// outlives the app. Drafts lived only in memory, and a handover carries
/// them across, but a plain quit or Restart to Update didn't: on 10 Nov a
/// message a restarting runner cut off went back into its thread's box,
/// and the next relaunch threw it away without a word.
///
/// Only Mike's own app keeps them (AppDelegate starts it); tests and
/// scripted perf or screenshot runs never do.
@MainActor
final class DraftStore {
    let url: URL
    private var pending: Task<Void, Never>?
    /// How long after the last keystroke it writes. A var only for tests.
    var delay: Duration = .milliseconds(500)

    init(url: URL) { self.url = url }

    static func url(dataDir: URL) -> URL { dataDir.appendingPathComponent("app-drafts.json") }

    private struct Saved: Codable {
        var text: String
        var images: [ImageRef]
    }

    /// What was saved, empty ones left out. Unreadable is none.
    func load() -> [String: Draft] {
        guard let data = try? Data(contentsOf: url),
              let saved = try? JSONDecoder().decode([String: Saved].self, from: data) else { return [:] }
        return saved.compactMapValues { Self.isEmpty($0.text, $0.images) ? nil : Draft(text: $0.text, images: $0.images) }
    }

    /// Puts what was saved back, keeping anything already in a box (a
    /// handover's "go" or a draft made before this ran).
    func restore(into app: AppModel) {
        for (key, draft) in load() where app.drafts[key].map({ Self.isEmpty($0.text, $0.images) }) ?? true {
            app.drafts[key] = draft
        }
    }

    /// A draft changed: written a moment later, once typing pauses.
    func changed(_ app: AppModel) {
        pending?.cancel()
        pending = Task { @MainActor [weak self, weak app] in
            guard let self else { return }
            try? await Task.sleep(for: self.delay)
            guard !Task.isCancelled, let app else { return }
            self.save(app)
        }
    }

    /// Writes now (after `changed`'s pause, and with `leaving` as this app
    /// quits or hands over). Leaving, a message still waiting to go again
    /// on a restarted runner goes in its thread's box too: the task that
    /// would send it ends with this app (Codex's review, 10 Nov).
    func save(_ app: AppModel, leaving: Bool = false) {
        pending?.cancel()
        pending = nil
        var all = Self.kept(drafts: app.drafts, editing: app.editing)
        if leaving {
            for (threadId, session) in app.sessions {
                for message in session.heldForResend {
                    all[threadId] = Self.adding(Draft(text: message.text, images: message.images), to: all[threadId])
                }
            }
        }
        do {
            if all.isEmpty {
                if FileManager.default.fileExists(atPath: url.path) { try FileManager.default.removeItem(at: url) }
                return
            }
            let saved = all.mapValues { Saved(text: $0.text, images: $0.images) }
            let data = try JSONEncoder().encode(saved)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try data.write(to: url, options: [.atomic])
            // Private words: only Mike's account reads them.
            try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        } catch {
            AppLog.write("drafts", "couldn't save the drafts: \(error.localizedDescription)")
        }
    }

    /// The drafts worth keeping. A thread whose box holds an edit of an
    /// earlier message keeps what was in the box before it instead: after
    /// a relaunch the edit is gone, and its words would go as a new message.
    nonisolated static func kept(drafts: [String: Draft], editing: [String: MessageEdit]) -> [String: Draft] {
        var all = drafts
        for (threadId, edit) in editing { all[threadId] = edit.before }
        return all.filter { !isEmpty($0.value.text, $0.value.images) }
    }

    /// `held` before whatever's typed, as `restoreDraft` puts a failed
    /// send back.
    nonisolated static func adding(_ held: Draft, to current: Draft?) -> Draft {
        guard let current, !isEmpty(current.text, current.images) else { return held }
        let base = held.normalized
        return Draft(text: base.text + "\n\n" + current.normalized.text, images: base.images + current.images)
    }

    nonisolated private static func isEmpty(_ text: String, _ images: [ImageRef]) -> Bool {
        images.isEmpty && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}
