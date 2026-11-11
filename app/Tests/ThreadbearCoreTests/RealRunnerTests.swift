import Foundation
import XCTest
@testable import ThreadbearCore

/// The app's client against the real runner (`../runner`), with no model and
/// no real accounts: a temp data dir, empty temp account folders, and
/// `/usr/bin/false` as the `claude` binary, so a turn fails straight away
/// and the runner reports it in events. Opt in with
/// `THREADBEAR_REAL_RUNNER=1 swift test --filter RealRunnerTests` (it needs
/// `npm ci` in ../runner).
final class RealRunnerTests: XCTestCase {
    private var process: Process?
    private var root: URL!

    private static var runnerDir: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("runner")
    }

    override func setUp() async throws {
        guard ProcessInfo.processInfo.environment["THREADBEAR_REAL_RUNNER"] == "1" else {
            throw XCTSkip("set THREADBEAR_REAL_RUNNER=1 to run against ../runner")
        }
        let runner = Self.runnerDir
        guard FileManager.default.fileExists(atPath: runner.appendingPathComponent("node_modules/ws").path) else {
            throw XCTSkip("../runner has no node_modules; run npm ci there")
        }
        // check.sh's Node (THREADBEAR_NODE) first, then PATH and Homebrew's.
        let env = ProcessInfo.processInfo.environment
        let nodes = [env["THREADBEAR_NODE"]].compactMap { $0 }
            + ((env["PATH"] ?? "").split(separator: ":").map(String.init) + ["/opt/homebrew/bin", "/usr/local/bin"]).map { $0 + "/node" }
        guard let node = nodes.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw XCTSkip("node isn't on PATH")
        }
        root = FileManager.default.temporaryDirectory.appendingPathComponent("tb-real-runner-\(UUID().uuidString)", isDirectory: true)
        for folder in ["home", "accounts", "claude-main", "cwd"] {
            try FileManager.default.createDirectory(at: root.appendingPathComponent(folder), withIntermediateDirectories: true)
        }
        try Data("{}".utf8).write(to: root.appendingPathComponent("claude-main.json"))
        let process = Process()
        process.executableURL = URL(fileURLWithPath: node)
        process.arguments = [runner.appendingPathComponent("src/main.ts").path]
        process.currentDirectoryURL = runner
        var environment = ProcessInfo.processInfo.environment
        environment["THREADBEAR_HOME"] = root.appendingPathComponent("home").path
        environment["THREADBEAR_PORT"] = "0"
        environment["THREADBEAR_LOG"] = root.appendingPathComponent("runner.log").path
        environment["THREADBEAR_ACCOUNTS_DIR"] = root.appendingPathComponent("accounts").path
        environment["THREADBEAR_CLAUDE_MAIN_DIR"] = root.appendingPathComponent("claude-main").path
        environment["THREADBEAR_CLAUDE_MAIN_JSON"] = root.appendingPathComponent("claude-main.json").path
        environment["THREADBEAR_CLAUDE_PATH"] = "/usr/bin/false"
        process.environment = environment
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        self.process = process
        // The token and runner.json (with the real port) are written on start.
        for _ in 0..<200 {
            if (try? RunnerConfig.endpoint(dataDir: dataDir)) != nil, let json = try? Data(contentsOf: dataDir.appendingPathComponent("runner.json")),
               let object = try? JSONSerialization.jsonObject(with: json) as? [String: Any], (object["port"] as? Int ?? 0) > 0 { break }
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    override func tearDown() async throws {
        process?.terminate()
        process?.waitUntilExit()
        if let root { try? FileManager.default.removeItem(at: root) }
    }

    private var dataDir: URL { root.appendingPathComponent("home") }

    func testCreateAThreadAndReadItsEvents() async throws {
        let endpoint = try RunnerConfig.endpoint(dataDir: dataDir)
        let client = RPCClient(transport: WebSocketTransport(url: endpoint.url, token: endpoint.token))
        try await client.start()
        defer { client.close() }
        let hello = try await client.hello(version: "test")
        XCTAssertFalse(hello.runnerVersion.isEmpty)
        let listed = try await client.listThreads()
        XCTAssertEqual(listed.count, 0)

        let cwd = root.appendingPathComponent("cwd").path
        let thread = try await client.createThread(NewThreadRequest(text: "Say hi", cwd: cwd, accountId: hello.defaults.accountId, model: "claude-haiku-5-5"))
        var feed = ThreadFeed()
        feed.beginSubscribe()
        let reply = try await client.subscribe(threadId: thread.id, afterSeq: 0)
        _ = feed.finishSubscribe(reply.events, inProgress: reply.inProgress)
        _ = feed.flush()
        // The CLI is /usr/bin/false, so the turn ends quickly; wait for it.
        var finished = false
        for _ in 0..<100 where !finished {
            let page = try await client.events(threadId: thread.id, afterSeq: 0, limit: 500)
            finished = page.events.contains { if case .turnCompleted = $0.payload { return true } else { return false } }
                || page.events.contains { if case .error = $0.payload { return true } else { return false } }
            if !finished { try await Task.sleep(for: .milliseconds(100)) }
        }
        let page = try await client.events(threadId: thread.id, afterSeq: 0, limit: 500)
        XCTAssertTrue(finished, "the runner reported the failed turn")
        print("real runner events: \(page.events.map(\.type).joined(separator: ", "))")
        // Every event the runner wrote decodes to a known payload, in order with no holes.
        XCTAssertEqual(page.events.map(\.seq), Array(1...(page.events.last?.seq ?? 0)))
        let unread = page.events.filter { $0.payload == .unknown }.map(\.type)
        XCTAssertEqual(unread, [], "event types the app couldn't read")
        let model = TranscriptModel(events: page.events)
        XCTAssertEqual(model.rows.first?.kind, .user)

        // Archive, snooze and back, as the app does it.
        let archived = try await client.updateThread(thread.id, ThreadUpdate(archived: true))
        XCTAssertTrue(archived.isArchived)
        let until = Date().addingTimeInterval(3600)
        let snoozed = try await client.updateThread(thread.id, ThreadUpdate(snoozedUntil: .some(until), archived: false))
        XCTAssertEqual(snoozed.snoozedUntil?.timeIntervalSince1970 ?? 0, until.timeIntervalSince1970, accuracy: 1, "the snooze time survives the trip as UTC")
    }
}
