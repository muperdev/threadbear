import SwiftUI
import ThreadbearCore

/// The mic beside Send (Dictation): grey to start, red with its level while
/// it listens, a spinner while it gets ready or catches the last words.
struct DictationButton: View {
    let draftKey: String
    let handle: ComposerHandle
    let size: CGFloat
    @State private var dictation = Dictation.shared
    @State private var account = CollabuteAccount.shared

    var body: some View {
        let active = dictation.isActive(in: draftKey)
        let listening = active && dictation.phase == .listening
        let busy = active && (dictation.phase == .starting || dictation.phase == .finishing)
        Button {
            dictation.toggle(draftKey: draftKey, textView: handle.textView)
        } label: {
            ZStack {
                if busy {
                    SpinnerMark(color: listening ? .white : .secondaryLabelColor)
                        .padding(zoomed(6))
                } else {
                    Image(systemName: listening ? "mic.fill" : "mic")
                        .font(.system(size: zoomed(13), weight: .medium))
                        // A little bigger the louder he is.
                        .scaleEffect(listening ? 1 + CGFloat(min(dictation.level, 1)) * 0.25 : 1)
                        .animation(.easeOut(duration: 0.1), value: dictation.level)
                }
            }
            .foregroundStyle(listening ? AnyShapeStyle(Color.white) : AnyShapeStyle(.secondary))
            .frame(width: size, height: size)
            .background(Circle().fill(listening ? Color.red : Color.primary.opacity(0.1)))
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .tip(tip(active: active, listening: listening))
        .accessibilityLabel(listening ? "Stop dictating" : "Dictate")
        .animation(.easeOut(duration: 0.15), value: listening)
        .popover(isPresented: failureShown) {
            if let message = dictation.failure(in: draftKey) {
                Text(message)
                    .padding(12)
                    .frame(maxWidth: 300)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .onDisappear { dictation.composerLeft(draftKey) }
    }

    private func tip(active: Bool, listening: Bool) -> String {
        if listening { return "Stop dictating; the words stay in the box" }
        if active { return dictation.phase == .finishing ? "Catching the last words" : "Getting ready to listen" }
        if !account.isSignedIn { return "Dictate with Collabute (sign in first)" }
        return "Dictate with Collabute: talk, and the words go in the box"
    }

    private var failureShown: Binding<Bool> {
        Binding(get: { dictation.failure(in: draftKey) != nil },
                set: { if !$0 { dictation.dismissFailure() } })
    }
}
