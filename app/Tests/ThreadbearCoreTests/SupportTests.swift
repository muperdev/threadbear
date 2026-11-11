import XCTest
@testable import ThreadbearCore

final class MarkdownBlocksTests: XCTestCase {
    func testParagraphsSplitOnBlankLines() {
        XCTAssertEqual(MarkdownBlocks.split("# Title\nIntro line\n\nSecond para\n\n\nThird"), ["# Title\nIntro line", "Second para", "Third"])
    }

    func testFencedCodeIsOneBlockWithBlankLines() {
        let text = "Before\n```swift\nlet a = 1\n\nlet b = 2\n```\nAfter"
        XCTAssertEqual(MarkdownBlocks.split(text), ["Before", "```swift\nlet a = 1\n\nlet b = 2\n```", "After"])
    }

    func testUnclosedFenceStaysOpenAtTheEnd() {
        XCTAssertEqual(MarkdownBlocks.split("Intro\n\n```\ncode\n\nmore"), ["Intro", "```\ncode\n\nmore"])
    }

    func testLooseListStaysTogether() {
        let text = "1. One\n\n2. Two\n   more\n\n   indented para\n\nAfter list"
        XCTAssertEqual(MarkdownBlocks.split(text), ["1. One\n\n2. Two\n   more\n\n   indented para", "After list"])
    }

    func testTildeFenceAndInlineBackticks() {
        XCTAssertEqual(MarkdownBlocks.split("~~~\na\n\n~~~\n``` not a fence ``` text"), ["~~~\na\n\n~~~", "``` not a fence ``` text"])
    }

    func testStreamingPrefixKeepsEarlierBlocksStable() {
        let full = "Para one.\n\nPara two is longer.\n\n- a\n- b"
        let partial = String(full.prefix(30))
        XCTAssertEqual(Array(MarkdownBlocks.split(partial).dropLast()), Array(MarkdownBlocks.split(full).prefix(1)))
    }
}

final class DiffBuilderTests: XCTestCase {
    func testSimpleReplacement() {
        let hunks = DiffBuilder.hunks(old: "a\nb\nc", new: "a\nB\nc")
        XCTAssertEqual(hunks, [DiffHunk(oldStart: 1, oldLines: 3, newStart: 1, newLines: 3, lines: [" a", "-b", "+B", " c"])])
    }

    func testFarApartChangesMakeTwoHunks() {
        let old = (1...20).map { "line \($0)" }
        var new = old
        new[1] = "changed 2"
        new[17] = "changed 18"
        let hunks = DiffBuilder.hunks(oldLines: old, newLines: new)
        XCTAssertEqual(hunks.count, 2)
        XCTAssertEqual(hunks[0].oldStart, 1)
        XCTAssertEqual(hunks[1].oldStart, 15)
        XCTAssertEqual(hunks[1].lines.filter { $0.hasPrefix("+") }, ["+changed 18"])
    }

    func testIdenticalTextHasNoHunks() {
        XCTAssertTrue(DiffBuilder.hunks(old: "same", new: "same").isEmpty)
    }

    func testWriteToolIsAllAdded() {
        let call = ToolCall(itemId: "c", name: "Write", kind: .write, title: "Write a.txt", input: .object(["file_path": .string("/a.txt"), "content": .string("x\ny\n")]))
        let diff = DiffBuilder.diff(forTool: call)
        XCTAssertEqual(diff?.hunks.first?.lines, ["+x", "+y"])
        XCTAssertEqual(diff.map(DiffBuilder.stats)?.added, 2)
    }

    func testMultiEdit() {
        let call = ToolCall(itemId: "c", name: "MultiEdit", kind: .edit, title: "MultiEdit", input: .object([
            "file_path": .string("/a"),
            "edits": .array([
                .object(["old_string": .string("a"), "new_string": .string("b")]),
                .object(["old_string": .string("c"), "new_string": .string("d")]),
            ]),
        ]))
        XCTAssertEqual(DiffBuilder.diff(forTool: call)?.hunks.count, 2)
    }

    func testDisplayLinesNumberAndCap() {
        let diff = EditDiff(itemId: "c", path: "/a", hunks: [DiffHunk(oldStart: 10, oldLines: 2, newStart: 10, newLines: 2, lines: [" keep", "-old", "+new"])])
        let lines = DiffBuilder.displayLines(diff)
        XCTAssertEqual(lines.map(\.kind), [.context, .removed, .added])
        XCTAssertEqual(lines[0].oldNumber, 10)
        XCTAssertEqual(lines[2].newNumber, 11)
        let capped = DiffBuilder.displayLines(diff, maxLines: 2)
        XCTAssertEqual(capped.last?.kind, .elided)
        XCTAssertEqual(capped.last?.text, "1 more lines")
    }
}

