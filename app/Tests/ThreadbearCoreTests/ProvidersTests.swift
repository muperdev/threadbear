import XCTest
@testable import ThreadbearCore

final class ProvidersTests: XCTestCase {
    func testClaudeAndCodexWithNoHelloAtAll() {
        XCTAssertEqual(Providers.info("claude").name, "Claude")
        XCTAssertEqual(Providers.info("codex").cliName, "Codex")
        XCTAssertEqual(Providers.info("").name, "Claude", "an empty id is a Claude thread from before Codex")
        XCTAssertEqual(Providers.info("claude").autoMode, "classifier")
        XCTAssertEqual(Providers.info("codex").autoMode, "asks")
    }

    func testAProviderNothingHasDescribedCanDoNothing() {
        let info = Providers.info("mystery-agent")
        XCTAssertEqual(info.name, "Mystery-agent")
        XCTAssertFalse(info.rewind)
        XCTAssertFalse(info.smallJobs)
        XCTAssertEqual(info.effort, "none")
        XCTAssertNil(Providers.known("mystery-agent"))
    }

    func testHelloDecodesProvidersLeniently() throws {
        let json = #"""
        {"runnerVersion":"1","protocol":1,"accounts":[],"defaults":{"cwd":"/","accountId":"x"},
         "providers":[{"id":"opencode","name":"OpenCode","cliName":"OpenCode","loginCommand":"opencode auth login","effort":"none","autoMode":"asks","sandbox":false,"rewind":false,"smallJobs":true,"usageReadings":false},
                      {"id":"thin"},{"nope":1}]}
        """#
        let hello = try JSONDecoder().decode(HelloResult.self, from: Data(json.utf8))
        XCTAssertEqual(hello.providers.map(\.id), ["opencode", "thin"], "an odd row is dropped, not fatal")
        XCTAssertEqual(hello.providers[1].name, "thin")
        Providers.update(hello.providers)
        XCTAssertEqual(Providers.info("opencode").loginCommand, "opencode auth login")
        XCTAssertEqual(ModelPicker.providerTitle("opencode"), "OpenCode")
        XCTAssertEqual(CliText.name("opencode"), "OpenCode")
        XCTAssertEqual(ContextMeter.compactNote(provider: "opencode"), "OpenCode compacts it by itself when it's nearly full.")
    }

    func testAnOlderRunnerSendsNone() throws {
        let hello = try JSONDecoder().decode(HelloResult.self, from: Data(#"{"runnerVersion":"1","protocol":1,"accounts":[],"defaults":{"cwd":"/","accountId":"x"}}"#.utf8))
        XCTAssertEqual(hello.providers, [])
    }

    func testEffortLevelsFollowTheProvider() {
        Providers.update([ProviderInfo(id: "noeffort", name: "N", cliName: "N")])
        XCTAssertEqual(ComposerChips.effortLevels(provider: "noeffort"), [])
        XCTAssertEqual(ModelPicker.effortLevels(for: "x", in: [], provider: "noeffort"), [])
        XCTAssertTrue(ComposerChips.effortLevels(provider: "codex").contains("ultra"))
        XCTAssertFalse(ComposerChips.effortLevels(provider: "claude").contains("ultra"))
    }

    func testModelRowsForAnAgentDoNotInventClaudesAliases() {
        Providers.update([ProviderInfo(id: "noeffort", name: "N", cliName: "N")])
        let rows = ModelPicker.rows(choices: [], defaultId: nil, selected: nil, provider: "noeffort")
        XCTAssertTrue(rows.isEmpty, "opus, sonnet and haiku aren't this agent's")
    }
}
