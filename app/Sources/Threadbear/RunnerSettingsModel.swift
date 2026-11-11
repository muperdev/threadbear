import AppKit
import ThreadbearCore

enum SettingsTab: String, Hashable, CaseIterable {
    case general, dictation, models, pools, runners, devices, team, keyboard
}

/// The Settings window's side of AppModel. Everything in it is the
/// runner's (runner.json); the app keeps none of it.
extension AppModel {
    /// After hello; `settings.updated` replaces them from then on.
    func loadSettings() {
        guard let client else { return }
        Task {
            do {
                receive(try await client.settingsGet())
            } catch RPCError.server(let code, _) where code == RPCError.methodNotFound {
                // A runner from before settings.
                settingsUnsupported = true
                runnerSettings = nil
            } catch {
                AppLog.write("rpc", "settings.get failed: \(error.localizedDescription)")
            }
        }
    }

    func receive(_ settings: RunnerSettings) {
        runnerSettings = settings
        settingsUnsupported = false
        // The new-thread composer starts in the default folder.
        if !settings.defaultCwd.isEmpty { defaults.cwd = (settings.defaultCwd as NSString).expandingTildeInPath }
    }

    /// Pools with a login that can take small jobs (titles, the librarian):
    /// a Claude one, or an ACP agent's in a pool of its own. Settings › Models and the Librarian's page pick
    /// from these.
    var claudePools: [Pool] {
        pools.filter { pool in
            pool.accountIds.contains { id in accounts.contains { $0.id == id && Providers.info($0.provider).smallJobs } }
        }
    }

    /// Settings › Models, from somewhere else in the app.
    func openModelSettings() {
        settingsTab = .models
        settingsRequest += 1
    }

    /// Changes what's given. The window shows the change straight away;
    /// the runner's answer (or its error) then has the last word.
    func updateSettings(_ update: SettingsUpdate) {
        guard let client else { return settingsError = "Not connected to the runner." }
        if var shown = runnerSettings {
            if let v = update.defaultCwd { shown.defaultCwd = v }
            if let v = update.idleMinutes { shown.idleMinutes = v }
            if let v = update.maxLiveCLIs { shown.maxLiveCLIs = v }
            if let v = update.followMain { shown.followMain = v }
            if let v = update.bulletReplies { shown.bulletReplies = v }
            if let v = update.oneReply { shown.oneReply = v }
            if let v = update.labReview { shown.labReview = v }
            if let v = update.keepAwake { shown.keepAwake = v }
            if let v = update.bringIn { shown.bringIn = v }
            if let v = update.pool { shown.models.pool = v }
            if let v = update.privateFolders { shown.models.privateFolders = v }
            if let v = update.experiments {
                for i in shown.experiments.indices { if let on = v[shown.experiments[i].id] { shown.experiments[i].on = on } }
            }
            runnerSettings = shown
        }
        Task {
            do {
                receive(try await client.settingsUpdate(update))
                settingsError = nil
                // Standing instructions' box shows only while its
                // experiment is on, so ask again rather than wait for a reopen.
                if update.experiments != nil { loadStandingInstructions() }
            } catch {
                settingsError = error.localizedDescription
                AppLog.write("rpc", "settings.update failed: \(error.localizedDescription)")
                loadSettings()
            }
        }
    }

}
