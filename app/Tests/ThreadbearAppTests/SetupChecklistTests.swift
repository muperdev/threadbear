import XCTest
import ThreadbearCore
@testable import Threadbear

/// Finish setting up ticks itself off from what the app knows, and leaves
/// out what this install can't do.
final class SetupChecklistTests: XCTestCase {
    /// `ran`: a turn reached the model, so the runner has its cost.
    private func thread(_ id: String, ran: Bool, status: ThreadStatus = .idle) -> ThreadSummary {
        var t = ThreadSummary(id: id, title: "", cwd: "/tmp", accountId: "a")
        // Every thread has one from the start, the turnless Welcome one too.
        t.nativeSessionId = "s"
        t.costUsd = ran ? 0.01 : nil
        t.status = status
        return t
    }

    private let account = Account(id: "a", title: "a")

    func testNewInstallHasEverythingLeft() {
        let list = SetupChecklist.build(threads: [], accounts: [account], devices: [], canPair: true)
        XCTAssertEqual(list.items.map(\.item), [.firstThread, .phone, .website])
        XCTAssertEqual(list.left, 3)
        XCTAssertFalse(list.noLogin)
    }

    /// setup --self-hosted --local: no phone can reach the backend, so
    /// pairing one isn't on the list, rather than left there undoable.
    func testABackendOnThisMacOnlyLeavesThePhoneOff() {
        let list = SetupChecklist.build(threads: [], accounts: [account], devices: [], canPair: true, localOnly: true)
        XCTAssertEqual(list.items.map(\.item), [.firstThread, .website])
    }

    func testEachTicksItselfOff() {
        let devices = [Device(id: "1", kind: .phone, name: "iPhone"), Device(id: "2", kind: .browser, name: "Chrome")]
        let list = SetupChecklist.build(threads: [thread("t", ran: true)], accounts: [account], devices: devices,
                                        canPair: true)
        XCTAssertEqual(list.left, 0)
    }

    func testAFailedOrTurnlessThreadDoesntCount() {
        for t in [thread("t", ran: false), thread("t", ran: true, status: .error)] {
            let list = SetupChecklist.build(threads: [t], accounts: [account], devices: [], canPair: false)
            XCTAssertEqual(list.items.first?.done, false)
        }
    }

    func testAnExpiredPhoneNeedsPairingAgain() {
        let list = SetupChecklist.build(threads: [], accounts: [account], devices: [Device(id: "1", kind: .phone, name: "iPhone", expired: true)],
                                        canPair: true)
        XCTAssertEqual(list.items.first { $0.item == .phone }?.done, false)
    }

    func testWithoutTheMirrorThePhoneAndWebsiteArentListed() {
        let list = SetupChecklist.build(threads: [], accounts: [], devices: [], canPair: false)
        XCTAssertEqual(list.items.map(\.item), [.firstThread])
        XCTAssertTrue(list.noLogin)
    }
}
