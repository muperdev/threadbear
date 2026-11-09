import Foundation
import XCTest
@testable import ThreadbearCore

/// What step 4 adds for Codex threads: patches that touch several files,
/// images it makes and looks at, a command that finishes after its turn,
/// its model list and effort levels, its CLI status and its token counts.
final class CodexTranscriptTests: XCTestCase {
    private func hunk(_ lines: [String], at start: Int = 1) -> [String: Any] {
        ["oldStart": start, "oldLines": lines.filter { !$0.hasPrefix("+") }.count, "newStart": start,
         "newLines": lines.filter { !$0.hasPrefix("-") }.count, "lines": lines]
    }

    private func diff(_ seq: Int, _ item: String, _ path: String, _ lines: [String]) -> Event {
        E.event(seq, "edit.diff", ["itemId": item, "path": path, "hunks": [hunk(lines)]])
    }

    private func patchCall(_ seq: Int) -> Event {
        E.call(seq, "p1", name: "apply_patch", kind: "edit", title: "Edit 3 files",
               input: ["paths": ["/w/site/events.json", "/w/site/events.html", "/w/site/styles.css"]])
    }

    // MARK: Multi-file edits

    func testOneItemWithThreePathsKeepsThreeDiffs() {
        let model = TranscriptModel(events: [
            E.user(1, "Add the meetup"), patchCall(2),
            diff(3, "p1", "/w/site/events.json", ["-[]", "+[{\"id\": 1}]"]),
            diff(4, "p1", "/w/site/events.html", ["+<li>Meetup</li>"]),
            diff(5, "p1", "/w/site/styles.css", ["+.poster {}"]),
            // Codex sends a path again as the patch changes: it replaces.
            diff(6, "p1", "/w/site/styles.css", ["+.poster {}", "+.map {}"]),
            E.result(7, "p1"), E.text(8, "m1", "Done"), E.done(9),
        ])
        guard case .tool(let tool)? = model.item(id: "t:p1")?.content else { return XCTFail("no tool row") }
        XCTAssertEqual(tool.diffs.map(\.path), ["/w/site/events.json", "/w/site/events.html", "/w/site/styles.css"])
        XCTAssertEqual(tool.diffs.last?.hunks.first?.lines, ["+.poster {}", "+.map {}"], "the repeated path replaced its diff")
        XCTAssertEqual(model.item(id: "t:p1")?.kind, .diff)
        // The Edited files card lists and counts all three.
        guard case .changes(let card)? = model.row(id: "cf:t1")?.content else { return XCTFail("no card") }
        XCTAssertEqual(card.title, "Edited 3 files")
        XCTAssertEqual(card.files.map(\.name), ["events.json", "events.html", "styles.css"])
        XCTAssertEqual(card.files.last?.added, 2)
        XCTAssertEqual(card.added, 4)
        XCTAssertEqual(card.removed, 1)
    }

    func testTheWorkLineCountsEachFileOfAPatch() {
        let model = TranscriptModel(events: [
            E.user(1, "Go"), E.event(2, "reasoning", ["itemId": "r1", "text": "Editing the three files."]), patchCall(3),
            diff(4, "p1", "/w/site/events.json", ["+a"]), diff(5, "p1", "/w/site/events.html", ["+b"]), diff(6, "p1", "/w/site/styles.css", ["+c"]),
            E.result(7, "p1"), E.text(8, "m1", "Done"),
        ])
        guard case .group(let group)? = model.rows.first(where: { $0.kind == .group })?.content else { return XCTFail("no group") }
        XCTAssertEqual(group.summary, "Edited 3 files")
        // Before any diff arrives, the call's `paths` still count.
        let early = TranscriptModel(events: [E.user(1, "Go"), patchCall(2)])
        XCTAssertEqual(WorkSummary.describe(early.rows.filter { $0.kind == .diff }).text, "Edited 3 files")
    }

    func testAClaudeEditStillBuildsItsDiffFromTheInput() {
        let model = TranscriptModel(events: [
            E.user(1, "Go"),
            E.call(2, "c1", name: "Edit", kind: "edit", title: "Edit a.swift", input: ["file_path": "/a.swift", "old_string": "let a = 1", "new_string": "let a = 2"]),
            E.result(3, "c1"), E.done(4),
        ])
        guard case .tool(let tool)? = model.item(id: "t:c1")?.content else { return XCTFail() }
        XCTAssertTrue(tool.diffs.isEmpty)
        XCTAssertEqual(tool.effectiveDiffs.first?.hunks.first?.lines, ["-let a = 1", "+let a = 2"])
    }

