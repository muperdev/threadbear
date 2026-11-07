import AppKit
import Foundation
import ThreadbearCore

/// The composer's mic button: dictation through Collabute's speech-to-text.
/// Press it and talk, and the words appear in the message box where the
/// cursor was as they're heard; press it again and the last words settle.
/// The message isn't sent: it's in the box to edit or send as typed text
/// would be. Unlike Voice mode (VoiceMode), nothing is read out, and
/// there's no thread conversation, only typing by voice.
@MainActor
@Observable
final class Dictation {
    static let shared = Dictation()

    /// Settings > Dictation's switch. Off hides every mic button.
    static let enabledKey = "dictationEnabled"

    enum Phase: Equatable {
        case idle
        /// Getting a token, the microphone or the socket.
        case starting
        case listening
        /// Stopped listening, waiting for the last words.
        case finishing
        case failed(String)
    }

    private(set) var phase: Phase = .idle
    /// The composer it's writing into (its draft key: a thread's id or "new").
    private(set) var draftKey: String?
    /// The microphone's level, 0...1.
    private(set) var level: Float = 0

    @ObservationIgnored private var stream: DictationStream?
    @ObservationIgnored private weak var textView: ComposerTextView?
    /// Where the words heard so far are in the text view, and what they are,
    /// so each update replaces them. Nil once the box was changed under
    /// them: the rest then goes in at the end.
    @ObservationIgnored private var written: (range: NSRange, text: String)?
    @ObservationIgnored private var session = 0

    func isActive(in draftKey: String) -> Bool {
        guard self.draftKey == draftKey else { return false }
        switch phase {
        case .starting, .listening, .finishing: return true
        default: return false
        }
    }

    func failure(in draftKey: String) -> String? {
        guard self.draftKey == draftKey, case .failed(let message) = phase else { return nil }
        return message
    }

    /// The mic button.
    func toggle(draftKey: String, textView: ComposerTextView?) {
        if self.draftKey == draftKey {
            switch phase {
            case .listening: finish(); return
            case .starting: cancel(); return
            case .finishing: return
            default: break
            }
        }
        start(draftKey: draftKey, textView: textView)
    }

    func dismissFailure() {
        if case .failed = phase { phase = .idle; draftKey = nil }
    }

    /// Going to another thread, or the box going away, ends it, keeping
    /// what was heard in the box it was in.
    func composerLeft(_ draftKey: String) {
        guard self.draftKey == draftKey else { return }
        switch phase {
        case .listening, .finishing: finish()
        case .starting: cancel()
        case .failed: dismissFailure()
        case .idle: break
        }
    }

    private func start(draftKey: String, textView: ComposerTextView?) {
        if isActive(in: self.draftKey ?? "") { cancel() }
        guard let textView else { return }
        guard CollabuteAccount.shared.isSignedIn else {
            CollabuteAccount.shared.signIn()
            self.draftKey = draftKey
            phase = .failed("Approve Threadbear in the Collabute page that opened, then press the mic again.")
            return
        }
        session += 1
        let session = session
        self.draftKey = draftKey
        self.textView = textView
        written = nil
        level = 0
        phase = .starting
        let stream = DictationStream(newToken: { try await CollabuteAccount.shared.voiceToken() })
        self.stream = stream
        stream.onText = { [weak self] text in
            guard let self, self.session == session else { return }
            self.write(text)
        }
        stream.onLevel = { [weak self] level in
            guard let self, self.session == session else { return }
            self.level = level
        }
        stream.onFailure = { [weak self] error in
            guard let self, self.session == session else { return }
            self.stream = nil
            self.level = 0
            self.phase = .failed(error.localizedDescription)
        }
        Task {
            do {
                let token = try await CollabuteAccount.shared.voiceToken()
                guard self.session == session else { return }
                try await stream.start(token: token, language: Self.language)
                guard self.session == session else { stream.cancel(); return }
                phase = .listening
                DictationLog.write("listening")
            } catch {
                guard self.session == session else { return }
                stream.cancel()
                self.stream = nil
                phase = .failed(error.localizedDescription)
                DictationLog.write("couldn't start: \(error.localizedDescription)")
            }
        }
    }

    private func finish() {
        guard let stream else { phase = .idle; return }
        let session = session
        phase = .finishing
        level = 0
        Task {
            let text = await stream.finish()
            guard self.session == session else { return }
            write(text, final: true)
            self.stream = nil
            phase = .idle
            draftKey = nil
            DictationLog.write("finished")
        }
    }

    private func cancel() {
        session += 1
        stream?.cancel()
        stream = nil
        level = 0
        phase = .idle
        draftKey = nil
    }

    /// The language to expect: this Mac's ("en", "pt", ...).
    private static var language: String? {
        Locale.preferredLanguages.first.map { Locale(identifier: $0).language.languageCode?.identifier ?? "" }
    }

    // MARK: Writing into the box

    /// Puts the transcript in the box: in place of what it wrote last time
    /// while that's still there untouched, else at the end. Live updates
    /// leave no undo of their own; the final words are one undo.
    private func write(_ heard: String, final: Bool = false) {
        guard let textView, textView.window != nil else { return }
        let storage = textView.string as NSString
        let current: NSRange
        if let written, NSMaxRange(written.range) <= storage.length, storage.substring(with: written.range) == written.text {
            current = written.range
        } else if written == nil {
            // First words: where the cursor (or the selection) is.
            current = textView.selectedRange().location == NSNotFound
                ? NSRange(location: storage.length, length: 0)
                : textView.selectedRange()
        } else {
            // He edited the box meanwhile: leave his edit, add at the end.
            current = NSRange(location: storage.length, length: 0)
        }
        let piece = Self.spaced(heard, before: current.location > 0 ? storage.substring(with: NSRange(location: current.location - 1, length: 1)) : "",
                                after: NSMaxRange(current) < storage.length ? storage.substring(with: NSRange(location: NSMaxRange(current), length: 1)) : "")
        if final {
            // Out with the live words, unrecorded, then in with the final
            // ones as one undo.
            Self.replace(current, with: "", undo: false, in: textView)
            if !piece.isEmpty { textView.insertText(piece, replacementRange: NSRange(location: current.location, length: 0)) }
            written = nil
            textView.window?.makeFirstResponder(textView)
        } else {
            Self.replace(current, with: piece, undo: false, in: textView)
            written = (NSRange(location: current.location, length: (piece as NSString).length), piece)
            textView.setSelectedRange(NSRange(location: current.location + (piece as NSString).length, length: 0))
        }
    }

    /// With the box's typing attributes, as typing would: a bare string
    /// takes no colour and drew black on the dark composer.
    static func replace(_ range: NSRange, with text: String, undo: Bool, in textView: ComposerTextView) {
        guard range.length > 0 || !text.isEmpty else { return }
        if !undo { textView.undoManager?.disableUndoRegistration() }
        defer { if !undo { textView.undoManager?.enableUndoRegistration() } }
        guard textView.shouldChangeText(in: range, replacementString: text) else { return }
        textView.textStorage?.replaceCharacters(in: range, with: NSAttributedString(string: text, attributes: textView.typingAttributes))
        textView.didChangeText()
    }

    /// The words with a space before them after other words, and after them
    /// before other words.
    static func spaced(_ words: String, before: String, after: String) -> String {
        let words = words.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !words.isEmpty else { return "" }
        let needsBefore = !(before.isEmpty || before.first?.isWhitespace == true)
        let needsAfter = !(after.isEmpty || after.first?.isWhitespace == true || after.first?.isPunctuation == true)
        return (needsBefore ? " " : "") + words + (needsAfter ? " " : "")
    }
}
