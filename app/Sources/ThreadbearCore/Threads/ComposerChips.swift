import Foundation

/// Labels and sizes for the composer. Its chips stay short and quiet; the
/// email and model id go in their tooltips and menus.
public enum ComposerChips {
    /// Whether two paths are the same folder, for marking the default in
    /// "+" > Folder. A trailing slash or a ~ doesn't taskpolicy -b make them differ; an
    /// empty path (the runner hasn't said its default yet) matches nothing.
    public static func sameFolder(_ a: String, _ b: String) -> Bool {
        let folder = standardised(a)
        return !folder.isEmpty && folder == standardised(b)
    }

    /// "~/x/" and "/Users/me/x" are the same folder.
    private static func standardised(_ path: String) -> String {
        var expanded = (path as NSString).expandingTildeInPath
        while expanded.count > 1, expanded.hasSuffix("/") { expanded.removeLast() }
        return expanded
    }

    /// The model chip. No model means the account's own default, whose name
    /// the app doesn't know, so it reads "Default". A menu alias reads as a
    /// word ("Opus") and a full Claude id as family and version
    /// ("claude-opus-5-5" is "Opus 5.5", with any date suffix dropped).
    /// A Codex id reads the way Codex titles it ("gpt-6.1-sol" is
    /// "GPT-6.1-Sol"), for before the account's list has loaded. Anything
    /// else is shown as it is.
    public static func model(_ model: String?) -> String {
        guard let model, !model.isEmpty else { return "Default" }
        if model.allSatisfy(\.isLetter) { return model.capitalized }
        if model.lowercased().hasPrefix("gpt-") {
            return model.split(separator: "-", omittingEmptySubsequences: false).enumerated().map { index, part in
                index == 0 ? part.uppercased() : part.prefix(1).uppercased() + part.dropFirst()
            }.joined(separator: "-")
        }
        var parts = model.split(separator: "-").map(String.init)
        guard parts.first == "claude" else { return model }
        parts.removeFirst()
        if let last = parts.last, last.count == 8, last.allSatisfy(\.isNumber) { parts.removeLast() }
        guard let family = parts.first, family.allSatisfy(\.isLetter), parts.count > 1,
              parts.dropFirst().allSatisfy({ !$0.isEmpty && $0.allSatisfy(\.isNumber) })
        else { return model }
        return family.capitalized + " " + parts.dropFirst().joined(separator: ".")
    }

    /// The first model id that's set: a thread's own choice, then the
    /// default that applies (the runner's for a new thread, then what the
    /// last turn ran on, then the account's). Nil when none is known.
    public static func resolvedModel(_ ids: String?...) -> String? {
        ids.first { !($0 ?? "").isEmpty } ?? nil
    }

    /// How a model id reads: the CLI's own title for it ("Fable 5.1")
    /// when the account's list is known, else as `model` formats it.
    public static func modelTitle(_ id: String?, choices: [ModelChoice]) -> String {
        guard let id, !id.isEmpty else { return "Default" }
        return choice(for: id, in: choices)?.title ?? model(id)
    }

    /// The picker row an id belongs to, by what's sent or what it runs as.
    public static func choice(for id: String, in choices: [ModelChoice]) -> ModelChoice? {
        choices.first { $0.id == id } ?? choices.first { $0.model == id }
    }

    /// The model chip: the model's title, the effort level after a dot
    /// when one is known, and "Fast" in fast mode ("Opus 5.5 · High · Fast").
    public static func chipTitle(model: String?, effort: String?, fast: Bool = false, choices: [ModelChoice]) -> String {
        var parts = [modelTitle(model, choices: choices)]
        if let effort, !effort.isEmpty { parts.append(effortTitle(effort)) }
        if fast { parts.append("Fast") }
        return parts.joined(separator: " · ")
    }

    /// The CLI's effort levels, lowest first.
    public static let effortLevels = ["low", "medium", "high", "xhigh", "max"]

    /// Codex's levels: Claude's plus "ultra", its multi-agent level.
    public static let codexEffortLevels = ["low", "medium", "high", "xhigh", "max", "ultra"]

    /// The levels a provider offers when the model's own list isn't known.
    /// Grok's: up to Extra High.
    public static let grokEffortLevels = ["low", "medium", "high", "xhigh"]

    /// Devin's (`thought_level` on SWE-2): Medium, High and Max.
    public static let devinEffortLevels = ["medium", "high", "max"]

    public static func effortLevels(provider: String) -> [String] {
        switch Providers.info(provider).effort {
        case "codex": codexEffortLevels
        case "grok": grokEffortLevels
        case "devin": devinEffortLevels
        case "none": []
        default: effortLevels
        }
    }

    /// Goldilocks: what the porridge is like at each level, under its name
    /// in the menu. The model's own level is "just right".
    public static func effortPorridge(_ level: String) -> String? {
        switch level {
        case "low": return "Too cold"
        case "medium": return "A bit cold"
        case "high": return "Warm"
        case "xhigh": return "Hot"
        case "max": return "Too hot"
        case "ultra": return "Scalding"
        default: return nil
        }
    }

    /// How an effort level reads in the menu and the chip.
    public static func effortTitle(_ level: String) -> String {
        switch level {
        case "xhigh": return "Extra high"
        case "max": return "Max"
        default: return level.isEmpty ? level : level.prefix(1).uppercased() + level.dropFirst()
        }
    }

    /// Whether there's anything to send: some words or at least one image.
    public static func canSend(text: String, imageCount: Int) -> Bool {
        imageCount > 0 || !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// The text box is at least two lines tall so it reads as a place to
    /// write, grows with the text, and scrolls past ten lines.
    public static let minLines: CGFloat = 2
    public static let maxLines: CGFloat = 10

    public static func boxHeight(textHeight: CGFloat, lineHeight: CGFloat) -> CGFloat {
        let low = (lineHeight * minLines).rounded(.up)
        let high = (lineHeight * maxLines).rounded(.up)
        return min(high, max(low, textHeight.rounded(.up)))
    }
}
