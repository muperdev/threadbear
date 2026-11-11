import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// Where the Terminal panel's shell runs, and what it's handed. None of
/// these start a shell.
@MainActor
final class TerminalPanelTests: XCTestCase {
    private let home = NSHomeDirectory()

    func testNoThreadIsAShellInHome() {
        let target = TerminalTarget.for(nil) { _ in nil }
        XCTAssertEqual(target, TerminalTarget(key: .home, cwd: home))
    }

    func testALocalThreadGetsItsOwnShellInItsFolder() {
        let folder = FileManager.default.temporaryDirectory.path
        let thread = ThreadSummary(id: "t1", title: "Work", cwd: folder)
        let target = TerminalTarget.for(thread) { _ in nil }
        XCTAssertEqual(target, TerminalTarget(key: .thread("t1"), cwd: folder))
    }

    func testARemoteThreadSharesTheHomeShellAndSaysWhere() {
        var thread = ThreadSummary(id: "t2", title: "Elsewhere", cwd: FileManager.default.temporaryDirectory.path)
        thread.runnerId = "bruce"
        let target = TerminalTarget.for(thread) { _ in "bruce" }
        XCTAssertEqual(target.key, .home)
        XCTAssertEqual(target.cwd, home)
        XCTAssertEqual(target.note?.contains("on bruce"), true)
    }

    func testAThreadWhoseFolderHasGoneSharesTheHomeShell() {
        let thread = ThreadSummary(id: "t3", title: "Gone", cwd: "/no/such/folder/\(UUID().uuidString)")
        let target = TerminalTarget.for(thread) { _ in nil }
        XCTAssertEqual(target.key, .home)
        XCTAssertEqual(target.cwd, home)
        XCTAssertNotNil(target.note)
    }

    /// An archived thread's worktree may be another thread's now, so its
    /// terminal is the home one, never a shell in that folder.
    func testAnArchivedThreadSharesTheHomeShell() {
        let thread = ThreadSummary(id: "t4", title: "Done", cwd: FileManager.default.temporaryDirectory.path, archivedAt: Date())
        let target = TerminalTarget.for(thread) { _ in nil }
        XCTAssertEqual(target.key, .home)
        XCTAssertEqual(target.cwd, home)
        XCTAssertEqual(target.note?.contains("archived"), true)
    }

    /// `tb` reads a thread id as "I'm that thread's agent", so the shell
    /// must never have one, under either name.
    func testTheShellIsNeverHandedAThreadId() {
        let env = TerminalSession.environment(from: [
            "THREADBEAR_THREAD_ID": "abc", "THREADBARE_THREAD_ID": "abc", "PATH": "/usr/bin", "LANG": "",
        ])
        XCTAssertFalse(env.contains { $0.hasPrefix("THREADBEAR_THREAD_ID=") })
        XCTAssertFalse(env.contains { $0.hasPrefix("THREADBARE_THREAD_ID=") })
        XCTAssertTrue(env.contains("PATH=/usr/bin"))
        XCTAssertTrue(env.contains("TERM=xterm-256color"))
        XCTAssertTrue(env.contains("LANG=en_US.UTF-8"))
    }

    /// Daybreak, 10 Nov: the home shell's key was the string "home", the
    /// same namespace as thread ids, so a thread called "home" was the
    /// home shell. It's a key of its own now.
    func testAThreadCalledHomeIsNeverTheHomeShell() {
        let thread = ThreadSummary(id: "home", title: "Home", cwd: FileManager.default.temporaryDirectory.path)
        XCTAssertEqual(TerminalTarget.for(thread) { _ in nil }.key, .thread("home"))
        XCTAssertNotEqual(TerminalTarget.for(thread) { _ in nil }.key, TerminalTarget.for(nil) { _ in nil }.key)
    }

    /// Another runner's index can name a thread anything, "home" included,
    /// and archive it: that stopped the home shell Mike was using, and an
    /// archived row of its own can't stop any shell here either. Only this
    /// Mac's own archived threads stop theirs.
    func testAnotherRunnersArchivedRowNeverStopsAShell() {
        func remote(_ id: String) -> ThreadSummary {
            var t = ThreadSummary(id: id, title: id, cwd: "/tmp", archivedAt: Date())
            t.runnerId = "scoped"
            return t
        }
        let threads: [String: ThreadSummary] = [
            "home": remote("home"),
            "r1": remote("r1"),
            "t1": ThreadSummary(id: "t1", title: "Done", cwd: "/tmp", archivedAt: Date()),
            "t2": ThreadSummary(id: "t2", title: "Live", cwd: "/tmp"),
        ]
        let keys: [TerminalKey] = [.home, .thread("r1"), .thread("t1"), .thread("t2")]
        XCTAssertEqual(Terminals.archived(keys, in: threads), [.thread("t1")])
    }

    /// After a reconnect, a thread's shell whose thread has gone stops;
    /// the home shell is never one of them.
    func testReconnectStopsOnlyGoneThreadsShells() {
        let keys: [TerminalKey] = [.home, .thread("a"), .thread("b"), .thread("home")]
        XCTAssertEqual(Terminals.gone(keys, keeping: ["a"]), [.thread("b"), .thread("home")])
    }

    /// Looking at a thread with the panel open starts nothing; only `open`
    /// (⌘J, or a click on the empty panel) does.
    func testLookingStartsNoShell() {
        let terminals = Terminals()
        XCTAssertNil(terminals.existing(for: TerminalTarget(key: .thread("t1"), cwd: home)))
        XCTAssertTrue(terminals.debugSummary.isEmpty)
    }
}