    // MARK: A late result

    /// A command Codex left running when its turn ended reports after the
    /// turn, with no turn id: it fills its row, which stops looking busy,
    /// and the turn's footer and work line stay where they were.
    func testALateResultWithNoTurnFillsItsRow() {
        var model = TranscriptModel(events: [
            E.user(1, "Build it"),
            E.call(2, "b1", name: "shell", kind: "command", title: "npm run build", input: ["command": "npm run build"]),
            E.text(3, "m1", "It's building."), E.done(4),
        ])
        let before = model.rows.map(\.id)
        let footer = model.indexOf(id: "f:t1")
        _ = model.apply([E.event(5, "tool.result", ["itemId": "b1", "ok": true, "text": "built in 8s", "truncated": false, "images": []], turn: nil)])
        guard case .tool(let tool)? = model.item(id: "t:b1")?.content else { return XCTFail("no row") }
        XCTAssertTrue(tool.isFinished)
        XCTAssertEqual(tool.result?.text, "built in 8s")
        XCTAssertEqual(model.item(id: "t:b1")?.turnId, "t1", "still in its turn")
        XCTAssertEqual(model.rows.map(\.id), before, "no row moved or appeared")
        XCTAssertEqual(model.indexOf(id: "f:t1"), footer)
        XCTAssertNil(model.rows.first { $0.kind == .working }, "nothing looks busy")
    }

    // MARK: Images

    func testWorkSummaryImageWords() {
        let image: [String: Any] = ["path": "/m/poster.png", "mediaType": "image/png", "width": 512, "height": 512]
        let model = TranscriptModel(events: [
            E.user(1, "Make a poster"),
            E.call(2, "ig1", name: "image_generation", kind: "image", title: "Generate image"),
            E.result(3, "ig1", text: "A poster", images: [image]),
            E.call(4, "v1", name: "view_image", kind: "read", title: "View docs/map.png", input: ["path": "/w/docs/map.png"]),
            E.result(5, "v1", images: [image]),
            E.call(6, "ig2", name: "image_generation", kind: "image", title: "Generate image"),
            E.result(7, "ig2", text: "Another", images: [image]),
            E.text(8, "m1", "Here they are"), E.done(9),
        ])
        let summaries = ["t:ig1", "t:v1", "t:ig2"].compactMap { id in model.item(id: id).map { WorkSummary.describe([$0]).text } }
        XCTAssertEqual(summaries, ["Made an image", "Viewed an image", "Made an image"])
        // The pictures it made are the answer, so they stay out of the
        // finished turn's line of work; one it only looked at folds away.
        guard case .images(_, _, let generated)? = model.row(id: "i:ig1")?.content else { return XCTFail("no image row") }
        XCTAssertTrue(generated, "a generated image is shown larger")
        XCTAssertNotNil(model.row(id: "i:ig2"))
        XCTAssertNil(model.row(id: "i:v1"))
        guard case .images(_, _, let viewed)? = model.item(id: "i:v1")?.content else { return XCTFail("no image row") }
        XCTAssertFalse(viewed)
        let rows = [
            TranscriptRow(id: "t:a", seq: 1, content: .tool(ToolItem(itemId: "a", call: ToolCall(itemId: "a", name: "image_generation", kind: .image, title: "Generate image"),
                                                                      result: ToolResult(itemId: "a", ok: true, text: "", images: [ImageRef(path: "/1.png", mediaType: "image/png"), ImageRef(path: "/2.png", mediaType: "image/png")])))),
        ]
        XCTAssertEqual(WorkSummary.describe(rows).text, "Made 2 images")
        XCTAssertEqual(ToolPresentation.symbol(for: .image, name: "image_generation"), "photo")
    }

    // MARK: Failures

    func testTheErrorRowLearnsWhyTheTurnFailed() {
        let model = TranscriptModel(events: [
            E.user(1, "Go"),
            E.event(2, "error", ["message": "The conversation is too long"]),
            E.event(3, "turn.completed", ["turnId": "t1", "outcome": "failed", "durationMs": 900, "error": "The conversation is too long", "errorKind": "contextWindow"]),
        ])
        guard case .error(let message, let kind)? = model.row(id: "e:2")?.content else { return XCTFail("no error row") }
        XCTAssertEqual(message, "The conversation is too long")
        XCTAssertEqual(kind, .contextWindow)
    }

    // MARK: Model picker

