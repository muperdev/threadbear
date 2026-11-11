import XCTest
@testable import ThreadbearCore

final class ScheduleTests: XCTestCase {
    // MARK: Decoding

    func testScheduleDecodesFullAndToleratesUnknownFields() throws {
        let json = """
        {"id":"sc1","title":"Morning check","text":"Check the deploy.","cron":"0 9 * * 1-5","timezone":"Australia/Perth",
         "whenText":"Weekdays at 9:00","threadId":"th1","paused":false,"createdAt":"2026-10-26T10:00:00Z",
         "updatedAt":"2026-10-26T11:00:00Z","createdBy":"th1","nextRunAt":"2026-10-28T01:00:00Z","runs":3,
         "lastRun":{"at":"2026-10-27T01:00:00Z","outcome":"skipped","threadId":"th1","note":"busy","extra":1},
         "somethingNew":{"a":[1,2]}}
        """
        let schedule = try JSONDecoder().decode(Schedule.self, from: Data(json.utf8))
        XCTAssertEqual(schedule.title, "Morning check")
        XCTAssertEqual(schedule.cron, "0 9 * * 1-5")
        XCTAssertEqual(schedule.timezone, "Australia/Perth")
        XCTAssertEqual(schedule.whenText, "Weekdays at 9:00")
        XCTAssertEqual(schedule.threadId, "th1")
        XCTAssertEqual(schedule.createdBy, "th1")
        XCTAssertEqual(schedule.runs, 3)
        XCTAssertEqual(schedule.nextRunAt, ISO8601.parse("2026-10-28T01:00:00Z"))
        XCTAssertEqual(schedule.lastRun?.outcome, .skipped)
        XCTAssertEqual(schedule.lastRun?.note, "busy")
        XCTAssertFalse(schedule.isOneOff)
        XCTAssertTrue(schedule.isActive)
        XCTAssertTrue(schedule.belongs(to: "th1"))
        XCTAssertFalse(schedule.belongs(to: "th2"))
    }

