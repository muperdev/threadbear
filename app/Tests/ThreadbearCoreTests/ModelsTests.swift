import XCTest
@testable import ThreadbearCore

final class ModelsTests: XCTestCase {
    func testUrlActionDecodesAndOnlyOpensWebPages() throws {
        let json = #"{"id":"links.asc","title":"App Store Connect","kind":"url","url":"https://appstoreconnect.apple.com/apps","patch":"links"}"#
        let action = try JSONDecoder().decode(Action.self, from: Data(json.utf8))
        XCTAssertEqual(action.kind, .url)
        XCTAssertEqual(action.webPage?.absoluteString, "https://appstoreconnect.apple.com/apps")
        // Never anything but a web page, even if a runner sent one.
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "x-apple.systempreferences:", "https://"] {
            XCTAssertNil(Action(id: "x", title: "x", kind: .url, url: bad).webPage, bad)
        }
        XCTAssertNil(Action(id: "x", title: "x", kind: .command, url: "https://a.dev").webPage)
        // A kind from a later runner still reads as a command.
        let later = try JSONDecoder().decode(Action.self, from: Data(#"{"id":"x","kind":"someday"}"#.utf8))
        XCTAssertEqual(later.kind, .command)
    }

    func testThreadSummaryWorktreeDecodesLeniently() throws {
        let json = #"{"id":"t1","title":"x","worktree":{"branch":"thread/ab12cd34","mainCheckout":"/Users/m/dev/me/threadbear","path":"/Users/m/dev/me/threadbear-threads/ab12cd34","later":1}}"#
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        XCTAssertEqual(thread.worktree, ThreadWorktree(branch: "thread/ab12cd34", mainCheckout: "/Users/m/dev/me/threadbear", path: "/Users/m/dev/me/threadbear-threads/ab12cd34"))
        let again = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread))
        XCTAssertEqual(again.worktree, thread.worktree)
        // Absent from an older runner; a branch alone is enough to show; a
        // malformed one is dropped rather than losing the thread.
        XCTAssertNil(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1","title":"x"}"#.utf8)).worktree)
        XCTAssertEqual(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1","worktree":{"branch":"thread/1"}}"#.utf8)).worktree?.branch, "thread/1")
        let odd = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1","title":"y","worktree":{"path":3}}"#.utf8))
        XCTAssertNil(odd.worktree)
        XCTAssertEqual(odd.title, "y")
        XCTAssertNil(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1","worktree":"thread/1"}"#.utf8)).worktree)
    }

    func testThreadSummaryDecodesFullAndToleratesUnknownFields() throws {
        let json = """
        {"id":"th1","title":"Fix the flaky test","cwd":"/Users/m/dev/me/misc","accountId":"claude-2","provider":"claude",
         "model":"claude-haiku-5-5","effort":"xhigh","modelInUse":"claude-haiku-5-5","effortInUse":"high",
         "permissionMode":"ask","status":"waiting","live":true,"needsYou":true,
         "createdAt":"2026-10-26T10:00:00Z","updatedAt":"2026-10-26T10:05:00.250Z","lastActivityAt":"2026-10-26T10:06:00Z",
         "snoozedUntil":"2026-10-27T09:00:00+01:00","preview":"Done.","lastSeq":42,"lastSeenSeq":40,
         "costUsd":0.42,"contextTokens":12345,"futureField":{"a":1},"status2":"x"}
        """
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        XCTAssertEqual(thread.accountId, "claude-2")
        XCTAssertEqual(thread.permissionMode, .ask)
        XCTAssertEqual(thread.status, .waiting)
        XCTAssertEqual(thread.lastSeq, 42)
        XCTAssertEqual(thread.costUsd, 0.42)
        XCTAssertEqual(thread.contextTokens, 12345)
        XCTAssertEqual(thread.effort, "xhigh")
        XCTAssertEqual(thread.modelInUse, "claude-haiku-5-5")
        XCTAssertEqual(thread.effortInUse, "high")
        let again = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread))
        XCTAssertEqual(again.effort, "xhigh")
        XCTAssertEqual(again.effortInUse, "high")
        XCTAssertEqual(thread.updatedAt.timeIntervalSince1970, 1_793_008_800 + 300.25, accuracy: 0.001)
        XCTAssertEqual(thread.snoozedUntil?.timeIntervalSince1970, ISO8601.parse("2026-10-27T08:00:00Z")?.timeIntervalSince1970)
    }

