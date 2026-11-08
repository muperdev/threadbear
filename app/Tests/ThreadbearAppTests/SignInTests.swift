import XCTest
import ThreadbearCore
@testable import Threadbear

final class SignInTests: XCTestCase {
    // claude-main is the Keychain login the runner starts with no
    // CLAUDE_CONFIG_DIR, so its sign-in mustn't set one either.
    func testMainClaudeLoginSetsNoConfigDir() {
        let script = SignIn.script(Account(id: "claude-main", provider: "claude", title: "me@example.com", configDir: "/Users/x/.claude"), cliPath: "/Users/x/.local/bin/claude")
        XCTAssertTrue(script.contains("\n'/Users/x/.local/bin/claude' auth login\n"), script)
        XCTAssertFalse(script.contains("CLAUDE_CONFIG_DIR"))
    }

    func testOtherClaudeLoginUsesItsFolder() {
        let script = SignIn.script(Account(id: "claude-1", provider: "claude", title: "two", configDir: "/Users/x/Library/Application Support/Threadbear/accounts/claude-1"), cliPath: "")
        XCTAssertTrue(script.contains("CLAUDE_CONFIG_DIR='/Users/x/Library/Application Support/Threadbear/accounts/claude-1' claude auth login"), script)
    }

    func testCodexUsesCodexHomeAndQuotes() {
        let script = SignIn.script(Account(id: "codex-1", provider: "codex", title: "c", configDir: "/tmp/it's here"), cliPath: "/opt/codex")
        XCTAssertTrue(script.contains("CODEX_HOME='/tmp/it'\\''s here' '/opt/codex' login"), script)
    }

    // grok-main is ~/.grok, where grok looks with no GROK_HOME; another
    // Grok login signs in to its own folder.
    func testGrokLoginsUseGrokHome() {
        let main = SignIn.script(Account(id: "grok-main", provider: "grok", title: "Grok · grok-main", configDir: "/Users/x/.grok"), cliPath: "")
        XCTAssertTrue(main.contains("\ngrok login\n"), main)
        XCTAssertFalse(main.contains("GROK_HOME"))
        XCTAssertTrue(main.contains("Signing in to Grok for Threadbear"), main)
        let other = SignIn.script(Account(id: "grok-1", provider: "grok", title: "Grok · grok-1", configDir: "/tmp/grok-1"), cliPath: "")
        XCTAssertTrue(other.contains("GROK_HOME='/tmp/grok-1' grok login"), other)
    }

    func testScriptRunsInZsh() throws {
        let script = SignIn.script(Account(id: "codex-1", provider: "codex", title: "c", configDir: "/tmp/it's here"), cliPath: "/bin/echo")
        XCTAssertTrue(script.hasPrefix("#!/bin/zsh -l\n"))
        // zsh parses it (the quoting holds), without running anything.
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-n", "-c", script]
        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0)
    }
}
