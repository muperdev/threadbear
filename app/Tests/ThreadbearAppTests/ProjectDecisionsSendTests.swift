import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// Decisions on the Projects page (the projectDecisions experiment): an
/// answer there is the same `threads.send` the thread's own card makes, it
/// shows "Sending" at once and takes no more clicks, and a failure puts the
/// picks back.
@MainActor
final class ProjectDecisionsSendTests: XCTestCase {
    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
    }

    private let card = #"{"id":"th1","title":"Fork watch","status":"idle","lastSeq":12,"needs":"you","asksSeq":11,"asks":[{"action":"Bring it in?","options":["Bring it in","Leave it"],"suggested":0},{"action":"Tell Wayne?","options":["Yes","No"]}]}"#

    private func connected(_ json: String) async throws -> (AppModel, HoldingTransport) {
        let app = AppModel()
        let transport = HoldingTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        app.useClientForTesting(client)
        app.threads["th1"] = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        app.experiments = ["projectDecisions"]
        return (app, transport)
    }

    private func until(_ condition: () -> Bool) async throws {
        for _ in 0..<400 where !condition() {
            try await Task.sleep(for: .milliseconds(5))
        }
    }

    func testAnsweringEveryAskSendsWhatTheThreadsCardWould() async throws {
        let (app, transport) = try await connected(card)
        app.drafts["th1"] = Draft(text: "half a thought")
        let thread = try XCTUnwrap(app.threads["th1"])
        app.pickDecision(thread: thread, ask: 0, option: 0)
        XCTAssertNil(app.decisionSends["th1"], "one ask still open: nothing goes yet")
        XCTAssertEqual(app.decisionCard(app.threads["th1"]!)?.picks, [0: 0], "the pick is the thread's own, on its card")
        app.pickDecision(thread: app.threads["th1"]!, ask: 1, option: 1)
        XCTAssertEqual(app.decisionSends["th1"], DecisionSend(asksSeq: 11, answers: ["Bring it in", "No"]), "Sending shows at once")
        XCTAssertTrue(ProjectDecisions.isSending(app.threads["th1"]!, sentFor: 11))
        try await until { transport.waiting("threads.send") }
        let params = try XCTUnwrap(transport.params("threads.send"))
        let thePicks = [0: 0, 1: 1]
        XCTAssertEqual(params["text"] as? String, Decisions.message(asks: thread.asks, picks: thePicks), "the thread's own Send, with an empty composer")
        let answers = try XCTUnwrap(params["answers"] as? [[String: Any]])
        XCTAssertEqual(answers.map { $0["answer"] as? String }, ["Bring it in", "No"])
        XCTAssertEqual(app.drafts["th1"]?.text, "half a thought", "what's typed in the thread stays there")
        // A second click while it's on its way does nothing.
        app.pickDecision(thread: app.threads["th1"]!, ask: 0, option: 1)
        XCTAssertTrue(app.picks(for: app.threads["th1"]!).isEmpty)
        XCTAssertEqual(transport.count("threads.send"), 1)
    }

    func testAFailedAnswerComesBackToTheCard() async throws {
        let one = card.replacingOccurrences(of: #",{"action":"Tell Wayne?","options":["Yes","No"]}"#, with: "")
        let (app, transport) = try await connected(one)
        app.pickDecision(thread: app.threads["th1"]!, ask: 0, option: 1)
        XCTAssertNotNil(app.decisionSends["th1"], "one click answers a card of one")
        try await until { transport.waiting("threads.send") }
        transport.fail("threads.send", message: "Busy")
        try await until { app.decisionSends["th1"] == nil }
        XCTAssertEqual(app.picks(for: app.threads["th1"]!), [0: 1], "the pick is back")
        XCTAssertEqual(app.errorMessage, "Busy")
    }

    /// Answered in the thread a moment ago, before the runner says it took
    /// it: the page doesn't send a second answer (Codex's review).
    func testAnAnswerFromTheThreadsOwnComposerBlocksThePage() async throws {
        let one = card.replacingOccurrences(of: #",{"action":"Tell Wayne?","options":["Yes","No"]}"#, with: "")
        let (app, transport) = try await connected(one)
        app.session(for: "th1").addPending(id: "p1", text: "Bring it in?\nBring it in", images: [], answers: [], summarySeq: 12)
        XCTAssertTrue(app.decisionInFlight(app.threads["th1"]!))
        app.pickDecision(thread: app.threads["th1"]!, ask: 0, option: 1)
        app.sendDecision(threadId: "th1")
        XCTAssertNil(app.decisionSends["th1"])
        XCTAssertEqual(transport.count("threads.send"), 0)
    }

    func testSomethingElseWaitsForItsBox() async throws {
        let one = card.replacingOccurrences(of: #",{"action":"Tell Wayne?","options":["Yes","No"]}"#, with: "")
        let (app, _) = try await connected(one)
        app.pickDecision(thread: app.threads["th1"]!, ask: 0, option: Decisions.ownAnswer)
        XCTAssertNil(app.decisionSends["th1"], "Something else opens its box rather than sending")
        app.setNote(thread: app.threads["th1"]!, ask: 0, note: AskNote(text: "Only the README"))
        app.decisionBoxReturn(threadId: "th1", command: false)
        XCTAssertEqual(app.decisionSends["th1"]?.answers, ["Only the README"])
    }
}
