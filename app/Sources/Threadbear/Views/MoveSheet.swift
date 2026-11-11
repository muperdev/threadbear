import SwiftUI
import ThreadbearCore

/// The Move sheet (docs/move-plan.md), for a move that can't just go: where
/// a thread will run on the other runner and on which login, both filled in
/// with that runner's own guess (threads.movePreview), what's in the way or
/// won't come, then Move.
/// Changing the folder or login asks the other runner again, so what it
/// says can't be done shows before the thread freezes.
struct MoveSheet: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    let request: AppModel.MoveRequest

    @State private var preview: MovePreview?
    @State private var asking = false
    @State private var folder = ""
    @State private var login = ""
    /// What the runner guessed, so only Mike's own changes are sent.
    @State private var guessedFolder = ""
    @State private var guessedLogin = ""
    /// The runner's first guess has arrived. Until then the fields are
    /// waiting on it, not asking Mike to fill them in.
    @State private var filled = false
    /// Bumped to check again what's in the fields.
    @State private var asks = 0

    private var connected: Bool { app.connection == .connected }
    private var thread: ThreadSummary? { app.threads[request.threadId] }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            #if DEBUG
            // For `dump`: what the sheet shows (AppDump.moveSheet).
            let _ = (AppDump.moveSheetNow = AppDump.MoveSheetInfo(
                title: "Move \u{201C}\(title)\u{201D} to \(request.runnerName)", subtitle: subtitle, folder: folder, login: login,
                problems: preview?.problems ?? [], leftBehind: preview?.leftBehind ?? [], clone: preview?.clone, asking: asking, canMove: canMove))
            #endif
            VStack(alignment: .leading, spacing: 4) {
                Text("Move \u{201C}\(title)\u{201D} to \(request.runnerName)")
                    .font(.headline)
                    .lineLimit(2)
                Text(subtitle)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                GridRow {
                    Text("Folder").foregroundStyle(.secondary).gridColumnAlignment(.trailing)
                    TextField("Folder on \(request.runnerName)", text: $folder)
                        .textFieldStyle(.roundedBorder)
                        .font(.system(.body, design: .monospaced))
                        .disabled(!filled)
                        .onSubmit { ask() }
                        .tip(preview?.worktree == true
                             ? "The repo on \(request.runnerName) it gets a new worktree of its own in. Press Return to check another folder."
                             : "Where it runs on \(request.runnerName). Press Return to check another folder.")
                }
                GridRow {
                    Text("Login").foregroundStyle(.secondary)
                    Picker("Login", selection: $login) {
                        ForEach(preview?.accounts ?? []) { account in
                            Text(account.email.map { "\(account.title) (\($0))" } ?? account.title).tag(account.id)
                        }
                    }
                    .labelsHidden()
                    .disabled((preview?.accounts ?? []).isEmpty)
                    .onChange(of: login) { old, _ in if !old.isEmpty { ask() } }
                    .tip("The login on \(request.runnerName) it carries on with")
                }
            }
            lines
            HStack {
                if asking || !connected {
                    ProgressView().controlSize(.small)
                    Text(connected ? "Asking \(request.runnerName)…" : "Waiting for this Mac's runner…").font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .tip("Close without moving it")
                Button("Move") { move() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canMove)
                    .tip(canMove ? "Move it to \(request.runnerName) once this turn ends" : "Fix what's in the way first")
            }
        }
        .padding(20)
        .frame(width: 460)
        // Every check runs here, so there's only ever one and a newer one
        // cancels the last. Keyed on the connection as well so it asks again
        // when the runner comes back: the sheet can open in the seconds a
        // runner update drops the app, and one ask then left it blank with
        // an error for good. The loop is for a connection swapped for another
        // with `connected` true throughout, which ends an ask without
        // changing the key.
        .task(id: CheckKey(connected: connected, asks: asks)) {
            if !filled, let known = request.answer {
                apply(known, cwd: nil, accountId: nil)
                return
            }
            asking = connected
            guard connected else { return }
            while !Task.isCancelled, await !check() {
                try? await Task.sleep(for: .seconds(1))
            }
            if !Task.isCancelled { asking = false }
        }
    }

    private struct CheckKey: Hashable {
        var connected: Bool
        var asks: Int
    }

    @ViewBuilder
    private var lines: some View {
        if let preview {
            ForEach(Array(preview.problems.enumerated()), id: \.offset) { _, line in
                Label(line, systemImage: "xmark.octagon")
                    .font(.callout)
                    .foregroundStyle(Color(nsColor: Theme.failed))
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let clone = preview.clone {
                Text("\(request.runnerName) has no copy of its repo, so it's cloned to \(clone) first.")
                    .font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(Array(preview.leftBehind.enumerated()), id: \.offset) { _, line in
                Text("Won't come: \(line)")
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var title: String {
        let t = thread?.title ?? ""
        return t.isEmpty ? "Untitled" : t
    }

    private var subtitle: String {
        if thread?.status.isBusy == true { return "It goes once this turn ends." }
        return "Its whole conversation and its git work go with it, and it carries on there."
    }

    private var canMove: Bool {
        guard let preview, !asking, connected else { return false }
        return preview.problems.isEmpty && !folder.isEmpty && !login.isEmpty
    }

    /// Asks again for Mike's folder and login.
    private func ask() {
        asks += 1
    }

    /// Asks for what the fields say, leaving out whatever is still the
    /// runner's guess. False when there's no answer and it's worth retrying.
    private func check() async -> Bool {
        let cwd = folder == guessedFolder ? nil : folder
        let accountId = login == guessedLogin ? nil : login
        guard let answer = await app.movePreview(request, cwd: cwd, accountId: accountId) else { return false }
        guard !Task.isCancelled else { return true }
        apply(answer, cwd: cwd, accountId: accountId)
        return true
    }

    private func apply(_ answer: MovePreview, cwd: String?, accountId: String?) {
        preview = answer
        // A field still on the guess follows it, so what shows is what the
        // runner will use, even if its guess changed across a reconnect.
        // One Mike has started typing in since asking is left alone.
        if cwd == nil, let guess = answer.cwd {
            if folder == guessedFolder { folder = guess }
            guessedFolder = guess
        }
        if accountId == nil, let guess = answer.accountId {
            if login == guessedLogin { login = guess }
            guessedLogin = guess
        }
        filled = true
    }

    private func move() {
        let cwd = folder == guessedFolder ? nil : folder
        let accountId = login == guessedLogin ? nil : login
        app.startMove(request, cwd: cwd, accountId: accountId)
        dismiss()
    }
}
