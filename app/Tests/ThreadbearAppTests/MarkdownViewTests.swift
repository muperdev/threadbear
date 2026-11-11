import XCTest
@testable import Threadbear

/// A Markdown file opens drawn (Preview) until its bar's menu picks
/// Markdown, and that pick is how the next one opens. Other files always
/// open to read.
@MainActor
final class MarkdownViewTests: XCTestCase {
    override func tearDown() async throws {
        UserDefaults.standard.removeObject(forKey: MarkdownView.key)
    }

    func testMarkdownOpensAsPreviewUntilMarkdownIsPicked() {
        UserDefaults.standard.removeObject(forKey: MarkdownView.key)
        XCTAssertEqual(MarkdownView.current, .preview)
        XCTAssertFalse(MarkdownView.opensRaw(path: "/tmp/README.md"))
        XCTAssertFalse(MarkdownView.opensRaw(path: "/tmp/notes.MDX"))
        MarkdownView.current = .markdown
        XCTAssertTrue(MarkdownView.opensRaw(path: "/tmp/README.markdown"), "the pick is remembered")
        XCTAssertFalse(MarkdownView.opensRaw(path: "/tmp/hello.ts"), "code still opens to read")
    }
}
