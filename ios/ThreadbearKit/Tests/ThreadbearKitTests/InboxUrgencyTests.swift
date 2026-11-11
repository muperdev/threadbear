import Foundation
import Testing
@testable import ThreadbearKit

/// The phone's inbox order and badge against the cases the Mac and website
/// run too (app/Tests/Fixtures/inbox-urgency-cases.json). An unread thread
/// whose turn said it needs nothing is done, as on the Mac (Mike, 9 Nov).
@Suite struct InboxUrgencyTests {
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

    @Test func theSharedCases() throws {
        // ios/ThreadbearKit/Tests/ThreadbearKitTests/ -> the repo.
        let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let url = repo.appendingPathComponent("app/Tests/Fixtures/inbox-urgency-cases.json")
        let cases = try JSONDecoder().decode(Cases.self, from: Data(contentsOf: url)).cases
        #expect(cases.count >= 9)
        let now = Date()
        for c in cases {
            let status = try #require(ThreadStatus(rawValue: c.thread.status ?? "idle"), "\(c.name)")
            let thread = PhoneThread(threadId: "t", title: "t", status: status, needsYou: c.thread.needsYou ?? false,
                                     unread: c.thread.unread ?? false, lastActivityAt: now, pendingReviews: c.thread.pendingReviews)
            #expect(InboxSections.urgency(thread) == c.urgency, "\(c.name)")
            #expect(InboxSections.build([thread], now: now).needsYouCount == (c.counted ? 1 : 0), "\(c.name)")
        }
    }
}
