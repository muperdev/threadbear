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
}