    private let codexModels = [
        ModelChoice(id: "gpt-6.1-sol", model: "gpt-6.1-sol", title: "GPT-6.1-Sol", effortLevels: ["low", "medium", "high", "xhigh", "max", "ultra"], defaultEffort: "low"),
        ModelChoice(id: "gpt-6-luna", model: "gpt-6-luna", title: "GPT-6-Luna", description: "Fast and affordable", effortLevels: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "medium"),
        ModelChoice(id: "gpt-5.6-sol", model: "gpt-5.6-sol", title: "GPT-5.6-Sol"),
    ]

    func testCodexModelsAreAllMainRowsInTheirOrder() {
        let rows = ModelPicker.rows(choices: codexModels, defaultId: "gpt-6.1-sol", selected: "gpt-6-luna", provider: "codex")
        XCTAssertEqual(rows.map(\.title), ["GPT-6.1-Sol", "GPT-6-Luna", "GPT-5.6-Sol"])
        XCTAssertTrue(rows.allSatisfy { !$0.isOlder }, "dashed ids aren't older models on Codex")
        XCTAssertEqual(rows.first { $0.isDefault }?.id, "gpt-6.1-sol")
        XCTAssertEqual(rows.first { $0.isSelected }?.id, "gpt-6-luna")
    }

    func testBeforeItsListCodexOffersOnlyItsDefault() {
        let rows = ModelPicker.rows(choices: [], defaultId: "gpt-6.1-sol", selected: nil, provider: "codex")
        XCTAssertEqual(rows.map(\.title), ["GPT-6.1-Sol"], "never the Claude aliases")
        XCTAssertTrue(rows[0].isDefault)
        XCTAssertTrue(ModelPicker.rows(choices: [], defaultId: nil, selected: nil, provider: "codex").isEmpty)
        // Claude still falls back to its aliases.
        XCTAssertEqual(ModelPicker.rows(choices: [], defaultId: nil, selected: nil).map(\.id), ["opus", "sonnet", "haiku"])
    }

    func testCodexEffortLevelsGoUpToUltra() {
        XCTAssertEqual(ModelPicker.effortLevels(for: "gpt-6.1-sol", in: codexModels, provider: "codex").last, "ultra")
        XCTAssertEqual(ModelPicker.effortLevels(for: "gpt-6-luna", in: codexModels, provider: "codex").last, "max")
        XCTAssertEqual(ModelPicker.effortLevels(for: "gpt-unknown", in: codexModels, provider: "codex"), ComposerChips.codexEffortLevels)
        XCTAssertEqual(ModelPicker.effortLevels(for: "opus", in: [], provider: "claude"), ComposerChips.effortLevels)
        XCTAssertEqual(ComposerChips.effortTitle("ultra"), "Ultra")
        XCTAssertEqual(ComposerChips.chipTitle(model: "gpt-6.1-sol", effort: "ultra", choices: codexModels), "GPT-6.1-Sol · Ultra")
    }

    func testTheEffortThatAppliesWithNoneChosen() {
        let account = Account(id: "codex-main", provider: "codex", title: "Codex · a@x", defaultEffort: "high")
        XCTAssertEqual(ModelPicker.impliedEffort(model: "gpt-6-luna", choices: codexModels, account: account, runnerDefault: "low"), "low", "the runner's default first")
        XCTAssertEqual(ModelPicker.impliedEffort(model: "gpt-6-luna", choices: codexModels, account: account, runnerDefault: nil), "high", "then the account's")
        let bare = Account(id: "codex-main", provider: "codex", title: "x")
        XCTAssertEqual(ModelPicker.impliedEffort(model: "gpt-6-luna", choices: codexModels, account: bare, runnerDefault: nil), "medium", "then the model's own")
        XCTAssertNil(ModelPicker.impliedEffort(model: "opus", choices: [], account: nil, runnerDefault: nil))
    }

    // MARK: Account picker

