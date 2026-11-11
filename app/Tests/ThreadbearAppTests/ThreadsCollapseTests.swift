import AppKit
import BearDock
import XCTest
@testable import Threadbear

/// ⌘/ folds the Threads panel away and back without losing its place or
/// width; Hide Threads still takes it out of the layout.
@MainActor
final class ThreadsCollapseTests: XCTestCase {
    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
        UserDefaults.standard.removeObject(forKey: AppModel.threadsCollapsedKey)
    }

    override func tearDown() {
        UserDefaults.standard.removeObject(forKey: AppModel.threadsCollapsedKey)
        super.tearDown()
    }

    func testCollapsingKeepsThePanelInTheLayout() {
        let app = AppModel()
        app.dock.setLayout(WindowDock.defaultLayout)
        let before = app.dock.layout
        XCTAssertTrue(app.threadsShowing)

        app.toggleThreadsCollapsed()
        XCTAssertTrue(app.threadsCollapsed)
        XCTAssertFalse(app.threadsShowing)
        XCTAssertEqual(app.dock.layout, before, "same place and width for when it comes back")

        app.toggleThreadsCollapsed()
        XCTAssertFalse(app.threadsCollapsed)
        XCTAssertTrue(app.threadsShowing)
        XCTAssertEqual(app.dock.layout, before)
    }

    func testItIsRememberedAcrossLaunches() {
        let app = AppModel()
        app.dock.setLayout(WindowDock.defaultLayout)
        app.toggleThreadsCollapsed()
        XCTAssertTrue(AppModel().threadsCollapsed)
    }

    func testShowThreadsBringsAFoldedPanelBack() {
        let app = AppModel()
        app.dock.setLayout(WindowDock.defaultLayout)
        app.toggleThreadsCollapsed()
        app.toggleThreadsPanel()
        XCTAssertTrue(app.threadsShowing, "Show Threads shows it, rather than taking it out of the layout")
    }

    func testCollapseAfterHideThreadsPutsItBack() {
        let app = AppModel()
        app.dock.setLayout(WindowDock.defaultLayout)
        app.toggleThreadsPanel()
        XCTAssertFalse(app.dock.layout.contains(WindowDock.threads))
        app.toggleThreadsCollapsed()
        XCTAssertTrue(app.threadsShowing)
    }
}
