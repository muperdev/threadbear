import AppKit
import XCTest
@testable import Threadbear

/// A picture the agent writes as Markdown, with a path on this Mac, is
/// drawn in the text (MarkdownRenderer.picture).
@MainActor
final class MarkdownPictureTests: XCTestCase {
    private func png(width: Int, height: Int) throws -> String {
        let rep = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
                                                 hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        let path = NSTemporaryDirectory() + "md-picture-\(UUID().uuidString).png"
        try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: URL(fileURLWithPath: path))
        addTeardownBlock { try? FileManager.default.removeItem(atPath: path) }
        return path
    }

    private func attachment(_ text: NSAttributedString) -> (NSTextAttachment, NSRange)? {
        var found: (NSTextAttachment, NSRange)?
        text.enumerateAttribute(.attachment, in: NSRange(location: 0, length: text.length)) { value, range, stop in
            if let a = value as? NSTextAttachment { found = (a, range); stop.pointee = true }
        }
        return found
    }

    func testALocalPictureIsDrawnFittedAndStillOpensTheFile() throws {
        let path = try png(width: 2000, height: 1000)
        let text = MarkdownRenderer.render("Here it is:\n\n![The chart](\(path))\n\nDone.").string
        let (picture, range) = try XCTUnwrap(attachment(text))
        XCTAssertLessThanOrEqual(picture.bounds.width, MarkdownRenderer.pictureMaxWidth)
        XCTAssertLessThanOrEqual(picture.bounds.height, MarkdownRenderer.pictureMaxHeight)
        XCTAssertEqual(picture.bounds.width / picture.bounds.height, 2, accuracy: 0.05)
        XCTAssertEqual((text.attribute(.link, at: range.location, effectiveRange: nil) as? URL)?.path, path)
        // Its line can grow to the picture's height.
        let style = try XCTUnwrap(text.attribute(.paragraphStyle, at: range.location, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(style.maximumLineHeight, 0)
    }

    func testAPictureInAFolderWithASpaceIsDrawn() throws {
        // Every thread's media is under ~/Library/Application Support.
        let folder = NSTemporaryDirectory() + "md picture \(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(atPath: folder) }
        let path = folder + "/before shot.png"
        try FileManager.default.moveItem(atPath: try png(width: 400, height: 300), toPath: path)
        let text = MarkdownRenderer.render("![Before](\(path)) ![After](\(path))").string
        let (_, range) = try XCTUnwrap(attachment(text))
        XCTAssertEqual((text.attribute(.link, at: range.location, effectiveRange: nil) as? URL)?.path, path)
        XCTAssertFalse(text.string.contains("]("))
    }

    func testAMissingFileOrAWebPictureStaysALink() {
        let gone = MarkdownRenderer.render("![Gone](/tmp/no-such-picture-\(UUID().uuidString).png)").string
        XCTAssertNil(attachment(gone))
        XCTAssertTrue(gone.string.contains("[Gone]"))
        let web = MarkdownRenderer.render("![Logo](https://example.com/logo.png)").string
        XCTAssertNil(attachment(web))
        XCTAssertTrue(web.string.contains("[Logo]"))
    }
}
