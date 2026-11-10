import Foundation
import XCTest
@testable import ThreadbearCore

/// A file dropped in the composer goes into the message as its path in
/// backticks; the composer and the bubble draw it as a chip with the
/// file's name.
final class FileMentionsTests: XCTestCase {
    private let exists: (String) -> Bool = { ["/Users/b/My Report.html", "/tmp/a", "/Users/b/dir/"].contains($0) }

    func testABacktickedPathOfAFileIsAMention() {
        let text = "Look at `/Users/b/My Report.html` please"
        let found = CardMentions.find(in: text, fileExists: exists)
        XCTAssertEqual(found.map(\.kind), [.file])
        XCTAssertEqual(found.first?.title, "My Report.html")
        XCTAssertEqual(found.first?.url, "/Users/b/My Report.html")
        XCTAssertEqual((text as NSString).substring(with: found[0].range), "`/Users/b/My Report.html`")
    }

    func testOnlyFilesThatAreThere() {
        XCTAssertEqual(CardMentions.find(in: "Call `/api/users` and `/tmp/b`", fileExists: exists).count, 0)
        XCTAssertEqual(CardMentions.find(in: "no path /tmp/a here", fileExists: exists).count, 0, "only in backticks")
        XCTAssertEqual(CardMentions.find(in: "`/tmp/a\n` and `relative/a`", fileExists: { _ in true }).count, 0, "one line, absolute")
        XCTAssertEqual(CardMentions.find(in: "```/tmp/a```", fileExists: exists).count, 0, "not a code fence")
    }

    func testAFolderIsNamedWithoutItsSlash() {
        XCTAssertEqual(CardMentions.find(in: "`/Users/b/dir/`", fileExists: exists).first?.title, "dir")
    }

    func testFilesCardsAndThreadsInOrder() {
        let text = "`/tmp/a` [c](https://notion.so/a) `/Users/b/dir/` [t](thread:x)"
        XCTAssertEqual(CardMentions.find(in: text, fileExists: exists).map(\.kind), [.file, .card, .file, .thread])
    }

    func testAPathInsideALinkIsTheLinks() {
        let found = CardMentions.find(in: "[see `/tmp/a`](https://notion.so/a)", fileExists: exists)
        XCTAssertEqual(found.map(\.kind), [.card])
    }
}
