import XCTest
@testable import ThreadbearCore

final class RPCClientTests: XCTestCase {
    func testCallSendsJSONRPCAndDecodesResult() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in
            let id = request["id"] as! Int
            return [E.json([
                "jsonrpc": "2.0", "id": id,
                "result": ["runnerVersion": "0.1.0", "protocol": 1,
                           "accounts": [["id": "claude-main", "provider": "claude", "title": "mike@example.com", "configDir": "/x", "extra": true]],
                           "defaults": ["cwd": "/Users/m/dev/me/misc", "accountId": "claude-main", "model": "claude-haiku-5-5"]],
            ])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        let hello = try await client.hello(version: "1.0")
        XCTAssertEqual(hello.runnerVersion, "0.1.0")
        XCTAssertEqual(hello.accounts.first?.title, "mike@example.com")
        XCTAssertEqual(hello.defaults.accountId, "claude-main")
        XCTAssertEqual(hello.defaults.model, "claude-haiku-5-5", "a dev runner's default model")
        let sent = transport.sent.first!
        XCTAssertEqual(sent["jsonrpc"] as? String, "2.0")
        XCTAssertEqual(sent["method"] as? String, "hello")
        XCTAssertEqual((sent["params"] as? [String: Any])?["client"] as? String, "threadbear-mac")
    }

    func testResponsesMatchedByIdEvenOutOfOrder() async throws {
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        async let first = client.call("a", as: TurnResult.self)
        async let second = client.call("b", as: TurnResult.self)
        await transport.waitForSent(2)
        let ids = transport.sent.reduce(into: [String: Int]()) { $0[$1["method"] as! String] = $1["id"] as? Int }
        transport.push(E.json(["jsonrpc": "2.0", "id": ids["b"]!, "result": ["turnId": "B"]]))
        transport.push(E.json(["jsonrpc": "2.0", "id": ids["a"]!, "result": ["turnId": "A"]]))
        let (a, b) = try await (first, second)
        XCTAssertEqual(a.turnId, "A")
        XCTAssertEqual(b.turnId, "B")
    }

