import XCTest
@testable import ThreadbearCore

final class PathLinksTests: XCTestCase {
    func testSpacesInFilePathDestinationsAreEscaped() {
        let shots = "![Before](/Users/me/Library/Application Support/Threadbear/a.png) ![After](~/My Shots/b (2).png)"
        XCTAssertEqual(PathLinks.escapeDestinations(shots),
                       "![Before](/Users/me/Library/Application%20Support/Threadbear/a.png) ![After](~/My%20Shots/b%20(2).png)")
    }

    func testOtherDestinationsStayAsWritten() {
        for text in [
            "[x](https://a.b/c d)",
            "[plan](/a/b.md)",
            "[titled](/a/b c.png \"A title\")",
            "Write `![x](/a b.png)` like that",
            "```\n![x](/a b.png)\n```",
            "[open](/a b",
        ] {
            XCTAssertEqual(PathLinks.escapeDestinations(text), text)
        }
        // Outside a code span on the same line it's still escaped.
        XCTAssertEqual(PathLinks.escapeDestinations("`[x](/a b)` [y](/a b)"), "`[x](/a b)` [y](/a%20b)")
    }

    func testALongLineOfUnclosedLinksStaysQuick() {
        let line = String(repeating: "[a](/x y ", count: 4_000)
        let clock = ContinuousClock()
        let took = clock.measure { _ = PathLinks.escapeDestinations(line) }
        XCTAssertLessThan(took, .seconds(1))
    }

    // Cross-lab review, 10 Nov 2026.
    func testTitlesTrailingSpacesAndCodeInListsStayAsWritten() {
        for text in [
            "[plan](/tmp/plan.md 'Read this')",
            "[plan](/tmp/plan.md (Read this))",
            "- Example:\n  ~~~markdown\n  [x](/a b)\n  ~~~",
            "- Example:\n  ```\n  [x](/a b)\n  ```\n- More",
            "Write `x\n![y](/a b.png)` like that",
        ] {
            XCTAssertEqual(PathLinks.escapeDestinations(text), text, text)
        }
        // A space before the `)` isn't part of the path.
        XCTAssertEqual(PathLinks.escapeDestinations("[a](/tmp/my plan.md )"), "[a](/tmp/my%20plan.md )")
        // After a fence closes, links are escaped again.
        XCTAssertEqual(PathLinks.escapeDestinations("```\n[x](/a b)\n```\n[y](/a b)"), "```\n[x](/a b)\n```\n[y](/a%20b)")
    }

    func testManyCodeSpansAndLinksStayQuick() {
        let block = String(repeating: "`x` [y](/a b) ", count: 20_000)
        let clock = ContinuousClock()
        var out = ""
        let took = clock.measure { out = PathLinks.escapeDestinations(block) }
        XCTAssertLessThan(took, .seconds(2))
        XCTAssertTrue(out.hasPrefix("`x` [y](/a%20b) "))
    }

    // The second pass of the cross-lab review, 10 Nov 2026.
    func testEscapedTitlesLongerFencesAndMatchingBackticks() {
        for text in [
            "[plan](/tmp/plan.md 'Bob\\'s plan')",
            "~~~~markdown\n~~~\n![x](/a b.png)\n~~~\n~~~~",
            "``a `[x](/a b)` b``",
        ] {
            XCTAssertEqual(PathLinks.escapeDestinations(text), text, text)
        }
        // A fence that closes with trailing words isn't closed by that line.
        XCTAssertEqual(PathLinks.escapeDestinations("```\n``` no\n[x](/a b)"), "```\n``` no\n[x](/a b)")
        // A lone backtick is plain text, so a link after it is still escaped.
        XCTAssertEqual(PathLinks.escapeDestinations("a ` b [x](/a b)"), "a ` b [x](/a%20b)")
    }

    func testALongRunOfSpacesStaysQuick() {
        let text = "[x](/tmp/a" + String(repeating: " ", count: 100_000) + "b)"
        let clock = ContinuousClock()
        var out = ""
        let took = clock.measure { out = PathLinks.escapeDestinations(text) }
        XCTAssertLessThan(took, .seconds(2))
        XCTAssertTrue(out.hasSuffix("%20b)"))
    }

    // The third pass of the cross-lab review, 10 Nov 2026.
    func testEscapedBackticksAndFencesInQuotesAndLists() {
        for text in [
            "\\` example `[x](/a b)`",
            "> ~~~\n> [x](/a b)\n> ~~~",
            "- ~~~\n  [x](/a b)\n  ~~~",
            "1. ```\n   [x](/a b)\n   ```",
        ] {
            XCTAssertEqual(PathLinks.escapeDestinations(text), text, text)
        }
        XCTAssertEqual(PathLinks.escapeDestinations("> ~~~\n> code\n> ~~~\n\n[y](/a b)"), "> ~~~\n> code\n> ~~~\n\n[y](/a%20b)")
    }
}
