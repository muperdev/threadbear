import AppKit
import ThreadbearCore

/// Restarting into a new build without the window going away (Mike, 29 Oct:
/// "restart with a very, very minimal flicker, if none at all").
///
/// The running app, the outgoing one, starts the new build as a second copy
/// with `--handover-from <its pid> --handover-below <its window number>`
/// and carries on as it was. That copy, the incoming one, keeps its windows
/// invisible (alpha 0) while it connects, loads the threads and opens the
/// thread that was open (`AppUpdater.restoreAfterRestart`), in the same
/// frame (MainWindowFrame). Once that's drawn it puts its window just
/// under the outgoing one's, makes it visible there, and says "ready". The
/// outgoing app hands over being the active app if it was, says "go" with
/// anything that changed in the meantime (the thread, the list, drafts),
/// waits for "done", then hides its windows and quits. So there's always
/// one of the two windows showing, in the same place, with the same thing
/// in it.
///
/// The three messages are distributed notifications named for the
/// outgoing pid, so two Threadbears (Mike's and a dev app) never mix them
/// up. If the incoming app never says ready, the outgoing one quits anyway
/// and the incoming one takes over when it sees it go; if the incoming app
/// didn't start at all, the outgoing one falls back to quitting and
/// opening the app again, as before.
@MainActor
final class Handover {
    static let shared = Handover()

    nonisolated static let fromFlag = "--handover-from"
    nonisolated static let belowFlag = "--handover-below"
    nonisolated static let frameFlag = "--handover-frame"
    nonisolated static let readyName = Notification.Name("com.mikerosoft.threadbear.handover.ready")
    nonisolated static let goName = Notification.Name("com.mikerosoft.threadbear.handover.go")
    nonisolated static let doneName = Notification.Name("com.mikerosoft.threadbear.handover.done")
    /// How long the outgoing app waits for "ready" before quitting anyway.
    /// A launch, a connect and a thread's first page take a second or two.
    nonisolated static let readyTimeout: TimeInterval = 20
    /// How long the incoming app waits to be loaded before showing itself
    /// anyway (a runner that's down never loads anything).
    nonisolated static let loadTimeout: TimeInterval = 15
    /// After everything's loaded: a few frames for the transcript to lay
    /// out and draw before it's shown.
    nonisolated static let settleTime: TimeInterval = 0.25
    /// How long the outgoing app waits for "done" (the incoming app active
    /// and in front) before hiding its window.
    nonisolated static let doneTimeout: TimeInterval = 1

    // MARK: Pure parts, for the tests

    /// The incoming app's arguments: the outgoing app's own (a dev app's
    /// `--data-dir` and `--automation`, say), without an earlier
    /// handover's, plus this one's.
    nonisolated static func arguments(current: [String], outgoingPid: Int32, windowNumber: Int, frame: NSRect) -> [String] {
        var kept: [String] = []
        var skip = false
        for argument in current {
            if skip { skip = false; continue }
            if [fromFlag, belowFlag, frameFlag].contains(argument) { skip = true; continue }
            kept.append(argument)
        }
        return kept + [fromFlag, String(outgoingPid), belowFlag, String(windowNumber), frameFlag, NSStringFromRect(frame)]
    }

