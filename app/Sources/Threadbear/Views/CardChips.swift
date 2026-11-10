import AppKit
import ThreadbearCore

/// A Notion card, another thread or a file mentioned among the words
/// (`CardMentions`): a small rounded tag with the card's or thread's title,
/// or the file's icon and name, in place of the raw link or path (Mike
/// asked 3 Nov, for threads 5 Nov; files, Behrooz 10 Nov).
/// It looks like a picture's chip, so a message's chips read as one family.
enum CardChip {
    /// A long title is cut off rather than taking the whole line.
    private static var maxLabel: CGFloat { zoomed(230) }

    private static func label(_ title: String, metrics: PictureChip.Metrics) -> NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        return NSAttributedString(string: title, attributes: [
            .font: metrics.labelFont,
            .foregroundColor: NSColor.secondaryLabelColor,
            .paragraphStyle: paragraph,
        ])
    }

    static func size(_ title: String, metrics: PictureChip.Metrics) -> NSSize {
        let words = min(maxLabel, label(title, metrics: metrics).size().width.rounded(.up))
        return NSSize(width: (metrics.inset + metrics.icon + metrics.gap + words + metrics.trailing).rounded(.up), height: metrics.height)
    }

    /// A card's icon, or a speech bubble for a thread. A file has Finder's
    /// icon for it instead, this one only if that can't be had.
    private static func symbol(_ kind: CardMentions.Kind) -> (name: String, label: String) {
        switch kind {
        case .thread: ("bubble.left", "Thread")
        case .file: ("doc", "File")
        case .card: ("square.text.square", "Notion card")
        }
    }

    /// Finder's icon for each file, read once: chips redraw with every
    /// keystroke in the box.
    private static let icons = NSCache<NSString, NSImage>()

    static func fileIcon(_ path: String) -> NSImage {
        if let icon = icons.object(forKey: path as NSString) { return icon }
        let icon = NSWorkspace.shared.icon(forFile: path)
        icons.setObject(icon, forKey: path as NSString)
        return icon
    }

    static func draw(in rect: NSRect, title: String, kind: CardMentions.Kind = .card, path: String? = nil, metrics: PictureChip.Metrics) {
        let shape = NSBezierPath(roundedRect: rect.insetBy(dx: 0.25, dy: 0.25), xRadius: metrics.radius, yRadius: metrics.radius)
        PictureChip.fill.setFill()
        shape.fill()
        NSColor.separatorColor.setStroke()
        shape.lineWidth = 0.5
        shape.stroke()

        let icon = NSRect(x: rect.minX + metrics.inset, y: rect.midY - metrics.icon / 2, width: metrics.icon, height: metrics.icon)
        if kind == .file, let path {
            // A touch larger than a symbol: Finder's icons have room around them.
            let box = icon.insetBy(dx: -metrics.icon * 0.06, dy: -metrics.icon * 0.06)
            fileIcon(path).draw(in: box, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high.rawValue])
        } else if let symbol = NSImage(systemSymbolName: symbol(kind).name, accessibilityDescription: symbol(kind).label)?
            .withSymbolConfiguration(.init(pointSize: metrics.icon * 0.78, weight: .regular)) {
            let size = symbol.size
            let tinted = NSImage(size: size, flipped: false) { box in
                // The colour cut to the symbol's shape, as a picture chip's is.
                NSColor.secondaryLabelColor.set()
                box.fill(using: .copy)
                symbol.draw(in: box, from: .zero, operation: .destinationIn, fraction: 1)
                return true
            }
            let box = NSRect(x: icon.midX - size.width / 2, y: icon.midY - size.height / 2, width: size.width, height: size.height)
            tinted.draw(in: box.integral, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        }

        let words = label(title, metrics: metrics)
        let height = words.size().height
        // As a picture chip's label: centred whichever way up the context is.
        let box = NSRect(x: icon.maxX + metrics.gap, y: (rect.midY - height / 2).rounded(), width: rect.maxX - metrics.trailing - icon.maxX - metrics.gap, height: height)
        words.draw(with: box, options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }

    /// Mike's bubble: the chip as an attachment that draws itself.
    static func attachment(title: String, kind: CardMentions.Kind = .card, path: String? = nil, font: NSFont) -> NSTextAttachment {
        let metrics = PictureChip.Metrics(font: font)
        let size = size(title, metrics: metrics)
        let attachment = NSTextAttachment()
        attachment.image = NSImage(size: size, flipped: false) { rect in
            draw(in: rect, title: title, kind: kind, path: path, metrics: metrics)
            return true
        }
        attachment.bounds = NSRect(x: 0, y: metrics.baselineOffset, width: size.width, height: size.height)
        return attachment
    }
}

/// A card, thread or file in the composer's words. It carries the link (or
/// backticked path) it stands for, so the draft read off the text is the
/// same plain words the message will hold.
final class CardAttachment: NSTextAttachment {
    let reference: String
    let title: String
    let kind: CardMentions.Kind

    init(reference: String, title: String, kind: CardMentions.Kind = .card) {
        self.reference = reference
        self.title = title
        self.kind = kind
        super.init(data: nil, ofType: nil)
        let cell = CardChipCell()
        attachmentCell = cell
        cell.attachment = self
    }

    /// A file's chip: its file and its name.
    convenience init(file url: URL) {
        let path = url.standardizedFileURL.path
        let name = (path as NSString).lastPathComponent
        self.init(reference: CardMentions.fileReference(path: path), title: name.isEmpty ? path : name, kind: .file)
    }

    /// The file a file's chip stands for: its reference without the backticks.
    var path: String? {
        guard kind == .file, reference.count > 2 else { return nil }
        return String(reference.dropFirst().dropLast())
    }

    required init?(coder: NSCoder) { fatalError("not coded") }
}

/// Draws a `CardAttachment` in the composer (TextKit 1).
final class CardChipCell: NSTextAttachmentCell {
    private var card: CardAttachment? { attachment as? CardAttachment }
    private var metrics: PictureChip.Metrics { PictureChip.Metrics(font: ComposerTextField.font) }

    override func cellSize() -> NSSize { CardChip.size(card?.title ?? "", metrics: metrics) }

    override func cellBaselineOffset() -> NSPoint { NSPoint(x: 0, y: metrics.baselineOffset) }

    // A click puts the cursor beside it, as on a word.
    override func wantsToTrackMouse() -> Bool { false }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        CardChip.draw(in: cellFrame, title: card?.title ?? "", kind: card?.kind ?? .card, path: card?.path, metrics: metrics)
    }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?, characterIndex charIndex: Int, layoutManager: NSLayoutManager) {
        draw(withFrame: cellFrame, in: controlView)
    }
}
