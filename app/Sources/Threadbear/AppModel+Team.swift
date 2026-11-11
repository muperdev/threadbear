import AppKit
import ThreadbearCore

/// Shared projects (docs/multiplayer-design.md): the team this Mac is on, and
/// what Mike does with it from Settings › Team and a project's page.
extension AppModel {
    func loadTeam() {
        guard let client else { return }
        Task {
            do {
                team = try await client.getTeam()
            } catch RPCError.server(let code, _) where code == RPCError.methodNotFound {
                // A runner from before teams.
                team = TeamState()
            } catch {
                AppLog.write("rpc", "team.get failed: \(error.localizedDescription)")
            }
        }
    }

    /// Makes a team hosted on this Mac's own backend.
    func createTeam(name: String, memberName: String) {
        guard let client else { return }
        whileBusy(BusyKey.teamJoin) { [self] in
            do {
                team = try await client.createTeam(name: name, memberName: memberName)
            } catch {
                show(error)
            }
        }
    }

    func joinTeam(invite: String, name: String) {
        guard let client else { return }
        whileBusy(BusyKey.teamJoin) { [self] in
            do {
                team = try await client.joinTeam(invite: invite, name: name)
            } catch {
                show(error)
            }
        }
    }

    /// Makes an invite and puts it on the clipboard, saying so.
    func copyTeamInvite() {
        guard let client else { return }
        whileBusy(BusyKey.teamInvite, note: "Making an invite…") { [self] in
            do {
                let invite = try await client.inviteToTeam()
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(invite.invite, forType: .string)
                teamInvite = invite.invite
            } catch {
                show(error)
            }
        }
    }

    func leaveTeam() {
        guard let client, let name = team.name else { return }
        let alert = NSAlert()
        alert.messageText = "Leave \(name)?"
        alert.informativeText = "Your copies of its shared projects stay here as your own, and stop following the team. You'd need a new invite to join again."
        alert.addButton(withTitle: "Leave")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        whileBusy(BusyKey.teamLeave) { [self] in
            do {
                team = try await client.leaveTeam()
            } catch {
                show(error)
            }
        }
    }

    func removeFromTeam(_ member: TeamMember) {
        guard let client else { return }
        let alert = NSAlert()
        alert.messageText = "Take \(member.name) off the team?"
        alert.informativeText = "Their key stops working, their threads leave the boards, and what was waiting for them is dismissed. Their copies of the projects stay on their Mac."
        alert.addButton(withTitle: "Remove")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        whileBusy(BusyKey.teamRemove(member.memberId)) { [self] in
            do {
                team = try await client.removeFromTeam(memberId: member.memberId)
            } catch {
                show(error)
            }
        }
    }

    func shareProjectWithTeam(_ projectId: String) {
        guard let client else { return }
        whileBusy(BusyKey.teamShare(projectId), note: "Sharing with the team…") { [self] in
            do {
                team = try await client.shareProject(projectId)
            } catch {
                show(error)
            }
        }
    }

    func stopSharingProject(_ projectId: String) {
        guard let client, let project = project(projectId) else { return }
        let alert = NSAlert()
        alert.messageText = "Stop sharing \u{201C}\(project.title)\u{201D}?"
        alert.informativeText = "Everyone keeps their copy as their own, and the team's board and notes for it go."
        alert.addButton(withTitle: "Stop Sharing")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        whileBusy(BusyKey.teamShare(projectId), note: "Stopping sharing…") { [self] in
            do {
                team = try await client.unshareProject(projectId)
            } catch {
                show(error)
            }
        }
    }

    /// A linked copy here of a project a teammate shared, and opens it.
    func addSharedProject(_ shared: SharedProject) {
        guard let client else { return }
        whileBusy(BusyKey.teamAdd(shared.id)) { [self] in
            do {
                let added = try await client.addSharedProject(shared.id)
                loadProjects()
                openProjectId = added.projectId
                teamNote = added.missing.isEmpty ? nil : "No clone on this Mac of \(added.missing.joined(separator: ", ")). Clone it, then add its folder to the project."

            } catch {
                show(error)
            }
        }
    }

    func setTeamDelivery(_ projectId: String, deliver: Bool) {
        guard let client else { return }
        whileBusy(BusyKey.teamDelivery(projectId)) { [self] in
            do {
                team = try await client.setTeamDelivery(projectId, deliver: deliver)
                loadProjects()
            } catch {
                show(error)
            }
        }
    }

    /// Passes on, dismisses, takes or declines something waiting from a teammate.
    func answerTeam(_ waiting: TeamWaiting, _ answer: TeamAnswer) {
        guard let client else { return }
        whileBusy(BusyKey.teamAnswer(waiting.id)) { [self] in
            do {
                team = try await client.answerTeam(waiting.id, answer)
            } catch {
                show(error)
            }
        }
    }
}
