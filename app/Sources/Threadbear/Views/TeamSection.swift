import AppKit
import SwiftUI
import ThreadbearCore

/// A shared project's team on its page (docs/multiplayer-design.md): who's on
/// it, anything a teammate sent that waits for Mike, the board of teammates'
/// threads in it, and the notes their agents added. Nothing shows for a
/// project that isn't shared.
struct TeamSection: View {
    @Environment(AppModel.self) private var app
    let project: Project

    var body: some View {
        if let link = project.team, app.team.on, app.team.joined {
            let shared = app.team.projects.first { $0.id == link.id }
            VStack(alignment: .leading, spacing: zoomed(12)) {
                PageHeading(title: "Team", detail: detail(shared)) {
                    HStack(spacing: zoomed(10)) {
                        Toggle("Pass messages straight in", isOn: Binding(get: { link.deliver }, set: { app.setTeamDelivery(project.id, deliver: $0) }))
                            .toggleStyle(.checkbox)
                            .controlSize(.small)
                            .busy(app.isBusy(BusyKey.teamDelivery(project.id)))
                            .tip("Off: a teammate's agent's message to one of these threads waits here for you. On: it goes straight into the thread.")
                        Menu {
                            Button("Copy Invite") { app.copyTeamInvite() }
                                .disabled(app.team.me?.host != true)
                            Button("Stop Sharing…") { app.stopSharingProject(project.id) }
                        } label: { Image(systemName: "ellipsis.circle") }
                            .menuStyle(.borderlessButton)
                            .fixedSize()
                            .busy(app.isBusy(BusyKey.teamShare(project.id)))
                            .tip("Invite someone, or stop sharing this project")
                    }
                    .font(.system(size: TypeScale.detail))
                }
                MembersRow(members: app.team.members)
                if !app.team.connected, let error = app.team.error {
                    Text("Can't reach the team: \(error)")
                        .font(.system(size: TypeScale.detail))
                        .foregroundStyle(.orange)
                        .textSelection(.enabled)
                }
                if let note = app.teamNote {
                    Text(note)
                        .font(.system(size: TypeScale.detail))
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                let waiting = app.team.waiting(for: link.id)
                if !waiting.isEmpty {
                    VStack(alignment: .leading, spacing: zoomed(8)) {
                        Text("Waiting for you")
                            .font(.system(size: TypeScale.detail, weight: .semibold))
                            .foregroundStyle(.secondary)
                        ForEach(waiting) { WaitingCard(item: $0) }
                    }
                }
                TeamBoard(cards: app.team.board(link.id))
                let log = app.team.log[link.id] ?? []
                if !log.isEmpty {
                    VStack(alignment: .leading, spacing: zoomed(4)) {
                        Text("Notes from the team's agents")
                            .font(.system(size: TypeScale.detail, weight: .semibold))
                            .foregroundStyle(.secondary)
                        ForEach(Array(log.suffix(5).enumerated()), id: \.offset) { _, entry in
                            (Text(entry.memberName).fontWeight(.semibold) + Text(entry.threadTitle.map { " in \u{201C}\($0)\u{201D}" } ?? "") + Text(": ") + Text(entry.text))
                                .font(.system(size: TypeScale.detail))
                                .foregroundStyle(.secondary)
                                .lineLimit(4)
                                .textSelection(.enabled)
                        }
                    }
                }
            }
        }
    }

    /// "Collabute · shared by Sam".
    private func detail(_ shared: SharedProject?) -> String {
        var parts = [app.team.name ?? "Team"]
        if let shared { parts.append(shared.mine ? "shared by you" : "shared by \(shared.sharedBy)") }
        return parts.joined(separator: " · ")
    }
}

/// Who's on the team, a green dot for whoever's runner was seen lately.
private struct MembersRow: View {
    let members: [TeamMember]

    var body: some View {
        HStack(spacing: zoomed(12)) {
            ForEach(members) { m in
                HStack(spacing: zoomed(5)) {
                    Circle()
                        .fill(m.online ? Color.green : Color(nsColor: Theme.faintText).opacity(0.5))
                        .frame(width: zoomed(7), height: zoomed(7))
                    Text(m.me ? "\(m.name) (you)" : m.name)
                        .font(.system(size: TypeScale.label))
                        .foregroundStyle(m.me ? .secondary : .primary)
                }
                .tip(m.online ? "\(m.name)'s Threadbear is running" : "\(m.name)'s Threadbear hasn't been seen in the last few minutes")
            }
            Spacer(minLength: 0)
        }
    }
}

/// A teammate's message or handover, with what Mike can do about it.
private struct WaitingCard: View {
    @Environment(AppModel.self) private var app
    let item: TeamWaiting

