import SwiftUI
import ThreadbearCore

/// ‹ and › beside the share icon in the main panel's bar: Back and Forward
/// through the threads and pages you have had open, the same steps as ⌘[
/// and ⌘], the mouse's side buttons and trackpad swipes (Navigation.swift).
/// For losing track of which thread you were on (Wayne Sutton, 8 Nov). The
/// tip names where each one goes.
struct HistoryButtons: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        HStack(spacing: 0) {
            HistoryButton(back: true)
            HistoryButton(back: false)
        }
    }
}

private struct HistoryButton: View {
    @Environment(AppModel.self) private var app
    let back: Bool

    var body: some View {
        let enabled = back ? app.canGoBack : app.canGoForward
        Button {
            if back { app.goBack() } else { app.goForward() }
        } label: {
            Image(systemName: back ? "chevron.left" : "chevron.right")
                .font(.system(size: TypeScale.label, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: zoomed(22), height: zoomed(26))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.35)
        .tip(HistoryText.tip(back: back, to: enabled ? app.destination(back: back) : nil,
                             shortcut: AppShortcuts.shared.display(back ? .back : .forward)))
        .accessibilityLabel(back ? "Back" : "Forward")
    }
}

/// What the buttons' tips say, kept apart so tests can read it.
enum HistoryText {
    static func tip(back: Bool, to destination: (title: String, detail: String?)?, shortcut: String?) -> String {
        let way = back ? "Back" : "Forward"
        var text = destination.map { "\(way) to \($0.title)" } ?? (back ? "Nothing to go back to" : "Nothing to go forward to")
        if let shortcut { text += " (\(shortcut))" }
        return text
    }
}
