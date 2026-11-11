import Foundation
import XCTest
@testable import ThreadbearCore

final class PoolsTests: XCTestCase {
    private var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Australia/Perth")!
        return calendar
    }

    /// Tuesday 27 Oct 2026, 10:00 in Perth.
    private var now: Date { at(10) }

    private func at(day: Int = 27, _ hour: Int, _ minute: Int = 0) -> Date {
        calendar.date(from: DateComponents(year: 2026, month: 10, day: day, hour: hour, minute: minute))!
    }

    private func room(_ fiveHour: Double, weekly: Double = 90, blocked: Bool = false, reset: Date? = nil) -> AccountUsage {
        AccountUsage(windows: [UsageWindow(kind: "five_hour", title: "5-hour", remainingPercent: fiveHour, resetsAt: reset),
                               UsageWindow(kind: "weekly", title: "Weekly", remainingPercent: weekly)], blocked: blocked)
    }

    /// Mike's logins, as in the plan: three own Claude ones, the Convex
    /// one, and Codex.
    private var accounts: [Account] {
        [
            Account(id: "claude-main", title: "sam@acme.io", email: "sam@acme.io", usage: room(61)),
            Account(id: "claude-1", title: "sam.lee@gmail.com", email: "sam.lee@gmail.com", usage: room(16)),
            Account(id: "claude-3", title: "sam@bearworks.com", email: "sam@bearworks.com", usage: room(0, blocked: true, reset: at(17))),
            Account(id: "claude-2", title: "mike@convex.dev", email: "mike@convex.dev", note: "Convex work only", usage: room(65)),
            Account(id: "codex-main", provider: "codex", title: "Codex · sam@acme.io", email: "sam@acme.io", usage: room(91, weekly: 95)),
        ]
    }

    private let claude = Pool(id: "claude", title: "Claude", accountIds: ["claude-main", "claude-1", "claude-3"], builtIn: true)
    private let convex = Pool(id: "convex", title: "Convex", accountIds: ["claude-2"], folders: ["~/dev/convex"], builtIn: true)
    private let codex = Pool(id: "codex", title: "Codex", accountIds: ["codex-main"], builtIn: true)
    private let mixed = Pool(id: "mixed", title: "Everything", accountIds: ["codex-main", "claude-main", "claude-1"])

    // MARK: Folders, the same cases as the runner

    private struct Cases: Decodable {
        struct Case: Decodable {
            var name: String
            var pools: [Pool]?
            var cwd: String
            var expected: String?
        }

        var home: String
        var pools: [Pool]
        var cases: [Case]
    }

    private static var casesURL: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Fixtures/pool-folder-cases.json")
    }

    func testForFolderMatchesTheRunnersCases() throws {
        let file = try JSONDecoder().decode(Cases.self, from: Data(contentsOf: Self.casesURL))
        XCTAssertGreaterThanOrEqual(file.cases.count, 15)
        for c in file.cases {
            XCTAssertEqual(Pools.forFolder(c.pools ?? file.pools, cwd: c.cwd, home: file.home), c.expected, c.name)
        }
    }

    /// The copy here is the shared file; once both are in one tree, they
    /// must be the same bytes.
    func testCasesAreTheRunnersCopy() throws {
        let runner = Self.casesURL.deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("runner/test/fixtures/pool-folder-cases.json")
        guard FileManager.default.fileExists(atPath: runner.path) else { throw XCTSkip("the runner's copy isn't in this tree yet") }
        XCTAssertEqual(try Data(contentsOf: runner), try Data(contentsOf: Self.casesURL),
                       "app/Tests/Fixtures/pool-folder-cases.json differs from runner/test/fixtures: copy the runner's over")
    }

    /// Mike's pick for the draft, then the folder's pool, then the one he
    /// last picked, then the runner's default, then the first.
    func testNewThreadPoolOrder() {
        let pools = [claude, convex, codex]
        let home = "/Users/mike"
        func pick(picked: String? = nil, cwd: String = "/Users/mike/dev/me/misc", last: String? = nil, runner: String? = nil) -> String? {
            Pools.newThreadPool(pools, picked: picked, cwd: cwd, home: home, last: last, runnerDefault: runner)
        }
        XCTAssertEqual(pick(picked: "codex", cwd: "/Users/mike/dev/convex/x", last: "claude"), "codex", "a pick for this draft wins")
        XCTAssertEqual(pick(cwd: "/Users/mike/dev/convex/x", last: "codex"), "convex", "the folder beats the last pick")
        XCTAssertEqual(pick(last: "codex", runner: "claude"), "codex")
        XCTAssertEqual(pick(last: "gone", runner: "codex"), "codex", "a pool that's gone is skipped")
        XCTAssertEqual(pick(picked: "gone"), "claude", "nothing else: the first")
        XCTAssertNil(Pools.newThreadPool([], picked: "claude", cwd: "/", home: home, last: "claude", runnerDefault: "claude"))
    }

    /// Unpin keeps the thread on its login: a pool that holds it.
    func testUnpinTarget() {
        let pools = [claude, convex, codex, mixed]
        let home = "/Users/mike"
        XCTAssertEqual(Pools.unpinTarget(pools, accountId: "claude-1", cwd: "/x", home: home, last: "mixed"), "mixed", "the last pick holds it")
        XCTAssertEqual(Pools.unpinTarget(pools, accountId: "claude-1", cwd: "/x", home: home, last: "codex"), "claude", "else the first that holds it")
        XCTAssertEqual(Pools.unpinTarget(pools, accountId: "claude-2", cwd: "/Users/mike/dev/convex", home: home, last: "claude"), "convex")
        XCTAssertEqual(Pools.unpinTarget(pools, accountId: "claude-9", cwd: "/x", home: home, last: "codex"), "codex", "a login no pool has: the last pick")
    }

    // MARK: Logins

    func testProvidersAndLoginsKeepPoolOrder() {
        XCTAssertEqual(Pools.providers(mixed, accounts: accounts), ["codex", "claude"])
        XCTAssertEqual(Pools.providers(claude, accounts: accounts), ["claude"])
        XCTAssertEqual(Pools.logins(mixed, accounts: accounts, provider: "claude").map(\.id), ["claude-main", "claude-1"])
        let missing = Pool(id: "p", title: "P", accountIds: ["claude-9", "claude-1"])
        XCTAssertEqual(Pools.logins(missing, accounts: accounts).map(\.id), ["claude-1"], "a login the runner doesn't have is left out")
    }

    /// The thread's own login while it has room; else the most room; a
    /// note doesn't matter inside a pool.
    func testPickLogin() {
        XCTAssertEqual(Pools.pickLogin(claude, accounts: accounts, provider: "claude"), "claude-main")
        XCTAssertEqual(Pools.pickLogin(claude, accounts: accounts, provider: "claude", current: "claude-1"), "claude-1", "16% is still room")
        XCTAssertEqual(Pools.pickLogin(claude, accounts: accounts, provider: "claude", current: "claude-3"), "claude-main", "a blocked one moves")
        XCTAssertEqual(Pools.pickLogin(convex, accounts: accounts, provider: "claude"), "claude-2", "the note only guards threads with no pool")
        XCTAssertEqual(Pools.pickLogin(mixed, accounts: accounts, provider: "codex"), "codex-main")
        XCTAssertNil(Pools.pickLogin(codex, accounts: accounts, provider: "claude"))
        let dry = accounts.map { account -> Account in
            var account = account
            account.usage = room(5)
            return account
        }
        XCTAssertEqual(Pools.pickLogin(claude, accounts: dry, provider: "claude"), "claude-main", "nothing with room: the first")
        let unknown = accounts.map { account -> Account in
            var account = account
            if account.id == "claude-main" { account.usage = nil } else { account.usage = room(5) }
            return account
        }
        XCTAssertEqual(Pools.pickLogin(claude, accounts: unknown, provider: "claude"), "claude-main", "unknown usage beats too little")
    }

    func testPickLoginSpendsTheWeekThatResetsFirst() {
        // Mike's three on 5 Nov: the fullest resets in 5 days, the others
        // today and tomorrow with a third of their week left.
        let now = at(8)
        func login(_ id: String, week: Double, resetsIn hours: Double, five: Double = 95) -> Account {
            Account(id: id, title: id, usage: AccountUsage(windows: [
                UsageWindow(kind: "five_hour", title: "5-hour", remainingPercent: five),
                UsageWindow(kind: "weekly", title: "Weekly", remainingPercent: week, resetsAt: now.addingTimeInterval(hours * 3600)),
            ]))
        }
        let three = [login("claude-main", week: 77, resetsIn: 120), login("claude-1", week: 35, resetsIn: 17), login("claude-3", week: 37, resetsIn: 5)]
        XCTAssertEqual(Pools.pickLogin(claude, accounts: three, provider: "claude", now: now), "claude-3")
        XCTAssertEqual(Pools.pickLogin(claude, accounts: three, provider: "claude", now: now.addingTimeInterval(6 * 3600)), "claude-1", "after its reset, its next one isn't known")
        XCTAssertEqual(Pools.pickLogin(claude, accounts: three, provider: "claude", current: "claude-main", now: now), "claude-main")
        let low = [three[0], three[1], login("claude-3", week: 8, resetsIn: 5)]
        XCTAssertEqual(Pools.pickLogin(claude, accounts: low, provider: "claude", now: now), "claude-1", "under the floor it's left")
    }

    func testRailSubtitle() {
        XCTAssertEqual(Pools.railSubtitle(claude, accounts: accounts), "3 · 61%")
        XCTAssertEqual(Pools.railSubtitle(convex, accounts: accounts), "1 · 65%")
        XCTAssertEqual(Pools.railSubtitle(codex, accounts: accounts), "1 · 91%")
        let old = accounts.map { Account(id: $0.id, provider: $0.provider, title: $0.title) }
        XCTAssertEqual(Pools.railSubtitle(claude, accounts: old), "3", "no usage known: just the count")
        let blocked = Pool(id: "b", title: "B", accountIds: ["claude-3"])
        XCTAssertEqual(Pools.railSubtitle(blocked, accounts: accounts), "1 · 0%")
    }

    /// The most room first, resting ones last with when they're back.
    func testStatusLine() {
        XCTAssertEqual(Pools.statusLine(claude, accounts: accounts, now: now, calendar: calendar),
                       "acme 61% · gmail 16% · bearworks resting until 5 pm")
        XCTAssertEqual(Pools.statusLine(mixed, accounts: accounts, now: now, calendar: calendar), "Codex 91% · acme 61% · gmail 16%")
        var unknown = accounts
        unknown[0].usage = nil
        unknown[2].usage = AccountUsage(windows: [], blocked: true)
        XCTAssertEqual(Pools.statusLine(claude, accounts: unknown, now: now, calendar: calendar), "gmail 16% · acme · bearworks resting")
        XCTAssertNil(Pools.statusLine(Pool(id: "e", title: "E", accountIds: []), accounts: accounts, now: now))
    }

    /// Only when the whole pool is low: one login with room is enough for
    /// the runner to move to.
    func testChipBadgeIsForTheWholePool() {
        XCTAssertNil(Pools.chipBadge(claude, accounts: accounts), "acme has 61%")
        var low = accounts
        low[0].usage = room(20)
        XCTAssertEqual(Pools.chipBadge(claude, accounts: low), .low("20%"))
        var out = accounts
        out[0].usage = room(0, blocked: true)
        out[1].usage = room(0, blocked: true)
        XCTAssertEqual(Pools.chipBadge(claude, accounts: out), .blocked)
        var unknown = low
        unknown[1].usage = nil
        XCTAssertNil(Pools.chipBadge(claude, accounts: unknown), "a login with unknown usage might have room")
        XCTAssertNil(Pools.chipBadge(Pool(id: "e", title: "E", accountIds: []), accounts: accounts))
    }

    /// Mike's logins share "mike" before the @, so pools name them by
    /// their domain, falling back when two share one.
    func testLoginNames() {
        XCTAssertEqual(accounts.map { AccountName.login($0.id, accounts: accounts) }, ["acme", "gmail", "bearworks", "convex", "Codex"])
        let twoGmails = [Account(id: "a", title: "a@gmail.com", email: "a@gmail.com"), Account(id: "b", title: "b@gmail.com", email: "b@gmail.com"),
                         Account(id: "c", title: "a@gmail.co.uk", email: "a@gmail.co.uk")]
        XCTAssertEqual(twoGmails.map { AccountName.login($0.id, accounts: twoGmails) }, ["a@gmail.com", "b", "a@gmail.co.uk"])
        XCTAssertEqual(AccountName.login("gone", accounts: accounts), "gone")
    }

    func testChipTitle() {
        let choices = [ModelChoice(id: "opus", model: "claude-opus-5-5", title: "Opus 5.5")]
        XCTAssertEqual(Pools.chipTitle(owner: "Claude", model: "opus", effort: "high", choices: choices), "Claude · Opus 5.5 · High")
        XCTAssertEqual(Pools.chipTitle(owner: "mike.cann", model: "claude-opus-5-5", effort: nil, choices: []), "mike.cann · Opus 5.5")
        XCTAssertEqual(Pools.chipTitle(owner: nil, model: "opus", effort: "xhigh", choices: choices), "Opus 5.5 · Extra high", "no pools: as before")
    }

    func testWaitingLine() {
        XCTAssertEqual(Pools.waitingLine(WaitingForRoom(until: at(17, 1)), now: now, calendar: calendar), "Waiting for room · 5:01 pm")
        XCTAssertEqual(Pools.waitingLine(WaitingForRoom(until: at(day: 28, 5)), now: now, calendar: calendar), "Waiting for room · tomorrow 5:00 am")
        XCTAssertEqual(Pools.waitingLine(WaitingForRoom(until: at(9)), now: now, calendar: calendar), "Waiting for room · any moment now")
        XCTAssertEqual(Pools.waitingTime(at(day: 28, 5), now: now, calendar: calendar), "tomorrow 5:00 am")
    }

    // MARK: Settings

    func testProblemSaysWhyTheRunnerWouldRefuse() {
        XCTAssertNil(Pools.problem([claude, convex, codex], accounts: accounts))
        XCTAssertEqual(Pools.problem([], accounts: accounts), "Keep at least one pool.")
        XCTAssertEqual(Pools.problem([Pool(id: "x", title: "  ", accountIds: ["claude-1"])], accounts: accounts), "Every pool needs a name.")
        XCTAssertEqual(Pools.problem([claude, Pool(id: "claude", title: "Again", accountIds: ["claude-1"])], accounts: accounts), "Two pools have the id claude.")
        XCTAssertEqual(Pools.problem([Pool(id: "x", title: "Work", accountIds: [])], accounts: accounts), "Work needs at least one login.")
        XCTAssertEqual(Pools.problem([Pool(id: "x", title: "Work", accountIds: ["claude-9"])], accounts: accounts),
                       "Work has a login the runner doesn't know (claude-9).")
    }

    func testLoginListAndEditing() {
        let list = Pools.loginList(mixed, accounts: accounts)
        XCTAssertEqual(list.map(\.account.id), ["codex-main", "claude-main", "claude-1", "claude-3", "claude-2"])
        XCTAssertEqual(list.map(\.included), [true, true, true, false, false])
        let ticked = Pools.setting(mixed, login: "claude-2", included: true)
        XCTAssertEqual(ticked.accountIds, ["codex-main", "claude-main", "claude-1", "claude-2"], "a newly ticked login goes last")
        XCTAssertEqual(Pools.setting(ticked, login: "codex-main", included: false).accountIds, ["claude-main", "claude-1", "claude-2"])
        XCTAssertEqual(Pools.moving(mixed, login: "claude-main", by: -1).accountIds, ["claude-main", "codex-main", "claude-1"])
        XCTAssertEqual(Pools.moving(mixed, login: "codex-main", by: -1).accountIds, mixed.accountIds, "the first can't go up")
        XCTAssertEqual(Pools.moving(mixed, login: "claude-1", by: 1).accountIds, mixed.accountIds, "the last can't go down")
        XCTAssertEqual(Pools.moving(mixed, login: "claude-1", onto: "codex-main").accountIds, ["claude-1", "codex-main", "claude-main"])
        XCTAssertEqual(Pools.moving(mixed, login: "codex-main", onto: "claude-1").accountIds, ["claude-main", "claude-1", "codex-main"])
        XCTAssertEqual(Pools.moving(mixed, login: "claude-3", onto: "claude-1").accountIds, mixed.accountIds, "an unticked login doesn't move in")
    }

    func testSymbolFallsBackToTheFirstProvider() {
        XCTAssertEqual(Pools.symbol(Pool(id: "a", title: "A", accountIds: ["claude-1"], icon: "leaf"), accounts: accounts), "leaf")
        XCTAssertEqual(Pools.symbol(mixed, accounts: accounts), "chevron.left.forwardslash.chevron.right", "Codex comes first in it")
        XCTAssertEqual(Pools.symbol(claude, accounts: accounts), "sparkle")
        XCTAssertEqual(Pools.symbol(Pool(id: "c", title: "C", accountIds: []), accounts: accounts), "square.stack")
    }

    func testIconRoundTrips() throws {
        let pool = Pool(id: "a", title: "A", accountIds: ["claude-1"], icon: "leaf")
        let decoded = try JSONDecoder().decode(Pool.self, from: JSONEncoder().encode(pool))
        XCTAssertEqual(decoded.icon, "leaf")
        let none = try JSONDecoder().decode(Pool.self, from: Data(#"{"id":"b","title":"B","accountIds":[],"icon":""}"#.utf8))
        XCTAssertNil(none.icon, "a blank icon is no icon")
    }

    func testRenamingKeepsTheId() {
        let pool = Pool(id: "convex", title: "Convex", accountIds: ["claude-1"])
        XCTAssertEqual(Pools.renaming(pool, to: "  Work  ")?.title, "Work")
        XCTAssertEqual(Pools.renaming(pool, to: "Work")?.id, "convex")
        XCTAssertNil(Pools.renaming(pool, to: "   "), "a blank name isn't saved")
        XCTAssertNil(Pools.renaming(pool, to: "Convex "), "nothing changed")
    }

    func testNewId() {
        XCTAssertEqual(Pools.newId(title: "Side projects", taken: ["claude"]), "side-projects")
        XCTAssertEqual(Pools.newId(title: "Claude", taken: ["claude", "claude-2"]), "claude-3")
        XCTAssertEqual(Pools.newId(title: "  !! ", taken: []), "pool")
    }

    // MARK: Protocol

    func testHelloWithAndWithoutPools() throws {
        let old = try JSONDecoder().decode(HelloResult.self, from: Data(#"{"runnerVersion":"1","accounts":[],"defaults":{"cwd":"/x","accountId":"claude-main"}}"#.utf8))
        XCTAssertNil(old.pools, "an older runner: no pools, so the app works as before")
        XCTAssertNil(old.defaults.poolId)
        let new = try JSONDecoder().decode(HelloResult.self, from: Data(#"""
        {"runnerVersion":"2","accounts":[],"defaults":{"cwd":"/x","accountId":"claude-main","poolId":"claude"},
         "pools":[{"id":"claude","title":"Claude","accountIds":["claude-main","claude-1"],"builtIn":true},
                  {"id":"convex","title":"Convex","accountIds":["claude-2"],"folders":["~/dev/convex"]},
                  {"title":"no id"}]}
        """#.utf8))
        XCTAssertEqual(new.pools?.map(\.id), ["claude", "convex"], "a pool that doesn't decode is skipped")
        XCTAssertEqual(new.pools?.first?.builtIn, true)
        XCTAssertEqual(new.pools?.last?.folders, ["~/dev/convex"])
        XCTAssertEqual(new.defaults.poolId, "claude")
    }

    func testThreadSummaryPoolFields() throws {
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"""
        {"id":"th1","accountId":"claude-1","poolId":"claude","waitingForRoom":{"until":"2026-10-27T21:01:00Z","accountId":"claude-3"}}
        """#.utf8))
        XCTAssertEqual(thread.poolId, "claude")
        XCTAssertEqual(thread.waitingForRoom?.accountId, "claude-3")
        XCTAssertEqual(thread.waitingForRoom?.until, ISO8601.parse("2026-10-27T21:01:00Z"))
        let pinned = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"th2","accountId":"claude-1","waitingForRoom":{"until":"soon"}}"#.utf8))
        XCTAssertNil(pinned.poolId, "no pool: pinned")
        XCTAssertNil(pinned.waitingForRoom, "a time it can't read is no wait at all")
        let again = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread))
        XCTAssertEqual(again.poolId, "claude")
        XCTAssertEqual(again.waitingForRoom, thread.waitingForRoom)
    }

    func testThreadSummaryWaitingForNetwork() throws {
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"th1","accountId":"claude-1","waitingForNetwork":{"since":"2026-11-03T03:59:55Z"}}"#.utf8))
        XCTAssertEqual(thread.waitingForNetwork, ISO8601.parse("2026-11-03T03:59:55Z"))
        let again = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread))
        XCTAssertEqual(again.waitingForNetwork, thread.waitingForNetwork)
        let bad = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"th2","accountId":"claude-1","waitingForNetwork":{"since":"soon"}}"#.utf8))
        XCTAssertNil(bad.waitingForNetwork, "a time it can't read is no wait at all")
    }

    func testRequestsCarryThePool() {
        XCTAssertEqual(ThreadUpdate(poolId: .some(nil)).params["poolId"], .null, "null pins")
        XCTAssertEqual(ThreadUpdate(poolId: "convex").params["poolId"], .string("convex"))
        XCTAssertNil(ThreadUpdate(title: "x").params["poolId"])
        XCTAssertEqual(convex.jsonValue, .object(["id": .string("convex"), "title": .string("Convex"), "accountIds": .array([.string("claude-2")]),
                                                  "folders": .array([.string("~/dev/convex")]), "builtIn": .bool(true)]))
    }

    func testPoolRequestsAndNotification() async throws {
        let transport = FakeTransport()
        transport.onSend = { message in
            guard let id = message["id"] as? Int else { return [] }
            let pools: [[String: Any]] = (message["params"] as? [String: Any])?["pools"] as? [[String: Any]]
                ?? [["id": "claude", "title": "Claude", "accountIds": ["claude-main"]]]
            return [E.json(["jsonrpc": "2.0", "id": id, "result": ["pools": pools]])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        let listed = try await client.listPools()
        XCTAssertEqual(listed.map(\.id), ["claude"])
        let saved = try await client.savePools([convex])
        XCTAssertEqual(saved, [convex])
        XCTAssertEqual(transport.sent.last?["method"] as? String, "pools.save")
        transport.push(E.json(["jsonrpc": "2.0", "method": "pools.updated", "params": ["pools": [["id": "codex", "title": "Codex", "accountIds": ["codex-main"]]]]]))
        var iterator = client.notifications.makeAsyncIterator()
        guard case .poolsUpdated(let pools)? = await iterator.next() else { return XCTFail("no pools") }
        XCTAssertEqual(pools.map(\.id), ["codex"])
    }
}
