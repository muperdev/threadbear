import SwiftUI
import ThreadbearKit

/// What a thread that has sat for a while is for and where it stands, above
/// the message box (docs/protocol.md, "The recap box"). × is remembered on
/// this phone, for this quiet spell.
struct RecapBox: View {
    let thread: PhoneThread
    @State private var dismissedAt: Double?

    private var key: String { "recapAway.\(thread.threadId)" }

    var body: some View {
        Group {
            if Recap.isDue(thread, dismissedAt: dismissedAt), let recap = thread.recap {
                let parts = Recap.parts(recap)
                VStack(alignment: .leading, spacing: 5) {
                    if let purpose = parts.purpose { Text(verbatim: purpose) }
                    ForEach(Array(parts.points.enumerated()), id: \.offset) { _, point in
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            Text(verbatim: "•")
                            Text(verbatim: point)
                        }
                    }
                    Text(verbatim: Recap.lastWorkedOn(thread.lastActivityAt))
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .font(.subheadline)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 11)
                .padding(.leading, 14)
                .padding(.trailing, 38)
                .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                .overlay(alignment: .topTrailing) {
                    Button {
                        let at = thread.lastActivityAt.timeIntervalSince1970
                        UserDefaults.standard.set(at, forKey: key)
                        dismissedAt = at
                    } label: {
                        Image(systemName: "xmark")
                            .font(.footnote.weight(.medium))
                            .foregroundStyle(.secondary)
                            .frame(width: 44, height: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Put this away")
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 8)
            }
        }
        .onChange(of: thread.threadId, initial: true) {
            dismissedAt = UserDefaults.standard.object(forKey: key) as? Double
        }
    }
}
