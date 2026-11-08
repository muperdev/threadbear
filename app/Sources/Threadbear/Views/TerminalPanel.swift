import AppKit
import SwiftUI
import SwiftTerm
import ThreadbearCore

/// The Terminal panel (⌘J, View > Show Terminal): a real terminal in the
/// dock, as VS Code and T3 Code have under their editor. SwiftTerm draws it
/// and runs your login shell in a pty, so colours, the cursor keys and
/// full-screen programs (vim, htop, claude) work as in Terminal.
///
/// Each thread has a shell of its own, started in the thread's folder (its
/// worktree when it has one) and kept running while you look at other
/// threads, so switching back finds it as you left it. A thread on another
/// Mac or the cloud gets a shell on this Mac in your home folder, with a
/// line saying so, as its folder isn't here. With no thread open it's a
/// shell in your home folder too. Shells stop when the app quits.
@MainActor
@Observable
final class Terminals {
    static let shared = Terminals()

    @ObservationIgnored private var sessions: [String: TerminalSession] = [:]
    /// Bumped to ask the showing terminal to take the keyboard.
    private(set) var focusTick = 0

    func session(for target: TerminalTarget) -> TerminalSession {
        if let s = sessions[target.key] { return s }
        let s = TerminalSession(target: target)
        sessions[target.key] = s
        return s
    }

    /// A new shell in place of one that has exited.
    func restart(_ key: String) {
        guard let old = sessions[key] else { return }
        let fresh = TerminalSession(target: old.target)
        sessions[key] = fresh
        old.view.removeFromSuperview()
        focusTick += 1
    }

    func requestFocus() { focusTick += 1 }

    /// For the automation and `dump`: what's running where.
    var debugSummary: [String] {
        sessions.values.sorted { $0.target.key < $1.target.key }.map { "\($0.target.key) \($0.exited ? "exited" : "running") in \($0.target.cwd)" }
    }
}

/// Where a terminal runs: its key (the thread, or "home") and folder.
struct TerminalTarget: Equatable {
    var key: String
    var cwd: String
    var threadId: String?
    /// Said above the terminal when its folder isn't the thread's.
    var note: String?

    @MainActor
    static func `for`(_ thread: ThreadSummary?, runnerName: (ThreadSummary) -> String?) -> TerminalTarget {
        let home = NSHomeDirectory()
        guard let thread else { return TerminalTarget(key: "home", cwd: home) }
        if thread.isRemote {
            let where_ = runnerName(thread).map { "on \($0)" } ?? "on another machine"
            return TerminalTarget(key: "home", cwd: home, note: "This thread runs \(where_), so this terminal is on this Mac, in your home folder.")
        }
        var isDir: ObjCBool = false
        guard !thread.cwd.isEmpty, FileManager.default.fileExists(atPath: thread.cwd, isDirectory: &isDir), isDir.boolValue else {
            return TerminalTarget(key: "home", cwd: home, note: "This thread's folder isn't on this Mac any more, so this terminal is in your home folder.")
        }
        return TerminalTarget(key: thread.id, cwd: thread.cwd, threadId: thread.id)
    }
}

/// One shell in one pty, and the view that draws it.
@MainActor
@Observable
final class TerminalSession: NSObject, LocalProcessTerminalViewDelegate {
    let target: TerminalTarget
    @ObservationIgnored let view: LocalProcessTerminalView
    private(set) var exited = false
    private(set) var title = ""

    init(target: TerminalTarget) {
        self.target = target
        view = LocalProcessTerminalView(frame: NSRect(x: 0, y: 0, width: 600, height: 240))
        super.init()
        view.processDelegate = self
        view.font = TerminalStyle.font
        TerminalStyle.apply(to: view)
        start()
    }

    private func start() {
        let shell = ProcessInfo.processInfo.environment["SHELL"].flatMap { $0.isEmpty ? nil : $0 } ?? "/bin/zsh"
        // A login shell ("-zsh" as its name), so it reads your profile and
        // has the PATH Terminal gives you, which an app's own doesn't.
        let name = "-" + (shell as NSString).lastPathComponent
        view.startProcess(executable: shell, args: [], environment: Self.environment(threadId: target.threadId), execName: name, currentDirectory: target.cwd)
    }

    static func environment(threadId: String?) -> [String] {
        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["TERM_PROGRAM"] = "Threadbear"
        if (env["LANG"] ?? "").isEmpty { env["LANG"] = "en_US.UTF-8" }
        if let threadId { env["THREADBEAR_THREAD_ID"] = threadId } else { env.removeValue(forKey: "THREADBEAR_THREAD_ID") }
        return env.map { "\($0.key)=\($0.value)" }
    }

