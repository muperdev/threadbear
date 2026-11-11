import Foundation
import Testing
@testable import ThreadbearKit

/// One row this build can't read (a role from a newer backend, a field the
/// wrong shape) is skipped, and the rest of the answer still shows. Decoded
/// as plain arrays, one such message hid the whole thread page and one such
/// row ended a live list (9 Nov review).
@Suite struct LossyListTests {
    @Test func aLiveListSkipsTheRowItCantRead() async throws {
        let transport = OneAnswerTransport(answer: """
        [{"scheduleId":"s1","threadId":"t1","nextRunAt":"2026-11-08T01:00:00.000Z"},
         {"scheduleId":5},
         "not even an object",
         {"scheduleId":"s3"}]
        """)
        let backend = ConvexBackend(transport: transport, token: "t")
        var iterator = backend.wakes().makeAsyncIterator()
        let rows = try #require(try await iterator.next())
        #expect(rows.map(\.id) == ["s1", "s3"])
    }

    @Test func aThreadPageSkipsAMessageWithARoleItDoesntKnow() async throws {
        let transport = OneAnswerTransport(answer: """
        {"messages":[
          {"seq":1,"role":"user","text":"Fix it","at":1762650000000},
          {"seq":2,"role":"narrator","text":"From a newer backend","at":1762650001000},
          {"seq":3,"role":"assistant","text":"Fixed","at":1762650002000}
         ],"hasMore":false,"images":[{"bad":true}]}
        """)
        let backend = ConvexBackend(transport: transport, token: "t")
        var iterator = backend.messages("t1", limit: 50).makeAsyncIterator()
        let page = try #require(try await iterator.next())
        #expect(page.messages.map(\.seq) == [1, 3])
        #expect(page.messages.map(\.role) == [.user, .assistant])
        #expect(page.images.isEmpty)
    }

    @Test func aWholeAnswerThatIsntAListStillFails() async throws {
        let transport = OneAnswerTransport(answer: #"{"not":"a list"}"#)
        let backend = ConvexBackend(transport: transport, token: "t")
        var iterator = backend.wakes().makeAsyncIterator()
        await #expect(throws: (any Error).self) { _ = try await iterator.next() }
    }
}
