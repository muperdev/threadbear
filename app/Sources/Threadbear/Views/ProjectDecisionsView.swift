import AppKit
import SwiftUI
import ThreadbearCore

/// Decisions on the Projects page (the projectDecisions experiment,
/// runner/src/experiments/projectDecisions.md): at the top of a project's
/// page, a card for each of its threads waiting on Mike with a card of asks,
/// newest first, to answer one after another. Each card is its thread's:
/// titled by it, a line on what it's doing, its asks drawn exactly as its
/// composer draws them, and Open thread for the whole story. Answering here
/// sends what answering in the thread sends (AppModel.sendDecision). Mike
/// picked it (direction A in ~/dev/me/misc/ghostex-notes, mockup coord-A)
/// on 10 Nov 2026.
struct DecisionsSection: View {
    @Environment(AppModel.self) private var app
    let project: Project

    var body: some View {
        let section = app.projectSections.sections.first { $0.id == project.id }
        let threads = ProjectDecisions.threads(section?.threads ?? [])
        if !threads.isEmpty {
            VStack(alignment: .leading, spacing: zoomed(12)) {
                PageHeading(title: "Decisions", detail: ProjectDecisions.heading(decisions: ProjectDecisions.count(threads), threads: threads.count))
                ForEach(threads) { thread in
                    DecisionCard(thread: thread)
                        .transition(.opacity.combined(with: .move(edge: .top)))
                }
            }
            .animation(.easeOut(duration: 0.2), value: threads.map(\.id))
        }
    }
}

/// One thread's decision: its title and how long ago, a line on what it's
/// doing, its asks, and Open thread. While an answer's on its way it's one
/// line, "Sending to <title>…" with a spinner, and takes no clicks.
private struct DecisionCard: View {
    @Environment(AppModel.self) private var app
    let thread: ThreadSummary

    private var title: String { thread.title.isEmpty ? "Untitled" : thread.title }