final class SnoozeTests: XCTestCase {
    var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Europe/London")!
        return calendar
    }

    func date(_ text: String) -> Date { ISO8601.parse(text)! }

    func testMorningPresets() {
        // Monday 26 Oct 2026, 10:00 London (GMT after the clocks change).
        let presets = Snooze.presets(now: date("2026-10-26T10:00:00Z"), calendar: calendar)
        XCTAssertEqual(presets.map(\.id), ["hour", "evening", "tomorrow", "monday"])
        XCTAssertEqual(presets[0].date, date("2026-10-26T11:00:00Z"))
        XCTAssertEqual(presets[1].date, date("2026-10-26T18:00:00Z"))
        XCTAssertEqual(presets[2].date, date("2026-10-27T09:00:00Z"))
        // Next Monday, not today.
        XCTAssertEqual(presets[3].date, date("2026-11-02T09:00:00Z"))
    }

    func testEveningPresetHiddenLate() {
        let presets = Snooze.presets(now: date("2026-10-30T17:30:00Z"), calendar: calendar)
        XCTAssertEqual(presets.map(\.id), ["hour", "tomorrow", "monday"])
        XCTAssertEqual(presets.last?.date, date("2026-11-02T09:00:00Z"))
    }

    func testPresetDateIsWorkedOutWhenChosen() {
        // The menu was built at 10:00; "1 hour" chosen at 11:50 means 12:50.
        XCTAssertEqual(Snooze.date(for: "hour", now: date("2026-10-26T11:50:00Z"), calendar: calendar), date("2026-10-26T12:50:00Z"))
        // "Tomorrow" chosen after midnight is the next day's 9 am.
        XCTAssertEqual(Snooze.date(for: "tomorrow", now: date("2026-10-27T00:10:00Z"), calendar: calendar), date("2026-10-28T09:00:00Z"))
        // "This evening" from a stale menu at 18:30 falls back to an hour, never the past.
        XCTAssertEqual(Snooze.date(for: "evening", now: date("2026-10-26T18:30:00Z"), calendar: calendar), date("2026-10-26T19:30:00Z"))
    }
}

final class RPCErrorTests: XCTestCase {
    func testAnUnknownMethodSaysTheRunnerIsCatchingUp() {
        let e = RPCError.server(code: RPCError.methodNotFound, message: "Unknown method threads.hibernateWhen")
        XCTAssertEqual(e.localizedDescription, "The runner hasn't caught up with this version of the app yet. Try again in a minute.")
        // Other errors keep the runner's own words.
        XCTAssertEqual(RPCError.server(code: -32000, message: "No room").localizedDescription, "No room")
    }
}

final class WakeWordsTests: XCTestCase {
    let now = ISO8601.parse("2026-10-29T06:00:00Z")!
    let never = ISO8601.parse("9999-12-31T00:00:00.000Z")!

    func testTopicAloneShowsTheTopicAndNoTime() {
        let t = ThreadSummary(id: "a", title: "Drawers", snoozedUntil: never, wakeWhen: WakeWhen(said: "next time I talk about woodworking", topic: "woodworking"))
        XCTAssertNil(WakeWords.time(t))
        XCTAssertEqual(WakeWords.row(t, now: now)?.text, "woodworking")
        XCTAssertEqual(WakeWords.row(t, now: now)?.tip, "Back when you next talk about woodworking. You said \"next time I talk about woodworking\".")
        XCTAssertEqual(ThreadRefs.status(t, now: now), "Hibernating until you talk about woodworking")
    }

    func testTopicWithALimitAndPending() {
        let limit = now.addingTimeInterval(30 * 86_400)
        let t = ThreadSummary(id: "a", title: "Drawers", snoozedUntil: limit, wakeWhen: WakeWhen(said: "woodworking or a month", topic: "woodworking"))
        XCTAssertEqual(WakeWords.time(t), limit)
        XCTAssertTrue(ThreadRefs.status(t, now: now).hasPrefix("Hibernating until you talk about woodworking, or "))
        let pending = ThreadSummary(id: "b", title: "x", snoozedUntil: never, wakeWhen: WakeWhen(said: "at the weekend", pending: true))
        XCTAssertEqual(WakeWords.row(pending, now: now)?.text, "Thinking…")
        // A plain time in words reads as any other snooze.
        let plain = ThreadSummary(id: "c", title: "x", snoozedUntil: limit, wakeWhen: WakeWhen(said: "in a month"))
        XCTAssertNil(WakeWords.row(plain, now: now))
        XCTAssertNil(WakeWords.status(plain, now: now))
    }