    func testRequestsGoOutInTheOrderTheyWereMade() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in [E.json(["jsonrpc": "2.0", "id": request["id"]!, "result": [:]])] }
        let client = RPCClient(transport: transport)
        try await client.start()
        await withTaskGroup(of: Void.self) { group in
            // Calls made one after another from one task must arrive in order,
            // even though each call awaits on its own.
            for index in 0..<50 {
                _ = try? await client.call("m\(index)", timeout: .seconds(5)) as EmptyResult
            }
            group.addTask {}
        }
        XCTAssertEqual(transport.sent.map { $0["method"] as? String }, (0..<50).map { "m\($0)" })
    }

    func testTimeoutsStillFireAndAnsweredCallsDoNotTimeOut() async throws {
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        do {
            _ = try await client.call("slow", timeout: .milliseconds(50)) as EmptyResult
            XCTFail("expected a timeout")
        } catch let error as RPCError {
            XCTAssertEqual(error, .timeout("slow"))
        }
        transport.onSend = { request in [E.json(["jsonrpc": "2.0", "id": request["id"]!, "result": [:]])] }
        _ = try await client.call("quick", timeout: .milliseconds(50)) as EmptyResult
        try await Task.sleep(for: .milliseconds(120))
        XCTAssertFalse(client.isClosed)
    }

    func testServerErrorBecomesRPCError() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in
            [E.json(["jsonrpc": "2.0", "id": request["id"]!, "error": ["code": -32004, "message": "Thread not found"]])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        do {
            _ = try await client.markSeen(threadId: "nope", seq: 1)
            XCTFail("expected an error")
        } catch let error as RPCError {
            XCTAssertEqual(error, .server(code: -32004, message: "Thread not found"))
        }
    }

    func testEmptyResultCalls() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in [E.json(["jsonrpc": "2.0", "id": request["id"]!, "result": [:]])] }
        let client = RPCClient(transport: transport)
        try await client.start()
        try await client.interrupt(threadId: "th1")
        try await client.respond(threadId: "th1", approvalId: "ap1", decision: .answered, answers: ["Which?": "B"])
        let respond = transport.sent[1]["params"] as! [String: Any]
        XCTAssertEqual(respond["decision"] as? String, "answered")
        XCTAssertEqual((respond["answers"] as? [String: String])?["Which?"], "B")
    }

    /// `secrets.answer` (docs/protocol.md, "Secrets"): the value or a
    /// decline, never both, and never shown when an answer is printed.
    func testAnswerSecretSendsTheValueOrADecline() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in [E.json(["jsonrpc": "2.0", "id": request["id"]!, "result": [:]])] }
        let client = RPCClient(transport: transport)
        try await client.start()
        try await client.answerSecret(threadId: "th1", approvalId: "sec1", .secret("hunter2 with spaces "))
        try await client.answerSecret(threadId: "th1", approvalId: "sec2", .decline)
        XCTAssertEqual(transport.sent.map { $0["method"] as? String }, ["secrets.answer", "secrets.answer"])
        let saved = transport.sent[0]["params"] as! [String: Any]
        XCTAssertEqual(saved["threadId"] as? String, "th1")
        XCTAssertEqual(saved["approvalId"] as? String, "sec1")
        XCTAssertEqual(saved["secret"] as? String, "hunter2 with spaces ", "sent as typed, not trimmed")
        XCTAssertNil(saved["decline"])
        let declined = transport.sent[1]["params"] as! [String: Any]
        XCTAssertEqual(declined["approvalId"] as? String, "sec2")
        XCTAssertEqual(declined["decline"] as? Bool, true)
        XCTAssertNil(declined["secret"])

        let answer = SecretAnswer.secret("hunter2")
        for printed in [String(describing: answer), String(reflecting: answer), "\(answer)", "\([answer])"] {
            XCTAssertFalse(printed.contains("hunter2"), printed)
        }
        XCTAssertFalse(SecretAnswer.canSave(""))
        XCTAssertFalse(SecretAnswer.canSave(" \n\t"))
        XCTAssertTrue(SecretAnswer.canSave(" x "))
    }

    func testNotificationsAreDecoded() async throws {
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        transport.push(E.json(["jsonrpc": "2.0", "method": "thread.updated", "params": ["thread": [
            "id": "th1", "title": "Fix tests", "status": "running", "needsYou": false, "lastSeq": 4,
            "createdAt": "2026-10-26T10:00:00Z", "brandNewField": [1, 2],
        ]]]))
        transport.push(E.json(["jsonrpc": "2.0", "method": "thread.event", "params": ["threadId": "th1", "event": [
            "seq": 4, "ts": "2026-10-26T10:00:01Z", "threadId": "th1", "type": "assistant.text.delta", "itemId": "m1", "text": "Hel",
        ]]]))
        transport.push(E.json(["jsonrpc": "2.0", "method": "thread.deleted", "params": ["threadId": "th2"]]))
        transport.push(E.json(["jsonrpc": "2.0", "method": "accounts.updated", "params": ["accounts": [
            ["id": "claude-3", "title": "mike@example.com", "defaultModel": "claude-fable-5-1",
             "models": [["id": "opus", "model": "claude-opus-5-5", "title": "Opus 5.5", "effortLevels": ["low", "max"]], ["broken": true]]],
        ]]]))
        transport.push(E.json(["jsonrpc": "2.0", "method": "some.future.thing", "params": [:]]))
        var iterator = client.notifications.makeAsyncIterator()
        guard case .threadUpdated(let thread)? = await iterator.next() else { return XCTFail("no update") }
        XCTAssertEqual(thread.title, "Fix tests")
        XCTAssertEqual(thread.status, .running)
        XCTAssertEqual(thread.cwd, "")
        guard case .threadEvent(let threadId, let event)? = await iterator.next() else { return XCTFail("no event") }
        XCTAssertEqual(threadId, "th1")
        XCTAssertEqual(event.payload, .assistantTextDelta(itemId: "m1", text: "Hel"))
        guard case .threadDeleted(let deleted)? = await iterator.next() else { return XCTFail("no delete") }
        XCTAssertEqual(deleted, "th2")
        guard case .accountsUpdated(let accounts)? = await iterator.next() else { return XCTFail("no accounts") }
        XCTAssertEqual(accounts.first?.defaultModel, "claude-fable-5-1")
        XCTAssertEqual(accounts.first?.models.map(\.title), ["Opus 5.5"], "a row that doesn't decode is skipped")
        XCTAssertEqual(accounts.first?.models.first?.effortLevels, ["low", "max"])
        guard case .other(let method)? = await iterator.next() else { return XCTFail("no other") }
        XCTAssertEqual(method, "some.future.thing")
    }

    func testCloseFailsPendingCallsAndReportsClosed() async throws {
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        let pending = Task { try await client.call("threads.list", as: ThreadsResult.self) }
        await transport.waitForSent(1)
        transport.close()
        do {
            _ = try await pending.value
            XCTFail("expected disconnect")
        } catch let error as RPCError {
            if case .disconnected = error {} else { XCTFail("wrong error \(error)") }
        }
        var closed = client.closed.makeAsyncIterator()
        let reason = await closed.next()
        XCTAssertNotNil(reason)
        XCTAssertTrue(client.isClosed)
        do {
            _ = try await client.call("hello", as: EmptyResult.self)
            XCTFail("calls after close should fail")
        } catch {}
    }

    func testTimeout() async throws {
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        do {
            _ = try await client.call("slow", as: EmptyResult.self, timeout: .milliseconds(50))
            XCTFail("expected timeout")
        } catch let error as RPCError {
            XCTAssertEqual(error, .timeout("slow"))
        }
    }

    func testUnauthorizedOpen() async throws {
        let transport = FakeTransport()
        transport.openError = .unauthorized
        let client = RPCClient(transport: transport)
        do {
            try await client.start()
            XCTFail("expected unauthorized")
        } catch let error as RPCError {
            XCTAssertEqual(error, .unauthorized)
        }
    }

    func testRefusedTokenAfterOpenFailsCallsAsUnauthorized() async throws {
        // The runner accepts the upgrade, then closes with 4401 on the first
        // request when the header token is wrong.
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        let hello = Task { try await client.hello(version: "1") }
        await transport.waitForSent(1)
        transport.close(with: .unauthorized)
        do {
            _ = try await hello.value
            XCTFail("expected unauthorized")
        } catch let error as RPCError {
            XCTAssertEqual(error, .unauthorized)
        }
        var closed = client.closed.makeAsyncIterator()
        let reason = await closed.next()
        XCTAssertEqual(reason, .unauthorized)
    }

    func testThreadUpdateParamsEncodeNullToClear() {
        let update = ThreadUpdate(model: .some(nil), effort: .some(nil), snoozedUntil: .some(nil), archived: false)
        XCTAssertEqual(update.params["model"], .null)
        XCTAssertEqual(update.params["effort"], .null)
        XCTAssertEqual(ThreadUpdate(effort: .some("xhigh")).params["effort"], .string("xhigh"))
        XCTAssertEqual(ThreadUpdate(fastMode: true).params["fastMode"], .bool(true))
        XCTAssertEqual(ThreadUpdate(fastMode: false).params["fastMode"], .bool(false))
        XCTAssertNil(update.params["fastMode"])
        XCTAssertEqual(update.params["snoozedUntil"], .null)
        XCTAssertEqual(update.params["archived"], .bool(false))
        XCTAssertNil(update.params["title"])
        let snooze = ThreadUpdate(snoozedUntil: Date(timeIntervalSince1970: 0))
        XCTAssertEqual(snooze.params["snoozedUntil"]?.stringValue, "1970-01-01T00:00:00.000Z")
        XCTAssertEqual(ThreadUpdate(snoozeUntilRestart: true).params["snoozedUntil"], .string("restart"))
    }

    func testJSONPeek() {
        XCTAssertEqual(JSONPeek.topLevel(Data(#"{"jsonrpc":"2.0","id":12,"result":{"id":99,"method":"x"}}"#.utf8)),
                       JSONPeek.Result(id: 12, hasId: true, method: nil))
        XCTAssertEqual(JSONPeek.topLevel(Data(#"{"result":{"text":"a \"}\" b","id":3},"id":7}"#.utf8)),
                       JSONPeek.Result(id: 7, hasId: true, method: nil))
        XCTAssertEqual(JSONPeek.topLevel(Data(#" { "method" : "thread.event", "params": [1, {"id": 2}] }"#.utf8)),
                       JSONPeek.Result(id: nil, hasId: false, method: "thread.event"))
        XCTAssertEqual(JSONPeek.topLevel(Data(#"{"id":"5","result":true}"#.utf8)).id, 5)
        XCTAssertEqual(JSONPeek.topLevel(Data("not json".utf8)), JSONPeek.Result(id: nil, hasId: false, method: nil))
    }
}
