import SwiftUI
import ThreadbearCore

/// What a thread that has sat for a while is for and where it stands, above
/// the message box, so it's there before he has to ask (Mike picked this
/// from mockups on 10 Nov). The librarian wrote it after the last turn.
/// Sending a message ends it, as the thread stops being quiet.
struct RecapBox: View {
    @Environment(AppModel.self) private var app
    let thread: ThreadSummary

    var body: some View {
        if Recap.isDue(thread, dismissedAt: app.dismissedRecaps[thread.id]), let recap = thread.recap {
            let parts = Recap.parts(recap)
            VStack(alignment: .leading, spacing: zoomed(5)) {
                if let purpose = parts.purpose {
                    Text(verbatim: purpose).fixedSize(horizontal: false, vertical: true)
                }
                ForEach(Array(parts.points.enumerated()), id: \.offset) { _, point in
                    HStack(alignment: .firstTextBaseline, spacing: zoomed(7)) {
                        Text(verbatim: "•")
                        Text(verbatim: point).fixedSize(horizontal: false, vertical: true)
                    }
                }
                Text(verbatim: Recap.lastWorkedOn(thread.lastActivityAt))
                    .font(.system(size: TypeScale.detail))
                    .foregroundStyle(.secondary)
            }
            .font(.system(size: TypeScale.cardTitle))
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, zoomed(11))
            .padding(.leading, zoomed(14))
            .padding(.trailing, zoomed(34))
            .background(Color(nsColor: Theme.cardBackground), in: RoundedRectangle(cornerRadius: zoomed(12)))
            .overlay { RoundedRectangle(cornerRadius: zoomed(12)).stroke(Color(nsColor: Theme.hairline), lineWidth: 1) }
            .overlay(alignment: .topTrailing) {
                Button {
                    app.dismissRecap(thread)
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: TypeScale.detail, weight: .medium))
                        .foregroundStyle(.secondary)
                        .frame(width: zoomed(24), height: zoomed(24))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .tip("Put this away")
                .padding(zoomed(5))
            }
            .padding(.bottom, zoomed(10))
        }
    }
}

extension AppModel {
    /// Remembers it, keeping only the newest as this is one entry a thread.
    func dismissRecap(_ thread: ThreadSummary) {
        dismissedRecaps[thread.id] = thread.lastActivityAt.timeIntervalSince1970
        if dismissedRecaps.count > 200 {
            dismissedRecaps = Dictionary(uniqueKeysWithValues: dismissedRecaps.sorted { $0.value > $1.value }.prefix(150).map { ($0.key, $0.value) })
        }
    }
}
