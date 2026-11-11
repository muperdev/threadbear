import Foundation
import Testing
@testable import ThreadbearKit

/// The done card shows when the thread's last shown message is the agent's
/// (ThreadView.isDone). The parity run (9 Nov) found it missing on the
/// phone when the reply came before end_turn, as the end_turn row was last.
struct LastShownTests {
    private func m(_ seq: Int, _ role: PhoneMessage.Role, _ text: String) -> PhoneMessage {
        PhoneMessage(seq: seq, role: role, text: text, at: Date(timeIntervalSince1970: Double(seq)))
    }

    @Test func threadbearsOwnToolRowsAreNotTheLastShown() {
        let messages = [m(1, .user, "Start"), m(2, .assistant, "Landed it."), m(3, .tool, "threadbear: end_turn"), m(4, .tool, "Find tools: select:x")]
        #expect(Turns.lastShown(messages)?.seq == 2)
    }

    @Test func otherToolsStillCount() {
        let messages = [m(1, .assistant, "Running it."), m(2, .tool, "Ran npm test")]
        #expect(Turns.lastShown(messages)?.role == .tool)
    }
}