    func testWaitingOnThreadsNamesThemWithTheLatestTime() throws {
        let limit = now.addingTimeInterval(86_400)
        let one = ThreadSummary(id: "a", title: "Plan", snoozedUntil: limit, waitingOn: [WaitingOn(threadId: "b", title: "Convex I/O")])
        XCTAssertEqual(WakeWords.row(one, now: now)?.text, "Waiting on \"Convex I/O\"")
        XCTAssertTrue(WakeWords.row(one, now: now)!.tip.hasPrefix("Back when \"Convex I/O\" finishes, or "))
        XCTAssertTrue(ThreadRefs.status(one, now: now).hasPrefix("Hibernating until \"Convex I/O\" finishes, or "))
        // Done ones drop out of the name; two left read as one and 1 more.
        let three = ThreadSummary(id: "a", title: "Plan", snoozedUntil: limit, waitingOn: [
            WaitingOn(threadId: "b", title: "Convex I/O", done: true), WaitingOn(threadId: "c", title: "Website"), WaitingOn(threadId: "d", title: "Docs"),
        ])
        XCTAssertEqual(WakeWords.row(three, now: now)?.text, "Waiting on \"Website\" and 1 more")
        // A condition reads on after the name.
        let until = ThreadSummary(id: "a", title: "Plan", snoozedUntil: limit, waitingOn: [WaitingOn(threadId: "b", title: "Convex I/O", until: "reports the restart saving")])
        XCTAssertTrue(ThreadRefs.status(until, now: now).hasPrefix("Hibernating until \"Convex I/O\" reports the restart saving, or "))
        let json = #"{"id":"a","title":"x","cwd":"/","accountId":"c","provider":"claude","permissionMode":"full","status":"idle","live":false,"needsYou":false,"createdAt":"2026-10-29T06:00:00Z","updatedAt":"2026-10-29T06:00:00Z","lastActivityAt":"2026-10-29T06:00:00Z","snoozedUntil":"2026-10-30T06:00:00Z","waitingOn":[{"threadId":"b","title":"Convex I/O","done":true},{"threadId":"c","title":"Website"}],"preview":"","lastSeq":1,"lastSeenSeq":1}"#
        let t = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        XCTAssertEqual(t.waitingOn, [WaitingOn(threadId: "b", title: "Convex I/O", done: true), WaitingOn(threadId: "c", title: "Website")])
        XCTAssertEqual(try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(t)).waitingOn, t.waitingOn)
    }

    func testDecodesFromTheRunner() throws {
        let json = #"{"id":"a","title":"x","cwd":"/","accountId":"c","provider":"claude","permissionMode":"full","status":"idle","live":false,"needsYou":false,"createdAt":"2026-10-29T06:00:00Z","updatedAt":"2026-10-29T06:00:00Z","lastActivityAt":"2026-10-29T06:00:00Z","snoozedUntil":"9999-12-31T00:00:00.000Z","wakeWhen":{"said":"soon","pending":true},"preview":"","lastSeq":1,"lastSeenSeq":1}"#
        let t = try JSONDecoder().decode(ThreadSummary.self, from: Data(json.utf8))
        XCTAssertEqual(t.wakeWhen, WakeWhen(said: "soon", pending: true))
        let back = try JSONDecoder().decode(ThreadSummary.self, from: JSONEncoder().encode(t))
        XCTAssertEqual(back.wakeWhen, t.wakeWhen)
    }
}

final class ThreadSectionsTests: XCTestCase {
    func thread(_ id: String, status: ThreadStatus = .idle, needsYou: Bool = false, minutesAgo: Double, snoozed: Date? = nil, archived: Date? = nil, background: [BackgroundTask] = []) -> ThreadSummary {
        let now = Date(timeIntervalSince1970: 1_000_000)
        return ThreadSummary(id: id, title: id, status: status, needsYou: needsYou, lastActivityAt: now.addingTimeInterval(-minutesAgo * 60), snoozedUntil: snoozed, archivedAt: archived, background: background)
    }

