import Foundation
import Testing
@testable import ThreadbearKit

/// What each answer does (backend API 61, `optionNotes`), as the Mac draws
/// under each option. The parity run found the phone drawing bare chips
/// without them (9 Nov).
struct AskNotesTests {
    @Test func notesDecodeAndLineUpWithTheOptions() throws {
        let json = #"{"action":"Same thread or new?","options":["Same","New"],"optionNotes":["One thread that moves","The old one is archived"],"suggested":1}"#
        let ask = try JSONDecoder().decode(PhoneAsk.self, from: Data(json.utf8))
        #expect(ask.note(0) == "One thread that moves")
        #expect(ask.note(1) == "The old one is archived")
        #expect(ask.hasNotes)
    }

    @Test func notesThatDontLineUpOrAreBlankAreNone() {
        #expect(!PhoneAsk(action: "x", options: ["A", "B"], optionNotes: ["only one"]).hasNotes)
        #expect(PhoneAsk(action: "x", options: ["A", "B"], optionNotes: ["", " "]).note(0) == nil)
        #expect(!PhoneAsk(action: "x", reply: "done", optionNotes: ["a"]).hasNotes, "a reply-only ask has no options to note")
        #expect(!PhoneAsk(action: "x", options: ["A"]).hasNotes)
    }
}
