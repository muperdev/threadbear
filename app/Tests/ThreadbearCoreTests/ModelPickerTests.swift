import XCTest
@testable import ThreadbearCore

final class ModelPickerTests: XCTestCase {
    private let efforts = ["low", "medium", "high", "xhigh", "max"]

    private var choices: [ModelChoice] {
        [
            ModelChoice(id: "opus", model: "claude-opus-5-5", title: "Opus 5.5", description: "For complex work", effortLevels: efforts),
            ModelChoice(id: "claude-fable-5-1[1m]", model: "claude-fable-5-1", title: "Fable 5.1", description: "For your toughest challenges", effortLevels: efforts),
            ModelChoice(id: "sonnet", model: "claude-sonnet-5-5", title: "Sonnet 5.5", description: "Most efficient", effortLevels: efforts),
            ModelChoice(id: "haiku", model: "claude-haiku-5-5", title: "Haiku 5.5", description: "Fastest", effortLevels: efforts),
            ModelChoice(id: "claude-haiku-4-5-20251001", model: "claude-haiku-4-5-20251001", title: "Haiku 4.5", description: "Fastest"),
            ModelChoice(id: "claude-opus-5", model: "claude-opus-5", title: "Opus 5", description: "Best for everyday", effortLevels: ["low", "high"]),
        ]
    }

    /// One column per provider, in the order the runner lists accounts,
    /// each with its accounts in that order.
    func testProvidersGroupAccountsInOrder() {
        let accounts = [
            Account(id: "claude-main", provider: "claude", title: "mike@example.com"),
            Account(id: "codex-main", provider: "codex", title: "mike@example.com"),
            Account(id: "claude-3", provider: "claude", title: "mike+3@example.com"),
        ]
        let providers = ModelPicker.providers(accounts)
        XCTAssertEqual(providers.map(\.id), ["claude", "codex"])
        XCTAssertEqual(providers.map(\.title), ["Claude", "Codex"])
        XCTAssertEqual(providers[0].accountIds, ["claude-main", "claude-3"])
        XCTAssertEqual(providers[1].accountIds, ["codex-main"])
        XCTAssertEqual(ModelPicker.providers([]).map(\.id), ["claude"], "nothing from the runner yet: still a Claude column")
        XCTAssertEqual(ModelPicker.providerTitle("gemini"), "Gemini")
    }

    /// Aliases and the default and chosen models are the main rows, each
    /// model once; dated and old ids are older; the default and the
    /// selection are marked.
    func testRowsSplitMainFromOlder() {
        let rows = ModelPicker.rows(choices: choices, defaultId: "claude-fable-5-1", selected: "opus")
        XCTAssertEqual(rows.filter { !$0.isOlder }.map(\.title), ["Opus 5.5", "Fable 5.1", "Sonnet 5.5", "Haiku 5.5"])
        XCTAssertEqual(rows.filter(\.isOlder).map(\.title), ["Haiku 4.5", "Opus 5"])
        XCTAssertEqual(rows.first(where: \.isDefault)?.title, "Fable 5.1")
        XCTAssertEqual(rows.first(where: \.isSelected)?.title, "Opus 5.5")
        XCTAssertEqual(rows.first?.subtitle, "For complex work")
    }

    /// The selection is found by the id the CLI runs it as, not only by
    /// what's sent, and a model the list doesn't know still gets a row.
    func testSelectionByEitherIdAndUnknownModels() {
        let byModel = ModelPicker.rows(choices: choices, defaultId: nil, selected: "claude-fable-5-1")
        XCTAssertEqual(byModel.first(where: \.isSelected)?.id, "claude-fable-5-1[1m]")
        let unknown = ModelPicker.rows(choices: choices, defaultId: nil, selected: "claude-sonnet-4")
        let extra = unknown.first(where: \.isSelected)
        XCTAssertEqual(extra?.title, "Sonnet 4")
        XCTAssertFalse(extra?.isOlder ?? true)
        XCTAssertEqual(unknown.count, choices.count + 1)
    }

    /// Before the runner has asked the CLI, the three aliases by name.
    func testRowsWithoutAListAreTheAliases() {
        let rows = ModelPicker.rows(choices: [], defaultId: nil, selected: "haiku")
        XCTAssertEqual(rows.map(\.title), ["Opus", "Sonnet", "Haiku"])
        XCTAssertEqual(rows.last?.isSelected, true)
        XCTAssertTrue(rows.allSatisfy { !$0.isOlder })
    }

    /// A search keeps any row that mentions it (title, id or description),
    /// older ones included and no longer set apart.
    func testSearchMatchesTitleIdAndDescriptionAcrossBothGroups() {
        let haiku = ModelPicker.rows(choices: choices, defaultId: nil, selected: nil, query: " haiku ")
        XCTAssertEqual(haiku.map(\.title), ["Haiku 5.5", "Haiku 4.5"])
        XCTAssertTrue(haiku.allSatisfy { !$0.isOlder })
        XCTAssertEqual(ModelPicker.rows(choices: choices, defaultId: nil, selected: nil, query: "toughest").map(\.title), ["Fable 5.1"])
        XCTAssertEqual(ModelPicker.rows(choices: choices, defaultId: nil, selected: nil, query: "20251001").map(\.title), ["Haiku 4.5"])
        XCTAssertEqual(ModelPicker.rows(choices: choices, defaultId: nil, selected: nil, query: "gpt"), [])
    }

    /// The levels come from the chosen model's row when the CLI gave any,
    /// else the usual five.
    /// Fast mode is offered only on a Claude model whose row says it has
    /// it, so nothing shows before the list is known.
    func testFastModeFollowsTheModel() {
        var withFast = choices
        withFast[0].fastMode = true
        XCTAssertTrue(ModelPicker.offersFastMode(model: "opus", in: withFast))
        XCTAssertTrue(ModelPicker.offersFastMode(model: "claude-opus-5-5", in: withFast), "by the full id too")
        XCTAssertFalse(ModelPicker.offersFastMode(model: "sonnet", in: withFast))
        XCTAssertFalse(ModelPicker.offersFastMode(model: "opus", in: []), "no list yet")
        XCTAssertFalse(ModelPicker.offersFastMode(model: nil, in: withFast))
        XCTAssertFalse(ModelPicker.offersFastMode(model: "opus", in: withFast, provider: "codex"))
    }

    func testEffortLevelsFollowTheModel() {
        XCTAssertEqual(ModelPicker.effortLevels(for: "claude-opus-5", in: choices), ["low", "high"])
        XCTAssertEqual(ModelPicker.effortLevels(for: "claude-haiku-4-5-20251001", in: choices), efforts, "no levels listed: the usual ones")
        XCTAssertEqual(ModelPicker.effortLevels(for: nil, in: choices), efforts)
        XCTAssertEqual(ModelPicker.effortLevels(for: "opus", in: []), efforts)
    }
}
