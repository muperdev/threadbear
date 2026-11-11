import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// The runner's name after the project on a sidebar row ("Threadbear • M5").
@MainActor
final class RunnerLabelTests: XCTestCase {
    private let bruce = RemoteRunner(id: "r-bruce", name: "bruce", online: true, rpcUrl: nil, tokenFile: nil)
    private let m5 = RunnerInfo(id: "r-m5", name: "M5", isSelf: true)

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
    }

    private func thread(on runnerId: String? = nil) -> ThreadSummary {
        var thread = ThreadSummary(id: "t1", title: "A thread", status: .idle)
        thread.runnerId = runnerId
        return thread
    }

    func testEveryRowNamesItsRunnerOnceThereIsMoreThanOne() {
        let app = AppModel()
        app.runners = [m5]
        app.remoteRunners = [bruce]
        XCTAssertEqual(app.runnerLabel(of: thread()), "M5")
        XCTAssertEqual(app.runnerLabel(of: thread(on: "r-bruce")), "bruce")
    }

    /// Anyone with one machine would see the same name on every row.
    func testNothingWithOnlyThisMac() {
        let app = AppModel()
        app.runners = [m5]
        XCTAssertNil(app.runnerLabel(of: thread()))
    }

    /// Before runners.list has answered: no name, rather than a wrong one.
    func testNothingUntilThisMacsNameIsKnown() {
        let app = AppModel()
        app.remoteRunners = [bruce]
        XCTAssertNil(app.runnerLabel(of: thread()))
    }
}
