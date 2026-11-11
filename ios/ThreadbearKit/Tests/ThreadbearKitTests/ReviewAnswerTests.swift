import Testing
@testable import ThreadbearKit

@Suite struct ReviewAnswerTests {
    static let id = "74d1e391-cbc6-4b23-bd6b-cfed7cb79488"

    @Test func aPickShowsTheOptionNotTheId() {
        let answer = ReviewAnswer.parse("Review: End of turn: summary + what's next (\(Self.id))\n\nPicked A. A: summary is the agent's last message.")
        #expect(answer?.title == "End of turn: summary + what's next")
        #expect(answer?.lines == [.init(label: "End of turn: summary + what's next", text: "Picked A: summary is the agent's last message")])
    }

    // The runner names the options he passed over for the agent; his bubble doesn't.
    @Test func whatHePassedOverIsLeftOut() {
        let text = "Review: Run commands (\(Self.id))\n\nPicked A. Small play button.\n\nNotes on A (Small play button): Nice\n\nPassed over: B. Run card. \(ReviewAnswer.passedOverTail)"
        #expect(ReviewAnswer.parse(text)?.lines == [
            .init(label: "Run commands", text: "Picked A: Small play button"),
            .init(label: "Notes on A: Small play button", text: "Nice"),
        ])
    }

    @Test func notesKeepTheirParagraphsAndBrackets() {
        let text = "Review: Picker (\(Self.id))\n\nPicked B. One picker.\n\nNotes on A (Auto (smart)): Too clever.\n\nSecond thought.\n\nNotes on B (One picker): Ship it"
        #expect(ReviewAnswer.parse(text)?.lines == [
            .init(label: "Picker", text: "Picked B: One picker"),
            .init(label: "Notes on A: Auto (smart)", text: "Too clever.\n\nSecond thought."),
            .init(label: "Notes on B: One picker", text: "Ship it"),
        ])
    }

    @Test func onlyNotesPutTheTitleOverTheFirst() {
        let answer = ReviewAnswer.parse("Review: Picker (\(Self.id))\n\nNotes on D (Moving): Keep this")
        #expect(answer?.lines == [.init(label: "Picker\nNotes on D: Moving", text: "Keep this")])
    }

    @Test func otherMessagesAreLeftAlone() {
        #expect(ReviewAnswer.parse("Review: this PR please") == nil)
        #expect(ReviewAnswer.parse("Review: Picker (not-a-uuid)\n\nPicked A. Thing.") == nil)
        #expect(ReviewAnswer.parse("Review: Picker (\(Self.id))") == nil)
    }

    // Since 4 Nov a review takes several picks, none, and a reply in his words.
    @Test func severalPicksAndAReply() {
        let text = "Review: Diagrams (\(Self.id))\n\nPicked A + D. Entity board + Sandbox.\n\nThe board for the app.\n\nThe sandbox for the video.\n\nNotes on C (Map): Too busy"
        #expect(ReviewAnswer.parse(text)?.lines == [
            .init(label: "Diagrams", text: "Picked A + D: Entity board + Sandbox"),
            .init(label: "", text: "The board for the app.\n\nThe sandbox for the video."),
            .init(label: "Notes on C: Map", text: "Too busy"),
        ])
    }

    @Test func noneOrOnlyAReply() {
        #expect(ReviewAnswer.parse("Review: Diagrams (\(Self.id))\n\nPicked none of them.")?.lines == [.init(label: "Diagrams", text: "None of them")])
        #expect(ReviewAnswer.parse("Review: Diagrams (\(Self.id))\n\nWhat are these for?")?.lines == [.init(label: "Diagrams", text: "What are these for?")])
    }
}
