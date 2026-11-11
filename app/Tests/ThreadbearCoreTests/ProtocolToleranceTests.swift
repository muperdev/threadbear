import Foundation
import Synchronization
import XCTest
@testable import ThreadbearCore

/// Both sides tolerate unknown fields and values (docs/protocol.md). What
/// the 9 Nov review found the Mac app didn't: a hello that one odd account
/// stopped, notifications dropped with nothing said, and a card left with
/// no buttons.
final class ProtocolToleranceTests: XCTestCase {
    /// One account the app can't read is left out; hello still connects.
    func testHelloSkipsAnAccountItCantRead() throws {
        let json = #"{"runnerVersion":7,"protocol":"1","accounts":[{"id":"claude-3"},{"provider":"claude"},"odd",{"id":"codex-main","provider":"codex"}],"defaults":{"cwd":"/w","accountId":"claude-3"}}"#
        let hello = try JSONDecoder().decode(HelloResult.self, from: Data(json.utf8))
        XCTAssertEqual(hello.accounts.map(\.id), ["claude-3", "codex-main"])
        XCTAssertEqual(hello.runnerVersion, "?", "not a string: unknown, not a failed hello")
        XCTAssertEqual(hello.protocol, 1)
    }

    /// A notification that won't decode is told, with the method and where
    /// it broke, never the message's text.
    func testAnUnreadableNotificationIsLoggedNotDroppedSilently() {
        let told = Told()
        let client = RPCClient(transport: FakeTransport()) { method, reason in told.items.withLock { $0.append((method, reason)) } }
        let data = Data(#"{"jsonrpc":"2.0","method":"thread.deleted","params":{"threadId":["secret words"]}}"#.utf8)
        XCTAssertNil(client.decodeNotification(method: "thread.deleted", data: data))
        let seen = told.items.withLock { $0 }
        XCTAssertEqual(seen.map(\.0), ["thread.deleted"])
        XCTAssertTrue(seen[0].1.contains("params.threadId"), seen[0].1)
        XCTAssertFalse(seen[0].1.contains("secret"), "no values from the message")

        // One that reads is passed on and nothing is told.
        let fine = Data(#"{"jsonrpc":"2.0","method":"thread.deleted","params":{"threadId":"t1"}}"#.utf8)
        guard case .threadDeleted(threadId: let id)? = client.decodeNotification(method: "thread.deleted", data: fine) else { return XCTFail("not read") }
        XCTAssertEqual(id, "t1")
        XCTAssertEqual(told.items.withLock { $0.count }, 1)
    }

    func testDecodeFailureNamesFieldsOnly() {
        struct Shape: Decodable { var a: [Int] }
        do {
            _ = try JSONDecoder().decode(Shape.self, from: Data(#"{"a":[1,"two words"]}"#.utf8))
            XCTFail("decoded")
        } catch {
            let reason = RPCClient.decodeFailure(error)
            XCTAssertTrue(reason.contains("a.[1]"), reason)
            XCTAssertFalse(reason.contains("two words"))
        }
    }

    /// A card whose options are all ones this app doesn't know still has
    /// Allow and Deny, rather than no way to answer it.
    func testAnApprovalWithOnlyNewOptionsStillHasButtons() throws {
        func options(_ raw: String) throws -> [ApprovalOption] {
            try JSONDecoder().decode(ApprovalRequest.self, from: Data(#"{"approvalId":"a","name":"Bash","options":\#(raw)}"#.utf8)).options
        }
        XCTAssertEqual(try options(#"["allowOnce","denyForever"]"#), [.allow, .deny])
        XCTAssertEqual(try options(#"["allowOnce","deny"]"#), [.deny], "known ones kept as they are")
        XCTAssertEqual(try options(#"["allow","allowAlways","deny"]"#), [.allow, .allowAlways, .deny])
    }
}

/// What a client told about the notifications it dropped. A class, as a
/// Mutex itself can't be captured by the escaping closure.
private final class Told: Sendable {
    let items = Mutex<[(String, String)]>([])
}