    var body: some View {
        let sent = app.decisionSends[thread.id]
        let sending = ProjectDecisions.isSending(thread, sentFor: sent?.asksSeq)
        VStack(alignment: .leading, spacing: zoomed(4)) {
            if sending, let sent {
                HStack(alignment: .center, spacing: zoomed(8)) {
                    SpinnerMark(color: .secondaryLabelColor)
                        .frame(width: zoomed(12), height: zoomed(12))
                    (Text("Sending to ") + Text(title).fontWeight(.semibold) + Text("…"))
                        .font(.system(size: TypeScale.label))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Text(sent.answers.joined(separator: "; "))
                        .font(.system(size: TypeScale.label))
                        .foregroundStyle(Color(nsColor: Theme.faintText))
                        .lineLimit(1)
                    Spacer(minLength: zoomed(8))
                    openButton
                }
            } else {
                HStack(alignment: .firstTextBaseline, spacing: zoomed(8)) {
                    Text(title)
                        .font(.system(size: TypeScale.label, weight: .semibold))
                        .lineLimit(1)
                    Text(RelativeTime.short(thread.lastActivityAt, now: app.now))
                        .font(.system(size: TypeScale.detail))
                        .foregroundStyle(Color(nsColor: Theme.faintText))
                        .monospacedDigit()
                        .fixedSize()
                    Spacer(minLength: zoomed(8))
                    openButton
                }
                if let line = ProjectDecisions.recapLine(thread) {
                    Text(line)
                        .font(.system(size: TypeScale.detail))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .tip(line)
                }
                if let card = app.decisionCard(thread) {
                    DecisionAsks(thread: thread, card: card)
                        .padding(.top, zoomed(2))
                }
            }
        }
        .padding(.horizontal, zoomed(16))
        .padding(.vertical, zoomed(sending ? 10 : 14))
        .background(Color(nsColor: Theme.cardBackground), in: RoundedRectangle(cornerRadius: zoomed(10), style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: zoomed(10), style: .continuous).strokeBorder(Color(nsColor: Theme.cardBorder), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(sending ? "Sending to \(title)" : title)
    }

    private var openButton: some View {
        Button("Open thread") { app.openThread(thread.id) }
            .controlSize(.small)
            .tip("Open \(title) for the whole thread")
    }
}

/// A thread's asks, drawn by the same renderer and cell as the card in its
/// composer (RowRenderer.asksText in a bare TextRowCell), so they look and
/// answer the same, Recommended star, rows and Something else box included.
/// Only the card's "Decision needed" heading is left off: the decision's
/// own card is titled by its thread.
private struct DecisionAsks: NSViewRepresentable {
    @Environment(AppModel.self) private var app
    let thread: ThreadSummary
    let card: AsksCard

    func makeNSView(context: Context) -> DecisionAsksHost {
        DecisionAsksHost(app: app)
    }

    func updateNSView(_ host: DecisionAsksHost, context: Context) {
        host.app = app
        host.show(card, thread: thread)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: DecisionAsksHost, context: Context) -> CGSize? {
        let width = ComposerCardHost.measuringWidth(proposal.width)
        return CGSize(width: width, height: nsView.rendered(card, local: !thread.isRemote, width: width).height)
    }
}

/// Hosts the card's cell and answers its clicks for that one thread, as the
/// transcript's controller does for the composer's card.
final class DecisionAsksHost: NSView, TranscriptThreadHost, RowCellDelegate {
    weak var app: AppModel?
    private let cell = TextRowCell()
    private var threadId: String?
    private var local = true
    private var card: AsksCard?
    /// What was built last, for SwiftUI's many asks for its size.
    private var built: (card: AsksCard, local: Bool, width: CGFloat, zoom: Int, render: RowRender)?
    private var shown: (card: AsksCard, width: CGFloat, zoom: Int)?
    /// The Something else box to put the cursor in once the card has made
    /// room for it.
    private var pendingFocus: Int?

    var hostedThreadId: String? { threadId }
    var linkThreadId: String? { threadId }
    override var isFlipped: Bool { true }

    init(app: AppModel) {
        self.app = app
        super.init(frame: .zero)
        cell.bare = true
        cell.delegate = self
        addSubview(cell)
        // A Run button's state lives with the buttons, not in the card, so
        // a press (or the runner's word on it) draws the card again, as
        // the composer's does (Codex's review: it kept showing a pressable
        // Run while the press was on its way).
        NotificationCenter.default.addObserver(self, selector: #selector(buttonsChanged), name: ReplyButtons.changed, object: nil)
    }

    @objc private func buttonsChanged() {
        guard let card, card.asks.contains(where: { $0.run != nil }) else { return }
        built = nil
        shown = nil
        needsLayout = true
    }

    required init?(coder: NSCoder) { fatalError() }

    func show(_ card: AsksCard, thread: ThreadSummary) {
        guard card != self.card || thread.id != threadId || local != !thread.isRemote else { return }
        self.card = card
        threadId = thread.id
        local = !thread.isRemote
        needsLayout = true
    }

    static func text(_ card: AsksCard, width: CGFloat) -> NSAttributedString {
        let text = NSMutableAttributedString(attributedString: RowRenderer.asksText(card, width: width))
        // Everything up to the first ask's line break is the heading.
        let firstBreak = (text.string as NSString).range(of: "\n")
        if firstBreak.location != NSNotFound { text.deleteCharacters(in: NSRange(location: 0, length: NSMaxRange(firstBreak))) }
        return text
    }

    func rendered(_ card: AsksCard, local: Bool, width: CGFloat) -> RowRender {
        let zoom = TypeScale.percent
        if let built, built.card == card, built.local == local, built.width == width, built.zoom == zoom { return built.render }
        let render = MarkdownRenderer.withLocalPictures(local) { () -> RowRender in
            let text = Self.text(card, width: width)
            let size = TextMeasurer.current.measure(text, width: width)
            return RowRender(height: ceil(size.height), text: text, textSize: size)
        }
        built = (card, local, width, zoom, render)
        return render
    }

    override func layout() {
        super.layout()
        guard let card, let threadId, bounds.width > 0 else { return }
        let zoom = TypeScale.percent
        cell.frame = bounds
        if let shown, shown.card == card, shown.width == bounds.width, shown.zoom == zoom { return }
        shown = (card, bounds.width, zoom)
        let render = rendered(card, local: local, width: bounds.width)
        let row = TranscriptRow(id: "decision:" + threadId, seq: card.seq, content: .asks(card))
        cell.configure(row: row, render: render, tableWidth: bounds.width, expanded: false)
        if let ask = pendingFocus, cell.focusAnswerBox(ask) { pendingFocus = nil }
    }

    private var thread: ThreadSummary? { threadId.flatMap { app?.threads[$0] } }

    // MARK: RowCellDelegate

    /// `pick:<ask>:<n>` picks an answer (sending the card once every ask has
    /// one), `link:<ask>:<n>` opens a link, `run:<ask>:0` presses an ask's
    /// Run. Nothing else on an open card of asks applies here.
    func askAction(_ card: AsksCard, action: String) {
        guard let app, let thread, thread.asksSeq == card.seq else { return }
        let parts = action.split(separator: ":")
        guard parts.count == 3, let index = Int(parts[1]), let n = Int(parts[2]), card.asks.indices.contains(index) else { return }
        let ask = card.asks[index]
        switch parts[0] {
        case "pick":
            app.pickDecision(thread: thread, ask: index, option: n)
            if n == Decisions.ownAnswer, let now = app.threads[thread.id], app.picks(for: now)[index] == Decisions.ownAnswer {
                pendingFocus = index
                DispatchQueue.main.async { [weak self] in
                    guard let self, let ask = self.pendingFocus, self.cell.focusAnswerBox(ask) else { return }
                    self.pendingFocus = nil
                }
            }
        case "run":
            if let id = ask.run?.buttonId { app.pressRunButton(id, threadId: thread.id) }
        case "link":
            guard ask.links.indices.contains(n), let url = ask.links[n].target else { return }
            // A design review opens in its window from the thread, where it
            // lives; anything else opens where it opens (the browser, its
            // own app), as the page has no pane of the thread's.
            if WorkLinks.reviewId(of: url) != nil { return app.openThread(thread.id) }
            if url.isFileURL { app.openLink(url, title: ask.links[n].title, in: thread, external: true) } else { app.openWebPage(url) }
        default: break
        }
    }

    func askNoteChanged(_ card: AsksCard, ask: Int, note: AskNote) {
        guard let app, let thread, thread.asksSeq == card.seq else { return }
        app.setNote(thread: thread, ask: ask, note: note)
    }

    func askNoteImages(_ card: AsksCard, ask: Int, urls: [URL]) {
        guard let app, let thread, thread.asksSeq == card.seq else { return }
        app.addNoteImages(thread: thread, ask: ask, urls: urls)
    }

    func askPicturesAllowed(_ card: AsksCard) -> Bool {
        guard let app, let thread, thread.asksSeq == card.seq else { return false }
        if let why = app.picturesRefusal(for: thread) {
            app.errorMessage = why
            return false
        }
        return true
    }

    func askImportPicture(_ card: AsksCard, url: URL) async -> ImageRef? {
        await app?.importImages([url]).first
    }

    func sendAnswers(_ card: AsksCard) {
        guard let threadId else { return }
        app?.sendDecision(threadId: threadId)
    }

    func answerBoxReturn(_ card: AsksCard, ask: Int, command: Bool) {
        guard let threadId else { return }
        app?.decisionBoxReturn(threadId: threadId, command: command)
    }

    // Rows this card never draws.
    func toggleExpanded(rowId: String) {}
    func toggleChangedFile(turnId: String, path: String) {}
    func showAllLines(_ key: String) {}
    func respond(approvalId: String, decision: ApprovalDecision, answers: [String: String]?, message: String?) {}
    func loadStartRequest(_ approvalId: String) {}
    func questionDraft(_ approvalId: String) -> [Int: Set<String>] { [:] }
    func setQuestionDraft(_ approvalId: String, _ draft: [Int: Set<String>]) {}
}