    /// A thread whose turn ended with sub-agents or jobs still going is
    /// still being worked on, so it sits under Running, unread reply and
    /// all; a question from it still needs Mike first.
    func testBackgroundWorkCountsAsRunning() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let cubs = [BackgroundTask(kind: .agent, description: "a")]
        let threads = [
            thread("bg", minutesAgo: 3, background: cubs),
            thread("bg-unread", needsYou: true, minutesAgo: 2, background: cubs),
            thread("bg-waiting", status: .waiting, needsYou: true, minutesAgo: 1, background: cubs),
            thread("running", status: .running, minutesAgo: 4),
        ]
        let sections = ThreadSections.build(threads, now: now)
        XCTAssertEqual(sections.inbox.map(\.id), ["bg-waiting"])
        XCTAssertEqual(sections.running.map(\.id), ["bg-unread", "bg", "running"])
    }

    /// A turn that ended asking Mike for something is waiting on him, even
    /// with a job left watching for him to do it, and even once he's read
    /// it. Ticking every ask hands it back to Running. A turn underway is
    /// still Running, and so is a reply that asked for nothing.
    func testAnOpenAskBeatsBackgroundWork() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let job = [BackgroundTask(kind: .shell, description: "Watch for the Pi")]
        func asking(_ id: String, ticked: Bool = false) -> ThreadSummary {
            var t = thread(id, minutesAgo: 1, background: job)
            t.needs = "you"
            t.asksSeq = 5
            t.asks = [Ask(action: "Put the card back", ticked: ticked)]
            return t
        }
        var read = asking("read")
        read.needsYou = false
        var unread = asking("unread")
        unread.needsYou = true
        var noCard = thread("no-card", minutesAgo: 1, background: job)
        noCard.needs = "you"
        var said = thread("said-nothing", minutesAgo: 1, background: job)
        said.needs = "nothing"
        var underway = asking("underway")
        underway.status = .running
        let sections = ThreadSections.build([read, unread, noCard, asking("ticked", ticked: true), said, underway], now: now)
        XCTAssertEqual(Set(sections.inbox.map(\.id)), ["read", "unread", "no-card"])
        XCTAssertEqual(Set(sections.running.map(\.id)), ["ticked", "said-nothing", "underway"])
        XCTAssertEqual(sections.needsYou.map(\.id), ["unread"])
    }

    /// Sub-agents are the exception to an open ask keeping its place: what
    /// they find can change what the thread asks, so it's Running until
    /// they're done, finished-but-unread ones and workflows included. A
    /// question the agent is stuck on still goes straight to the inbox.
    func testAnOpenAskWaitsForSubagents() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        func asking(_ id: String, _ background: [BackgroundTask]) -> ThreadSummary {
            var t = thread(id, minutesAgo: 1, background: background)
            t.needs = "you"
            t.asksSeq = 5
            t.asks = [Ask(action: "Pick a format")]
            return t
        }
        let agents = asking("agents", [BackgroundTask(kind: .agent, description: "Sweep"), BackgroundTask(kind: .shell, description: "Watch")])
        let finished = asking("finished", [BackgroundTask(kind: .agent, description: "Sweep", finished: true)])
        let workflow = asking("workflow", [BackgroundTask(kind: .other, description: "Review")])
        var question = asking("question", [BackgroundTask(kind: .agent, description: "Sweep")])
        question.status = .waiting
        let sections = ThreadSections.build([agents, finished, workflow, question, asking("job", [BackgroundTask(kind: .shell, description: "Watch")])], now: now)
        XCTAssertEqual(Set(sections.running.map(\.id)), ["agents", "finished", "workflow"])
        XCTAssertEqual(Set(sections.inbox.map(\.id)), ["question", "job"])
    }

    /// One inbox, ordered by what needs Mike most: questions, then unread
    /// failures, unread replies, read failures, then the rest, newest first
    /// within each. Running threads are their own section.
    func testSections() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let threads = [
            thread("idle-old", minutesAgo: 50),
            thread("idle-new", minutesAgo: 5),
            thread("waiting-old", status: .waiting, minutesAgo: 30),
            thread("waiting-new", status: .waiting, needsYou: true, minutesAgo: 20),
            thread("unread", needsYou: true, minutesAgo: 1),
            thread("unread-failed", status: .error, needsYou: true, minutesAgo: 40),
            thread("read-stopped", status: .stopped, minutesAgo: 45),
            thread("running", status: .running, minutesAgo: 2),
            thread("snoozed", minutesAgo: 1, snoozed: now.addingTimeInterval(60)),
            thread("woke", minutesAgo: 1, snoozed: now.addingTimeInterval(-60)),
            thread("archived", minutesAgo: 1, archived: now),
        ]
        let sections = ThreadSections.build(threads, now: now)
        XCTAssertEqual(sections.inbox.map(\.id), ["waiting-new", "waiting-old", "unread-failed", "unread", "read-stopped", "woke", "idle-new", "idle-old"])
        XCTAssertEqual(sections.running.map(\.id), ["running"])
        XCTAssertEqual(sections.needsYou.map(\.id), ["waiting-new", "waiting-old", "unread-failed", "unread"])
        XCTAssertEqual(ThreadSections.snoozed(threads, now: now).map(\.id), ["snoozed"])
        // Put-away threads have sections of their own at the foot, and
        // aren't among the live ones.
        XCTAssertEqual(sections.hibernating.map(\.id), ["snoozed"])
        XCTAssertEqual(sections.archived.map(\.id), ["archived"])
        XCTAssertEqual(sections[.archived].map(\.id), ["archived"])
        XCTAssertFalse(sections.all.contains { $0.id == "snoozed" || $0.id == "archived" })
    }

    /// Hibernating soonest to wake first; Archive most recently archived
    /// first, whatever state each thread was in when put away.
    func testPutAwaySectionsOrder() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let threads = [
            thread("wakes-late", minutesAgo: 1, snoozed: now.addingTimeInterval(7200)),
            thread("wakes-soon", status: .waiting, needsYou: true, minutesAgo: 1, snoozed: now.addingTimeInterval(60)),
            thread("archived-old", status: .running, minutesAgo: 1, archived: now.addingTimeInterval(-600)),
            thread("archived-new", needsYou: true, minutesAgo: 1, archived: now),
        ]
        let sections = ThreadSections.build(threads, now: now)
        XCTAssertEqual(sections.hibernating.map(\.id), ["wakes-soon", "wakes-late"])
        XCTAssertEqual(sections.archived.map(\.id), ["archived-new", "archived-old"])
        XCTAssertTrue(sections.inbox.isEmpty)
        XCTAssertTrue(sections.running.isEmpty)
        XCTAssertTrue(InboxSection.hibernating.isPutAway && InboxSection.archived.isPutAway)
        XCTAssertFalse(InboxSection.running.isPutAway)
    }

    /// The inbox's sort menu: newest or oldest first go by last activity
    /// alone, so a question doesn't jump the queue (Mike asked 29 Oct, to
    /// work through a long inbox from the oldest). Running is unchanged.
    func testInboxOrder() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let threads = [
            thread("idle-old", minutesAgo: 50),
            thread("question", status: .waiting, needsYou: true, minutesAgo: 20),
            thread("unread", needsYou: true, minutesAgo: 1),
            thread("idle-new", minutesAgo: 5),
            thread("running-old", status: .running, minutesAgo: 30),
            thread("running-new", status: .running, minutesAgo: 2),
        ]
        let newest = ThreadSections.build(threads, now: now, order: .newest)
        XCTAssertEqual(newest.inbox.map(\.id), ["unread", "idle-new", "question", "idle-old"])
        let oldest = ThreadSections.build(threads, now: now, order: .oldest)
        XCTAssertEqual(oldest.inbox.map(\.id), ["idle-old", "question", "idle-new", "unread"])
        XCTAssertEqual(oldest.running.map(\.id), ["running-new", "running-old"])
        XCTAssertEqual(ThreadSections.build(threads, now: now).inbox.map(\.id), ["question", "unread", "idle-new", "idle-old"])
    }

    /// A thread that has booked itself a message for later, and has nothing
    /// for Mike, waits in its own section at the bottom, soonest first,
    /// rather than sitting in the inbox (Mike asked 29 Oct). Anything it
    /// still wants from him keeps it in the inbox.
    func testStarredThreadsStayAtTheTop() {
        let now = Date()
        func starred(_ id: String, _ minutesAgo: Double, status: ThreadStatus = .idle, needsYou: Bool = false, snoozed: Bool = false, archived: Bool = false) -> ThreadSummary {
            ThreadSummary(id: id, title: id, status: status, needsYou: needsYou, snoozedUntil: snoozed ? now.addingTimeInterval(600) : nil,
                          archivedAt: archived ? now : nil, starredAt: now.addingTimeInterval(-minutesAgo * 60))
        }
        let sections = ThreadSections.build([
            starred("second", 10),
            starred("asleep", 20, needsYou: true, snoozed: true),
            starred("first", 30, status: .running),
            starred("asks", 5, needsYou: true),
            starred("archived", 40, archived: true),
            ThreadSummary(id: "plain", title: "plain"),
        ], now: now)
        XCTAssertEqual(sections.starred.map(\.id), ["first", "asleep", "second", "asks"], "in the order they were starred, whatever they're doing")
        XCTAssertEqual(sections.inbox.map(\.id), ["plain"])
        XCTAssertTrue(sections.running.isEmpty)
        XCTAssertTrue(sections.hibernating.isEmpty)
        XCTAssertEqual(sections.archived.map(\.id), ["archived"], "archiving still puts one away")
        XCTAssertEqual(sections.needsYou.map(\.id), ["asks"], "a star doesn't hide it from the badge, but hibernating does")
        XCTAssertEqual(sections.all.first?.id, "first", "the arrow keys and archive's next thread start at the top")
        XCTAssertEqual(sections.matching("asks").starred.map(\.id), ["asks"])

        var thread = ThreadSummary(id: "t", title: "t")
        thread = ThreadUpdate(starred: true).applied(to: thread, now: now)
        XCTAssertEqual(thread.starredAt, now)
        XCTAssertEqual(ThreadUpdate(starred: true).applied(to: thread, now: now.addingTimeInterval(9)).starredAt, now, "starred again keeps its place")
        XCTAssertNil(ThreadUpdate(starred: false).applied(to: thread).starredAt)
        XCTAssertEqual(ThreadUpdate(starred: true).params["starred"], .bool(true))
    }

    func testBookedThreadsWait() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        var said = thread("said-done-unread", needsYou: false, minutesAgo: 1)
        said.needs = "nothing"
        var asked = thread("asked-read", minutesAgo: 2)
        asked.needs = "you"
        // Hibernated by Mike: it sits among the scheduled ones by when it wakes.
        var snoozed = thread("snoozed", minutesAgo: 10)
        snoozed.snoozedUntil = now.addingTimeInterval(300)
        let threads = [
            thread("read", minutesAgo: 5),
            snoozed,
            said,
            asked,
            thread("unread", needsYou: true, minutesAgo: 3),
            thread("question", status: .waiting, needsYou: true, minutesAgo: 4),
            thread("failed", status: .error, minutesAgo: 6),
            thread("running", status: .running, minutesAgo: 7),
            thread("no-wake", minutesAgo: 8),
            thread("woken", minutesAgo: 9),
        ]
        let schedules = [
            Schedule(id: "1", title: "a", threadId: "read", nextRunAt: now.addingTimeInterval(600)),
            Schedule(id: "2", title: "b", threadId: "said-done-unread", nextRunAt: now.addingTimeInterval(120)),
            // A later one for the same thread: the soonest counts.
            Schedule(id: "3", title: "c", cron: "0 9 * * *", threadId: "said-done-unread", nextRunAt: now.addingTimeInterval(9000)),
            Schedule(id: "4", title: "d", threadId: "asked-read", nextRunAt: now.addingTimeInterval(60)),
            Schedule(id: "5", title: "e", threadId: "unread", nextRunAt: now.addingTimeInterval(60)),
            Schedule(id: "6", title: "f", threadId: "question", nextRunAt: now.addingTimeInterval(60)),
            Schedule(id: "7", title: "g", threadId: "failed", nextRunAt: now.addingTimeInterval(60)),
            Schedule(id: "8", title: "h", threadId: "running", nextRunAt: now.addingTimeInterval(60)),
            Schedule(id: "9", title: "i", threadId: "no-wake", paused: true, nextRunAt: now.addingTimeInterval(60)),
            Schedule(id: "10", title: "j", threadId: "woken", nextRunAt: now.addingTimeInterval(60), finishedAt: now),
            // Starts new threads: nobody waits on it.
            Schedule(id: "11", title: "k", nextRunAt: now.addingTimeInterval(30)),
        ]
        let wakes = ThreadSections.wakes(schedules)
        XCTAssertEqual(wakes["said-done-unread"]?.id, "2")
        XCTAssertNil(wakes["no-wake"])
        XCTAssertNil(wakes["woken"])
        let sections = ThreadSections.build(threads, now: now, wakes: wakes)
        // Waiting on a schedule is hibernating too (Mike merged the two, 3 Nov).
        XCTAssertEqual(sections.hibernating.map(\.id), ["said-done-unread", "snoozed", "read"])
        XCTAssertEqual(Set(sections.inbox.map(\.id)), ["asked-read", "unread", "question", "failed", "no-wake", "woken"])
        XCTAssertEqual(sections.running.map(\.id), ["running"])
        XCTAssertEqual(InboxSection.allCases.map(\.title), ["Starred", "Inbox", "Running", "Hibernating", "Archived"])
    }

    func testPreviewTextDropsMarkdown() {
        XCTAssertEqual(PreviewText.plain("## 0.4 - **Uploads resume** after the app quits"), "0.4 - Uploads resume after the app quits")
        XCTAssertEqual(PreviewText.plain("Done. The tabs now read **General**, `Accounts` and [Usage](https://x.y)."), "Done. The tabs now read General, Accounts and Usage.")
        XCTAssertEqual(PreviewText.plain("Plain files win here: 1. Events are append-only. > quoted ### Next"), "Plain files win here: 1. Events are append-only. quoted Next")
        XCTAssertEqual(PreviewText.plain("C# and a#b stay"), "C# and a#b stay")
    }

    // Highlights' Needs you list, notifications and a closed card show an
    // ask as plain text, so a link reads as its words (30 Oct).
    func testAskPlainActionShowsLinkWords() {
        let ask = Ask(action: "Look at [the review](review:review_account) and pick **one**", where_: "In [Highlights](thread:abc)")
        XCTAssertEqual(ask.plainAction, "Look at the review and pick one")
        XCTAssertEqual(ask.plainWhere, "In Highlights")
        XCTAssertNil(Ask(action: "x").plainWhere)
    }

    func testSearchFindsThreadsInEverySection() {
        let now = Date()
        var archivedOld = thread("archived-old", minutesAgo: 1, archived: Date(timeIntervalSince1970: 10))
        archivedOld.title = "Résumé builder"
        var archivedNew = thread("archived-new", minutesAgo: 1, archived: Date(timeIntervalSince1970: 20))
        archivedNew.preview = "Fixed the resume upload"
        var inbox = thread("inbox", minutesAgo: 1)
        inbox.title = "Resume the upload"
        var running = thread("running", status: .running, minutesAgo: 1)
        running.preview = "It'll resume where it stopped"
        var hibernating = thread("hibernating", minutesAgo: 1, snoozed: now.addingTimeInterval(3600))
        hibernating.title = "Resume after lunch"
        var other = thread("other", minutesAgo: 1)
        other.title = "Something else"
        let sections = ThreadSections.build([archivedOld, archivedNew, inbox, running, hibernating, other], now: now)

        let found = sections.matching("  RESUME ")
        XCTAssertEqual(found.inbox.map(\.id), ["inbox"])
        XCTAssertEqual(found.running.map(\.id), ["running"])
        XCTAssertEqual(found.hibernating.map(\.id), ["hibernating"])
        // Still most recently archived first.
        XCTAssertEqual(found.archived.map(\.id), ["archived-new", "archived-old"])
        XCTAssertFalse(found.isEmpty)

        XCTAssertEqual(sections.matching(""), sections)
        XCTAssertEqual(sections.matching("upload").inbox.map(\.id), ["inbox"])
        XCTAssertTrue(sections.matching("nothing like it").isEmpty)
    }

    func testRelativeTime() {
        let now = ISO8601.parse("2026-10-26T12:00:00Z")!
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-10), now: now), "now")
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-300), now: now), "5m")
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-7200), now: now), "2h")
        XCTAssertEqual(RelativeTime.duration(milliseconds: 850), "850 ms")
        XCTAssertEqual(RelativeTime.duration(milliseconds: 72_000), "1m 12s")
    }

    /// Aliases from the model menu read as words; full model ids as typed.
    func testModelName() {
        XCTAssertEqual(ModelName.display("haiku"), "Haiku")
        XCTAssertEqual(ModelName.display("claude-haiku-5-5"), "claude-haiku-5-5")
        XCTAssertEqual(ModelName.display("opus[1m]"), "opus[1m]")
    }

    /// A small model's turn costs well under a cent; "$0.00" read as free.
    func testCostText() {
        XCTAssertEqual(CostText.format(1.234), "$1.23")
        XCTAssertEqual(CostText.format(0.05), "$0.05")
        XCTAssertEqual(CostText.format(0.0044), "$0.004")
        XCTAssertEqual(CostText.format(0.0004), "<$0.001")
        XCTAssertNil(CostText.format(0))
        XCTAssertNil(CostText.format(nil))
    }

    func testTurnTokens() {
        let claude = TurnTokens(usage: .object(["inputTokens": .number(16), "outputTokens": .number(1661),
                                                "cacheReadTokens": .number(337_191), "cacheWriteTokens": .number(44033)]))
        XCTAssertEqual(claude?.read, 381_240)
        XCTAssertEqual(claude?.tip(costUsd: 0.453),
                       "1.7k tokens out, 381k in (337k cache reads, 44k cache writes, 16 uncached). $0.45 at API prices, not what a subscription charges.")
        // Codex's input includes its cached part; no cost, no dollars.
        let codex = TurnTokens(usage: .object(["inputTokens": .number(24177), "cachedInputTokens": .number(12032), "outputTokens": .number(310)]))
        XCTAssertEqual(codex, TurnTokens(input: 12145, output: 310, cacheRead: 12032))
        XCTAssertEqual(codex?.tip(costUsd: nil), "310 tokens out, 24k in (12k cache reads, 12k uncached).")
        // What a merged footer stores reads back the same.
        let sum = claude! + codex!
        XCTAssertEqual(TurnTokens(usage: sum.usage), sum)
        XCTAssertNil(TurnTokens(usage: nil))
        XCTAssertNil(TurnTokens(usage: .object(["inputTokens": .number(0), "outputTokens": .number(0)])))
    }
}

