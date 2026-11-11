import AppKit
import ThreadbearCore

/// The two icons at the top right of a ```bash box (ShellBlock): play puts
/// the command in the thread's Terminal, and copy copies it. They're drawn
/// with the box (BackgroundLayoutManager) rather than as an attachment, so
/// the box stays as tight as any other code block and the command can still
/// be selected like text.
enum ShellBlockIcons {
    enum Icon: CaseIterable {
        case put, copy

        var symbol: String {
            switch self {
            case .put: return "play"
            case .copy: return "doc.on.doc"
            }
        }

        var tip: String {
            switch self {
            case .put: return "Put in Terminal"
            case .copy: return "Copy"
            }
        }
    }

    static var size: CGFloat { zoomed(24) }
    private static var gap: CGFloat { zoomed(2) }
    private static var margin: CGFloat { zoomed(6) }

    /// Room the command's text leaves for them, so a long one wraps before
    /// it reaches them.
    static var reserve: CGFloat { size * 2 + gap + zoomed(6) }

    /// The icons for a block in `threadId`'s transcript (the thread of the
    /// view it's drawn in, TranscriptTextView.transcriptThreadId). The play
    /// icon is left out where the Terminal would be a shell on the wrong
    /// machine, in the wrong folder or not this thread's: a thread on
    /// another runner, an archived one, one whose folder has gone, one in a
    /// panel of its own rather than the main panel, or text in no thread
    /// (AppModel.canPutInTerminal(from:)).
    @MainActor
    static func shown(in threadId: String?) -> [Icon] {
        AppModel.shared.canPutInTerminal(from: threadId) ? Icon.allCases : [.copy]
    }

    /// `box` is the code block's rounded box. Lined up with its first line,
    /// so a one-line command has them centred.
    static func rect(_ icon: Icon, in box: NSRect) -> NSRect {
        let top = box.minY + max(zoomed(2), MarkdownRenderer.codePad + (Theme.codeLine - size) / 2)
        let right = box.maxX - margin - (icon == .copy ? 0 : size + gap)
        return NSRect(x: right - size, y: top, width: size, height: size)
    }

    // MARK: Pressed

    /// When each icon of each command was last pressed, so it shows a tick
    /// for a moment. Read when drawing, so no render is needed. Only touched
    /// on the main thread.
    nonisolated(unsafe) private static var pressed: [String: Date] = [:]
    static let tickFor: TimeInterval = 1.5
    /// Where Copy goes; tests give it a pasteboard of their own.
    nonisolated(unsafe) static var pasteboard: NSPasteboard = .general

    private static func key(_ icon: Icon, _ command: String) -> String { "\(icon)|\(command)" }

    static func markPressed(_ icon: Icon, _ command: String, now: Date = Date()) {
        pressed = pressed.filter { now.timeIntervalSince($0.value) < tickFor }
        pressed[key(icon, command)] = now
    }

    static func clearPressed(_ icon: Icon, _ command: String) {
        pressed[key(icon, command)] = nil
    }

    static func wasJustPressed(_ icon: Icon, _ command: String, now: Date = Date()) -> Bool {
        pressed[key(icon, command)].map { now.timeIntervalSince($0) < tickFor } ?? false
    }

    static func copy(_ command: String) {
        pasteboard.clearContents()
        pasteboard.setString(command, forType: .string)
    }

    // MARK: Drawing

    @MainActor
    static func draw(command: String, box: NSRect, hovered: Icon?, threadId: String?) {
        for icon in shown(in: threadId) {
            let rect = rect(icon, in: box)
            if hovered == icon {
                NSColor.labelColor.withAlphaComponent(0.1).setFill()
                NSBezierPath(roundedRect: rect, xRadius: zoomed(5), yRadius: zoomed(5)).fill()
            }
            let ticked = wasJustPressed(icon, command)
            let color: NSColor = ticked ? .systemGreen : (hovered == icon ? .labelColor : .secondaryLabelColor)
            let config = NSImage.SymbolConfiguration(pointSize: TypeScale.detail, weight: ticked ? .bold : .regular)
                .applying(.init(paletteColors: [color]))
            guard let image = NSImage(systemSymbolName: ticked ? "checkmark" : icon.symbol, accessibilityDescription: icon.tip)?.withSymbolConfiguration(config) else { continue }
            let s = image.size
            image.draw(in: NSRect(x: rect.midX - s.width / 2, y: rect.midY - s.height / 2, width: s.width, height: s.height),
                       from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        }
    }
}