    func testAccountPickerSectionsByProvider() {
        let fiveHour = UsageWindow(kind: "five_hour", title: "5-hour", remainingPercent: 89)
        let accounts = [
            Account(id: "codex-main", provider: "codex", title: "Codex · mike@example.com", email: "mike@example.com",
                    usage: AccountUsage(windows: [UsageWindow(kind: "weekly", title: "Weekly", remainingPercent: 64)])),
            Account(id: "claude-main", title: "mike@example.com", email: "mike@example.com", usage: AccountUsage(windows: [fiveHour])),
            Account(id: "claude-2", title: "work@example.com", note: "Convex work only"),
            Account(id: "claude-4", title: "mike+4@example.com", usage: AccountUsage(windows: [UsageWindow(kind: "five_hour", title: "5-hour", remainingPercent: 0)], blocked: true)),
        ]
        let sections = AccountMenu.sections(accounts, selected: "claude-main", now: Date())
        XCTAssertEqual(sections.map(\.title), ["Claude", "Codex"], "Claude first whatever the runner's order")
        XCTAssertEqual(sections[0].items.map(\.title), ["mike@example.com", "work@example.com", "mike+4@example.com"])
        XCTAssertEqual(sections[0].items[1].subtitle, "Convex work only")
        XCTAssertTrue(sections[0].items[2].isBlocked, "still listed, and pickable")
        XCTAssertEqual(sections[1].items[0].subtitle, "week 64%")
        let lines = AccountMenu.lines(sections, footer: "Usage from Token Stats at 7:15 am")
        XCTAssertEqual(lines.first, "# Claude")
        XCTAssertTrue(lines.contains("[x] mike@example.com - 5h 89%"))
        XCTAssertTrue(lines.contains("[ ] mike+4@example.com - Limit reached"))
        XCTAssertEqual(lines.last, "--- Usage from Token Stats at 7:15 am")
    }

    // MARK: CLIs and tokens

    func testCliText() {
        let claude = CliStatus(provider: "claude", installedVersion: "2.1.311", latestVersion: "2.1.312", status: "behind")
        XCTAssertEqual(CliText.statusLine(claude), "Claude Code 2.1.311, 2.1.312 available")
        XCTAssertEqual(CliText.updateTitle(claude), "Update")
        var current = claude
        current.status = "current"
        XCTAssertEqual(CliText.statusLine(current), "Claude Code 2.1.311, up to date")
        XCTAssertNil(CliText.updateTitle(current))
        let codex = CliStatus(provider: "codex", installedVersion: "0.164.0", latestVersion: "0.165.0", status: "held", previousVersion: "0.163.0",
                              hold: CliStatus.Hold(version: "0.165.0", reason: "the app-server protocol changed"))
        XCTAssertEqual(CliText.statusLine(codex), "Codex 0.165.0 is held back because the app-server protocol changed")
        XCTAssertEqual(CliText.updateTitle(codex), "Update anyway")
        XCTAssertEqual(CliText.goBackTitle(codex), "Go back to 0.163.0")
        var running = codex
        running.update = CliStatus.Update(state: "running", targetVersion: "0.165.0")
        XCTAssertEqual(CliText.bannerText(running), "Installing Codex 0.165.0")
        XCTAssertNil(CliText.updateTitle(running), "no second update while one runs")
        var updating = claude
        updating.update = CliStatus.Update(state: "running", targetVersion: "2.1.312")
        XCTAssertEqual(CliText.bannerText(updating), "Updating Claude Code to 2.1.312")
        updating.update = CliStatus.Update(state: "failed", targetVersion: "2.1.312", message: "exit 1")
        XCTAssertEqual(CliText.bannerText(updating), "Couldn't update Claude Code to 2.1.312: exit 1")
        updating.update = CliStatus.Update(state: "succeeded", targetVersion: "2.1.312")
        XCTAssertNil(CliText.bannerText(updating))
        XCTAssertEqual(CliText.policies.map(CliText.policyTitle), ["Automatic", "Ask me", "Off"])
    }

    /// Update all installs every CLI that's behind: not one held back, not
    /// one already updating.
    func testUpdateAllTakesTheOnesBehind() {
        let claude = CliStatus(provider: "claude", installedVersion: "2.1.311", latestVersion: "2.1.312", status: "behind")
        let grok = CliStatus(provider: "grok", installedVersion: "1.0.77", latestVersion: "1.0.78", status: "behind")
        var devin = CliStatus(provider: "devin", installedVersion: "3000.11.205", latestVersion: "3000.12.32", status: "behind")
        let codex = CliStatus(provider: "codex", installedVersion: "0.164.0", latestVersion: "0.165.0", status: "held")
        XCTAssertEqual(CliText.updatable([claude, codex, grok, devin]), ["claude", "grok", "devin"])
        devin.update = CliStatus.Update(state: "running", targetVersion: "3000.12.32")
        XCTAssertEqual(CliText.updatable([claude, codex, grok, devin]), ["claude", "grok"])
        XCTAssertEqual(CliText.statusLine(grok), "Grok Build 1.0.77, 1.0.78 available")
        XCTAssertEqual(CliText.bannerText(devin), "Updating Devin to 3000.12.32")
    }

