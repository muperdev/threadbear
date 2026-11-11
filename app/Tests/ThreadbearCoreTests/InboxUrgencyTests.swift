import Foundation
import XCTest
@testable import ThreadbearCore

/// The Mac's sidebar order and Dock badge against the cases the phone and
/// website run too (app/Tests/Fixtures/inbox-urgency-cases.json).
final class InboxUrgencyTests: XCTestCase {
    struct Cases: Decodable {
        struct Case: Decodable {
            struct Thread: Decodable {
                var status: String?
                var needsYou: Bool?
                var unread: Bool?
                var pendingReviews: Double?
            }
            var name: String
            var thread: Thread
            var urgency: Int
            var counted: Bool
        }
        var cases: [Case]
    }

    func testTheSharedCases() throws {
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Fixtures/inbox-urgency-cases.json")
        let cases = try JSONDecoder().decode(Cases.self, from: Data(contentsOf: url)).cases
        XCTAssertGreaterThanOrEqual(cases.count, 9)
        let now = Date()
        for c in cases {
            var json: [String: Any] = ["id": "t", "title": "t", "status": c.thread.status ?? "idle",
                                       "needsYou": c.thread.needsYou ?? false, "lastActivityAt": ISO8601.format(now)]
            // Unread is lastSeenSeq behind lastSeq.
            json["lastSeq"] = 5
            json["lastSeenSeq"] = c.thread.unread == true ? 4 : 5
            if let reviews = c.thread.pendingReviews { json["pendingReviews"] = reviews }
            let thread = try JSONDecoder().decode(ThreadSummary.self, from: JSONSerialization.data(withJSONObject: json))
            XCTAssertEqual(thread.lastSeq > thread.lastSeenSeq, c.thread.unread == true, "\(c.name): the case's unread")
            XCTAssertEqual(ThreadSections.urgency(thread), c.urgency, c.name)
            XCTAssertEqual(ThreadSections.needsYouCount([thread], now: now), c.counted ? 1 : 0, c.name)
            XCTAssertEqual(ThreadSections.build([thread], now: now).needsYou.count, c.counted ? 1 : 0, c.name)
        }
    }
}
