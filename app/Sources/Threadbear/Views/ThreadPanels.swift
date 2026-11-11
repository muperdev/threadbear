import AppKit
import BearDock
import SwiftUI
import ThreadbearCore

// The main panel and the panels around it (WindowDock). The main panel
// follows the context Mike picks in the sidebar; tabs opened for that
// context go with it, and a thread opened in a panel of its own stays put.

extension AppModel {
    /// Opens a panel as a tab beside the main panel, for what it's showing
    /// now. It closes when the main panel moves on, unless Mike drags it
    /// out of the main panel's group first.
    func openMainTab(_ panel: PanelID) {
        guard let main = dock.layout.group(containing: WindowDock.main) else {
            dock.open(panel, .window(.right))
            return
        }
        mainTabs.insert(panel)
        dock.open(panel, .tab(group: main.id))
    }

    /// The main panel moved to another thread, a new thread or a project:
    /// the tabs opened for what it showed before go.
    func mainContextChanged() {
        let main = dock.layout.group(containing: WindowDock.main)
        let stale = mainTabs.filter { main?.panels.contains($0) == true }
        mainTabs = []
        for panel in stale { dock.close(panel) }
        if dock.layout.contains(WindowDock.main) { dock.select(WindowDock.main) }
    }

    /// After every layout change: a tab dragged out of the main panel's
    /// group is Mike's to keep, and panels that closed let go of what they
    /// held.
    func dockChanged(_ layout: DockLayout) {
        let main = layout.group(containing: WindowDock.main)
        mainTabs = mainTabs.filter { main?.panels.contains($0) == true }
        for (threadId, transcript) in panelTranscripts where !layout.contains(WindowDock.threadPanel(threadId)) {
            transcript.show(nil)
            panelTranscripts[threadId] = nil
        }
    }

    /// Opens a thread in a panel of its own, to the right of the main panel,
    /// where it stays whatever the sidebar picks. The selected thread moves
    /// out of the main panel, which goes to a new thread.
    func openThreadInPanel(_ threadId: String) {
        let panel = WindowDock.threadPanel(threadId)
        if dock.layout.contains(panel) {
            dock.show(panel)
            return
        }
        if selection == threadId { newThread() }
        if let main = dock.layout.group(containing: WindowDock.main) {
            dock.open(panel, .edge(group: main.id, .right))
        } else {
            dock.open(panel, .window(.right))
        }
    }

    /// The transcript for a thread's own panel, made the first time.
    func panelTranscript(for threadId: String) -> TranscriptController {
        if let existing = panelTranscripts[threadId] { return existing }
        let made = TranscriptController()
        panelTranscripts[threadId] = made
        return made
    }

    /// Loads the thread for its own panel, as `select` does for the main one.
    func openPanelSession(_ threadId: String) -> ThreadSession {
        let session = session(for: threadId)
        let transcript = panelTranscript(for: threadId)
        if session.state == .idle || session.state.isFailed {
            session.open(width: transcript.width > 0 ? transcript.width : transcript.currentTableWidth)
        }
        if let thread = threads[threadId], thread.needsYou || thread.lastSeq > thread.lastSeenSeq {
            scheduleMarkSeen(threadId)
        }
        return session
    }
}

/// A thread open in a panel of its own: its transcript and composer, as in
/// the main panel, with its title line and menu in the panel's bar.
struct PinnedThreadView: View {
    @Environment(AppModel.self) private var app
    let threadId: String

    var body: some View {
        let thread = app.threads[threadId]
        let session = app.session(for: threadId)
        VStack(spacing: 0) {
            if let thread { SignInBanner(thread: thread) }
            if let thread, thread.hasPendingReviews {
                PendingReviewsButton(thread: thread)
            }
            ComposerPlacement(centred: false) {
                ZStack {
                    TranscriptView(session: session, controller: app.panelTranscript(for: threadId))
                    if session.state == .loading, session.displayCount == 0 {
                        ProgressView().controlSize(.small)
                    } else if case .failed(let message) = session.state {
                        VStack(spacing: zoomed(8)) {
                            Text(session.failureHeadline ?? "Couldn't load this thread.").font(.headline)
                            Text(message).foregroundStyle(.secondary)
                                .multilineTextAlignment(.center)
                            Button("Try again") { _ = app.openPanelSession(threadId) }
                                .tip("Load the thread again")
                        }
                    }
                }
                .layoutValue(key: PaneRole.self, value: .transcript)
                ComposerView(thread: thread, session: session, controller: app.panelTranscript(for: threadId))
                    .layoutValue(key: PaneRole.self, value: .composer)
            }
        }
        .frame(maxWidth: .infinity)
        .onAppear { _ = app.openPanelSession(threadId) }
    }
}

/// In the main panel, for a thread that's open in a panel of its own
/// (reached by Back, say): it can only show in one place at a time.
struct OpenElsewhereNote: View {
    @Environment(AppModel.self) private var app
    let threadId: String

    var body: some View {
        VStack(spacing: zoomed(10)) {
            Text("This thread is open in its own panel.")
                .font(.system(size: TypeScale.label))
                .foregroundStyle(.secondary)
            HStack(spacing: zoomed(8)) {
                Button("Go to it") { app.dock.show(WindowDock.threadPanel(threadId)) }
                    .tip("Bring its panel to the front")
                Button("Show it here") {
                    app.dock.close(WindowDock.threadPanel(threadId))
                    app.select(threadId)
                }
                .tip("Close its panel and show the thread in the main panel")
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