final class ImageSizeTests: XCTestCase {
    func testPNG() {
        var bytes: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]
        bytes += [0, 0, 0x05, 0x00, 0, 0, 0x02, 0xD0, 8, 6, 0, 0, 0]
        let size = ImageSize.read(data: Data(bytes))
        XCTAssertEqual(size?.width, 1280)
        XCTAssertEqual(size?.height, 720)
    }

    func testGIF() {
        let bytes: [UInt8] = Array("GIF89a".utf8) + [0x40, 0x01, 0xF0, 0x00, 0, 0, 0]
        let size = ImageSize.read(data: Data(bytes))
        XCTAssertEqual(size?.width, 320)
        XCTAssertEqual(size?.height, 240)
    }

    func testJPEGSkipsAPPSegments() {
        var bytes: [UInt8] = [0xFF, 0xD8]
        bytes += [0xFF, 0xE0, 0x00, 0x06, 1, 2, 3, 4] // APP0, length 6
        bytes += [0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80, 0x03] // SOF0 480 x 640
        bytes += Array(repeating: 0, count: 16)
        let size = ImageSize.read(data: Data(bytes))
        XCTAssertEqual(size?.width, 640)
        XCTAssertEqual(size?.height, 480)
    }

    func testWebPVP8X() {
        var bytes: [UInt8] = Array("RIFF".utf8) + [0, 0, 0, 0] + Array("WEBPVP8X".utf8) + [10, 0, 0, 0, 0, 0, 0, 0]
        bytes += [0x1F, 0x03, 0x00, 0x57, 0x02, 0x00] // 800 x 600 minus one
        bytes += [0, 0]
        let size = ImageSize.read(data: Data(bytes))
        XCTAssertEqual(size?.width, 800)
        XCTAssertEqual(size?.height, 600)
    }

    func testNotAnImage() {
        XCTAssertNil(ImageSize.read(data: Data("hello world, not an image".utf8)))
    }
}

