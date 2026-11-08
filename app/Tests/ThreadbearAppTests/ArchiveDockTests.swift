import XCTest
@testable import Threadbear

/// The Archived section sits at the foot of the inbox's column. Folded, its
/// heading is pinned below the scrolling list, so it stays in sight however
/// many threads there are. Open, or while searching, it's the list's last
/// section, pushed to the foot when the list is short.
final class ArchiveDockTests: XCTestCase {
    func testFoldedArchiveIsPinnedBelowTheList() {
        XCTAssertEqual(ArchiveDock.place(collapsed: true, searching: false), .pinned)
    }

    func testOpenArchiveEndsTheList() {
        XCTAssertEqual(ArchiveDock.place(collapsed: false, searching: false), .endOfList)
    }

    func testSearchingShowsArchivedMatchesInTheList() {
        // A search opens every section, folded or not.
        XCTAssertEqual(ArchiveDock.place(collapsed: true, searching: true), .endOfList)
    }

    func testTheListFillsTheColumnSoTheEndSitsAtTheFoot() {
        XCTAssertEqual(ArchiveDock.gap(viewport: 600, bottomPadding: 32, rows: 200), 368)
        // A list taller than the column has no gap: it scrolls.
        XCTAssertEqual(ArchiveDock.gap(viewport: 600, bottomPadding: 32, rows: 900), 0)
    }
}
