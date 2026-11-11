import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// The Move sheet can open in the seconds a runner update drops the app.
/// That's a wait, not an answer about the move: when it came back as a
/// problem, the sheet sat blank with a red error even after reconnecting.
@MainActor
final class MovePreviewTests: XCTestCase {
    private let request = AppModel.MoveRequest(threadId: "th1", runnerId: "r2", runnerName: "Hetzner Cloud")

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
    }

    func testNoConnectionIsAWaitNotAProblem() async {
        let answer = await AppModel().movePreview(request)
        XCTAssertNil(answer)
    }

    func testTheConnectionDroppingMidAskIsAWaitToo() async throws {
        let app = AppModel()
        let transport = HoldingTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        app.useClientForTesting(client)

        let request = request
        let asking = Task { await app.movePreview(request) }
        for _ in 0..<400 where !transport.waiting("threads.movePreview") {
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertTrue(transport.waiting("threads.movePreview"))
        client.close()
        let answer = await asking.value
        XCTAssertNil(answer)
    }
}
