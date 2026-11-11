import Foundation

/// A ```sh, ```bash, ```zsh or ```shell block in a reply: a command the agent
/// is handing over, often one only Mike can run. The Mac app draws a play
/// icon on it that types the command into the thread's Terminal, and stops
/// short of Return, so a person always reads what's about to run (Yuriy on
/// X, 10 Nov: copying "the command only you can run on prod" by hand).
///
/// What's in a reply can come from anything the agent read, so this only
/// takes text that means what it looks like. A control character could end
/// a bracketed paste early (ESC [ 201 ~) and let the next line run by
/// itself, and invisible or bidi characters could show a different command
/// from the one typed. A block holding any of those gets no play icon.
public struct ShellBlock: Hashable, Sendable {
    public let command: String

    public init(command: String) {
        self.command = command
    }

    static let languages: Set<String> = ["sh", "bash", "zsh", "shell"]

    /// Past this it's a script, not a command to hand someone.
    public static let maxSource = 8_000

    /// The command for a fence's info string (only the first word counts),
    /// or nil for any other fence, or one that can't safely be typed.
    public static func from(language: String?, code: String) -> ShellBlock? {
        guard let word = language?.split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "{" }).first?.lowercased(),
              languages.contains(word) else { return nil }
        var source = code.replacingOccurrences(of: "\r\n", with: "\n")
        while source.hasSuffix("\n") { source.removeLast() }
        guard !source.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, source.count <= maxSource, isPlain(source) else { return nil }
        return ShellBlock(command: withoutPrompts(source))
    }

    /// Whether a block of a reply's text could hold one, so only those are
    /// checked for a closed fence.
    public static func mayHold(_ text: String) -> Bool {
        guard text.contains("```") || text.contains("~~~") else { return false }
        let lowered = text.lowercased()
        return lowered.contains("sh") // sh, bash, zsh and shell all have it.
    }

    /// Only ordinary visible text, spaces, tabs and newlines.
    private static func isPlain(_ text: String) -> Bool {
        text.unicodeScalars.allSatisfy { scalar in
            if scalar == "\n" || scalar == "\t" { return true }
            switch scalar.properties.generalCategory {
            case .control, .format, .lineSeparator, .paragraphSeparator, .privateUse, .surrogate, .unassigned: return false
            default: return true
            }
        }
    }

    /// Agents sometimes write `$ npm test`. The "$ " only goes when every
    /// line has one, so a line that really starts with `$` is left alone.
    private static func withoutPrompts(_ text: String) -> String {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        let written = lines.filter { !$0.allSatisfy(\.isWhitespace) }
        guard written.allSatisfy({ $0.hasPrefix("$ ") }) else { return text }
        return lines.map { $0.hasPrefix("$ ") ? String($0.dropFirst(2)) : String($0) }.joined(separator: "\n")
    }
}

/// What putting a command in a shell should do now, worked out from what
/// the terminal says about itself and what's known to be on the prompt.
public enum TerminalPut {
    /// What's on the shell's prompt, from the keys that have gone to it.
    /// Read from the keys and not the screen because the screen can't say:
    /// it doesn't show where the prompt ends or where the cursor is in the
    /// line, and a command edited by hand can still end the same way.
    public enum Line: Equatable, Sendable {
        case empty
        /// Exactly this, cursor at the end, nothing pressed since.
        case put(String)
        /// Someone has typed, moved the cursor or pasted.
        case edited
    }

    public enum Step: Equatable, Sendable {
        /// The shell is still starting; ask again in a moment.
        case wait
        case type
        /// The last command put here is still at the prompt, unrun, and
        /// this is a different one: it goes in place of that one, as run
        /// together they'd make a third command nobody wrote.
        case replaceLast
        /// It's already sitting at the prompt (a second click): only the
        /// keyboard needs to go there.
        case alreadyThere
        /// Something else has the terminal, or someone's own text is on
        /// the prompt. It isn't typed; it's copied instead.
        case busy
    }

    /// - shellInFront: the shell itself owns the terminal, not a program
    ///   it's running (vim, a dev server, a command still going).
    /// - bracketedPaste: the terminal is in bracketed paste mode, which zsh,
    ///   fish and modern bash switch on only while they're editing a line.
    ///   Both are needed: vim turns it on too, and a shell running a builtin
    ///   like `read` owns the terminal with it off. Off means a newline, or
    ///   simply filling `read -k`, could run something with no Return pressed,
    ///   so nothing is typed, which includes macOS's bash 3, which never
    ///   turns it on.
    /// - starting: the shell has only just been started.
    public static func step(for block: ShellBlock, shellInFront: Bool, bracketedPaste: Bool, starting: Bool, line: Line) -> Step {
        guard shellInFront, bracketedPaste else { return starting ? .wait : .busy }
        switch line {
        case .empty: return .type
        case .put(block.command): return .alreadyThere
        case .put: return .replaceLast
        case .edited: return .busy
        }
    }

    /// What's on the prompt after a key goes to the shell. `characters` is
    /// the key with no modifiers applied.
    public static func line(_ line: Line, afterKey characters: String?, control: Bool, option: Bool, shift: Bool, command: Bool) -> Line {
        // ⌘ shortcuts aren't input; ⌘V arrives as a paste.
        if command { return line }
        // "\u{3}" is the keypad's Enter.
        let isReturn = characters == "\r" || characters == "\u{3}"
        if isReturn && !control && !option && !shift { return .empty }
        if control && characters == "c" { return .empty }
        return .edited
    }

    /// Whether the screen ends with the command, ignoring whitespace, as a
    /// terminal wraps a long one and pads its lines. Only good for checking
    /// that a line just cleared has gone: it can't tell `"a b"` from `"ab"`.
    public static func endsWith(_ screen: String, _ command: String) -> Bool {
        let wanted = command.filter { !$0.isWhitespace }
        return !wanted.isEmpty && screen.filter { !$0.isWhitespace }.hasSuffix(wanted)
    }
}