    nonisolated static func string(after flag: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: flag), index + 1 < arguments.count else { return nil }
        return arguments[index + 1]
    }

    nonisolated static func value(after flag: String, in arguments: [String]) -> Int? {
        string(after: flag, in: arguments).flatMap { Int($0) }
    }

    /// Only a window that's plainly on screen is handed over: a minimised
    /// or full-screen one, or a hidden app, restarts as before (the new
    /// window would show where the old one wasn't).
    nonisolated static func canHandOver(visible: Bool, miniaturized: Bool, fullScreen: Bool, appHidden: Bool) -> Bool {
        visible && !miniaturized && !fullScreen && !appHidden
    }

    /// The incoming app is ready to be seen: the threads are in, the
    /// thread that was open has its first page, or it's waited long enough.
    nonisolated static func loaded(threadsLoaded: Bool, connected: Bool, selectionLoaded: Bool, waited: TimeInterval) -> Bool {
        (threadsLoaded && connected && selectionLoaded) || waited >= loadTimeout
    }

    // MARK: The incoming app

    /// The outgoing app's pid, while this app is the incoming one and
    /// hasn't taken over.
    nonisolated static let incomingFrom: Int32? = value(after: fromFlag, in: CommandLine.arguments).map { Int32($0) }
    private(set) var incoming = Handover.incomingFrom != nil
    private var concealed: [ObjectIdentifier: NSWindow] = [:]
    private var observers: [NSObjectProtocol] = []
    private var exitWatch: DispatchSourceProcess?
    private var onTakeOver: (() -> Void)?

    /// Keeps one of this app's windows from showing until it takes over.
    /// The main window comes here from MainWindowFrameTracker as soon as
    /// it's in one, before it's drawn, and goes exactly where the outgoing
    /// one is (a dev app's frame isn't saved, and Mike's may not have been
    /// since it last moved); others (a floating panel) when they first show.
    func conceal(_ window: NSWindow, main: Bool = false) {
        guard incoming, concealed[ObjectIdentifier(window)] == nil else { return }
        concealed[ObjectIdentifier(window)] = window
        window.alphaValue = 0
        if main, let frame = Self.string(after: Self.frameFlag, in: CommandLine.arguments).map(NSRectFromString), frame.width > 0 {
            window.setFrame(frame, display: false)
        }
        window.ignoresMouseEvents = true
        window.animationBehavior = .none
        // A dev app's window lives a level down (tuckBehind); the incoming
        // one goes there too, so it can sit under the outgoing one.
        if CommandLine.arguments.contains("--automation") {
            window.level = NSWindow.Level(rawValue: NSWindow.Level.normal.rawValue - 1)
        }
    }

    /// The incoming app's side, from applicationDidFinishLaunching.
    /// `takeOver` starts what only one of the two apps should run (the
    /// updater, a dev app's automation), once this one is the app.
    func startIncoming(app: AppModel, takeOver: @escaping () -> Void) {
        guard let from = Self.incomingFrom else { return }
        onTakeOver = takeOver
        AppLog.write("handover", "incoming from \(from)")
        observers.append(NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: nil, queue: .main) { note in
            MainActor.assumeIsolated {
                if let window = note.object as? NSWindow { Handover.shared.conceal(window) }
            }
        })
        let center = DistributedNotificationCenter.default()
        observers.append(center.addObserver(forName: Self.goName, object: String(from), queue: .main) { note in
            let info = note.userInfo ?? [:]
            MainActor.assumeIsolated { Handover.shared.go(info, app: app) }
        })
        // The outgoing app quit (or crashed) without a "go": take over.
        let watch = DispatchSource.makeProcessSource(identifier: pid_t(from), eventMask: .exit, queue: .main)
        watch.setEventHandler {
            MainActor.assumeIsolated { Handover.shared.takeOver(activate: false, reason: "the outgoing app is gone") }
        }
        watch.resume()
        exitWatch = watch
        if kill(pid_t(from), 0) != 0 {
            self.takeOver(activate: false, reason: "the outgoing app had gone before launch")
            return
        }
        Task { @MainActor in
            let started = Date()
            while incoming {
                let selectionLoaded: Bool = {
                    guard let id = app.selection else { return true }
                    switch app.loadedSession(id)?.state {
                    case .loaded, .failed: return true
                    // A selection with no session yet is a view (a project,
                    // the schedules) or a thread about to open.
                    case nil: return app.threads[id] == nil
                    default: return false
                    }
                }()
                if Self.loaded(threadsLoaded: app.threadsLoaded, connected: app.connection == .connected,
                               selectionLoaded: selectionLoaded, waited: Date().timeIntervalSince(started)) { break }
                try? await Task.sleep(for: .milliseconds(50))
            }
            guard incoming else { return }
            try? await Task.sleep(for: .seconds(Self.settleTime))
            guard incoming else { return }
            showUnderOutgoing()
            AppLog.write("handover", String(format: "ready after %.2f s", Date().timeIntervalSince(started)))
            Self.post(Self.readyName, from: from, ["pid": Int(ProcessInfo.processInfo.processIdentifier)])
        }
    }

    /// Visible, just under the outgoing app's window: nothing to see yet,
    /// as the two are in the same place showing the same thing.
    private func showUnderOutgoing() {
        let below = Self.value(after: Self.belowFlag, in: CommandLine.arguments)
        for window in concealed.values where window.isVisible {
            if let below { window.order(.below, relativeTo: below) }
            window.alphaValue = 1
        }
    }

    private func go(_ info: [AnyHashable: Any], app: AppModel) {
        guard incoming else { return }
        // What Mike did in the outgoing window while this one loaded.
        if let mode = info["mode"] as? String, let selection = info["selection"] as? String {
            AppUpdater.restore(mode: mode, selection: selection, scroll: nil, app: app)
        }
        // The file the outgoing app saved just before "go", pictures and
        // all; the words in "go" for any it couldn't save.
        takeDrafts(app)
        if let drafts = info["drafts"] as? [String: String] {
            for (id, text) in drafts where app.drafts[id]?.text.isEmpty ?? true { app.drafts[id, default: Draft()].text = text }
        }
        takeOver(activate: info["active"] as? Bool ?? false, reason: "go")
        if let from = Self.incomingFrom { Self.post(Self.doneName, from: from, [:]) }
    }

    /// The drafts store, kept back until this incoming app takes over.
    var pendingDraftStore: DraftStore?

    /// Reads the saved drafts and starts keeping them. Not at launch: the
    /// outgoing app is still taking typing then, and a draft read early
    /// would be stale by "go" (Codex's review, 10 Nov).
    private func takeDrafts(_ app: AppModel) {
        guard let store = pendingDraftStore else { return }
        pendingDraftStore = nil
        store.restore(into: app)
        app.draftStore = store
    }

    private func takeOver(activate: Bool, reason: String) {
        guard incoming else { return }
        // Gone without a "go": what it last saved is all there is.
        takeDrafts(AppModel.shared)
        incoming = false
        exitWatch?.cancel()
        exitWatch = nil
        observers.forEach { NotificationCenter.default.removeObserver($0); DistributedNotificationCenter.default().removeObserver($0) }
        observers = []
        for window in concealed.values {
            window.alphaValue = 1
            window.ignoresMouseEvents = false
        }
        concealed = [:]
        if activate {
            NSApp.activate()
            MainWindowFrame.window?.makeKeyAndOrderFront(nil)
        }
        AppLog.write("handover", "took over (\(reason))" + (activate ? ", in front" : ""))
        onTakeOver?()
        onTakeOver = nil
    }

    // MARK: The outgoing app

    private var outgoing = false

    /// The main window, if it can be handed over.
    static func windowToHandOver() -> NSWindow? {
        guard let window = MainWindowFrame.window else { return nil }
        let ok = canHandOver(visible: window.isVisible, miniaturized: window.isMiniaturized,
                             fullScreen: window.styleMask.contains(.fullScreen), appHidden: NSApp.isHidden)
        return ok ? window : nil
    }

    /// Starts `bundle` as the incoming app and quits once it's showing.
    /// `fallback` restarts the old way if it can't be started.
    func handOver(to bundle: URL, window: NSWindow, app: AppModel, fallback: @escaping () -> Void) {
        guard !outgoing else { return }
        outgoing = true
        let pid = ProcessInfo.processInfo.processIdentifier
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        configuration.activates = false
        configuration.addsToRecentItems = false
        configuration.arguments = Self.arguments(current: Array(CommandLine.arguments.dropFirst()), outgoingPid: pid,
                                                 windowNumber: window.windowNumber, frame: window.frame)
        // Where a dev app's bundle and data are, which `open` wouldn't pass on.
        configuration.environment = ProcessInfo.processInfo.environment.filter { $0.key.hasPrefix("THREADBEAR_") || $0.key.hasPrefix("THREADBARE_") }
        listen(Self.readyName, from: pid) { info in
            Handover.shared.ready(incomingPid: (info["pid"] as? Int).map { pid_t($0) }, app: app)
        }
        AppLog.write("handover", "starting \(bundle.path) to hand over to")
        NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { running, error in
            let incomingPid = running?.processIdentifier
            let message = error?.localizedDescription
            Task { @MainActor in
                guard let incomingPid else {
                    AppLog.write("handover", "couldn't start the new app (\(message ?? "no app")); restarting the old way")
                    Handover.shared.stopListening()
                    fallback()
                    return
                }
                AppLog.write("handover", "incoming is pid \(incomingPid)")
                try? await Task.sleep(for: .seconds(Self.readyTimeout))
                guard Handover.shared.outgoing, !Handover.shared.quitting else { return }
                Handover.shared.stopListening()
                if kill(incomingPid, 0) == 0 {
                    // It takes over when it sees this app go.
                    AppLog.write("handover", "no ready after \(Int(Self.readyTimeout)) s; quitting for it")
                    Handover.shared.quit()
                } else {
                    AppLog.write("handover", "the new app went away; restarting the old way")
                    fallback()
                }
            }
        }
    }

    private func ready(incomingPid: pid_t?, app: AppModel) {
        guard outgoing else { return }
        let pid = ProcessInfo.processInfo.processIdentifier
        let active = NSApp.isActive
        if active, let incomingPid, let incoming = NSRunningApplication(processIdentifier: incomingPid) {
            NSApp.yieldActivation(to: incoming)
        }
        var info: [String: Any] = ["active": active]
        let state = AppUpdater.restoreState(app: app)
        info["mode"] = state.mode
        info["selection"] = state.selection
        // Words only: a picture's chip would be a mark with no picture behind it.
        // Not an edit's words: without its edit they'd go as a new message.
        let drafts = DraftStore.kept(drafts: app.drafts, editing: app.editing).compactMapValues { draft in
            let words = InlinePictures.words(draft.text)
            return words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : words
        }
        if !drafts.isEmpty { info["drafts"] = drafts }
        // On disk before "go", for the incoming app to read with pictures.
        // Messages held for a runner go with them, so this app stops
        // sending them (AppModel.post).
        app.handingOver = true
        app.draftStore?.save(app, leaving: true)
        // The file is the incoming app's from here.
        app.draftStore = nil
        stopListening()
        listen(Self.doneName, from: pid) { _ in Handover.shared.quit() }
        Self.post(Self.goName, from: pid, info)
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(Self.doneTimeout))
            Handover.shared.quit()
        }
    }

    private var quitting = false
    private var outgoingObservers: [NSObjectProtocol] = []

    private func listen(_ name: Notification.Name, from pid: Int32, _ handle: @escaping @MainActor ([AnyHashable: Any]) -> Void) {
        outgoingObservers.append(DistributedNotificationCenter.default().addObserver(forName: name, object: String(pid), queue: .main) { note in
            let info = note.userInfo ?? [:]
            MainActor.assumeIsolated { handle(info) }
        })
    }

    private func stopListening() {
        outgoingObservers.forEach { DistributedNotificationCenter.default().removeObserver($0) }
        outgoingObservers = []
    }

    private func quit() {
        guard outgoing, !quitting else { return }
        quitting = true
        stopListening()
        AppLog.write("handover", "handed over; quitting")
        for window in NSApp.windows where window.isVisible {
            window.animationBehavior = .none
            window.orderOut(nil)
        }
        NSApp.terminate(nil)
    }

    nonisolated private static func post(_ name: Notification.Name, from pid: Int32, _ info: [String: Any]) {
        // deliverImmediately: the other app is usually not the active one,
        // and an inactive app's distributed notifications wait otherwise.
        DistributedNotificationCenter.default().postNotificationName(name, object: String(pid), userInfo: info, deliverImmediately: true)
    }
}
