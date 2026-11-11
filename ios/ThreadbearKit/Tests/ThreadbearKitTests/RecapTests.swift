import Foundation
import Testing
@testable import ThreadbearKit

@Suite struct RecapTests {
    private let now = Date(timeIntervalSince1970: 2_000_000_000)

    private func thread(hoursAgo: Double, status: ThreadStatus = .idle) -> PhoneThread {
        var t = PhoneThread(threadId: "t", title: "T", status: status, preview: "", lastActivityAt: now.addingTimeInterval(-hoursAgo * 3600))
        t.recap = "For this.\n- Done"
        return t
    }

    @Test func showsOnAQuietIdleThreadUntilPutAway() {
        let t = thread(hoursAgo: 12)
        #expect(Recap.isDue(t, dismissedAt: nil, now: now))
        #expect(!Recap.isDue(thread(hoursAgo: 11.9), dismissedAt: nil, now: now))
        #expect(!Recap.isDue(thread(hoursAgo: 72, status: .running), dismissedAt: nil, now: now))
        #expect(!Recap.isDue(t, dismissedAt: t.lastActivityAt.timeIntervalSince1970, now: now))
    }

    @Test func splitsThePurposeFromTheBullets() {
        let parts = Recap.parts("For this.\n- One\n- Two")
        #expect(parts.purpose == "For this.")
        #expect(parts.points == ["One", "Two"])
    }

    @Test func saysHowLongInHoursThenDays() {
        #expect(Recap.lastWorkedOn(now.addingTimeInterval(-72 * 3600), now: now) == "Last worked on 3 days ago")
    }
}