    func testThreadSummaryWithOnlyAnId() throws {
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"x","status":"brand-new-status"}"#.utf8))
        XCTAssertEqual(thread.status, .idle)
        XCTAssertEqual(thread.permissionMode, .full)
        XCTAssertFalse(thread.needsYou)
    }

    func testISO8601() {
        XCTAssertEqual(ISO8601.parse("1970-01-01T00:00:00Z")?.timeIntervalSince1970, 0)
        XCTAssertEqual(ISO8601.parse("2000-03-01T12:30:15.5Z")?.timeIntervalSince1970, 951_913_815.5)
        XCTAssertEqual(ISO8601.parse("2000-03-01T13:30:15-01:00")?.timeIntervalSince1970, 951_921_015)
        XCTAssertNil(ISO8601.parse("yesterday"))
        let now = Date(timeIntervalSince1970: 1_793_000_000.125)
        XCTAssertEqual(ISO8601.parse(ISO8601.format(now))!.timeIntervalSince1970, now.timeIntervalSince1970, accuracy: 0.001)
        // UTC with a Z, as the protocol says; the runner reads zone-less times as local.
        XCTAssertEqual(ISO8601.format(Date(timeIntervalSince1970: 1_793_000_000.25)), "2026-10-26T07:33:20.250Z")
    }

    func testEveryEventTypeDecodes() {
        XCTAssertEqual(E.user(1, "hi").payload, .userMessage(text: "hi", images: []))
        XCTAssertEqual(E.text(2, "m1", "**Hi**").payload, .assistantText(itemId: "m1", text: "**Hi**"))
        XCTAssertEqual(E.event(3, "reasoning", ["itemId": "r1", "text": "Thinking"]).payload, .reasoning(itemId: "r1", text: "Thinking"))
        guard case .toolCall(let call) = E.call(4, "c1", input: ["command": "ls -la"]).payload else { return XCTFail() }
        XCTAssertEqual(call.kind, .command)
        XCTAssertEqual(call.input["command"], .string("ls -la"))
        guard case .toolResult(let result) = E.result(5, "c1", ok: false, text: "boom",
                                                       images: [["path": "/tmp/a.png", "mediaType": "image/png", "width": 10, "height": 20]]).payload
        else { return XCTFail() }
        XCTAssertFalse(result.ok)
        XCTAssertEqual(result.images.first?.width, 10)
        guard case .editDiff(let diff) = E.event(6, "edit.diff", ["itemId": "c2", "path": "/a.swift", "hunks": [
            ["oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-a", "+b"]],
        ]]).payload else { return XCTFail() }
        XCTAssertEqual(diff.hunks.first?.lines, ["-a", "+b"])
        guard case .approvalRequested(let approval) = E.event(7, "approval.requested", [
            "approvalId": "ap1", "name": "Bash", "title": "rm -rf build", "input": [:], "options": ["allow", "allowAlways", "deny", "somethingNew"],
        ]).payload else { return XCTFail() }
        XCTAssertEqual(approval.options, [.allow, .allowAlways, .deny])
        guard case .questionRequested(let question) = E.event(8, "question.requested", [
            "approvalId": "q1", "questions": [["question": "Which?", "header": "Pick", "multiSelect": false, "options": [["label": "A"], ["label": "B", "description": "bee"]]]],
        ]).payload else { return XCTFail() }
        XCTAssertEqual(question.questions.first?.options.last?.description, "bee")
        XCTAssertEqual(E.event(9, "approval.resolved", ["approvalId": "ap1", "decision": "allowAlways"]).payload,
                       .approvalResolved(ApprovalResolution(approvalId: "ap1", decision: .allowAlways)))
        XCTAssertEqual(E.event(10, "plan.updated", ["entries": [["text": "Step", "status": "in_progress"]]]).payload,
                       .planUpdated([PlanEntry(text: "Step", status: .in_progress)]))
        XCTAssertEqual(E.event(11, "turn.started", ["turnId": "t1"]).payload, .turnStarted())
        XCTAssertEqual(E.event(11, "turn.started", ["turnId": "t1", "wokeBy": [["kind": "agent", "description": "Mine the repo"]]]).payload,
                       .turnStarted(wokeBy: [BackgroundTask(kind: .agent, description: "Mine the repo")]))
        guard case .turnCompleted(let done) = E.event(12, "turn.completed", ["turnId": "t1", "outcome": "failed", "durationMs": 10, "error": ["message": "Overloaded"]]).payload
        else { return XCTFail() }
        XCTAssertEqual(done.outcome, .failed)
        XCTAssertEqual(done.error, "Overloaded")
        XCTAssertEqual(E.event(13, "system.note", ["text": "Resumed session"]).payload, .systemNote("Resumed session"))
        XCTAssertEqual(E.event(14, "error", ["message": "CLI crashed"]).payload, .error("CLI crashed"))
        XCTAssertEqual(E.event(14, "reasoning.delta", ["itemId": "r", "text": "x"]).payload, .reasoningDelta(itemId: "r", text: "x"))
        XCTAssertEqual(E.event(14, "tool.output.delta", ["itemId": "c", "text": "x"]).payload, .toolOutputDelta(itemId: "c", text: "x"))
        XCTAssertEqual(E.event(15, "something.new", ["foo": 1]).payload, .unknown)
        XCTAssertTrue(E.delta(15, "m", "x").isTransient)
        XCTAssertFalse(E.text(15, "m", "x").isTransient)
    }

    /// docs/protocol.md, "Secrets": the card's text, tolerant of what it
    /// doesn't know, and never anything like a value.
    func testSecretRequestedDecodes() {
        guard case .secretRequested(let secret) = E.event(1, "secret.requested", [
            "approvalId": "sec1", "itemId": "toolu_1", "label": "Mac administrator password",
            "reason": "Install the Keep Awake rule.", "placeholder": "Your login password", "somethingNew": ["x": 1],
        ]).payload else { return XCTFail("secret.requested") }
        XCTAssertEqual(secret, SecretRequest(approvalId: "sec1", itemId: "toolu_1", label: "Mac administrator password",
                                             reason: "Install the Keep Awake rule.", placeholder: "Your login password"))
        // Only the required fields; an empty item id or placeholder is none.
        guard case .secretRequested(let bare) = E.event(2, "secret.requested", [
            "approvalId": "sec2", "itemId": "", "label": "  API key ", "reason": "For wrangler", "placeholder": " ",
        ]).payload else { return XCTFail("secret.requested without extras") }
        XCTAssertNil(bare.itemId)
        XCTAssertNil(bare.placeholder)
        XCTAssertEqual(bare.label, "API key")
        // A label of the wrong type still makes a card that says something.
        guard case .secretRequested(let odd) = E.event(3, "secret.requested", ["approvalId": "sec3", "label": 7]).payload
        else { return XCTFail("secret.requested with an odd label") }
        XCTAssertEqual(odd.label, "A secret")
        XCTAssertEqual(odd.reason, "")
    }

    func testEventsPageSkipsJunkButKeepsTheSeqOfUnreadableEvents() throws {
        // An object with a seq stays (as .unknown) so the seq order has no
        // hole; anything that isn't an object is skipped.
        let json = #"{"events":[{"seq":1,"type":"user.message","text":"a","images":[]},{"seq":2},"junk",{"seq":3,"type":"system.note","text":"n"}],"firstSeq":1,"lastSeq":3,"inProgress":[{"itemId":"m9","type":"assistant.text","text":"Par"}]}"#
        let page = try JSONDecoder().decode(EventsPage.self, from: Data(json.utf8))
        XCTAssertEqual(page.events.map(\.seq), [1, 2, 3])
        XCTAssertEqual(page.events[1].payload, .unknown)
        XCTAssertEqual(page.inProgress.first?.text, "Par")
    }

    /// Shapes the strict decoders used to reject. Each one used to drop the
    /// whole event, which opened a seq gap the feed could never close.
    func testKnownEventsInOtherShapesStillDecode() {
        // A hunk with no counts: they come from its lines.
        guard case .editDiff(let diff) = E.event(1, "edit.diff", ["itemId": "c1", "path": "/a", "hunks": [
            ["oldStart": 3, "newStart": 3, "lines": [" x", "-a", "+b", "+c"]],
        ]]).payload else { return XCTFail("edit.diff without oldLines") }
        XCTAssertEqual(diff.hunks.first?.oldLines, 2)
        XCTAssertEqual(diff.hunks.first?.newLines, 3)
        // TodoWrite's `content` instead of `text`, and an unknown status.
        XCTAssertEqual(E.event(2, "plan.updated", ["entries": [["content": "Step one", "status": "blocked"], "junk"]]).payload,
                       .planUpdated([PlanEntry(text: "Step one", status: .pending)]))
        // An option with no label is dropped; a bare string is a label.
        guard case .questionRequested(let question) = E.event(3, "question.requested", [
            "approvalId": "q1", "questions": [["question": "Which?", "options": [["description": "no label"], "Plain", ["label": "B"]]]],
        ]).payload else { return XCTFail("question with an unlabelled option") }
        XCTAssertEqual(question.questions.first?.options.map(\.label), ["Plain", "B"])
        // approval.resolved with no decision.
        guard case .approvalResolved(let resolution) = E.event(4, "approval.resolved", ["approvalId": "ap1"]).payload
        else { return XCTFail("approval.resolved without a decision") }
        XCTAssertEqual(resolution.approvalId, "ap1")
        // A result with one broken image keeps the good one.
        guard case .toolResult(let result) = E.event(5, "tool.result", ["itemId": "c1", "images": [["mediaType": "image/png"], ["path": "/a.png"]]]).payload
        else { return XCTFail("tool.result with a broken image") }
        XCTAssertEqual(result.images.map(\.path), ["/a.png"])
    }

    // MARK: Step 4: Codex, accounts, usage and CLI updates

    func testCodexAccountWithUsageNoteAndModelDefaults() throws {
        let json = """
        {"id":"codex-main","provider":"codex","title":"Codex · me@example.com","configDir":"/Users/me/.codex",
         "email":"me@example.com","plan":"Pro Lite","note":"Side projects","defaultModel":"gpt-6.1-sol","defaultEffort":"high",
         "models":[{"id":"gpt-6-luna","model":"gpt-6-luna","title":"GPT-6-Luna","effortLevels":["low","medium","high","ultra"],"defaultEffort":"medium"}],
         "usage":{"windows":[
            {"kind":"five_hour","title":"5-hour","remainingPercent":89,"resetsAt":"2026-10-27T05:00:00Z"},
            {"kind":"allowance","title":"$250 allowance","remainingPercent":53.11612,"detail":"$132.79 left"},
            {"kind":"hourly","title":"Hourly","remainingPercent":140},
            {"kind":"weekly","title":"No number"}],
          "readAt":"2026-10-27T01:00:00Z","source":"live","blocked":true,"notice":"64,390 credits","later":1},
         "somethingNew":[1]}
        """
        let account = try JSONDecoder().decode(Account.self, from: Data(json.utf8))
        XCTAssertEqual(account.provider, "codex")
        XCTAssertEqual(account.email, "me@example.com")
        XCTAssertEqual(account.plan, "Pro Lite")
        XCTAssertEqual(account.note, "Side projects")
        XCTAssertEqual(account.defaultEffort, "high")
        XCTAssertEqual(account.models.first?.effortLevels.last, "ultra")
        XCTAssertFalse(account.models.first?.fastMode ?? true, "absent is no fast mode")
        let opus = try JSONDecoder().decode(ModelChoice.self, from: Data(#"{"id":"opus","fastMode":true}"#.utf8))
        XCTAssertTrue(opus.fastMode)
        let fast = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"th1","fastMode":true}"#.utf8))
        XCTAssertTrue(fast.fastMode)
        XCTAssertTrue(try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(fast)).fastMode, "kept through the cache")
        XCTAssertFalse(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"th1"}"#.utf8)).fastMode)
        XCTAssertEqual(account.models.first?.defaultEffort, "medium")
        let usage = try XCTUnwrap(account.usage)
        XCTAssertEqual(usage.source, "live")
        XCTAssertTrue(usage.blocked)
        XCTAssertFalse(usage.stale)
        XCTAssertEqual(usage.notice, "64,390 credits")
        XCTAssertEqual(usage.readAt, ISO8601.parse("2026-10-27T01:00:00Z"))
        // An unknown kind keeps its title; a percentage past 100 is capped;
        // a window with no percentage is skipped.
        XCTAssertEqual(usage.windows.map(\.kind), ["five_hour", "allowance", "hourly"])
        XCTAssertEqual(usage.windows[1].remainingPercent, 53.11612, accuracy: 0.0001)
        XCTAssertEqual(usage.windows[1].detail, "$132.79 left")
        XCTAssertEqual(usage.windows[2].remainingPercent, 100)
        XCTAssertNotNil(usage.windows[0].resetsAt)
        let again = try JSONDecoder().decode(Account.self, from: JSONEncoder().encode(account))
        XCTAssertEqual(again, account)
    }

    func testAccountFromAnOlderRunnerOrWithUnreadableUsage() throws {
        let old = try JSONDecoder().decode(Account.self, from: Data(#"{"id":"claude-1","provider":"claude","title":"a@b.c","configDir":"/x"}"#.utf8))
        XCTAssertNil(old.usage)
        XCTAssertNil(old.email)
        XCTAssertNil(old.note)
        // A usage block or field of the wrong shape costs only that field.
        let odd = try JSONDecoder().decode(Account.self, from: Data(#"{"id":"claude-2","usage":"soon","note":7,"defaultEffort":["x"]}"#.utf8))
        XCTAssertEqual(odd.title, "claude-2")
        XCTAssertNil(odd.usage)
        XCTAssertNil(odd.note)
        XCTAssertNil(odd.defaultEffort)
    }

    func testCodexThreadSummary() throws {
        let json = #"{"id":"th1","provider":"codex","accountId":"codex-main","effort":"ultra","effortInUse":"ultra","contextTokens":1200,"contextWindow":258400}"#
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        XCTAssertEqual(thread.provider, "codex")
        XCTAssertEqual(thread.effort, "ultra")
        XCTAssertEqual(thread.contextWindow, 258_400)
        XCTAssertNil(thread.costUsd)
        let again = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread))
        XCTAssertEqual(again.contextWindow, 258_400)
        XCTAssertNil(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"x","contextWindow":"big"}"#.utf8)).contextWindow)
    }

    func testHelloWithPerProviderDefaultsAndCLIs() throws {
        let json = """
        {"runnerVersion":"0.2.0+r9","protocol":1,"accounts":[{"id":"claude-3"},{"id":"codex-main","provider":"codex"}],
         "defaults":{"cwd":"/w","accountId":"claude-3","model":"claude-haiku-5-5",
                     "models":{"claude":"claude-haiku-5-5","codex":"gpt-6-luna","grok":7},"efforts":{"codex":"low"}},
         "clis":[
           {"provider":"codex","path":"/h/tools/codex/current","managedBy":"threadbear","installedVersion":"0.164.0","latestVersion":"0.165.0",
            "status":"held","policy":"auto","inUse":[{"version":"0.164.0","threadIds":["th1"]},{"broken":true}],"previousVersion":"0.163.0",
            "checkedAt":"2026-10-27T01:00:00Z","hold":{"version":"0.165.0","reason":"the app-server protocol changed","missing":["turn/steer"]},
            "update":{"state":"failed","targetVersion":"0.165.0","finishedAt":"2026-10-27T01:01:00Z","message":"held"}},
           {"provider":"claude","status":"behind","policy":"notify","update":{"state":"waiting","waitingFor":["th2"]},"newThing":true},
           {"path":"no provider"}]}
        """
        let hello = try JSONDecoder().decode(HelloResult.self, from: Data(json.utf8))
        XCTAssertEqual(hello.defaults.models, ["claude": "claude-haiku-5-5", "codex": "gpt-6-luna"])
        XCTAssertEqual(hello.defaults.efforts, ["codex": "low"])
        XCTAssertEqual(hello.defaults.model(for: "codex"), "gpt-6-luna")
        XCTAssertEqual(hello.clis.map(\.provider), ["codex", "claude"], "a status with no provider is skipped")
        let codex = hello.clis[0]
        XCTAssertEqual(codex.managedBy, "threadbear")
        XCTAssertEqual(codex.status, "held")
        XCTAssertEqual(codex.inUse, [CliStatus.InUse(version: "0.164.0", threadIds: ["th1"])])
        XCTAssertEqual(codex.hold?.missing, ["turn/steer"])
        XCTAssertEqual(codex.update?.state, "failed")
        XCTAssertNotNil(codex.update?.finishedAt)
        XCTAssertNotNil(codex.checkedAt)
        let claude = hello.clis[1]
        XCTAssertEqual(claude.policy, "notify")
        XCTAssertEqual(claude.update?.waitingFor, ["th2"])
        XCTAssertEqual(claude.managedBy, "cli")
    }

    func testHelloFromAnOlderRunner() throws {
        let json = #"{"runnerVersion":"0.1.0+r3","protocol":1,"accounts":[{"id":"claude-3"}],"defaults":{"cwd":"/w","accountId":"claude-3","model":"claude-haiku-5-5"}}"#
        let hello = try JSONDecoder().decode(HelloResult.self, from: Data(json.utf8))
        XCTAssertTrue(hello.clis.isEmpty)
        XCTAssertTrue(hello.defaults.models.isEmpty)
        XCTAssertEqual(hello.defaults.model(for: "claude"), "claude-haiku-5-5", "defaults.model is Claude's")
        XCTAssertNil(hello.defaults.model(for: "codex"))
        // Defaults of the wrong shape fall back rather than failing hello.
        let odd = try JSONDecoder().decode(HelloResult.self, from: Data(#"{"accounts":[{"id":"claude-1"}],"defaults":{"cwd":4}}"#.utf8))
        XCTAssertEqual(odd.defaults.accountId, "claude-1")
    }

    func testCodexTurnAndToolKinds() {
        guard case .toolCall(let image) = E.event(1, "tool.call", ["itemId": "ig1", "name": "imageGeneration", "kind": "image", "title": "A cat", "input": [:]]).payload
        else { return XCTFail() }
        XCTAssertEqual(image.kind, .image)
        // A kind newer than this app reads as other, as "image" does in apps
        // that predate it.
        guard case .toolCall(let future) = E.event(2, "tool.call", ["itemId": "x1", "name": "x", "kind": "hologram", "title": "x", "input": [:]]).payload
        else { return XCTFail() }
        XCTAssertEqual(future.kind, .other)
        guard case .turnCompleted(let done) = E.event(3, "turn.completed", [
            "turnId": "t1", "outcome": "failed", "durationMs": 900, "error": "Usage limit reached", "errorKind": "usageLimit",
            "usage": ["inputTokens": 10, "cachedInputTokens": 4, "outputTokens": 2, "reasoningTokens": 1],
        ]).payload else { return XCTFail() }
        XCTAssertEqual(done.errorKind, .usageLimit)
        XCTAssertNil(done.costUsd)
        XCTAssertEqual(done.usage?["reasoningTokens"], .number(1))
        guard case .turnCompleted(let unknown) = E.event(4, "turn.completed", ["turnId": "t2", "outcome": "failed", "durationMs": 1, "errorKind": "solarFlare"]).payload
        else { return XCTFail("an unknown errorKind must not drop the event") }
        XCTAssertNil(unknown.errorKind)
    }

    func testUnreadablePayloadBecomesUnknownButKeepsItsSeq() {
        // `hunks` should be an array; `text` on a delta should be a string.
        let broken = E.event(7, "edit.diff", ["itemId": 12, "hunks": "nope", "path": ["x"]])
        XCTAssertEqual(broken.seq, 7)
        XCTAssertFalse(broken.isTransient)
        let delta = E.event(7, "assistant.text.delta", ["itemId": "m1", "text": ["not", "text"]])
        XCTAssertTrue(delta.isTransient, "a delta stays transient even if it can't be read")
    }

    func testThreadSummaryBackgroundDecodesAndTakesUnknownKindsAsOther() throws {
        let json = """
        {"id":"th1","title":"t","cwd":"/","accountId":"a","provider":"claude","permissionMode":"full","status":"idle","live":true,"needsYou":false,
         "createdAt":"2026-10-26T10:00:00Z","updatedAt":"2026-10-26T10:00:00Z","lastActivityAt":"2026-10-26T10:00:00Z","preview":"","lastSeq":1,"lastSeenSeq":1,
         "background":[{"kind":"agent","description":"Rebuild page A"},{"kind":"shell","description":"npm test","finished":true},{"kind":"dream"},"junk"]}
        """
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        XCTAssertEqual(thread.background, [
            BackgroundTask(kind: .agent, description: "Rebuild page A"),
            BackgroundTask(kind: .shell, description: "npm test", finished: true),
            BackgroundTask(kind: .other, description: ""),
        ])
        XCTAssertTrue(thread.isWorkingInBackground)
        let again = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread))
        XCTAssertEqual(again.background, thread.background)
        // Absent means none, and a running turn isn't "in the background".
        var none = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.replacingOccurrences(of: "\"background\":[{\"kind\":\"agent\",\"description\":\"Rebuild page A\"},{\"kind\":\"shell\",\"description\":\"npm test\",\"finished\":true},{\"kind\":\"dream\"},\"junk\"]", with: "\"x\":1").utf8))
        XCTAssertEqual(none.background, [])
        XCTAssertFalse(none.isWorkingInBackground)
        none.background = thread.background
        none.status = .running
        XCTAssertFalse(none.isWorkingInBackground)
    }
}
