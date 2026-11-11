import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// Move to ▸ a runner goes straight away when there's nothing to decide,
/// and the sheet only comes up when there is.
@MainActor
final class MoveToTests: XCTestCase {
    private let cloud = try! JSONDecoder().decode(RunnerInfo.self, from: Data(#"{"id":"r2","name":"Hetzner Cloud","online":true}"#.utf8))

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
    }

    private func connected() async throws -> (AppModel, HoldingTransport) {
        let app = AppModel()
        let transport = HoldingTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        app.useClientForTesting(client)
        app.threads["th1"] = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"th1","title":"Fix it","lastSeq":12}"#.utf8))
        return (app, transport)
    }

    private func until(_ condition: () -> Bool) async throws {
        for _ in 0..<400 where !condition() {
            try await Task.sleep(for: .milliseconds(5))
        }
    }

    func testACleanAnswerMovesWithoutTheSheetAndTheRowSpinsFromTheClick() async throws {
        let (app, transport) = try await connected()
        app.openMove("th1", to: cloud)
        XCTAssertEqual(app.threads["th1"]?.moving?.toName, "Hetzner Cloud", "shown before the runner has answered")
        try await until { transport.waiting("threads.movePreview") }
        transport.answer("threads.movePreview", ["cwd": "/home/mike/x", "accountId": "claude-main", "accounts": [], "leftBehind": [], "problems": [], "sourceChecked": true])
        try await until { transport.waiting("threads.move") }
        XCTAssertTrue(transport.waiting("threads.move"))
        XCTAssertNil(app.moveRequest)
        XCTAssertNil(transport.params("threads.move")?["cwd"], "goes on the target's own guess")
    }

    func testSomethingThatWontComeOpensTheSheetAndDoesNotMove() async throws {
        let (app, transport) = try await connected()
        app.openMove("th1", to: cloud)
        try await until { transport.waiting("threads.movePreview") }
        transport.answer("threads.movePreview", ["cwd": "/home/mike/x", "accountId": "claude-main", "accounts": [], "leftBehind": ["1 background job"], "problems": []])
        try await until { app.moveRequest != nil }
        XCTAssertEqual(app.moveRequest?.runnerId, "r2")
        XCTAssertEqual(app.moveRequest?.answer?.leftBehind, ["1 background job"], "the sheet opens with the answer it was opened for")
        XCTAssertNil(app.threads["th1"]?.moving, "the sheet is asking now, so the row stops saying it's moving")
        XCTAssertEqual(transport.count("threads.move"), 0)
    }

    func testAnOlderRunnerThatCantSayWhatItLeavesOpensTheSheet() async throws {
        let (app, transport) = try await connected()
        app.openMove("th1", to: cloud)
        try await until { transport.waiting("threads.movePreview") }
        transport.answer("threads.movePreview", ["cwd": "/home/mike/x", "accountId": "claude-main", "accounts": [], "leftBehind": [], "problems": []])
        try await until { app.moveRequest != nil }
        XCTAssertEqual(transport.count("threads.move"), 0)
    }

    func testAProblemOpensTheSheet() async throws {
        let (app, transport) = try await connected()
        app.openMove("th1", to: cloud)
        try await until { transport.waiting("threads.movePreview") }
        transport.answer("threads.movePreview", ["accounts": [], "leftBehind": [], "problems": ["There's no folder there."]])
        try await until { app.moveRequest != nil }
        XCTAssertEqual(transport.count("threads.move"), 0)
    }

    func testNoConnectionOpensTheSheetToWait() {
        let app = AppModel()
        app.openMove("th1", to: cloud)
        XCTAssertNotNil(app.moveRequest)
    }

    func testASummaryLandingWhileAskingKeepsTheRowMoving() async throws {
        let (app, transport) = try await connected()
        app.openMove("th1", to: cloud)
        try await until { transport.waiting("threads.movePreview") }
        var summary = try XCTUnwrap(app.threads["th1"])
        summary.moving = nil
        app.receiveForTesting(summary)
        XCTAssertEqual(app.threads["th1"]?.moving?.toName, "Hetzner Cloud")
    }
}
