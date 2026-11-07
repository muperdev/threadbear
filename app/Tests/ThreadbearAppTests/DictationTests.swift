import AppKit
import XCTest
@testable import Threadbear

@MainActor
final class DictationTests: XCTestCase {
    /// Live words were put in as a bare string, with no colour, so they
    /// drew black on the dark composer until the final words replaced them
    /// (Behrooz, 7 Nov). They take what typing would.
    func testLiveWordsLookLikeTypedText() throws {
        let composer = ComposerTextView()
        composer.typingAttributes = [.foregroundColor: NSColor.textColor, .font: NSFont.systemFont(ofSize: 15)]
        Dictation.replace(NSRange(location: 0, length: 0), with: "Okay, so I'm trying", undo: false, in: composer)
        XCTAssertEqual(composer.string, "Okay, so I'm trying")
        let storage = try XCTUnwrap(composer.textStorage)
        let color = storage.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
        XCTAssertEqual(color, NSColor.textColor)
        let font = storage.attribute(.font, at: 0, effectiveRange: nil) as? NSFont
        XCTAssertEqual(font?.pointSize, 15)

        // Replacing them as more is heard keeps the look too.
        Dictation.replace(NSRange(location: 0, length: 19), with: "Okay, so I'm trying this", undo: false, in: composer)
        XCTAssertEqual(composer.string, "Okay, so I'm trying this")
        XCTAssertEqual(storage.attribute(.foregroundColor, at: 20, effectiveRange: nil) as? NSColor, NSColor.textColor)
    }

    func testSpacing() {
        XCTAssertEqual(Dictation.spaced("hello", before: "", after: ""), "hello")
        XCTAssertEqual(Dictation.spaced("hello", before: "x", after: "y"), " hello ")
        XCTAssertEqual(Dictation.spaced("hello", before: " ", after: "."), "hello")
    }
}
