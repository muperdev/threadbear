import AppKit
import SwiftUI
import ThreadbearCore

/// Settings › Team (docs/multiplayer-design.md): make a team or join one,
/// who's on it, invites, and the projects it shares. Only while the
/// multiplayer experiment is on.
struct TeamSettings: View {
    @Environment(AppModel.self) private var app
    @State private var teamName = ""
    @State private var yourName = NSFullUserName()
    @State private var invite = ""

    var body: some View {
        Form {
            if app.connection != .connected {
                Section {
                    Text("Not connected to the runner.")
                        .foregroundStyle(.secondary)
                }
            } else if !app.team.on {
                Section {
                    Text("Shared projects are an experiment that's off. Turn on \u{201C}multiplayer\u{201D} in the runner's experiments to make or join a team.")
                        .foregroundStyle(.secondary)
                }
            } else if !app.team.joined {
                SettingsSection("Join a team",
                                about: "Paste the invite a teammate sent you (it starts with tbteam:). Your Threadbear keeps its own threads; the team only sees the projects you share and the threads in them.",
                                ask: "explain what a team can see of my threads.") {
                    TextField("Your name", text: $yourName)
                    TextField("Invite", text: $invite, prompt: Text("tbteam:https://…#…"))
                    HStack {
                        Spacer()
                        Button("Join") { app.joinTeam(invite: invite.trimmingCharacters(in: .whitespacesAndNewlines), name: yourName) }
                            .disabled(invite.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || app.isBusy(BusyKey.teamJoin))
                            .busy(app.isBusy(BusyKey.teamJoin))
                            .tip("Join the team the invite is for")
                    }
                }
                SettingsSection("Or make one",
                                about: "The team lives on this Mac's own Convex backend: you host it and invite the others. It holds only what's shared: the projects, the board of threads in them, and messages and handovers between your agents. Never transcripts, tool output, folders or secrets.",
                                ask: "help me set up a team for my projects.") {
                    TextField("Your name", text: $yourName)
                    TextField("Team name", text: $teamName, prompt: Text("Collabute"))
                    HStack {
                        Spacer()
                        Button("Make Team") { app.createTeam(name: teamName.trimmingCharacters(in: .whitespacesAndNewlines), memberName: yourName) }
                            .disabled(teamName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || app.isBusy(BusyKey.teamJoin))
                            .busy(app.isBusy(BusyKey.teamJoin))
                            .tip("Make a team hosted on this Mac's backend")
                    }
                }
            } else {
                SettingsSection(app.team.name ?? "Team",
                                about: app.team.me?.host == true
                                    ? "You host this team on your Convex backend. An invite lets one person join, for 3 days."
                                    : "You joined this team. Whoever hosts it can invite others.",
                                ask: "explain how the team works.") {
                    if !app.team.connected {
                        Label(app.team.error.map { "Can't reach the team: \($0)" } ?? "Connecting…", systemImage: "wifi.exclamationmark")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(app.team.members) { member in
                        HStack(spacing: 8) {
                            Circle()
                                .fill(member.online ? Color.green : Color.secondary.opacity(0.4))
                                .frame(width: 8, height: 8)
                            Text(member.me ? "\(member.name) (you)" : member.name)
                            if member.host {
                                Text("host").foregroundStyle(.secondary)
                            }
                            Spacer()
                            if app.team.me?.host == true, !member.me {
                                Button("Remove…") { app.removeFromTeam(member) }
                                    .buttonStyle(.borderless)
                                    .busy(app.isBusy(BusyKey.teamRemove(member.memberId)))
                                    .tip("Take \(member.name) off the team")
                            }
                        }
                        .tip(member.online ? "\(member.name)'s Threadbear is running" : "\(member.name)'s Threadbear hasn't been seen in the last few minutes")
                    }
                    HStack {
                        if let made = app.teamInvite {
                            Text(made)
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .textSelection(.enabled)
                            Text("copied").foregroundStyle(.secondary)
                        }
                        Spacer()
                        if app.team.me?.host == true {
                            Button("Copy Invite") { app.copyTeamInvite() }
                                .busy(app.isBusy(BusyKey.teamInvite))
                                .tip("Make an invite for one person and copy it")
                        } else {
                            Button("Leave Team…") { app.leaveTeam() }
                                .busy(app.isBusy(BusyKey.teamLeave))
                                .tip("Leave the team; your copies of its projects stay here as yours")
                        }
                    }
                }
                SettingsSection("Shared projects",
                                about: "Share one of yours from its menu in Projects. A teammate's shows here; Add makes a copy of it here, linked to theirs, with your own clones of its repos as its folders.",
                                ask: "help me share a project with my team.") {
                    if app.team.projects.isEmpty {
                        Text("Nothing shared yet").foregroundStyle(.secondary)
                    }
                    ForEach(app.team.projects) { shared in
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(shared.title)
                                Text(shared.mine ? "Shared by you" : "Shared by \(shared.sharedBy)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            if let local = shared.localProjectId {
                                Button("Open") { app.openProjectPage(local) }
                                    .buttonStyle(.borderless)
                                    .tip("Open your copy of \(shared.title)")
                            } else {
                                Button("Add") { app.addSharedProject(shared) }
                                    .busy(app.isBusy(BusyKey.teamAdd(shared.id)))
                                    .tip("Make a copy of \(shared.title) here, linked to the team's")
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .fixedSize(horizontal: false, vertical: true)
        .scrollIndicators(.never)
        .onAppear { app.loadTeam() }
    }
}
