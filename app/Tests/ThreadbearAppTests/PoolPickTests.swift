import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// The composer with pools (docs/pools-plan.md): which pool a new thread
/// starts on, what the picker lists and does, pinning, and the fallback to
/// accounts with a runner older than pools.
@MainActor
final class PoolPickTests: XCTestCase {
    private let keys = [AppModel.lastModelKey, AppModel.lastEffortKey, AppModel.lastAccountKey, "lastModel.codex", "lastEffort.codex",
                        "lastAccountId.byProvider", "lastAccountId.codex.byProvider", AppModel.lastPoolKey, AppModel.lastPoolProvidersKey]

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
        keys.forEach { UserDefaults.standard.removeObject(forKey: $0) }
    }

    override func tearDown() {
        keys.forEach { UserDefaults.standard.removeObject(forKey: $0) }
        super.tearDown()
    }

    private func room(_ percent: Double, blocked: Bool = false) -> AccountUsage {
        AccountUsage(windows: [UsageWindow(kind: "five_hour", title: "5-hour", remainingPercent: percent),
                               UsageWindow(kind: "weekly", title: "Weekly", remainingPercent: 90)], blocked: blocked)
    }

    private let opus = ModelChoice(id: "opus", model: "claude-opus-5-5", title: "Opus 5.5", effortLevels: ["low", "high"])
    private let sol = ModelChoice(id: "gpt-6.1-sol", model: "gpt-6.1-sol", title: "GPT-6.1-Sol")

    private var accounts: [Account] {
        [
            Account(id: "claude-main", title: "sam@acme.io", models: [opus], email: "sam@acme.io", usage: room(61)),
            Account(id: "claude-1", title: "sam.lee@gmail.com", models: [opus], email: "sam.lee@gmail.com", usage: room(16)),
            Account(id: "claude-2", title: "sam@convex.dev", models: [opus], email: "sam@convex.dev", note: "Convex work only", usage: room(80)),
            Account(id: "codex-main", provider: "codex", title: "Codex · sam@acme.io", models: [sol], email: "sam@acme.io", usage: room(91)),
        ]
    }

    private var pools: [Pool] {
        [
            Pool(id: "claude", title: "Claude", accountIds: ["claude-main", "claude-1"], builtIn: true),
            Pool(id: "convex", title: "Convex", accountIds: ["claude-2"], folders: [NSHomeDirectory() + "/dev/convex"], builtIn: true),
            Pool(id: "codex", title: "Codex", accountIds: ["codex-main"], builtIn: true),
            Pool(id: "mixed", title: "Everything", accountIds: ["codex-main", "claude-1"]),
        ]
    }

    private func app(pools: Bool = true) -> AppModel {
        let app = AppModel()
        app.accounts = accounts
        app.defaults.cwd = "/tmp/misc"
        if pools {
            app.pools = self.pools
            app.poolsSupported = true
            app.defaults.poolId = "claude"
        }
        return app
    }

    // MARK: Without pools

    /// A runner older than pools: the rail lists providers and the model
    /// chip has no owner, as before.
    func testNoPoolsFallsBackToProviders() {
        let app = app(pools: false)
        let settings = ThreadSettings(app: app, thread: nil)
        XCTAssertFalse(settings.usesPools)
        XCTAssertEqual(settings.columns.map(\.id), ["claude", "codex"])
        XCTAssertNotNil(settings.columns.first?.symbol)
        XCTAssertNil(settings.owner)
        XCTAssertEqual(settings.modelChipTitle, "Default", "no pool or login in front")
        XCTAssertNil(settings.modelChipBadge)
        XCTAssertEqual(settings.sections(for: "claude").count, 1)
        XCTAssertNil(app.newThreadPoolId)
        let request = app.newThreadRequest(text: "hi", images: [])
        XCTAssertNil(request.poolId)
        XCTAssertEqual(request.accountId, "claude-main")
        // An app that knows pools but a runner that has none at all.
        app.poolsSupported = true
        XCTAssertFalse(app.usesPools)
    }

    // MARK: New threads

    func testNewThreadStartsOnTheFolderPoolElseTheLastPicked() {
        let app = app()
        XCTAssertEqual(app.newThreadPoolId, "claude", "the runner's default to begin with")
        app.rememberPool("codex", provider: "codex")
        XCTAssertEqual(app.newThreadPoolId, "codex", "the last pool picked")
        app.newThreadCwd = NSHomeDirectory() + "/dev/convex/chef"
        XCTAssertEqual(app.newThreadPoolId, "convex", "a pool's folder beats the last pick")
        XCTAssertEqual(app.newThreadAccountId, "claude-2")
        app.newThreadPool = "claude"
        XCTAssertEqual(app.newThreadPoolId, "claude", "a pick for this draft beats the folder")
        app.newThreadAccount = "claude-1"
        XCTAssertNil(app.newThreadPoolId, "a draft pinned to a login has no pool")
    }

    /// The chip names the pool, or the pinned login; the request names the
    /// pool, or the login when pinned.
    func testChipAndRequestForANewThread() {
        let app = app()
        app.remember(model: "opus", provider: "claude")
        app.remember(effort: "high", provider: "claude")
        var settings = ThreadSettings(app: app, thread: nil)
        XCTAssertEqual(settings.modelChipTitle, "Claude · Opus 5.5 · High")
        XCTAssertFalse(settings.isPinned)
        var request = app.newThreadRequest(text: "hi", images: [])
        XCTAssertEqual(request.poolId, "claude")
        XCTAssertNil(request.accountId, "the runner picks the login")
        XCTAssertEqual(request.model, "opus")
        settings.pin("claude-1")
        settings = ThreadSettings(app: app, thread: nil)
        XCTAssertTrue(settings.isPinned)
        XCTAssertEqual(settings.modelChipTitle, "gmail · Opus 5.5 · High")
        XCTAssertEqual(settings.pinnedLine, "Pinned to gmail")
        XCTAssertEqual(settings.columnId, ThreadSettings.pinnedColumn)
        request = app.newThreadRequest(text: "hi", images: [])
        XCTAssertEqual(request.accountId, "claude-1")
        XCTAssertNil(request.poolId)
        settings.unpin()
        XCTAssertEqual(app.newThreadPoolId, "claude")
    }

    /// A mixed pool lists each provider's models under its name, and a
    /// pick there moves the draft to that pool and provider and names the
    /// model, so the runner knows which provider was meant.
    func testMixedPoolForANewThread() throws {
        let app = app()
        let settings = ThreadSettings(app: app, thread: nil)
        let sections = settings.sections(for: "mixed")
        XCTAssertEqual(sections.map(\.id), ["codex", "claude"], "the pool's order")
        XCTAssertEqual(sections.map(\.title), ["Codex", "Claude"])
        XCTAssertEqual(settings.sections(for: "claude").map(\.title), [nil], "one provider: no heading")
        let row = try XCTUnwrap(sections[0].rows.first { $0.id == "gpt-6.1-sol" })
        settings.pick(row, section: "codex", column: "mixed")
        XCTAssertEqual(app.newThreadPool, "mixed")
        XCTAssertEqual(app.newThreadProvider(in: "mixed"), "codex")
        XCTAssertEqual(app.lastPoolId, "mixed", "remembered as the last pool picked")
        XCTAssertEqual(app.lastPoolProviders["mixed"], "codex")
        XCTAssertEqual(app.newThreadAccountId, "codex-main")
        let request = app.newThreadRequest(text: "hi", images: [])
        XCTAssertEqual(request.poolId, "mixed")
        XCTAssertEqual(request.model, "gpt-6.1-sol")
        // Its default model is named too, in a mixed pool.
        app.remember(model: nil, provider: "codex")
        app.defaults.models["codex"] = "gpt-6-luna"
        XCTAssertEqual(app.newThreadRequest(text: "hi", images: []).model, "gpt-6-luna")
        let after = ThreadSettings(app: app, thread: nil)
        XCTAssertTrue(after.pickerLines().contains { $0.hasPrefix("# [x] Everything · 2 · ") }, after.pickerLines().joined(separator: "\n"))
        XCTAssertTrue(after.pickerLines().contains("## Codex"))
    }

    // MARK: Threads

    func testPoolThreadChipBadgeAndStatus() {
        let app = app()
        var low = accounts
        low[0].usage = room(20)
        app.accounts = low
        let thread = ThreadSummary(id: "th1", title: "t", accountId: "claude-main", model: "opus", effort: "high", poolId: "claude")
        app.threads["th1"] = thread
        let settings = ThreadSettings(app: app, thread: thread)
        XCTAssertEqual(settings.modelChipTitle, "Claude · Opus 5.5 · High")
        XCTAssertEqual(settings.modelChipBadge, .low("20%"), "every login in the pool is low")
        XCTAssertNil(settings.pinnedLine)
        // An old thread, pinned.
        let pinned = ThreadSummary(id: "th2", title: "t", accountId: "claude-1")
        let pinnedSettings = ThreadSettings(app: app, thread: pinned)
        XCTAssertTrue(pinnedSettings.isPinned)
        XCTAssertEqual(pinnedSettings.modelChipBadge, .low("16%"), "its own login's badge")
        XCTAssertEqual(pinnedSettings.pinSections.flatMap(\.items).filter(\.isSelected).map(\.id), ["claude-1"], "the pin list ticks the live login")
    }

    /// Same provider: one update with the pool and the model. Another
    /// provider: pin to that provider's login with the model, then into
    /// the pool, in that order.
    func testMovingAThreadIntoAPool() async throws {
        let app = app()
        let transport = PoolRecordingTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        app.useClientForTesting(client)
        app.threads["th1"] = ThreadSummary(id: "th1", title: "t", accountId: "claude-main", poolId: "claude")
        app.remember(effort: "high", provider: "codex")
        var settings = ThreadSettings(app: app, thread: app.threads["th1"])
        let opusRow = try XCTUnwrap(settings.sections(for: "mixed").first { $0.id == "claude" }?.rows.first { $0.id == "opus" })
        settings.pick(opusRow, section: "claude", column: "mixed")
        for _ in 0..<200 where transport.updates.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertEqual(transport.updates.count, 1)
        XCTAssertEqual(transport.updates.first?["poolId"] as? String, "mixed")
        XCTAssertEqual(transport.updates.first?["model"] as? String, "opus")
        XCTAssertNil(transport.updates.first?["accountId"])

        app.threads["th1"] = ThreadSummary(id: "th1", title: "t", accountId: "claude-main", poolId: "claude")
        settings = ThreadSettings(app: app, thread: app.threads["th1"])
        let solRow = try XCTUnwrap(settings.sections(for: "mixed").first { $0.id == "codex" }?.rows.first)
        settings.pick(solRow, section: "codex", column: "mixed")
        for _ in 0..<200 where transport.updates.count < 3 { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertEqual(transport.updates.count, 3)
        XCTAssertEqual(transport.updates[1]["accountId"] as? String, "codex-main")
        XCTAssertEqual(transport.updates[1]["model"] as? String, "gpt-6.1-sol")
        XCTAssertEqual(transport.updates[1]["effort"] as? String, "high")
        XCTAssertEqual(transport.updates[2]["poolId"] as? String, "mixed")
        XCTAssertEqual(transport.updates[2].count, 2, "just the thread and its pool")
    }

    func testPinAndUnpinAThread() async throws {
        let app = app()
        let transport = PoolRecordingTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        app.useClientForTesting(client)
        app.threads["th1"] = ThreadSummary(id: "th1", title: "t", accountId: "claude-main", poolId: "claude")
        ThreadSettings(app: app, thread: app.threads["th1"]).pin("claude-1")
        for _ in 0..<200 where transport.updates.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertEqual(transport.updates.first?["accountId"] as? String, "claude-1")
        app.threads["th1"] = ThreadSummary(id: "th1", title: "t", accountId: "claude-2")
        XCTAssertTrue(ThreadSettings(app: app, thread: app.threads["th1"]).pickerClick("Unpin"))
        for _ in 0..<200 where transport.updates.count < 2 { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertEqual(transport.updates.last?["poolId"] as? String, "convex", "back into a pool that has its login")
        XCTAssertFalse(ThreadSettings(app: app, thread: ThreadSummary(id: "th3", title: "t", accountId: "claude-1", poolId: "claude")).pickerClick("Unpin"),
                       "nothing to unpin")
    }

    /// A pool thread's limit line: nothing while the pool has room (the
    /// runner moves it), and when it's waiting, when it carries on.
    func testLimitLineForPoolThreads() {
        let app = app()
        var blocked = accounts
        blocked[0].usage = room(0, blocked: true)
        app.accounts = blocked
        let moving = ThreadSummary(id: "th1", title: "t", accountId: "claude-main", poolId: "claude")
        XCTAssertNil(app.limitAdvice(for: moving), "gmail has room, so the runner moves it")
        let until = Date().addingTimeInterval(3600)
        let waiting = ThreadSummary(id: "th2", title: "t", accountId: "claude-main", poolId: "claude", waitingForRoom: WaitingForRoom(until: until))
        let advice = app.limitAdvice(for: waiting)
        XCTAssertEqual(advice?.text, "Every login in Claude is out of room.")
        XCTAssertEqual(advice?.carriesOnAt, until)
        XCTAssertEqual(advice?.stopped, true, "offers turning carrying on off")
        XCTAssertEqual(advice?.pool, true, "the switch pins it outside the pool")
        let pinned = ThreadSummary(id: "th3", title: "t", accountId: "claude-main")
        XCTAssertEqual(app.limitAdvice(for: pinned)?.text, "sam@acme.io has reached its limit.", "a pinned thread: as before")
        XCTAssertEqual(app.limitAdvice(for: pinned)?.stopped, false, "no turn stopped, so nothing to carry on")
        // A pinned thread waiting on its own login (from 10 Nov).
        let pinnedWaiting = ThreadSummary(id: "th4", title: "t", accountId: "claude-main", waitingForRoom: WaitingForRoom(until: until))
        XCTAssertEqual(app.limitAdvice(for: pinnedWaiting)?.carriesOnAt, until)
        XCTAssertEqual(app.limitAdvice(for: pinnedWaiting)?.stopped, true)
        XCTAssertEqual(app.limitAdvice(for: pinnedWaiting)?.pool, false)
    }

    /// The limit box's Carry on menu: No shows at once (the wait ends) and
    /// sends carryOnAtReset false.
    func testCarryOnAtResetIsOptimistic() async throws {
        let app = app()
        let transport = PoolRecordingTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        app.useClientForTesting(client)
        let until = Date().addingTimeInterval(3600)
        app.threads["th1"] = ThreadSummary(id: "th1", title: "t", accountId: "claude-main", waitingForRoom: WaitingForRoom(until: until))
        app.setCarryOnAtReset("th1", false)
        XCTAssertEqual(app.threads["th1"]?.carryOnAtReset, false)
        XCTAssertNil(app.threads["th1"]?.waitingForRoom)
        for _ in 0..<200 where transport.updates.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertEqual(transport.updates.first?["carryOnAtReset"] as? Bool, false)
    }
}

/// Answers every request with a thread and records each `threads.update`.
private final class PoolRecordingTransport: RPCTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var inbox: [Data] = []
    private var waiter: CheckedContinuation<Data, Error>?
    private var recorded: [[String: Any]] = []

    var updates: [[String: Any]] { lock.withLock { recorded } }

    func open() async throws {}

    func send(_ text: String) async throws {
        guard let object = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any],
              let id = object["id"] as? Int, let method = object["method"] as? String else { return }
        if method == "threads.update" { lock.withLock { recorded.append(object["params"] as? [String: Any] ?? [:]) } }
        let reply: [String: Any] = ["jsonrpc": "2.0", "id": id, "result": ["thread": ["id": "th1", "title": "t"]]]
        push(try JSONSerialization.data(withJSONObject: reply))
    }

    func receive() async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if !inbox.isEmpty {
                let data = inbox.removeFirst()
                lock.unlock()
                continuation.resume(returning: data)
            } else {
                waiter = continuation
                lock.unlock()
            }
        }
    }

    func close() {}

    private func push(_ data: Data) {
        lock.lock()
        if let waiter {
            self.waiter = nil
            lock.unlock()
            waiter.resume(returning: data)
        } else {
            inbox.append(data)
            lock.unlock()
        }
    }
}
