import XCTest
@testable import Threadbear

/// The pane bar names a file or folder by its own name, with its full path
/// on hover, whatever the link to it was called.
final class PanePreviewTitleTests: XCTestCase {
    func testFileShowsItsNameAndFullPath() {
        let preview = PanePreview(target: .file("/Users/me/repo/docs/plan.md"), title: "the plan", from: .highlights)
        XCTAssertEqual(preview.barTitle, "plan.md")
        XCTAssertEqual(preview.barTip, "/Users/me/repo/docs/plan.md")
    }

    func testAnotherMacsFileShowsItsPathThereNotTheCachedCopy() {
        var preview = PanePreview(target: .file("/cache/abc123/plan.md"), title: "plan", from: .highlights)
        preview.away = "bruce"
        preview.remotePath = "/Users/mike/repo/plan.md"
        XCTAssertEqual(preview.barTitle, "plan.md")
        XCTAssertEqual(preview.barTip, "/Users/mike/repo/plan.md on bruce")
    }

    func testFolderShowsItsName() {
        let preview = PanePreview(target: .folder("/Users/me/repo/docs"), title: "docs folder", from: .highlights)
        XCTAssertEqual(preview.barTitle, "docs")
        XCTAssertEqual(preview.barTip, "/Users/me/repo/docs")
    }

    func testPageKeepsItsTitleAndHasNoTip() {
        let preview = PanePreview(target: .page(URL(string: "https://example.com/a")!), title: "Example", from: .highlights)
        XCTAssertEqual(preview.barTitle, "Example")
        XCTAssertNil(preview.barTip)
    }
}