    /// Text typed into the shell, for the automation.
    func type(_ text: String) {
        view.send(txt: text)
    }

    // MARK: LocalProcessTerminalViewDelegate

    nonisolated func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

    nonisolated func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
        MainActor.assumeIsolated { self.title = title }
    }

    nonisolated func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

    nonisolated func processTerminated(source: TerminalView, exitCode: Int32?) {
        MainActor.assumeIsolated { self.exited = true }
    }
}

/// The terminal's font and colours: the app's code font, on the transcript's
/// own background, light or dark with the app.
enum TerminalStyle {
    @MainActor static var font: NSFont { .monospacedSystemFont(ofSize: Theme.codeSize + 1, weight: .regular) }

    @MainActor
    static func apply(to view: TerminalView) {
        let dark = view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        view.nativeBackgroundColor = dark ? NSColor(srgbRed: 0.11, green: 0.11, blue: 0.12, alpha: 1) : NSColor(white: 1, alpha: 1)
        view.nativeForegroundColor = dark ? NSColor(white: 0.9, alpha: 1) : NSColor(white: 0.12, alpha: 1)
        view.caretColor = .controlAccentColor
        view.selectedTextBackgroundColor = NSColor.selectedTextBackgroundColor
    }
}

struct TerminalPanel: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        let target = TerminalTarget.for(app.selectedThread) { app.runnerName(of: $0) }
        let session = Terminals.shared.session(for: target)
        VStack(spacing: 0) {
            if let note = target.note {
                Text(note)
                    .font(.system(size: TypeScale.detail))
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, zoomed(10))
                    .padding(.vertical, zoomed(5))
                    .tip(note)
            }
            ZStack(alignment: .bottom) {
                TerminalHost(session: session, focusTick: Terminals.shared.focusTick)
                    .dockDragDisabled()
                if session.exited {
                    HStack(spacing: zoomed(8)) {
                        Text("The shell has exited.").foregroundStyle(.secondary)
                        Button("New Shell") { Terminals.shared.restart(target.key) }
                            .tip("Start a new shell in \((target.cwd as NSString).abbreviatingWithTildeInPath)")
                    }
                    .font(.system(size: TypeScale.label))
                    .padding(zoomed(8))
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
                    .padding(zoomed(10))
                }
            }
        }
    }
}

/// Hosts the session's view, swapping it when the thread changes, and
/// gives it the keyboard when asked (⌘J).
private struct TerminalHost: NSViewRepresentable {
    let session: TerminalSession
    let focusTick: Int

    /// Holds the terminal a little in from the panel's edges, on the
    /// terminal's own background.
    final class Container: NSView {
        var focusTick = 0
        static var inset: NSSize { NSSize(width: zoomed(10), height: zoomed(6)) }

        override init(frame: NSRect) {
            super.init(frame: frame)
            wantsLayer = true
        }

        required init?(coder: NSCoder) { fatalError() }

        override func layout() {
            super.layout()
            let inset = Self.inset
            for view in subviews { view.frame = bounds.insetBy(dx: inset.width, dy: inset.height) }
        }

        override func viewDidChangeEffectiveAppearance() {
            super.viewDidChangeEffectiveAppearance()
            for case let view as TerminalView in subviews {
                TerminalStyle.apply(to: view)
                layer?.backgroundColor = view.nativeBackgroundColor.cgColor
            }
        }
    }

    func makeNSView(context: Context) -> Container {
        let c = Container()
        c.focusTick = focusTick - 1
        return c
    }

    func updateNSView(_ container: Container, context: Context) {
        let view = session.view
        if view.superview !== container {
            container.subviews.forEach { $0.removeFromSuperview() }
            view.removeFromSuperview()
            view.autoresizingMask = []
            container.addSubview(view)
            container.needsLayout = true
            TerminalStyle.apply(to: view)
            container.layer?.backgroundColor = view.nativeBackgroundColor.cgColor
        }
        let font = TerminalStyle.font
        if view.font.pointSize != font.pointSize { view.font = font }
        if container.focusTick != focusTick {
            container.focusTick = focusTick
            DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
        }
    }

    static func dismantleNSView(_ container: Container, coordinator: ()) {
        // The session keeps the view (and its shell) for next time.
        container.subviews.forEach { $0.removeFromSuperview() }
    }
}