final class MarkdownExportTests: XCTestCase {
    func testTranscriptExport() {
        let model = TranscriptModel(events: [
            E.user(1, "Run the tests"),
            E.call(2, "c1", title: "npm test"),
            E.result(3, "c1", text: "ok ```weird```"),
            E.text(4, "m1", "All **green**."),
            E.done(5),
        ])
        let markdown = MarkdownExport.transcript(title: "Tests", rows: model.items)
        XCTAssertTrue(markdown.hasPrefix("# Tests\n\n## You\n\nRun the tests\n\n"))
        XCTAssertTrue(markdown.contains("**Bash**: `npm test`\n\n````\nok ```weird```\n````"))
        XCTAssertTrue(markdown.contains("## Claude\n\nAll **green**."))
        XCTAssertTrue(markdown.contains("_4s, $0.12_"))
        XCTAssertEqual(MarkdownExport.message(for: model.items[2]), "All **green**.")
    }
}

final class RunnerConfigTests: XCTestCase {
    func testEndpointFromDataDir() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir.appendingPathComponent("secrets"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        XCTAssertThrowsError(try RunnerConfig.endpoint(dataDir: dir))
        try "test-token\n".write(to: dir.appendingPathComponent("secrets/runner-token"), atomically: true, encoding: .utf8)
        XCTAssertEqual(try RunnerConfig.endpoint(dataDir: dir), RunnerEndpoint(url: URL(string: "ws://127.0.0.1:8797/rpc")!, token: "test-token"))
        try #"{"port": 9123, "other": 1}"#.write(to: dir.appendingPathComponent("runner.json"), atomically: true, encoding: .utf8)
        XCTAssertEqual(try RunnerConfig.endpoint(dataDir: dir).url.absoluteString, "ws://127.0.0.1:9123/rpc")
    }

    func testDataDirOverrides() {
        XCTAssertEqual(RunnerConfig.dataDir(arguments: ["app", "--data-dir", "/tmp/x"], environment: [:]).path, "/tmp/x")
        XCTAssertEqual(RunnerConfig.dataDir(arguments: ["app"], environment: ["THREADBEAR_DATA_DIR": "/tmp/y"]).path, "/tmp/y")
        XCTAssertTrue(RunnerConfig.dataDir(arguments: ["app"], environment: [:]).path.hasSuffix("Library/Application Support/Threadbear"))
    }

    func testBackoff() {
        var backoff = Backoff()
        XCTAssertEqual((0..<8).map { _ in backoff.next() }, [0.25, 0.5, 1, 2, 4, 8, 10, 10])
        backoff.reset()
        XCTAssertEqual(backoff.next(), 0.25)
    }
}
