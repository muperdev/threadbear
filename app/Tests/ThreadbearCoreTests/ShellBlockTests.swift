import XCTest
@testable import ThreadbearCore

/// ```bash blocks get a play icon that types the command into the Terminal.
final class ShellBlockTests: XCTestCase {
    func testOnlyShellFencesCount() {
        for language in ["sh", "bash", "zsh", "shell", "Bash title=x"] {
            XCTAssertEqual(ShellBlock.from(language: language, code: "ls\n")?.command, "ls", language)
        }
        for language in ["python", "bashful", "console", "", nil] as [String?] {
            XCTAssertNil(ShellBlock.from(language: language, code: "ls"), language ?? "nil")
        }
        XCTAssertNil(ShellBlock.from(language: "bash", code: " \n\n"))
    }

    func testPromptsComeOffOnlyWhenEveryLineHasOne() {
        XCTAssertEqual(ShellBlock.from(language: "sh", code: "$ a\n\n$ b\n")?.command, "a\n\nb")
        XCTAssertEqual(ShellBlock.from(language: "sh", code: "$ a\nb")?.command, "$ a\nb")
        XCTAssertEqual(ShellBlock.from(language: "sh", code: "$HOME/x")?.command, "$HOME/x")
    }

    // Each of these could make what runs differ from what's on screen, or
    // run a line without Return being pressed.
    func testRefusesWhatCantBeTypedSafely() {
        let bad = [
            "echo hi\u{1b}[201~\nrm -rf ~", // ends a bracketed paste early
            "echo\u{07}", "a\rb", "a\u{7f}b", "a\u{9b}201~",
            "rm\u{200b} x", // zero width
            "echo \u{202e}cod", // right-to-left override
            "a\u{2028}b",
        ]
        for code in bad { XCTAssertNil(ShellBlock.from(language: "bash", code: code), code.debugDescription) }
        XCTAssertNotNil(ShellBlock.from(language: "bash", code: "echo 'héllo 🐻'\n\tls"))
        XCTAssertNil(ShellBlock.from(language: "bash", code: String(repeating: "a", count: ShellBlock.maxSource + 1)))
    }

    func testMayHold() {
        XCTAssertTrue(ShellBlock.mayHold("x\n```bash\nls\n```"))
        XCTAssertFalse(ShellBlock.mayHold("no fence, just bash"))
        XCTAssertFalse(ShellBlock.mayHold("```python\nx\n```"))
    }

    private func step(_ command: String = "ls", front: Bool = true, bracketed: Bool = true, starting: Bool = false, line: TerminalPut.Line = .empty) -> TerminalPut.Step {
        TerminalPut.step(for: ShellBlock(command: command), shellInFront: front, bracketedPaste: bracketed, starting: starting, line: line)
    }

    func testTypesOnAnEmptyPrompt() {
        XCTAssertEqual(step(), .type)
        XCTAssertEqual(step("a\nb"), .type)
    }

    func testNeverTypesIntoARunningProgram() {
        // vim turns bracketed paste on too, so that alone isn't enough.
        XCTAssertEqual(step(front: false, bracketed: true), .busy)
    }

    // `read -k 11 answer; eval "$answer"` owns the terminal from inside the
    // shell, so eleven typed characters would have run with no Return.
    func testNeedsTheShellToBeEditingALine() {
        XCTAssertEqual(step(front: true, bracketed: false), .busy)
        XCTAssertEqual(step("a\nb", front: true, bracketed: false), .busy)
    }

    func testWaitsForAShellThatsStarting() {
        XCTAssertEqual(step(bracketed: false, starting: true), .wait)
        XCTAssertEqual(step(front: false, starting: true), .wait)
        XCTAssertEqual(step(starting: true), .type)
    }

    func testNeverAddsToWhatSomeoneTyped() {
        XCTAssertEqual(step(line: .edited), .busy)
    }

    func testADifferentBlockReplacesTheOneStillAtThePrompt() {
        XCTAssertEqual(step("ls", line: .put("npm test")), .replaceLast)
    }

    func testASecondClickDoesntTypeItAgain() {
        XCTAssertEqual(step("npm test", line: .put("npm test")), .alreadyThere)
    }

    func testCommandsThatDifferOnlyInSpacesAreDifferent() {
        XCTAssertEqual(step(#"echo "a b""#, line: .put(#"echo "ab""#)), .replaceLast)
    }

    private func after(_ key: String?, _ line: TerminalPut.Line = .put("ls"), control: Bool = false, option: Bool = false, shift: Bool = false, command: Bool = false) -> TerminalPut.Line {
        TerminalPut.line(line, afterKey: key, control: control, option: option, shift: shift, command: command)
    }

    func testKeysTrackWhatsOnThePrompt() {
        XCTAssertEqual(after("\r"), .empty)
        XCTAssertEqual(after("\r", .edited), .empty)
        XCTAssertEqual(after("\u{3}"), .empty, "the keypad's Enter")
        XCTAssertEqual(after("c", .edited, control: true), .empty)
        XCTAssertEqual(after("a", .empty), .edited)
        XCTAssertEqual(after("a", control: true), .edited, "Ctrl-A moves the cursor")
        XCTAssertEqual(after("\r", shift: true), .edited)
        XCTAssertEqual(after("\u{F700}"), .edited, "the up arrow brings back a command")
        XCTAssertEqual(after("j", command: true), .put("ls"), "a shortcut isn't input")
        XCTAssertEqual(after("c"), .edited, "a plain c is just a c")
    }

    func testEndsWithSeesThroughWrapping() {
        XCTAssertTrue(TerminalPut.endsWith("~ $ npx convex\n run --prod x   \n\n", "npx convex run --prod x"))
        XCTAssertFalse(TerminalPut.endsWith("~ $ ls\nfile\n~ $ ", "ls"))
    }
}
