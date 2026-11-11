import Foundation
import XCTest
@testable import ThreadbearCore

/// End to end over a real WebSocket: starts Tools/mock-runner (which speaks
/// docs/protocol.md) in a temp data folder and drives it with the real
/// `WebSocketTransport` and `RPCClient`. Skipped when node or the mock's
/// `ws` dependency isn't installed (`npm install` in Tools/mock-runner).
final class MockRunnerTests: XCTestCase {
    private var process: Process?
    private var dataDir: URL!

    private static var appRoot: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    override func setUp() async throws {
        let mock = Self.appRoot.appendingPathComponent("Tools/mock-runner")
        guard FileManager.default.fileExists(atPath: mock.appendingPathComponent("node_modules/ws").path) else {
            throw try Self.missing("Tools/mock-runner has no node_modules; run npm install there")
        }
        guard let node = Self.findNode() else { throw try Self.missing("node isn't on PATH") }
        dataDir = FileManager.default.temporaryDirectory.appendingPathComponent("tb-mock-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: node)
        // Only the committed synthetic fixtures, never the private ones.
        process.arguments = [mock.appendingPathComponent("server.mjs").path, "--data-dir", dataDir.path,
                             "--fixtures", mock.appendingPathComponent("fixtures").path, "--quiet"]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        self.process = process
        // Wait for the mock's own port, not just the file: without one,
        // RunnerConfig.endpoint falls back to 8797, the live runner's, which
        // refuses the mock's token as unauthorized.
        for _ in 0..<200 where Self.mockPort(dataDir) == nil {
            try await Task.sleep(for: .milliseconds(25))
        }
        guard Self.mockPort(dataDir) != nil else {
            throw XCTSkip("the mock runner didn't start")
        }
    }

    private static func mockPort(_ dataDir: URL) -> Int? {
        guard let data = try? Data(contentsOf: dataDir.appendingPathComponent("runner.json")),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return object["port"] as? Int
    }

    override func tearDown() async throws {
        process?.terminate()
        process?.waitUntilExit()
        if let dataDir { try? FileManager.default.removeItem(at: dataDir) }
    }

    /// A skip, unless THREADBEAR_REQUIRE_MOCK=1 (./check.sh sets it, having
    /// installed the mock's packages): then a missing mock is a failure,
    /// since these are the only tests of the real WebSocket client and the
    /// restart over it, and a quiet skip once let check.sh pass without them.
    private static func missing(_ why: String) throws -> XCTSkip {
        if ProcessInfo.processInfo.environment["THREADBEAR_REQUIRE_MOCK"] == "1" {
            XCTFail("\(why) (THREADBEAR_REQUIRE_MOCK is set)")
            throw CancellationError()
        }
        return XCTSkip(why)
    }

    private static func findNode() -> String? {
        // check.sh's Node (THREADBEAR_NODE) first, then PATH and Homebrew's.
        let env = ProcessInfo.processInfo.environment
        let nodes = [env["THREADBEAR_NODE"]].compactMap { $0 }
            + ((env["PATH"] ?? "").split(separator: ":").map(String.init) + ["/opt/homebrew/bin", "/usr/local/bin"]).map { $0 + "/node" }
        return nodes.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    private func connect() async throws -> RPCClient {
        let endpoint = try RunnerConfig.endpoint(dataDir: dataDir)
        let client = RPCClient(transport: WebSocketTransport(url: endpoint.url, token: endpoint.token))
        try await client.start()
        return client
    }

    func testHelloListSearchAndEvents() async throws {
        let client = try await connect()
        defer { client.close() }
        let hello = try await client.hello(version: "test")
        XCTAssertEqual(hello.protocol, 1)
        XCTAssertFalse(hello.accounts.isEmpty)

        let inbox = try await client.listThreads()
        XCTAssertTrue(inbox.contains { $0.id == "demo_upload_resume" })
        XCTAssertFalse(inbox.contains { $0.isArchived }, "archived threads stay out of the inbox")
        let archived = try await client.listThreads(include: [.archived])
        XCTAssertTrue(archived.allSatisfy(\.isArchived))
        XCTAssertFalse(archived.isEmpty)

        let found = try await client.searchThreads("tunnel")
        XCTAssertEqual(found.first?.id, "demo_tunnel", "search covers archived threads too")

        let page = try await client.events(threadId: "demo_upload_resume")
        let model = TranscriptModel(events: page.events)
        let kinds = Set(model.items.map(\.kind))
        for kind in [RowKind.user, .assistant, .reasoning, .tool, .diff, .images, .plan, .approval, .footer] {
            XCTAssertTrue(kinds.contains(kind), "demo thread should have a \(kind) item")
        }
        // Its turns fold their work into groups and end with a changes card.
        let shown = Set(model.rows.map(\.kind))
        XCTAssertTrue(shown.contains(.group))
        XCTAssertTrue(shown.contains(.changes))
        // Image paths come back absolute so the app can read them.
        let images = page.events.flatMap { event -> [ImageRef] in
            if case .toolResult(let result) = event.payload { return result.images }
            return []
        }
        XCTAssertFalse(images.isEmpty)
        XCTAssertTrue(images.allSatisfy { $0.path.hasPrefix("/") && FileManager.default.fileExists(atPath: $0.path) })

        do {
            _ = try await client.events(threadId: "nope")
            XCTFail("expected thread not found")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, RPCError.threadNotFound)
        }
    }

    /// The demo's secret card, answered over a real connection: an empty
    /// value is refused and leaves it open; a real one closes it as
    /// answered, with no message and nothing of the value in any event.
    func testSecretCardIsAnsweredWithSecretsAnswer() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let threadId = "demo_keep_awake"
        let before = try await client.events(threadId: threadId)
        guard case .secretRequested(let card)? = before.events.last?.payload else { return XCTFail("the demo should end on a secret card") }
        XCTAssertEqual(card.label, "Mac administrator password")
        XCTAssertTrue(TranscriptModel(events: before.events).rows.contains { $0.id == "s:" + card.approvalId })

        do {
            try await client.answerSecret(threadId: threadId, approvalId: card.approvalId, .secret("   "))
            XCTFail("a blank secret should be refused")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, -32602)
        }
        try await client.answerSecret(threadId: threadId, approvalId: card.approvalId, .secret("correct horse"))
        let after = try await client.events(threadId: threadId)
        let resolved = after.events.compactMap { event -> ApprovalResolution? in
            if case .approvalResolved(let resolution) = event.payload { return resolution }
            return nil
        }
        XCTAssertEqual(resolved, [ApprovalResolution(approvalId: card.approvalId, decision: .answered)])
        let raw = try JSONSerialization.data(withJSONObject: after.events.map { "\($0)" })
        XCTAssertFalse(String(decoding: raw, as: UTF8.self).contains("correct horse"))
        do {
            try await client.answerSecret(threadId: threadId, approvalId: card.approvalId, .decline)
            XCTFail("an answered card can't be answered again")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, -32602)
        }
    }

    func testSendStreamsIntoTheFeed() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let threadId = "demo_dark_mode"
        let page = try await client.events(threadId: threadId)
        var feed = ThreadFeed(model: TranscriptModel(events: page.events))
        feed.beginSubscribe()
        let subscribed = try await client.subscribe(threadId: threadId, afterSeq: feed.acceptedSeq)
        _ = feed.finishSubscribe(subscribed.events, inProgress: subscribed.inProgress)

        _ = try await client.send(threadId: threadId, text: "Make the header sticky too")
        var sawDelta = false
        var done = false
        let deadline = Date().addingTimeInterval(15)
        for await notification in client.notifications {
            guard case .threadEvent(let id, let event) = notification, id == threadId else { continue }
            if event.isTransient { sawDelta = true }
            switch feed.receive(event) {
            case .needsCatchUp(let after):
                _ = feed.catchUp(try await client.events(threadId: threadId, afterSeq: after).events)
            default:
                break
            }
            _ = feed.flush()
            if case .turnCompleted = event.payload { done = true }
            if done || Date() > deadline { break }
        }
        XCTAssertTrue(done, "the turn should finish")
        XCTAssertTrue(sawDelta, "the reply should stream as deltas first")
        let rows = feed.model.rows
        XCTAssertTrue(rows.contains { row in
            if case .user(let message) = row.content { return message.text == "Make the header sticky too" }
            return false
        })
        let reply = rows.last { $0.kind == .assistant }
        guard case .assistant(let item)? = reply?.content else { return XCTFail("no reply row") }
        XCTAssertTrue(item.text.contains("mock runner"))
        XCTAssertFalse(item.isStreaming, "the persisted text replaces the streamed one")
        XCTAssertEqual(rows.last?.kind, .footer)
    }

    func testArchiveSnoozeAndMediaImport() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")

        let archived = try await client.updateThread("demo_settings_tabs", ThreadUpdate(archived: true))
        XCTAssertTrue(archived.isArchived)
        let inboxAfterArchive = try await client.listThreads()
        XCTAssertFalse(inboxAfterArchive.contains { $0.id == "demo_settings_tabs" })
        let back = try await client.updateThread("demo_settings_tabs", ThreadUpdate(archived: false))
        XCTAssertFalse(back.isArchived)

        let later = Date().addingTimeInterval(3600)
        let snoozed = try await client.updateThread("demo_settings_tabs", ThreadUpdate(snoozedUntil: .some(later)))
        XCTAssertTrue(snoozed.isSnoozed(at: Date()))
        let snoozedList = try await client.listThreads(include: [.snoozed])
        XCTAssertTrue(snoozedList.contains { $0.id == "demo_settings_tabs" })
        let woken = try await client.updateThread("demo_settings_tabs", ThreadUpdate(snoozedUntil: .some(nil)))
        XCTAssertNil(woken.snoozedUntil)
        let untilRestart = try await client.updateThread("demo_settings_tabs", ThreadUpdate(snoozeUntilRestart: true))
        XCTAssertTrue(untilRestart.snoozedUntilRestart)
        XCTAssertTrue(untilRestart.isSnoozed(at: Date()))
        XCTAssertEqual(ThreadRefs.status(untilRestart, now: Date()), "Hibernating until the runner restarts")
        let dated = try await client.updateThread("demo_settings_tabs", ThreadUpdate(snoozedUntil: .some(nil)))
        XCTAssertFalse(dated.snoozedUntilRestart)

        let png = Self.appRoot.appendingPathComponent("Tools/mock-runner/fixtures/demo-upload-resume/media/upload-times.png")
        let image = try await client.importMedia(path: png.path)
        XCTAssertEqual(image.mediaType, "image/png")
        XCTAssertEqual(image.width, 760)
        XCTAssertEqual(image.height, 420)
        XCTAssertTrue(image.path.hasPrefix(dataDir.appendingPathComponent("inbox").path))
    }

    /// The mock's restart helpers behave like runner.restart: a pending
    /// restart shows in runner.status with a notice, the restart drops the
    /// connection and comes back on a new version, and the resumed thread
    /// gets the note and a message sent by the runner.
    func testRestartPendingThenRestartResumesAThread() async throws {
        let client = try await connect()
        _ = try await client.hello(version: "test")
        var restart = RunnerRestart()
        _ = try await client.call("mock.restartPending", .object(["resume": .object(["threadId": .string("demo_settings_tabs")])]), as: JSONValue.self)
        for await notification in client.notifications {
            if case .runnerNotice(_, let message) = notification, restart.notice(message, now: Date()) { break }
        }
        XCTAssertEqual(restart.pendingText, "The runner will restart when 1 thread finishes and you answer 3.", "three of the four wait on a card")
        let status = try await client.runnerStatus()
        XCTAssertEqual(Set(status.pendingRestart?.waitingFor ?? []), ["demo_button_colour", "demo_ci_slow", "demo_keep_awake", "demo_upload_resume"])
        XCTAssertEqual(status.pendingRestart?.resumeThreadId, "demo_settings_tabs")

        let before = try await client.events(threadId: "demo_settings_tabs").lastSeq
        _ = try await client.call("mock.restart", .object(["downMs": .number(300)]), as: JSONValue.self)
        for await _ in client.closed { break }
        client.close()

        var again: RPCClient?
        for _ in 0..<100 where again == nil {
            try await Task.sleep(for: .milliseconds(50))
            again = try? await connect()
        }
        let back = try XCTUnwrap(again, "the mock should listen again on the same port")
        defer { back.close() }
        let hello = try await back.hello(version: "test")
        XCTAssertEqual(hello.runnerVersion, "0.0.0-mock+r1")
        let after = try await back.runnerStatus()
        XCTAssertNil(after.pendingRestart)
        var events: [Event] = []
        for _ in 0..<100 {
            events = try await back.events(threadId: "demo_settings_tabs", afterSeq: before).events
            if events.contains(where: { if case .turnCompleted = $0.payload { return true }; return false }) { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        let model = TranscriptModel(events: events)
        guard case .note(let note)? = model.items.first?.content else { return XCTFail("no restart note") }
        XCTAssertEqual(note, "Runner restarted: 0.0.0-mock to 0.0.0-mock+r1")
        guard case .user(let message)? = model.items.dropFirst().first?.content else { return XCTFail("no resume message") }
        XCTAssertTrue(message.isFromRunner)
    }

    /// The mock's schedules: its demo list, a preview and a cron it can't
    /// read, and a run that sends a schedule's message into the thread.
    func testSchedules() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let list = try await client.listSchedules()
        XCTAssertTrue(list.contains { $0.paused }, "the demo has a paused schedule")
        let forThread = try await client.listSchedules(threadId: "demo_release_notes")
        XCTAssertEqual(forThread.map(\.id), ["sc_demo_prs"])

        let preview = try await client.previewSchedule(.cron("0 9 * * 1-5"))
        XCTAssertEqual(preview.whenText, "Weekdays at 9:00")
        XCTAssertEqual(preview.runs.count, 3)
        do {
            _ = try await client.previewSchedule(.cron("0 25 * * *"))
            XCTFail("hour 25 should be refused")
        } catch RPCError.server(let code, let message) {
            XCTAssertEqual(code, -32602)
            XCTAssertTrue(message.contains("hour"), message)
        }

        let made = try await client.createSchedule(NewScheduleRequest(text: "Check it.", title: "Check", when: .cron("0 * * * *"), threadId: "demo_settings_tabs"))
        XCTAssertEqual(made.whenText, "Every hour")
        XCTAssertNotNil(made.nextRunAt)
        let paused = try await client.updateSchedule(made.id, ScheduleUpdate(paused: true))
        XCTAssertTrue(paused.paused)
        XCTAssertNil(paused.nextRunAt)

        let before = try await client.events(threadId: "demo_settings_tabs").lastSeq
        let run = try await client.runSchedule(made.id)
        XCTAssertEqual(run.threadId, "demo_settings_tabs")
        XCTAssertEqual(run.schedule.runs, 1)
        var message: UserMessage?
        for _ in 0..<100 where message == nil {
            let events = try await client.events(threadId: "demo_settings_tabs", afterSeq: before).events
            if case .user(let found)? = TranscriptModel(events: events).items.first?.content { message = found }
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertTrue(message?.isFromSchedule == true)
        XCTAssertEqual(message?.scheduledHeader?.title, "Check")
        XCTAssertEqual(message?.scheduledHeader?.body, "Check it.")

        try await client.deleteSchedule(made.id)
        do {
            try await client.deleteSchedule(made.id)
            XCTFail("it's gone")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, RPCError.scheduleNotFound)
        }
    }

    /// Schedules on another runner, as the mock plays them: runners.list with
    /// this runner and bruce, a schedule made for bruce by name keeping its
    /// folder as typed, Run now on it queued with no thread, a thread's
    /// schedule refused a runner, and an old runner's -32601.
    func testSchedulesOnAnotherRunner() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let runners = try await client.listRunners()
        XCTAssertEqual(runners.map(\.name), ["m5", "bruce"])
        XCTAssertEqual(runners.map(\.isSelf), [true, false])
        XCTAssertTrue(runners[1].online)
        let me = runners[0], bruce = runners[1]

        let list = try await client.listSchedules()
        XCTAssertTrue(list.contains { $0.runsElsewhere(than: me.id) }, "the demo has a schedule on bruce")
        XCTAssertTrue(list.contains { $0.runRequestedAt != nil }, "and one waiting for bruce")
        XCTAssertTrue(list.contains { $0.runnerId == me.id }, "this runner's own carry its id too")

        let made = try await client.createSchedule(NewScheduleRequest(text: "Check the drive.", when: .cron("0 7 * * *"), cwd: "/Volumes/Bruce/x", runnerId: "bruce"))
        XCTAssertEqual(made.runnerId, bruce.id)
        XCTAssertEqual(made.runnerName, "bruce")
        XCTAssertEqual(made.cwd, "/Volumes/Bruce/x", "a path on bruce, kept as typed")
        XCTAssertNil(made.accountId, "left to bruce's default")
        let noFolder = try await client.createSchedule(NewScheduleRequest(text: "Default folder.", when: .cron("0 7 * * *"), runnerId: bruce.id))
        XCTAssertNil(noFolder.cwd, "no folder means bruce's default")

        let run = try await client.runSchedule(made.id)
        XCTAssertTrue(run.queued)
        XCTAssertNil(run.threadId)
        XCTAssertNotNil(run.schedule.runRequestedAt)
        XCTAssertEqual(run.schedule.runs, 0, "bruce hasn't run it yet")

        do {
            let params: [String: JSONValue] = ["text": .string("x"), "cron": .string("0 7 * * *"), "threadId": .string("demo_settings_tabs"), "runnerId": .string("bruce")]
            _ = try await client.call("schedules.create", .object(params), as: JSONValue.self)
            XCTFail("a thread's schedule has no runner choice")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, -32602)
        }

        _ = try await client.call("mock.accounts", .object(["old": .bool(true)]), as: JSONValue.self)
        let old = try await client.listRunners()
        XCTAssertEqual(old, [], "an old runner's -32601 is just this runner")
    }

    /// The mock's projects and librarian: its demo, a thread taken out of a
    /// project and put back (pinned), a merge, and an old runner's -32601.
    func testProjectsAndTheLibrarian() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let projects = try await client.listProjects()
        XCTAssertEqual(Set(projects.map(\.title)), ["Threadbear", "Photo sync", "Walkies"])
        let threads = try await client.listThreads(include: [.inbox, .snoozed, .archived])
        let ci = try XCTUnwrap(threads.first { $0.id == "demo_ci_slow" })
        XCTAssertEqual(ci.projects.count, 2, "the demo has a thread in two projects")
        XCTAssertTrue(threads.contains { $0.projects.isEmpty && !$0.isArchived }, "and some in none")
        let dark = try XCTUnwrap(threads.first { $0.id == "demo_dark_mode" })
        XCTAssertTrue(dark.patches.contains { $0.projectId != nil }, "a patch from a project's folder")

        let tb = try XCTUnwrap(projects.first { $0.title == "Threadbear" })
        let got = try await client.getProject(tb.id)
        XCTAssertTrue(got.threadIds.contains("demo_settings_tabs"))
        XCTAssertEqual(got.project.threadCount, got.threadIds.count)

        var tabs = try await client.setThreadProjects("demo_settings_tabs", remove: [tb.id])
        XCTAssertFalse(tabs.projects.contains { $0.id == tb.id })
        tabs = try await client.setThreadProjects("demo_settings_tabs", add: [tb.id])
        XCTAssertEqual(tabs.projects.first { $0.id == tb.id }?.pinned, true, "added by hand is pinned")

        let renamed = try await client.updateProject(tb.id, ProjectUpdate(title: "Threadbear"))
        XCTAssertEqual(renamed.title, "Threadbear")
        let made = try await client.createProject(NewProjectRequest(title: "Scratch", threadIds: ["demo_landing_pages"]))
        let kept = try await client.mergeProject(made.id, into: tb.id)
        XCTAssertEqual(kept.id, tb.id)
        let afterMerge = try await client.getProject(tb.id)
        XCTAssertTrue(afterMerge.threadIds.contains("demo_landing_pages"))
        do {
            _ = try await client.getProject(made.id)
            XCTFail("the merged one is gone")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, RPCError.projectNotFound)
        }

        var librarian = try await client.librarianStatus()
        XCTAssertEqual(librarian.state, .working)
        XCTAssertEqual(librarian.queue.count, 3)
        XCTAssertEqual(librarian.recent.count, 8)
        librarian = try await client.updateLibrarian(paused: true)
        XCTAssertEqual(librarian.state, .paused)
        XCTAssertNil(librarian.current)
        librarian = try await client.runLibrarian(threadId: "demo_tunnel")
        XCTAssertTrue(librarian.queue.contains { $0.threadId == "demo_tunnel" && $0.reason == .asked })

        _ = try await client.call("mock.accounts", .object(["old": .bool(true)]), as: JSONValue.self)
        do {
            _ = try await client.listProjects()
            XCTFail("an old runner has no projects")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, RPCError.methodNotFound)
        }
    }

    /// Pools as the mock plays them: hello lists them, a new thread with a
    /// pool gets one of its logins, null pins, a pool id moves it back in,
    /// and a save comes back as pools.updated. An old runner has none.
    func testPools() async throws {
        let client = try await connect()
        defer { client.close() }
        let hello = try await client.hello(version: "test")
        XCTAssertEqual(hello.pools?.map(\.id), ["claude", "convex", "codex", "side-projects", "evenings"])
        XCTAssertEqual(hello.defaults.poolId, "claude")
        let created = try await client.createThread(NewThreadRequest(text: "hi", poolId: "side-projects", model: "opus"))
        XCTAssertEqual(created.poolId, "side-projects")
        XCTAssertEqual(created.accountId, "claude-3", "the named model's provider")
        let pinned = try await client.updateThread(created.id, ThreadUpdate(poolId: .some(nil)))
        XCTAssertNil(pinned.poolId)
        XCTAssertEqual(pinned.accountId, "claude-3")
        let moved = try await client.updateThread(created.id, ThreadUpdate(poolId: "convex"))
        XCTAssertEqual(moved.poolId, "convex")
        XCTAssertEqual(moved.accountId, "claude-2", "onto one of the pool's logins")
        let byLogin = try await client.updateThread(created.id, ThreadUpdate(accountId: "claude-main"))
        XCTAssertNil(byLogin.poolId, "naming a login pins")
        do {
            _ = try await client.savePools([Pool(id: "x", title: "X", accountIds: [])])
            XCTFail("a pool with no logins is refused")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, -32602)
        }
        var iterator = client.notifications.makeAsyncIterator()
        let saved = try await client.savePools([Pool(id: "all", title: "All", accountIds: ["claude-main", "codex-main"])])
        XCTAssertEqual(saved.map(\.id), ["all"])
        var heard: [Pool]?
        while heard == nil, let next = await iterator.next() {
            if case .poolsUpdated(let pools) = next { heard = pools }
        }
        XCTAssertEqual(heard?.map(\.id), ["all"])
        _ = try await client.call("mock.accounts", .object(["old": .bool(true)]), as: JSONValue.self)
        let old = try await client.hello(version: "test")
        XCTAssertNil(old.pools, "a runner from before pools")
        XCTAssertNil(old.defaults.poolId)
    }

    func testHighlightsAndListPanels() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let fetched = try await client.threadHighlights(threadId: "demo_landing_pages")
        let sections = HighlightSections.build(fetched)
        XCTAssertNotNil(sections.brief)
        XCTAssertEqual(sections.pinned.count, 1)
        XCTAssertTrue(sections.pinned[0].show)
        // The mock's files are real, so the app can preview them.
        let pin = try XCTUnwrap(sections.pinned[0].path)
        XCTAssertTrue(FileManager.default.fileExists(atPath: pin))
        XCTAssertEqual(sections.commits.map(\.pushed), [false, true])
        XCTAssertFalse(sections.edited.isEmpty)
        let review = try XCTUnwrap(sections.files.first { $0.title == "copy-review.md" })
        let thread = try await client.removeHighlight(threadId: "demo_landing_pages", highlightId: review.id)
        XCTAssertNotNil(thread.highlightsAt)
        let after = try await client.threadHighlights(threadId: "demo_landing_pages")
        XCTAssertFalse(after.highlights.contains { $0.id == review.id })
        // A thread with none, and the list panel's rows.
        let none = try await client.threadHighlights(threadId: "demo_tunnel")
        XCTAssertEqual(none, ThreadHighlights())
        let render = try await client.renderPanel(threadId: "demo_landing_pages", panelId: "deploys.previews")
        XCTAssertEqual(render.items?.map(\.tone), [.done, .attention, .failed, nil, nil])
    }

    /// Other runners' threads: listed by this runner, opened on their own
    /// (the mock plays bruce at /bruce/rpc), as the app does it.
    func testRemoteThreadsOpenOnTheirOwnRunner() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let remote = try await client.remoteThreads()
        let bruce = try XCTUnwrap(remote.runners.first)
        XCTAssertEqual(bruce.name, "bruce")
        XCTAssertEqual(bruce.icon, "macmini")
        XCTAssertTrue(bruce.online)
        XCTAssertEqual(Set(remote.threads.map(\.threadId)), ["bruce_backup", "bruce_media", "bruce_render"])
        XCTAssertTrue(remote.threads.allSatisfy { $0.runnerId == bruce.id })
        XCTAssertEqual(remote.threads.first { $0.threadId == "bruce_backup" }?.unread, true)
        XCTAssertEqual(remote.threads.first { $0.threadId == "bruce_render" }?.status, .running)

        // Not this runner's to list...
        let own = try await client.listThreads(include: [.inbox, .snoozed, .archived])
        XCTAssertFalse(own.contains { $0.id.hasPrefix("bruce_") })
        // ...but it answers for them through the backend, the app's default
        // way in (docs/protocol.md, "Other runners' threads through the backend").
        let relayed = try await client.events(threadId: "bruce_backup")
        XCTAssertFalse(relayed.events.isEmpty)
        // A thread nobody has is still not found.
        do {
            _ = try await client.events(threadId: "no_such_thread")
            XCTFail("nobody has this thread")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, RPCError.threadNotFound)
        }

        try await Self.openOnBruce(bruce, local: client)
    }

    /// The app's side: RemoteClients is the main actor's.
    @MainActor
    private static func openOnBruce(_ bruce: RemoteRunner, local client: RPCClient) async throws {
        // Bruce's own connection, from its rpcUrl and token file.
        let clients = RemoteClients(version: "test")
        let away = try await clients.client(for: bruce)
        defer { clients.closeAll() }
        let theirs = try await away.listThreads()
        XCTAssertEqual(Set(theirs.map(\.id)), ["bruce_backup", "bruce_media", "bruce_render"])
        let page = try await away.events(threadId: "bruce_backup")
        let images = page.events.flatMap { event -> [ImageRef] in
            if case .userMessage(_, let images, _, _, _, _) = event.payload { return images }
            return []
        }
        XCTAssertEqual(images.count, 1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: images[0].path), "a path on bruce")

        // A send there streams back over bruce's connection, and this
        // runner's clients hear the sidebar row change.
        var feed = ThreadFeed(model: TranscriptModel(events: page.events))
        feed.beginSubscribe()
        let subscribed = try await away.subscribe(threadId: "bruce_backup", afterSeq: feed.acceptedSeq)
        _ = feed.finishSubscribe(subscribed.events, inProgress: subscribed.inProgress)
        var heard: [String] = []
        clients.onNotification = { _, notification in
            if case .threadEvent(let id, let event) = notification, event.isTransient == false { heard.append(id) }
        }
        let rowChanged = Task {
            for await notification in client.notifications {
                if case .remoteThreadsUpdated(let list) = notification, list.threads.contains(where: { $0.threadId == "bruce_backup" && $0.status == .running }) { return true }
            }
            return false
        }
        _ = try await away.send(threadId: "bruce_backup", text: "Yes, keep it awake")
        for _ in 0..<400 where !heard.contains("bruce_backup") { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(heard.contains("bruce_backup"))
        // A stream's iteration ends when its task is cancelled.
        let timeout = Task { try? await Task.sleep(for: .seconds(5)); rowChanged.cancel() }
        let changed = await rowChanged.value
        timeout.cancel()
        XCTAssertTrue(changed, "remoteThreads.updated after bruce's thread changed")
    }

    /// Bruce's files come over its connection (files.read), and a new
    /// thread made over it is bruce's, not this runner's.
    func testFilesAndNewThreadsOnBruce() async throws {
        let client = try await connect()
        defer { client.close() }
        _ = try await client.hello(version: "test")
        let remote = try await client.remoteThreads()
        let bruce = try XCTUnwrap(remote.runners.first)
        try await Self.filesAndNewThreadOnBruce(bruce, local: client)
    }

    @MainActor
    private static func filesAndNewThreadOnBruce(_ bruce: RemoteRunner, local client: RPCClient) async throws {
        let clients = RemoteClients(version: "test")
        let away = try await clients.client(for: bruce)
        defer { clients.closeAll() }
        let hello = try await away.hello(version: "test")
        XCTAssertEqual(hello.defaults.cwd, "/Users/bruce/dev/me/misc", "its own folder")

        // The demo picture is a real PNG; asked again with what came, it's unchanged.
        let page = try await away.events(threadId: "bruce_backup")
        let picture = try XCTUnwrap(page.events.lazy.compactMap { event -> String? in
            if case .userMessage(_, let images, _, _, _, _) = event.payload { return images.first?.path }
            return nil
        }.first)
        let read = try await away.readFile(path: picture)
        XCTAssertEqual(read.mediaType, "image/png")
        XCTAssertEqual(read.data?.prefix(4), Data([0x89, 0x50, 0x4e, 0x47]))
        XCTAssertEqual(read.data?.count, read.stamp.size)
        let again = try await away.readFile(path: picture, known: read.stamp)
        XCTAssertTrue(again.unchanged)
        XCTAssertNil(again.data)
        do {
            _ = try await away.readFile(path: "/Users/bruce/not-there.txt")
            XCTFail("a missing file is an error")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, -32602)
        }

        // A new thread there, in a folder on bruce, on one of its logins.
        let made = try await away.createThread(NewThreadRequest(text: "Check the drive", cwd: "/Volumes/CannMedia", accountId: "codex-main", permissionMode: .full))
        XCTAssertEqual(made.cwd, "/Volumes/CannMedia")
        XCTAssertEqual(made.accountId, "codex-main")
        let theirs = try await away.listThreads()
        XCTAssertTrue(theirs.contains { $0.id == made.id })
        let own = try await client.listThreads(include: [.inbox, .snoozed, .archived])
        XCTAssertFalse(own.contains { $0.id == made.id }, "not this runner's")
        // Its canned reply comes over bruce's connection.
        var replied = false
        for _ in 0..<400 where !replied {
            try await Task.sleep(for: .milliseconds(10))
            replied = try await away.events(threadId: made.id).events.contains { if case .assistantText = $0.payload { return true } else { return false } }
        }
        XCTAssertTrue(replied)
    }

    func testWrongTokenIsUnauthorized() async throws {
        let endpoint = try RunnerConfig.endpoint(dataDir: dataDir)
        let client = RPCClient(transport: WebSocketTransport(url: endpoint.url, token: "not-the-token"))
        do {
            try await client.start()
            _ = try await client.hello(version: "test")
            XCTFail("a wrong token should be refused")
        } catch let error as RPCError {
            XCTAssertEqual(error, .unauthorized)
        }
        client.close()
    }
}
