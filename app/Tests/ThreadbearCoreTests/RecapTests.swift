import Foundation
import Testing
@testable import ThreadbearCore

@Suite struct RecapTests {
    private let now = Date(timeIntervalSince1970: 2_000_000_000)

    private func thread(hoursAgo: Double, status: ThreadStatus = .idle, recap: String? = "For this.\n- Done") -> ThreadSummary {
        let at = now.addingTimeInterval(-hoursAgo * 3600)
        var t = ThreadSummary(id: "t", title: "")
        t.status = status
        t.lastActivityAt = at
        t.recap = recap
        return t
    }

    @Test func showsOnlyOnAQuietIdleThreadWithARecap() {
        #expect(Recap.isDue(thread(hoursAgo: 12), dismissedAt: nil, now: now))
        #expect(!Recap.isDue(thread(hoursAgo: 11.9), dismissedAt: nil, now: now))
        #expect(!Recap.isDue(thread(hoursAgo: 72, status: .running), dismissedAt: nil, now: now))
        #expect(!Recap.isDue(thread(hoursAgo: 72, recap: nil), dismissedAt: nil, now: now))
        var archived = thread(hoursAgo: 72)
        archived.archivedAt = now
        #expect(!Recap.isDue(archived, dismissedAt: nil, now: now))
    }

    @Test func staysAwayOnlyForTheQuietSpellItWasDismissedIn() {
        let t = thread(hoursAgo: 72)
        #expect(!Recap.isDue(t, dismissedAt: t.lastActivityAt.timeIntervalSince1970, now: now))
        #expect(Recap.isDue(t, dismissedAt: t.lastActivityAt.timeIntervalSince1970 - 1000, now: now))
    }

    @Test func splitsThePurposeFromTheBullets() {
        let both = Recap.parts("What it's for.\n- One\n- Two")
        #expect(both.purpose == "What it's for.")
        #expect(both.points == ["One", "Two"])
        // A note from before purposes is only bullets.
        let old = Recap.parts("- One\n- Two")
        #expect(old.purpose == nil)
        #expect(old.points == ["One", "Two"])
    }

    @Test func saysHowLongInHoursThenDays() {
        #expect(Recap.lastWorkedOn(now.addingTimeInterval(-14 * 3600), now: now) == "Last worked on 14 hours ago")
        #expect(Recap.lastWorkedOn(now.addingTimeInterval(-72 * 3600), now: now) == "Last worked on 3 days ago")
    }
}
