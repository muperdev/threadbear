import XCTest
@testable import ThreadbearCore

/// Decisions on the Projects page (the projectDecisions experiment).
final class ProjectDecisionsTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_000_000)
    private let inA = [ThreadProject(id: "a", title: "A")]

    private func asking(_ id: String, minutesAgo: Double = 1, asks: [Ask], status: ThreadStatus = .idle, needs: String? = "you", seq: Int? = 7) -> ThreadSummary {
        ThreadSummary(id: id, title: id.uppercased(), status: status, needsYou: true, lastActivityAt: now.addingTimeInterval(-minutesAgo * 60),
                      projects: inA, needs: needs, asks: asks, asksSeq: seq)
    }

    private let one = [Ask(action: "Merge it?", options: ["Yes", "No"], suggested: 0)]

    func testOnlyThreadsWaitingOnACardOfAsksAreDecisions() {
        XCTAssertTrue(ProjectDecisions.isWaiting(asking("t", asks: one)))
        XCTAssertFalse(ProjectDecisions.isWaiting(asking("t", asks: one, status: .running)), "running: not his move yet")
        XCTAssertFalse(ProjectDecisions.isWaiting(asking("t", asks: [])), "needs him with no asks is a plain reply")
        XCTAssertFalse(ProjectDecisions.isWaiting(asking("t", asks: one, needs: "nothing")))
        XCTAssertFalse(ProjectDecisions.isWaiting(asking("t", asks: one, seq: nil)), "no card to answer")
        var ticked = one
        ticked[0].ticked = true
        XCTAssertFalse(ProjectDecisions.isWaiting(asking("t", asks: ticked)), "every ask ticked hands it back")
        var archived = asking("t", asks: one)
        archived.archivedAt = now
        XCTAssertFalse(ProjectDecisions.isWaiting(archived))
    }

    func testCountsOpenAsksNewestThreadFirst() {
        var two = [Ask(action: "One"), Ask(action: "Two"), Ask(action: "Three")]
        two[2].ticked = true
        let threads = [
            asking("old", minutesAgo: 60, asks: one),
            asking("new", minutesAgo: 1, asks: two),
            asking("busy", minutesAgo: 0, asks: one, status: .running),
        ]
        XCTAssertEqual(ProjectDecisions.threads(threads).map(\.id), ["new", "old"])
        XCTAssertEqual(ProjectDecisions.count(threads), 3, "two open on one card, one on the other; the running one waits")
        XCTAssertEqual(ProjectDecisions.label(1), "1 decision")
        XCTAssertEqual(ProjectDecisions.heading(decisions: 3, threads: 2), "3 decisions from 2 threads")
        XCTAssertEqual(ProjectDecisions.heading(decisions: 2, threads: 2), "2 decisions")
    }

    func testTheRecapLineIsWhatTheThreadIsForElseItsHeadline() {
        var thread = asking("t", asks: one)
        thread.preview = "Batching is in (#67).\nMore after"
        XCTAssertEqual(ProjectDecisions.recapLine(thread), "Batching is in (#67).")
        thread.recap = "Speeds up the landing queue.\n- Batching is in"
        XCTAssertEqual(ProjectDecisions.recapLine(thread), "Speeds up the landing queue.")
        thread.recap = "- Only bullets, from before purposes"
        XCTAssertEqual(ProjectDecisions.recapLine(thread), "Batching is in (#67).")
        thread.preview = ""
        XCTAssertNil(ProjectDecisions.recapLine(thread))
    }

    func testSendingLastsUntilTheThreadTakesTheAnswer() {
        let waiting = asking("t", asks: one, seq: 7)
        XCTAssertTrue(ProjectDecisions.isSending(waiting, sentFor: 7))
        XCTAssertFalse(ProjectDecisions.isSending(waiting, sentFor: nil))
        XCTAssertFalse(ProjectDecisions.isSending(waiting, sentFor: 3), "an answer to an older card")
        XCTAssertFalse(ProjectDecisions.isSending(asking("t", asks: one, status: .running, seq: 7), sentFor: 7), "taken: its turn started")
    }

    func testTheProjectsCardSaysHowManyDecisions() {
        let projects = [Project(id: "a", title: "A", updatedAt: now)]
        let threads = [
            asking("ask", asks: one),
            ThreadSummary(id: "new", title: "Finished", needsYou: true, lastActivityAt: now, projects: inA),
        ]
        let sections = ProjectSections.build(projects: projects, threads: threads, now: now)
        XCTAssertEqual(ProjectCard.build(sections)[0].status.parts.map(\.text), ["2 new"], "the experiment off: as today")
        let on = ProjectCard.build(sections, decisions: true)[0].status
        XCTAssertEqual(on.parts.map(\.text), ["1 decision", "1 new"], "a thread with a decision isn't counted as new too")
        XCTAssertFalse(on.isQuiet)
    }
}
