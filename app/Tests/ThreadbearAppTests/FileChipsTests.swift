import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// A file that isn't a picture, dropped, pasted or picked in the composer:
/// a chip with its icon and name among the words, and its full path in
/// backticks in the message the agent gets.
@MainActor
final class FileChipsTests: XCTestCase {
    private var windows: [NSWindow] = []
    private var folder: URL!

    override func setUpWithError() throws {
        _ = NSApplication.shared
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("file-chips-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        windows.removeAll()
        try? FileManager.default.removeItem(at: folder)
    }

    private func composer() -> ComposerTextView {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 500, height: 200), styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        let textView = ComposerTextField.makeTextView()
        textView.frame = NSRect(x: 0, y: 0, width: 500, height: 200)
        window.contentView = textView
        windows.append(window)
        return textView
    }

    private func file(_ name: String) throws -> URL {
        let url = folder.appendingPathComponent(name)
        try Data("<html></html>".utf8).write(to: url)
        return url
    }

    private func fileChips(_ textView: NSTextView) -> [CardAttachment] {
        var found: [CardAttachment] = []
        let storage = textView.textStorage!
        storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { value, _, _ in
            if let chip = value as? CardAttachment, chip.kind == .file { found.append(chip) }
        }
        return found
    }

    func testADroppedFileIsAChipAndItsPathInTheDraft() throws {
        let url = try file("journal comparison.html")
        let path = url.standardizedFileURL.path
        let textView = composer()
        textView.replaceText("read")
        textView.setSelectedRange(NSRange(location: 4, length: 0))
        textView.insertFiles([url])
        XCTAssertEqual(textView.string, "read \u{FFFC} ", "one chip, spaced, not the path's words")
        XCTAssertEqual(fileChips(textView).map(\.title), ["journal comparison.html"])
        XCTAssertEqual(textView.draftNow.text, "read `\(path)` ")
        XCTAssertEqual(textView.selectedRange().location, 7, "the cursor goes after it")
    }

    func testADraftWithAPathShowsItAsAChip() throws {
        let path = try file("a.html").standardizedFileURL.path
        let textView = composer()
        textView.show(Draft(text: "see `\(path)` and `/no/such/file`"))
        XCTAssertEqual(fileChips(textView).map(\.title), ["a.html"])
        XCTAssertEqual(textView.string, "see \u{FFFC} and `/no/such/file`", "a path that isn't a file stays words")
        XCTAssertEqual(textView.draftNow.text, "see `\(path)` and `/no/such/file`")
    }

    func testTheBubbleDrawsItAsAChipLinkingTheFile() throws {
        let url = try file("b.html")
        let text = RowRenderer.userText(UserMessage(text: "open `\(url.standardizedFileURL.path)`"))
        XCTAssertEqual(text.string, "open \u{FFFC}")
        XCTAssertEqual((text.attribute(.link, at: 5, effectiveRange: nil) as? URL)?.path, url.standardizedFileURL.path)
        XCTAssertNotNil(text.attribute(.attachment, at: 5, effectiveRange: nil))
    }
}