    /// After going back from a version the runner holds it, with its own
    /// reason; "Go back" to it would just be "Update anyway" again.
    func testAVersionWentBackFromIsHeldWithItsReason() {
        let cli = CliStatus(provider: "codex", installedVersion: "0.163.0", latestVersion: "0.165.0", status: "held", previousVersion: "0.164.0",
                            hold: CliStatus.Hold(version: "0.164.0", reason: "you went back from it"))
        XCTAssertEqual(CliText.statusLine(cli), "Codex 0.164.0 is held back because you went back from it")
        XCTAssertEqual(CliText.updateTitle(cli), "Update anyway")
        XCTAssertNil(CliText.goBackTitle(cli), "the kept version is the held one")
    }

    func testTokenLineForDebugInfo() {
        let codex: JSONValue = .object(["inputTokens": .number(24177), "cachedInputTokens": .number(12032), "outputTokens": .number(310), "reasoningTokens": .number(120)])
        XCTAssertEqual(TokenText.line(codex), "24,177 in (12,032 cached), 310 out, 120 reasoning")
        let claude: JSONValue = .object(["inputTokens": .number(12), "outputTokens": .number(900), "cacheReadTokens": .number(40000), "cacheWriteTokens": .number(2100)])
        XCTAssertEqual(TokenText.line(claude), "12 in (40,000 cached), 2,100 cache written, 900 out")
        XCTAssertNil(TokenText.line(nil))
        // The footer's own words in a dump and "Copy debug info".
        let footer = TranscriptRow(id: "f:t1", seq: 9, content: .footer(TurnCompletion(turnId: "t1", outcome: .completed, durationMs: 12_000, usage: codex)))
        XCTAssertEqual(RowDebug.text(footer), "12s, 24,177 in (12,032 cached), 310 out, 120 reasoning")
    }

    func testCopyDebugInfoHasTheTurnsTokens() {
        let usage: JSONValue = .object(["inputTokens": .number(49058), "cachedInputTokens": .number(24320), "outputTokens": .number(125), "reasoningTokens": .number(0)])
        let footer = TranscriptRow(id: "f:t1", seq: 9, turnId: "t1", content: .footer(TurnCompletion(turnId: "t1", outcome: .completed, durationMs: 6000, usage: usage)))
        let info = RowDebug.info(threadId: "th1", row: footer, seqs: [9], runnerVersion: "0.2.0")
        XCTAssertTrue(info.contains("\ntokens: 49,058 in (24,320 cached), 125 out\n"), info)
    }

    func testAViewedImageReadsAsViewNotRead() {
        let call = ToolCall(itemId: "v1", name: "view_image", kind: .read, title: "View orange.png", input: .object(["path": .string("/w/orange.png")]))
        let tool = ToolItem(itemId: "v1", call: call, root: "/w")
        XCTAssertEqual(ToolPresentation.fileTitle(tool)?.verb, "View")
        XCTAssertEqual(ToolPresentation.fileTitle(tool)?.name, "orange.png")
        XCTAssertEqual(ToolPresentation.symbol(for: .read, name: "view_image"), "photo")
        XCTAssertEqual(ToolPresentation.symbol(for: .read, name: "Read"), "doc.text")
    }

    func testClisCallsAndNotification() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in
            guard let id = request["id"] else { return [] }
            if request["method"] as? String == "clis.update" {
                return [E.json(["jsonrpc": "2.0", "id": id, "result": ["clis": [["provider": "codex", "status": "held", "update": ["state": "running"]]]]])]
            }
            return [E.json(["jsonrpc": "2.0", "id": id, "error": ["code": -32601, "message": "Unknown method"]])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        let clis = try await client.updateCli(provider: "codex", force: true)
        XCTAssertEqual(clis.first?.update?.state, "running")
        let params = transport.sent[0]["params"] as? [String: Any]
        XCTAssertEqual(params?["force"] as? Bool, true)
        XCTAssertNil(params?["version"])
        do {
            _ = try await client.listClis()
            XCTFail("an old runner has no clis.list")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, RPCError.methodNotFound)
        }
        transport.push(E.json(["jsonrpc": "2.0", "method": "clis.updated", "params": ["clis": [
            ["provider": "claude", "installedVersion": "2.1.312", "status": "current", "someNewField": true],
            ["no": "provider"],
        ]]]))
        var iterator = client.notifications.makeAsyncIterator()
        guard case .clisUpdated(let list)? = await iterator.next() else { return XCTFail("no clis.updated") }
        XCTAssertEqual(list.map(\.provider), ["claude"], "an entry that doesn't decode is skipped")
        XCTAssertEqual(list.first?.installedVersion, "2.1.312")
    }
}