    var body: some View {
        VStack(alignment: .leading, spacing: zoomed(6)) {
            HStack(spacing: zoomed(6)) {
                Image(systemName: item.kind == .handover ? "arrow.triangle.turn.up.right.circle" : "bubble.left.and.text.bubble.right")
                    .foregroundStyle(.secondary)
                Text(headline)
                    .font(.system(size: TypeScale.label, weight: .semibold))
                    .lineLimit(2)
                Spacer(minLength: zoomed(8))
                Text(RelativeTime.short(item.createdAt, now: app.now))
                    .font(.system(size: TypeScale.small))
                    .foregroundStyle(Color(nsColor: Theme.faintText))
            }
            Text(item.text)
                .font(.system(size: TypeScale.detail))
                .lineLimit(8)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            if let repo = item.repo {
                Text(item.branch.map { "\(repo), branch \($0)" } ?? repo)
                    .font(.system(size: TypeScale.small, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            if !item.links.isEmpty {
                HStack(spacing: zoomed(10)) {
                    ForEach(item.links, id: \.url) { link in
                        PlainLink(title: link.title, help: link.url) { if let url = URL(string: link.url) { OpenGate.openWeb(url) } }
                            .font(.system(size: TypeScale.detail))
                    }
                }
            }
            HStack(spacing: zoomed(8)) {
                if item.kind == .handover {
                    Button("Take It") { app.answerTeam(item, .take) }
                        .keyboardShortcut(.defaultAction)
                        .tip("Start a thread here in this project with \(item.fromName)'s brief, and tell them")
                    Button("Decline") { app.answerTeam(item, .decline) }
                        .tip("Tell \(item.fromName) you won't take it")
                } else {
                    Button("Pass It On") { app.answerTeam(item, .pass) }
                        .tip(item.toThreadTitle.map { "Send it into \u{201C}\($0)\u{201D}, marked as from \(item.fromName)'s agent" } ?? "Start a thread in this project with it")
                    Button("Dismiss") { app.answerTeam(item, .dismiss) }
                        .tip("Drop it; \(item.fromName)'s thread is told")
                }
            }
            .controlSize(.small)
            .disabled(app.isBusy(BusyKey.teamAnswer(item.id)))
            .busy(app.isBusy(BusyKey.teamAnswer(item.id)))
        }
        .padding(zoomed(10))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: Theme.cardBackground), in: RoundedRectangle(cornerRadius: zoomed(8)))
        .overlay(RoundedRectangle(cornerRadius: zoomed(8)).strokeBorder(Color(nsColor: Theme.cardBorder), lineWidth: 0.5))
    }

    private var headline: String {
        if item.kind == .handover { return "\(item.fromName) handed you \u{201C}\(item.fromTitle)\u{201D}" }
        if let to = item.toThreadTitle { return "\(item.fromName)'s agent, for \u{201C}\(to)\u{201D}" }
        return "\(item.fromName)'s agent, from \u{201C}\(item.fromTitle)\u{201D}"
    }
}

/// Teammates' threads in the project, a column for each kind of state.
/// Done shows its newest few.
struct TeamBoard: View {
    let cards: [TeamCard]
    static let doneShown = 3

    var body: some View {
        if cards.isEmpty {
            Text("No teammates' threads in it yet. Theirs show here as they start them.")
                .font(.system(size: TypeScale.label))
                .foregroundStyle(Color(nsColor: Theme.faintText))
        } else {
            HStack(alignment: .top, spacing: zoomed(8)) {
                ForEach(TeamCard.Status.allCases, id: \.self) { status in
                    let items = cards.filter { $0.status == status }
                    let shown = status == .done ? Array(items.prefix(Self.doneShown)) : items
                    VStack(alignment: .leading, spacing: zoomed(6)) {
                        HStack(spacing: zoomed(6)) {
                            Circle().fill(Self.color(status)).frame(width: zoomed(6), height: zoomed(6))
                            Text(status.title)
                                .font(.system(size: TypeScale.detail, weight: .semibold))
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                            Text("\(items.count)")
                                .font(.system(size: TypeScale.detail))
                                .foregroundStyle(Color(nsColor: Theme.faintText))
                                .monospacedDigit()
                        }
                        .padding(.horizontal, zoomed(4))
                        .padding(.bottom, zoomed(2))
                        ForEach(shown) { TeamCardView(card: $0) }
                        if shown.count < items.count {
                            Text("\(items.count - shown.count) more")
                                .font(.system(size: TypeScale.detail))
                                .foregroundStyle(Color(nsColor: Theme.faintText))
                                .padding(.horizontal, zoomed(4))
                        }
                    }
                    .padding(zoomed(6))
                    .frame(maxWidth: .infinity, minHeight: zoomed(80), alignment: .topLeading)
                    .background(Color(nsColor: Theme.cardBackground), in: RoundedRectangle(cornerRadius: zoomed(8)))
                }
            }
        }
    }

    static func color(_ status: TeamCard.Status) -> Color {
        switch status {
        case .working: .blue
        case .needsThem: .orange
        case .idle: Color(nsColor: Theme.faintText)
        case .done: .green
        }
    }
}

/// One teammate's thread: its title, whose it is, the branch, and what it
/// last said. A click copies its id, which agents message it by.
private struct TeamCardView: View {
    let card: TeamCard
    @State private var hovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: zoomed(4)) {
            Text(card.title)
                .font(.system(size: TypeScale.detail))
                .foregroundStyle(card.status == .done ? .secondary : .primary)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
            if let headline = card.headline {
                Text(headline)
                    .font(.system(size: TypeScale.small))
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(meta)
                .font(.system(size: TypeScale.small))
                .foregroundStyle(Color(nsColor: Theme.faintText))
                .lineLimit(2)
        }
        .padding(.horizontal, zoomed(8))
        .padding(.vertical, zoomed(7))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(hovering ? Color(nsColor: Theme.hoverFill) : Color(nsColor: Theme.codeBackground), in: RoundedRectangle(cornerRadius: zoomed(6)))
        .overlay(RoundedRectangle(cornerRadius: zoomed(6)).strokeBorder(Color(nsColor: Theme.cardBorder), lineWidth: 0.5))
        .contentShape(Rectangle())
        .onTapGesture {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(card.id, forType: .string)
        }
        .onHover { hovering = $0 }
        .tip("Click to copy \(card.id): an agent messages this thread with send_message to it")
    }

    /// "Sam · fix/alarms · handed to Ana".
    private var meta: String {
        var parts = [card.memberName]
        if let branch = card.branch { parts.append(branch) }
        if let to = card.handedToName { parts.append("handed to \(to)") }
        return parts.joined(separator: " · ")
    }
}
