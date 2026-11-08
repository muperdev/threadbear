import XCTest
@testable import ThreadbearCore

final class ComposerChipsTests: XCTestCase {
    /// "+" > Folder marks the default; a trailing slash or a ~ doesn't
    /// make a folder differ from it.
    func testSameFolder() {
        let misc = "/Users/mike/dev/me/misc"
        XCTAssertTrue(ComposerChips.sameFolder(misc, misc))
        XCTAssertTrue(ComposerChips.sameFolder(misc + "/", misc))
        XCTAssertTrue(ComposerChips.sameFolder(misc, misc + "//"))
        XCTAssertFalse(ComposerChips.sameFolder("/Users/mike/dev/photo-sync", misc))
        XCTAssertFalse(ComposerChips.sameFolder("/Users/mike/dev/me", misc))
        XCTAssertTrue(ComposerChips.sameFolder("~/dev/me/misc", NSHomeDirectory() + "/dev/me/misc"))
        // Before the runner says what its default is, nothing is it.
        XCTAssertFalse(ComposerChips.sameFolder(misc, ""))
        XCTAssertFalse(ComposerChips.sameFolder("", ""))
        XCTAssertTrue(ComposerChips.sameFolder("/", "/"))
    }

    /// No model is the account's default; aliases and Claude ids read as
    /// words; anything unexpected stays as typed.
    func testModelChip() {
        XCTAssertEqual(ComposerChips.model(nil), "Default")
        XCTAssertEqual(ComposerChips.model(""), "Default")
        XCTAssertEqual(ComposerChips.model("opus"), "Opus")
        XCTAssertEqual(ComposerChips.model("claude-opus-5-5"), "Opus 5.5")
        XCTAssertEqual(ComposerChips.model("claude-haiku-4-5-20251001"), "Haiku 4.5")
        XCTAssertEqual(ComposerChips.model("claude-sonnet-4"), "Sonnet 4")
        XCTAssertEqual(ComposerChips.model("claude-3-5-sonnet-20241022"), "claude-3-5-sonnet-20241022")
        XCTAssertEqual(ComposerChips.model("opus[1m]"), "opus[1m]")
        XCTAssertEqual(ComposerChips.model("gpt-6"), "GPT-6")
        XCTAssertEqual(ComposerChips.model("gpt-6.1-sol"), "GPT-6.1-Sol", "Codex ids read as Codex titles them")
        XCTAssertEqual(ComposerChips.model("gpt-5.6-terra"), "GPT-5.6-Terra")
    }

    /// The chip reads the CLI's own title for a model once the list is
    /// known, falls back to formatting the id, and adds the effort level.
    func testChipTitleUsesTheAccountsListAndEffort() {
        let choices = [
            ModelChoice(id: "opus", model: "claude-opus-5-5", title: "Opus 5.5"),
            ModelChoice(id: "claude-fable-5-1[1m]", model: "claude-fable-5-1", title: "Fable 5.1", effortLevels: ["low", "max"]),
        ]
        XCTAssertEqual(ComposerChips.modelTitle("claude-fable-5-1", choices: choices), "Fable 5.1", "by the id it runs as")
        XCTAssertEqual(ComposerChips.modelTitle("claude-fable-5-1[1m]", choices: choices), "Fable 5.1", "by what's sent")
        XCTAssertEqual(ComposerChips.modelTitle("opus", choices: choices), "Opus 5.5")
        XCTAssertEqual(ComposerChips.modelTitle("claude-sonnet-5-5", choices: choices), "Sonnet 5.5", "not in the list: formatted")
        XCTAssertEqual(ComposerChips.modelTitle("opus", choices: []), "Opus", "no list yet: the alias as a word")
        XCTAssertEqual(ComposerChips.modelTitle(nil, choices: choices), "Default")
        XCTAssertEqual(ComposerChips.chipTitle(model: "claude-fable-5-1", effort: "xhigh", choices: choices), "Fable 5.1 · Extra high")
        XCTAssertEqual(ComposerChips.chipTitle(model: "claude-fable-5-1", effort: nil, choices: choices), "Fable 5.1")
        XCTAssertEqual(ComposerChips.chipTitle(model: nil, effort: "low", choices: choices), "Default · Low")
        XCTAssertEqual(ComposerChips.chipTitle(model: "claude-fable-5-1", effort: "high", fast: true, choices: choices), "Fable 5.1 · High · Fast")
        XCTAssertEqual(ComposerChips.chipTitle(model: "claude-fable-5-1", effort: nil, fast: true, choices: choices), "Fable 5.1 · Fast")
        XCTAssertEqual(ComposerChips.choice(for: "claude-fable-5-1", in: choices)?.effortLevels, ["low", "max"])
    }

    /// A thread's own model first, then whichever default applies.
    func testResolvedModelTakesTheFirstThatIsSet() {
        XCTAssertEqual(ComposerChips.resolvedModel("haiku", "claude-fable-5-1"), "haiku")
        XCTAssertEqual(ComposerChips.resolvedModel(nil, "", "claude-fable-5-1"), "claude-fable-5-1")
        XCTAssertNil(ComposerChips.resolvedModel(nil, nil))
        XCTAssertNil(ComposerChips.resolvedModel())
    }

    func testEffortTitles() {
        XCTAssertEqual(ComposerChips.effortLevels.map(ComposerChips.effortTitle), ["Low", "Medium", "High", "Extra high", "Max"])
        XCTAssertEqual(ComposerChips.effortTitle("turbo"), "Turbo")
        XCTAssertEqual(ComposerChips.effortTitle(""), "")
        XCTAssertEqual(ComposerChips.effortLevels.map(ComposerChips.effortPorridge), ["Too cold", "A bit cold", "Warm", "Hot", "Too hot"])
        XCTAssertNil(ComposerChips.effortPorridge("turbo"))
    }

    /// Blank text alone can't be sent; an image on its own can.
    func testCanSend() {
        XCTAssertFalse(ComposerChips.canSend(text: "", imageCount: 0))
        XCTAssertFalse(ComposerChips.canSend(text: "  \n\t", imageCount: 0))
        XCTAssertTrue(ComposerChips.canSend(text: "hi", imageCount: 0))
        XCTAssertTrue(ComposerChips.canSend(text: " ", imageCount: 1))
    }

    /// Two lines at least, ten at most, whole points in between.
    func testBoxHeightClampsToTwoToTenLines() {
        let line: CGFloat = 17
        XCTAssertEqual(ComposerChips.boxHeight(textHeight: 0, lineHeight: line), 34)
        XCTAssertEqual(ComposerChips.boxHeight(textHeight: 17, lineHeight: line), 34)
        XCTAssertEqual(ComposerChips.boxHeight(textHeight: 50.2, lineHeight: line), 51)
        XCTAssertEqual(ComposerChips.boxHeight(textHeight: 400, lineHeight: line), 170)
    }
}