    func testScheduleWithOnlyAnIdAndText() throws {
        let schedule = try JSONDecoder().decode(Schedule.self, from: Data(#"{"id":"x","text":"First line\nSecond","runs":"2"}"#.utf8))
        XCTAssertEqual(schedule.title, "First line", "the title defaults to the first line of the text")
        XCTAssertFalse(schedule.paused)
        XCTAssertNil(schedule.threadId)
        XCTAssertEqual(schedule.runs, 2)
        XCTAssertTrue(schedule.isOneOff)
    }

    func testFinishedOneOffAndUnknownOutcome() throws {
        let json = #"{"id":"x","title":"t","text":"t","at":"2026-10-27T05:30:00Z","whenText":"Once","paused":false,"finishedAt":"2026-10-27T05:30:02Z","lastRun":{"outcome":"teleported"}}"#
        let schedule = try JSONDecoder().decode(Schedule.self, from: Data(json.utf8))
        XCTAssertEqual(schedule.at, ISO8601.parse("2026-10-27T05:30:00Z"))
        XCTAssertTrue(schedule.isFinished)
        XCTAssertFalse(schedule.isActive)
        XCTAssertEqual(schedule.lastRun?.outcome, .sent, "an outcome from a newer runner reads as sent, not as a failure")
    }

    func testListSkipsSchedulesThatDontDecode() throws {
        let json = #"{"schedules":[{"id":"a","text":"one"},{"title":"no id"},{"id":"b","text":"two"}]}"#
        let result = try JSONDecoder().decode(SchedulesResult.self, from: Data(json.utf8))
        XCTAssertEqual(result.schedules.map(\.id), ["a", "b"])
    }

    // MARK: Runners (docs/protocol.md, "Schedules", "Runners")

    func testScheduleOnAnotherRunner() throws {
        let json = #"{"id":"x","text":"t","cron":"0 8 * * 1","runnerId":"r_bruce","runnerName":"bruce","runnerIcon":"macmini","runRequestedAt":"2026-10-29T01:02:03Z"}"#
        let schedule = try JSONDecoder().decode(Schedule.self, from: Data(json.utf8))
        XCTAssertEqual(schedule.runnerId, "r_bruce")
        XCTAssertEqual(schedule.runnerName, "bruce")
        XCTAssertEqual(schedule.runnerIcon, "macmini")
        XCTAssertEqual(schedule.runRequestedAt, ISO8601.parse("2026-10-29T01:02:03Z"))
        XCTAssertTrue(schedule.runsElsewhere(than: "r_m5"))
        XCTAssertFalse(schedule.runsElsewhere(than: "r_bruce"))
        XCTAssertFalse(schedule.runsElsewhere(than: nil), "without runners.list every schedule is this runner's")
    }

    func testScheduleRunnerFieldsDecodeTolerantly() throws {
        let json = #"{"id":"x","text":"t","runnerId":"","runnerName":5,"runnerIcon":"","runRequestedAt":"soon"}"#
        let schedule = try JSONDecoder().decode(Schedule.self, from: Data(json.utf8))
        XCTAssertNil(schedule.runnerId)
        XCTAssertNil(schedule.runnerName)
        XCTAssertNil(schedule.runnerIcon)
        XCTAssertNil(schedule.runRequestedAt)
        XCTAssertFalse(schedule.runsElsewhere(than: "r_m5"))
    }

    func testRunnersDecodeLeniently() throws {
        let json = #"""
        {"runners":[{"id":"r_m5","name":"m5","icon":"laptopcomputer","online":true,"self":true,"lastSeenAt":"2026-10-29T01:00:00Z"},
                    {"id":"r_bruce","name":"bruce","online":false,"self":false,"somethingNew":1},
                    {"name":"no id"},{"id":"r_bare"}]}
        """#
        let runners = try JSONDecoder().decode(RunnersResult.self, from: Data(json.utf8)).runners
        XCTAssertEqual(runners.map(\.id), ["r_m5", "r_bruce", "r_bare"])
        XCTAssertTrue(runners[0].isSelf)
        XCTAssertEqual(runners[0].icon, "laptopcomputer")
        XCTAssertNil(runners[1].icon, "a runner from before icons has none")
        XCTAssertTrue(runners[0].online)
        XCTAssertEqual(runners[0].lastSeenAt, ISO8601.parse("2026-10-29T01:00:00Z"))
        XCTAssertFalse(runners[1].isSelf)
        XCTAssertFalse(runners[1].online)
        XCTAssertEqual(runners[2].name, "r_bare", "a runner with no name goes by its id")
        XCTAssertFalse(runners[2].online)
    }

    func testRunResultQueuedOrSent() throws {
        let queued = try JSONDecoder().decode(ScheduleRunResult.self, from: Data(#"{"schedule":{"id":"x","text":"t"},"queued":true}"#.utf8))
        XCTAssertTrue(queued.queued)
        XCTAssertNil(queued.threadId)
        XCTAssertNil(queued.turnId)
        let sent = try JSONDecoder().decode(ScheduleRunResult.self, from: Data(#"{"schedule":{"id":"x","text":"t"},"threadId":"th1","turnId":"tu1"}"#.utf8))
        XCTAssertFalse(sent.queued)
        XCTAssertEqual(sent.threadId, "th1")
        XCTAssertEqual(sent.turnId, "tu1")
    }

    func testRunnersListFromAnOlderRunnerIsJustThisOne() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in
            guard let id = request["id"] else { return [] }
            return [E.json(["jsonrpc": "2.0", "id": id, "error": ["code": -32601, "message": "Unknown method runners.list"]])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        let runners = try await client.listRunners()
        XCTAssertEqual(runners, [], "-32601 means no other runner to offer, not an error")
        XCTAssertEqual(transport.sent.first?["method"] as? String, "runners.list")
    }

    func testRunnersListOtherErrorsStillThrow() async throws {
        let transport = FakeTransport()
        transport.onSend = { request in
            guard let id = request["id"] else { return [] }
            return [E.json(["jsonrpc": "2.0", "id": id, "error": ["code": -32000, "message": "No backend"]])]
        }
        let client = RPCClient(transport: transport)
        try await client.start()
        do {
            _ = try await client.listRunners()
            XCTFail("only -32601 reads as just this runner")
        } catch RPCError.server(let code, _) {
            XCTAssertEqual(code, -32000)
        }
    }

    func testPreviewDecodesRuns() throws {
        let json = #"{"whenText":"Weekdays at 9:00","runs":["2026-10-28T01:00:00Z","not a date","2026-10-29T01:00:00Z"]}"#
        let preview = try JSONDecoder().decode(SchedulePreview.self, from: Data(json.utf8))
        XCTAssertEqual(preview.whenText, "Weekdays at 9:00")
        XCTAssertEqual(preview.runs.count, 2)
    }

    func testSchedulesUpdatedNotification() async throws {
        let transport = FakeTransport()
        let client = RPCClient(transport: transport)
        try await client.start()
        transport.push(E.json(["jsonrpc": "2.0", "method": "schedules.updated",
                               "params": ["schedules": [["id": "sc1", "text": "hi", "whenText": "Every hour", "cron": "0 * * * *"]]]]))
        var iterator = client.notifications.makeAsyncIterator()
        guard case .schedulesUpdated(let list)? = await iterator.next() else { return XCTFail("no schedules.updated") }
        XCTAssertEqual(list.map(\.id), ["sc1"])
        XCTAssertEqual(list.first?.whenText, "Every hour")
    }

    // MARK: Requests

    func testCreateParamsLeaveNewThreadSettingsOutForAThread() {
        let at = ISO8601.parse("2026-10-28T01:00:00Z")!
        let forThread = NewScheduleRequest(text: "hi", when: .at(at), threadId: "th1", cwd: "/x", accountId: "claude-3").params
        XCTAssertEqual(forThread["threadId"], .string("th1"))
        XCTAssertEqual(forThread["at"], .string(ISO8601.format(at)))
        XCTAssertNil(forThread["cwd"])
        XCTAssertNil(forThread["accountId"])
        XCTAssertNil(forThread["cron"])
        let fresh = NewScheduleRequest(text: "hi", title: "T", when: .cron("0 9 * * *"), cwd: "/x", accountId: "claude-3", permissionMode: .ask).params
        XCTAssertEqual(fresh["cron"], .string("0 9 * * *"))
        XCTAssertEqual(fresh["cwd"], .string("/x"))
        XCTAssertEqual(fresh["permissionMode"], .string("ask"))
        XCTAssertEqual(fresh["title"], .string("T"))
        XCTAssertNil(fresh["threadId"])
        // A pool picks the login, so the composer's login stays out.
        let pooled = NewScheduleRequest(text: "hi", when: .cron("0 9 * * *"), cwd: "/x", accountId: "claude-3", poolId: "claude").params
        XCTAssertEqual(pooled["poolId"], .string("claude"))
        XCTAssertNil(pooled["accountId"])
    }

    func testCreateParamsCarryTheRunnerOnlyForANewThread() {
        let at = ISO8601.parse("2026-10-28T01:00:00Z")!
        let away = NewScheduleRequest(text: "hi", when: .at(at), cwd: "/Volumes/x", permissionMode: .full, runnerId: "bruce").params
        XCTAssertEqual(away["runnerId"], .string("bruce"))
        XCTAssertEqual(away["cwd"], .string("/Volumes/x"))
        XCTAssertNil(away["accountId"])
        XCTAssertNil(away["model"])
        let forThread = NewScheduleRequest(text: "hi", when: .at(at), threadId: "th1", runnerId: "bruce").params
        XCTAssertNil(forThread["runnerId"], "a thread's schedule runs where the thread is")
        XCTAssertNil(NewScheduleRequest(text: "hi", when: .at(at)).params["runnerId"])
    }

    func testUpdateParamsOnlyWhatChanged() {
        XCTAssertEqual(ScheduleUpdate(paused: true).params, ["paused": .bool(true)])
        let params = ScheduleUpdate(text: "new", when: .cron("0 * * * *")).params
        XCTAssertEqual(params["cron"], .string("0 * * * *"))
        XCTAssertEqual(params["text"], .string("new"))
        XCTAssertNil(params["at"])
        XCTAssertNil(params["model"], "a model left out stays as it is")
        let models = ScheduleUpdate(model: .some("opus"), effort: .some(nil)).params
        XCTAssertEqual(models["model"], .string("opus"))
        XCTAssertEqual(models["effort"], .null, "nil effort goes back to the default")
    }

    // MARK: Model and effort

    private let opus = ModelChoice(id: "opus", model: "claude-opus-5-5", title: "Opus 5.5", effortLevels: ["low", "medium", "high", "xhigh", "max"])
    private let haiku = ModelChoice(id: "haiku", model: "claude-haiku-5-5", title: "Haiku 5.5")
    private let sol = ModelChoice(id: "gpt-6.1-sol", model: "gpt-6.1-sol", title: "GPT-6.1 Sol", effortLevels: ["low", "high", "ultra"], defaultEffort: "high")

    private var logins: RunnerLogins {
        RunnerLogins(
            accounts: [
                Account(id: "claude-1", title: "One", defaultModel: "opus", models: [opus, haiku]),
                Account(id: "codex-1", provider: "codex", title: "Codex", defaultModel: "gpt-6.1-sol", models: [sol]),
            ],
            pools: [Pool(id: "mixed", title: "Mixed", accountIds: ["claude-1", "codex-1"])],
            defaults: RunnerDefaults(cwd: "/tmp", accountId: "claude-1", efforts: ["claude": "high"])
        )
    }

    func testScheduleModelsFromAPinnedLogin() {
        let models = ScheduleModels(accountId: "claude-1", poolId: nil, model: nil, logins: logins)
        XCTAssertEqual(models.provider, "claude")
        XCTAssertEqual(models.rows.map(\.id), ["opus", "haiku"])
        XCTAssertEqual(models.defaultModelTitle, "Default (Opus 5.5)")
        XCTAssertEqual(models.defaultEffortTitle, "Default (High)", "the runner's default effort for Claude")
        XCTAssertEqual(models.effortLevels, ["low", "medium", "high", "xhigh", "max"])
    }

    func testScheduleModelsOnAPoolFollowTheChosenModelsProvider() {
        let codex = ScheduleModels(accountId: nil, poolId: "mixed", model: "gpt-6.1-sol", logins: logins)
        XCTAssertEqual(codex.provider, "codex")
        XCTAssertEqual(codex.effortLevels, ["low", "high", "ultra"])
        XCTAssertEqual(codex.defaultEffortTitle, "Default (High)", "Sol's own level")
        let unset = ScheduleModels(accountId: nil, poolId: "mixed", model: nil, logins: logins)
        XCTAssertEqual(unset.provider, "claude", "no model: the pool's first login")
    }

    func testScheduleModelsWithNoLoginUseTheRunnersDefault() {
        let models = ScheduleModels(accountId: nil, poolId: nil, model: nil, logins: logins)
        XCTAssertEqual(models.account?.id, "claude-1")
        let empty = ScheduleModels(accountId: nil, poolId: nil, model: nil, logins: RunnerLogins(accounts: [], pools: [], defaults: RunnerDefaults(cwd: "", accountId: "")))
        XCTAssertEqual(empty.defaultModelTitle, "Default")
        XCTAssertEqual(empty.rows.map(\.id), ModelPicker.aliases, "before hello, the aliases the CLI always takes")
    }

    // MARK: Presets

    func testPresetsBuildCron() {
        XCTAssertEqual(RepeatChoice(kind: .hourly).cron, "0 * * * *")
        XCTAssertEqual(RepeatChoice(kind: .daily, hour: 7, minute: 30).cron, "30 7 * * *")
        XCTAssertEqual(RepeatChoice(kind: .weekdays, hour: 9, minute: 0).cron, "0 9 * * 1-5")
        XCTAssertEqual(RepeatChoice(kind: .weekly, hour: 17, minute: 5, weekday: 5).cron, "5 17 * * 5")
        XCTAssertEqual(RepeatChoice(kind: .monthly, hour: 8, minute: 0, day: 15).cron, "0 8 15 * *")
        XCTAssertEqual(RepeatChoice(kind: .custom, custom: "  */15 9-17 * * *\n").cron, "*/15 9-17 * * *")
    }

    func testCronReadsBackAsItsPreset() {
        for choice in [
            RepeatChoice(kind: .hourly), RepeatChoice(kind: .daily, hour: 7, minute: 30),
            RepeatChoice(kind: .weekdays, hour: 9, minute: 0), RepeatChoice(kind: .weekly, hour: 17, minute: 5, weekday: 0),
            RepeatChoice(kind: .monthly, hour: 8, minute: 0, day: 31),
        ] {
            let back = RepeatChoice(cron: choice.cron)
            XCTAssertEqual(back.kind, choice.kind, choice.cron)
            XCTAssertEqual(back.cron, choice.cron)
        }
        XCTAssertEqual(RepeatChoice(cron: "0 9 * * 7").weekday, 0, "7 is Sunday too")
        for other in ["*/15 * * * *", "0 9-17 * * *", "0 9 1 * 1", "0 9 * 6 *", "@daily", "30 * * * *", "0 9 * * mon", "nonsense"] {
            let choice = RepeatChoice(cron: other)
            XCTAssertEqual(choice.kind, .custom, other)
            XCTAssertEqual(choice.cron, other, "a custom cron is kept as it was")
        }
    }

    // MARK: Words

    func testRelativeNextRun() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        func text(_ seconds: TimeInterval) -> String { ScheduleTime.relative(now.addingTimeInterval(seconds), now: now) }
        XCTAssertEqual(text(-30), "now")
        XCTAssertEqual(text(0), "now")
        XCTAssertEqual(text(20), "in 1m")
        XCTAssertEqual(text(12 * 60), "in 12m")
        XCTAssertEqual(text(59 * 60 + 30), "in 1h")
        XCTAssertEqual(text(3 * 3600 + 10 * 60), "in 3h")
        XCTAssertEqual(text(3 * 3600 + 40 * 60), "in 4h")
        XCTAssertEqual(text(23 * 3600), "in 23h")
        XCTAssertEqual(text(26 * 3600), "in 1 day")
        XCTAssertEqual(text(6 * 86_400 + 3600), "in 6 days")
        XCTAssertEqual(ScheduleTime.next(now.addingTimeInterval(3 * 3600), now: now), "next in 3h")
        XCTAssertEqual(ScheduleTime.next(now.addingTimeInterval(-5), now: now), "due now")
    }

    func testDefaultOnceIsAWholeHourAtLeastAQuarterAway() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let at = { (text: String) in ISO8601.parse(text)! }
        XCTAssertEqual(ScheduleTime.defaultOnce(now: at("2026-10-27T10:20:00Z"), calendar: calendar), at("2026-10-27T11:00:00Z"))
        XCTAssertEqual(ScheduleTime.defaultOnce(now: at("2026-10-27T10:50:00Z"), calendar: calendar), at("2026-10-27T12:00:00Z"))
        XCTAssertEqual(ScheduleTime.defaultOnce(now: at("2026-10-27T10:45:00Z"), calendar: calendar), at("2026-10-27T11:00:00Z"))
    }

    func testHighlightRows() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let soon = Schedule(id: "a", title: "A", whenText: "Weekdays at 9:00", threadId: "th1", nextRunAt: now.addingTimeInterval(3 * 3600))
        let later = Schedule(id: "b", title: "B", whenText: "Mondays at 10:00", cwd: "~/dev/me/misc", createdBy: "th1", nextRunAt: now.addingTimeInterval(30 * 3600))
        let paused = Schedule(id: "c", title: "", whenText: "Every hour", threadId: "th1", paused: true)
        let elsewhere = Schedule(id: "d", title: "D", whenText: "Every day at 2:00", threadId: "th2", nextRunAt: now.addingTimeInterval(60))
        let ran = Schedule(id: "e", title: "E", whenText: "Once, Tue 28 Oct at 15:30", threadId: "th1", finishedAt: now)
        let thread = ThreadSummary(id: "th1", title: "T")
        let rows = ScheduleRows.rows(for: thread, in: [paused, later, elsewhere, ran, soon], now: now) { _ in "New thread in misc" }
        XCTAssertEqual(rows.map(\.id), ["a", "b", "c"], "soonest first, paused last, other threads' and spent one-offs left out")
        XCTAssertEqual(rows[0], ScheduleRows.Row(id: "a", title: "A", when: "in 3h", detail: "Weekdays at 9:00", paused: false))
        XCTAssertEqual(rows[1].detail, "Mondays at 10:00 · New thread in misc", "one it made for somewhere else says where")
        XCTAssertEqual(rows[2].title, "Untitled")
        XCTAssertEqual(rows[2].when, "paused")

        // A thread a schedule started shows that schedule, though it sends
        // to new threads rather than this one.
        var started = ThreadSummary(id: "th3", title: "Q3 invoices")
        started.schedule = ThreadSchedule(id: "d", title: "D")
        let origin = ScheduleRows.rows(for: started, in: [elsewhere, soon], now: now) { _ in "unused" }
        XCTAssertEqual(origin.map(\.detail), ["Started this thread · Every day at 2:00"])
        XCTAssertEqual(origin.first?.when, "in 1m")
    }

    // MARK: The transcript

    func testScheduledHeader() throws {
        let header = try XCTUnwrap(ScheduledHeader("[Scheduled: \"Check \"prod\", again\", Once, Tue 28 Oct at 15:30]\nIs it up?\nSay so."))
        XCTAssertEqual(header.title, "Check \"prod\", again")
        XCTAssertEqual(header.whenText, "Once, Tue 28 Oct at 15:30")
        XCTAssertEqual(header.body, "Is it up?\nSay so.")
        XCTAssertNil(ScheduledHeader("Just a message"))
        XCTAssertNil(ScheduledHeader("[Scheduled: no quotes]"))
        XCTAssertEqual(ScheduledHeader("[Scheduled: \"T\", Every hour]")?.body, "")
    }

    func testScheduleMessagesAreNotes() {
        let event = E.event(1, "user.message", ["text": "[Scheduled: \"T\", Every hour]\nGo.", "images": [], "origin": "schedule", "scheduleId": "sc1"])
        let model = TranscriptModel(events: [event])
        guard case .user(let message)? = model.items.first?.content else { return XCTFail("no message") }
        XCTAssertTrue(message.isFromSchedule)
        XCTAssertFalse(message.isFromRunner)
        XCTAssertTrue(message.showsAsNote)
        XCTAssertEqual(message.scheduledHeader?.title, "T")
        XCTAssertFalse(UserMessage(text: "[Scheduled: \"T\", Every hour]").showsAsNote, "Mike typing the header is still Mike")
        XCTAssertNil(UserMessage(text: "[Scheduled: \"T\", Every hour]").scheduledHeader)
    }

    func testAgentHeader() throws {
        let header = try XCTUnwrap(AgentHeader("[Sent by the agent in the thread \"Mobile app for \"Threadbare\" (v2)\" (f5247e5b)]\n\nFrom f5247e5b: hi.\nBye."))
        XCTAssertEqual(header.title, "Mobile app for \"Threadbare\" (v2)")
        XCTAssertEqual(header.threadId, "f5247e5b")
        XCTAssertEqual(header.body, "From f5247e5b: hi.\nBye.")
        XCTAssertNil(AgentHeader("Just a message"))
        XCTAssertNil(AgentHeader("[Sent by the agent in the thread X]"))
        XCTAssertNotNil(UserMessage(text: "[Sent by the agent in the thread \"T\" (12345678)]\n\nGo.").agentHeader)
        XCTAssertNil(UserMessage(text: "[Sent by the agent in the thread \"T\" (12345678)]", origin: "phone").agentHeader)
        XCTAssertFalse(UserMessage(text: "[Sent by the agent in the thread \"T\" (12345678)]").showsAsNote)
        XCTAssertNil(header.movedTo)
    }

    func testMoveThenHeader() throws {
        let header = try XCTUnwrap(AgentHeader("[Sent by the agent before moving to /Users/m/dev/threadbear]\n\nCarry on with the tests."))
        XCTAssertEqual(header.movedTo, "/Users/m/dev/threadbear")
        XCTAssertEqual(header.threadId, "")
        XCTAssertEqual(header.body, "Carry on with the tests.")
        XCTAssertNotNil(UserMessage(text: "[Sent by the agent before moving to /tmp]\n\nGo.").agentHeader)
        XCTAssertNil(UserMessage(text: "[Sent by the agent before moving to /tmp]\n\nGo.", origin: "phone").agentHeader)
        XCTAssertNil(AgentHeader("[Sent by the agent before moving to ]"))
        XCTAssertNil(AgentHeader("[Sent by the agent before moving to /tmp"))
    }

    func testStartedByScheduleTip() {
        let origin = ThreadSchedule(id: "sc1", title: "Quarterly invoice export")
        let live = Schedule(id: "sc1", title: "Invoices", whenText: "Weekdays at 9:00")
        XCTAssertEqual(StartedBySchedule.tip(origin, current: live), "Started by a schedule: Quarterly invoice export (Weekdays at 9:00). Click to edit it.")
        XCTAssertEqual(StartedBySchedule.tip(origin, current: Schedule(id: "sc1", title: "x", whenText: "Every hour", paused: true)),
                       "Started by a schedule: Quarterly invoice export (paused). Click to edit it.")
        XCTAssertEqual(StartedBySchedule.tip(origin, current: nil), "Started by a schedule: Quarterly invoice export. It has since been deleted.")
    }

    func testThreadSummaryScheduleDecodesLeniently() throws {
        let thread = try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1","title":"x","schedule":{"id":"sc1","title":"Daily brief","later":1}}"#.utf8))
        XCTAssertEqual(thread.schedule, ThreadSchedule(id: "sc1", title: "Daily brief"))
        XCTAssertEqual(try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(thread)).schedule, thread.schedule)
        XCTAssertNil(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1"}"#.utf8)).schedule)
        // A malformed one is dropped rather than losing the thread.
        XCTAssertNil(try JSONDecoder().decode(ThreadSummary.self, from: Data(#"{"id":"t1","schedule":"sc1"}"#.utf8)).schedule)
    }

    func testScheduleSections() {
        let now = Date()
        let soon = Schedule(id: "a", title: "A", whenText: "Weekdays at 9:00", nextRunAt: now)
        let paused = Schedule(id: "b", title: "B", whenText: "Every hour", paused: true)
        let ran = Schedule(id: "c", title: "C", whenText: "Once", finishedAt: now)
        // Paused and then run by hand: done, as it won't run again.
        let pausedRan = Schedule(id: "d", title: "D", whenText: "Once", paused: true, finishedAt: now)
        let later = Schedule(id: "e", title: "E", whenText: "Mondays at 10:00", nextRunAt: now.addingTimeInterval(60))
        let groups = ScheduleSection.group([soon, paused, ran, pausedRan, later])
        XCTAssertEqual(groups[.active]?.map(\.id), ["a", "e"])
        XCTAssertEqual(groups[.paused]?.map(\.id), ["b"])
        XCTAssertEqual(groups[.done]?.map(\.id), ["c", "d"])
        XCTAssertEqual(ScheduleSection.allCases.map(\.title), ["Active", "Paused", "Done"])
    }
}
