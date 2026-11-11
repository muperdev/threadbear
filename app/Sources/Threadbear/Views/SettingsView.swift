import AppKit
import SwiftUI
import ThreadbearCore

/// The Settings window (⌘,): the runner's settings, in the grouped form
/// every Mac app's settings use. All of it lives in the runner, and this
/// only shows it and sends changes, apart from the appearance, the ding
/// and the activity log's switch, which are this Mac's own (AppAppearance,
/// DingPlayer, ActivityRecorder).
struct SettingsView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        @Bindable var app = app
        // A sidebar like System Settings rather than tabs along the top: the
        // window keeps one height and each pane scrolls, where tabs sized
        // the window to the pane and General grew taller than the screen
        // (10 Nov).
        NavigationSplitView(columnVisibility: .constant(.all)) {
            SettingsSidebar(selection: $app.settingsTab)
                // The column width alone is ignored in a Settings window (it
                // stayed at about 145, cutting "Phone & browser" short).
                .frame(minWidth: SettingsLayout.sidebarWidth)
                .navigationSplitViewColumnWidth(SettingsLayout.sidebarWidth)
                .toolbar(removing: .sidebarToggle)
        } detail: {
            pane(app.settingsTab)
                .navigationTitle(app.settingsTab.title)
        }
        .settingsSizes()
        .background(SettingsWindowFinder())
    }

    @ViewBuilder private func pane(_ tab: SettingsTab) -> some View {
        switch tab {
        case .general: GeneralSettings()
        case .dictation: DictationSettings()
        case .models: ModelSettings()
        case .pools: PoolSettings()
        case .runners: RunnersSettings()
        case .devices: DevicesSettings()
        case .team: TeamSettings()
        case .keyboard: KeyboardSettings()
        }
    }
}

/// The Settings sidebar: grey symbols, with the open section on a soft grey
/// fill rather than a List's blue selection (Mike preferred Ghostex's
/// settings to the blue, 10 Nov). Up and down arrows still move through it.
private struct SettingsSidebar: View {
    @Binding var selection: SettingsTab
    @State private var hovered: SettingsTab?
    @FocusState private var focused: Bool

    var body: some View {
        ScrollView {
            VStack(spacing: 2) {
                ForEach(SettingsTab.allCases, id: \.self) { tab in
                    row(tab)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
        }
        .scrollIndicators(.never)
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onMoveCommand { direction in
            let all = SettingsTab.allCases
            guard let at = all.firstIndex(of: selection) else { return }
            switch direction {
            case .up where at > all.startIndex: selection = all[at - 1]
            case .down where at < all.endIndex - 1: selection = all[at + 1]
            default: break
            }
        }
    }

    private func row(_ tab: SettingsTab) -> some View {
        let on = tab == selection
        return Button {
            selection = tab
        } label: {
            HStack(spacing: 9) {
                Image(systemName: tab.symbol)
                    .font(.system(size: 13))
                    .foregroundStyle(on ? .primary : .secondary)
                    .frame(width: 18)
                Text(tab.title)
                    .font(.system(size: 13, weight: on ? .medium : .regular))
                    .foregroundStyle(.primary)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .frame(height: 30)
            .background(
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .fill(Color.primary.opacity(on ? 0.1 : hovered == tab ? 0.05 : 0))
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 ? tab : (hovered == tab ? nil : hovered) }
    }
}

extension SettingsTab {
    var title: String {
        switch self {
        case .general: "General"
        case .dictation: "Dictation"
        case .models: "Models"
        case .pools: "Pools"
        case .runners: "Runners"
        case .devices: "Phone & browser"
        case .team: "Team"
        case .keyboard: "Keyboard"
        }
    }

    var symbol: String {
        switch self {
        case .general: "gearshape"
        case .dictation: "mic"
        case .models: "sparkles"
        case .pools: "square.stack"
        case .runners: "server.rack"
        case .devices: "iphone"
        case .team: "person.2"
        case .keyboard: "keyboard"
        }
    }
}

/// The Settings window's size. A grouped Form stops its rows at about 700
/// points of its own accord (macOS 26), so wider than that adds even sides.
enum SettingsLayout {
    static let sidebarWidth: CGFloat = 215
    /// Narrowest it goes: the sidebar and a pane whose rows still fit.
    static let minWidth: CGFloat = 640
    /// What the window opens at: the sidebar and a full-width form.
    static let idealWidth: CGFloat = 880
    static let minHeight: CGFloat = 420
    /// Short enough for a laptop screen with room for the Dock.
    static let idealHeight: CGFloat = 620
    /// Largest it goes, either way: bigger than any screen, but not infinite.
    /// SwiftUI makes the window's largest size the content's, and an
    /// unbounded one gave it an infinite size, which a Settings window can
    /// have as SwiftUI never lets one be resized (SettingsWindow.makeResizable
    /// does). Dragging its edge on macOS 27 then worked out an infinite frame,
    /// and AppKit threw, crashing the app (THREADBEAR-1S, 10 Nov).
    static let maxSide: CGFloat = 10_000
}

extension View {
    /// The Settings window's size: drag it wider or taller for more room; it
    /// opens at a size that fits a laptop screen, and General's rows scroll
    /// past that.
    func settingsSizes() -> some View {
        frame(minWidth: SettingsLayout.minWidth, idealWidth: SettingsLayout.idealWidth, maxWidth: SettingsLayout.maxSide,
              minHeight: SettingsLayout.minHeight, idealHeight: SettingsLayout.idealHeight, maxHeight: SettingsLayout.maxSide)
    }
}

/// What the runner's settings tabs show until there are settings to show.
private struct SettingsGate<Content: View>: View {
    @Environment(AppModel.self) private var app
    @ViewBuilder var content: (RunnerSettings) -> Content

    var body: some View {
        if let settings = app.runnerSettings {
            content(settings)
        } else {
            Form {
                Text(settingsWaiting(app))
                    .foregroundStyle(.secondary)
            }
            .formStyle(.grouped)
        }
    }
}

/// Why there are no runner settings to show yet.
@MainActor
private func settingsWaiting(_ app: AppModel) -> String {
    if app.settingsUnsupported { return "This runner is too old for settings. It gets them when it next updates." }
    if app.connection != .connected { return "Not connected to the runner." }
    return "Loading…"
}

/// The last change that didn't take, under the form it came from.
private struct SettingsErrorRow: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        if let error = app.settingsError {
            Section {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
            }
        }
    }
}

// MARK: General

private struct GeneralSettings: View {
    @Environment(AppModel.self) private var app
    @AppStorage(ActivityRecorder.enabledKey) private var activityLog = false
    @AppStorage(AppAppearance.key) private var appearance: AppAppearance = .system
    @AppStorage(CompletionSound.enabledKey) private var dingOn = false
    @AppStorage(CompletionSound.dingKey) private var ding = CompletionSound.Ding.chime.rawValue
    @AppStorage(CompletionSound.volumeKey) private var dingVolume = CompletionSound.defaultVolume
    @AppStorage(QuitPrompt.askKey) private var askBeforeQuitting = true

    var body: some View {
        if app.runnerSettings == nil {
            // Appearance and the ding are this Mac's own, so they're here
            // while the runner's away too; the rest waits for it.
            Form {
                thisMac
                Section {
                    Text(settingsWaiting(app))
                        .foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
        } else {
            runner
        }
    }

    /// The settings kept on this Mac (UserDefaults), not in the runner.
    @ViewBuilder private var thisMac: some View {
        SettingsSection("Appearance",
                        about: "Light or dark for Threadbear only. Same as the Mac follows System Settings > Appearance.",
                        ask: "change it for you.") {
            Picker("Appearance", selection: $appearance) {
                ForEach(AppAppearance.allCases) { Text($0.title).tag($0) }
            }
            .tip("Light or dark for Threadbear, whatever the Mac is set to")
            .onChange(of: appearance) { _, new in new.apply() }
        }

        SettingsSection("Quitting",
                        about: "Quit Threadbear (⌘Q or the Threadbear menu) asks first, so a stray ⌘Q doesn't close the app. Ticking Don't ask again turns this off; turn it back on here. Your threads keep running on the runner while the app is closed.",
                        ask: "change it for you.") {
            Toggle("Ask before quitting", isOn: $askBeforeQuitting)
                .tip("Ask to confirm when you quit Threadbear")
        }

        SettingsSection("Notifications",
                        about: "A soft ding when a thread finishes working: its reply is done, it's waiting for your answer, or it hit an error. It plays wherever you are, even in another app, and stays quiet for a thread you stopped yourself. Notifications themselves are in System Settings > Notifications.",
                        ask: "change when the ding plays.") {
            Toggle("Play a sound when a thread finishes", isOn: $dingOn)
                .tip("A soft ding when a thread is done and ready for you")
                .onChange(of: dingOn) { _, on in if on { DingPlayer.play() } }
            Picker("Sound", selection: $ding) {
                ForEach(CompletionSound.Ding.allCases) { Text($0.title).tag($0.rawValue) }
            }
            .tip("Which ding plays")
            .onChange(of: ding) { _, new in DingPlayer.play(CompletionSound.ding(new)) }
            LabeledContent("Volume") {
                HStack(spacing: 8) {
                    Slider(value: $dingVolume, in: 0.1...1) { editing in if !editing { DingPlayer.play() } }
                        .frame(maxWidth: 180)
                        .tip("How loud the ding is")
                    Button("Play") { DingPlayer.play() }
                        .tip("Hear the ding")
                }
            }
        }
    }

    private var runner: some View {
        SettingsGate { settings in
            Form {
                thisMac

                SettingsSection("New threads",
                                about: "Where a new thread starts when you don't pick a folder.",
                                ask: "change it, or anything else about how threads run. Most settings aren't shown here, and your agent edits them in runner.json.") {
                    LabeledContent("Default folder") {
                        HStack(spacing: 8) {
                            Text(SettingsText.folder(settings.defaultCwd))
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .tip(settings.defaultCwd)
                            Button("Choose…") { chooseFolder(from: settings.defaultCwd) }
                                .tip("Pick the folder new threads start in")
                        }
                    }
                }

                InstructionsSection()

                // An experiment from 6 Nov until Mike made it standard on
                // 10 Nov 2026. A switch rather than always on: with no login
                // of the other provider it reviews on the same one, which
                // costs tokens and isn't another lab, so people may want it off.
                if settings.labReviewSupported {
                    SettingsSection("Code review",
                                    about: "When a thread changes code and is done, Threadbear has your other provider (Codex for a Claude thread, Claude for a Codex one) try to break the change in a review thread of its own. Your thread waits for what it finds, fixes what holds up, and tells you in its reply. With only one provider, a fresh session of the same one reviews instead.",
                                    ask: "explain what a review found, or turn reviews on or off for you.") {
                        Toggle("Review code changes before a thread is done",
                               isOn: Binding(get: { settings.labReview },
                                             set: { app.updateSettings(SettingsUpdate(labReview: $0)) }))
                            .tip("Have another lab check each code change a thread makes")
                    }
                }

                // One reply, Bullet replies, the two live-CLI steppers and
                // Follow main left here on 2 Nov: Mike wants as few settings
                // as possible, the defaults suit nearly everyone, and anyone
                // who wants one changed asks an agent to edit runner.json.

                // Why it's worth turning on (Mike, 29 Oct), then what's in it,
                // for anyone wary of turning it on.
                SettingsSection("Activity log",
                                about: "When you ask a thread to change Threadbear, its agent can check this log first to see how you really use the app. It notes the buttons, rows and menu items you use (by their tooltips and titles), your shortcuts, the threads and pages you open, and when you send or answer something, never what you type. It stays on this Mac for 30 days and Threadbear never sends it anywhere.",
                                ask: "read it with tb activity and tell you what's in it.") {
                    Toggle("Keep an activity log", isOn: $activityLog)
                        .tip("Note what you click and open in this app")
                }

                // On by default (an experiment on Mike's Mac, 6 to 10 Nov);
                // Mike wanted a switch for anyone who'd rather it slept.
                // Only a Mac runner sends it.
                if let keepAwake = settings.keepAwake {
                    SettingsSection("Sleep",
                                    about: "While any thread is working, and for three minutes after, your Mac won't go to sleep on its own, so a long turn, or one you started from your phone, isn't cut off halfway. The screen still dims and locks as usual. It stops below 15% battery when you're not plugged in, and closing the lid on battery still puts the Mac to sleep.",
                                    ask: "check whether it's holding your Mac awake right now.") {
                        Toggle("Keep the Mac awake while threads work",
                               isOn: Binding(get: { keepAwake }, set: { app.updateSettings(SettingsUpdate(keepAwake: $0)) }))
                            .tip("Stop your Mac sleeping on its own while a thread is working")
                    }
                }

                // Only on someone's fork: Mike's own copy has nothing to
                // bring in (its upstream check is off).
                if settings.bringInSupported, let upstream = app.runnerUpdates?.upstream, upstream.state != "off" {
                    SettingsSection("Updates from Mike",
                                    about: "When Mike has a release your copy doesn't have, Threadbear starts a thread that merges it in and runs the checks, then asks you before it lands. Never more than one at a time. A release Mike marks important doesn't wait.",
                                    ask: "bring in Mike's latest release now, or change how often this happens.") {
                        Picker("Bring them in", selection: Binding(get: { settings.bringIn },
                                                                   set: { app.updateSettings(SettingsUpdate(bringIn: $0)) })) {
                            ForEach(SettingsText.bringInChoices(current: settings.bringIn), id: \.value) { choice in
                                Text(choice.title).tag(choice.value)
                            }
                        }
                        .tip("How often a thread starts to bring in Mike's new releases")
                    }
                }

                // runner.json's `experiments`, so nobody has to edit it by
                // hand. The list comes from the runner (each
                // runner/src/experiments/<id>.md), so a new experiment shows
                // here by itself, and each switch is read at its next use.
                if !settings.experiments.isEmpty {
                    SettingsSection("Experiments",
                                    about: "New features still being tried out. Each starts off; turn one on to try it, and off again whenever you like. Once one has proved itself it becomes a standard part of Threadbear and its switch goes. The ⓘ beside each says what it does.",
                                    ask: "explain an experiment, or turn one on or off for you.") {
                        ForEach(settings.experiments) { experiment in
                            ExperimentRow(experiment: experiment) { on in
                                app.updateSettings(SettingsUpdate(experiments: [experiment.id: on]))
                            }
                        }
                    }
                }

                SettingsSection("Runner",
                                about: "The background program that runs your threads. It updates itself.",
                                ask: "check on it if something seems wrong. Your agent can read its status and logs.") {
                    LabeledContent("Version") {
                        Text(app.runnerVersion.isEmpty ? "Not connected" : app.runnerVersion)
                            .textSelection(.enabled)
                    }
                }

                SettingsErrorRow()
            }
            .task { app.loadStandingInstructions() }
            .formStyle(.grouped)
        }
    }

    private func chooseFolder(from current: String) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.directoryURL = URL(fileURLWithPath: (current as NSString).expandingTildeInPath)
        if panel.runModal() == .OK, let url = panel.url {
            app.updateSettings(SettingsUpdate(defaultCwd: url.path))
        }
    }
}

// MARK: Models

private struct ModelSettings: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        SettingsGate { settings in
            Form {
                if settings.models.poolSupported {
                    SettingsSection("Small jobs",
                                    about: "Titles and the librarian use Haiku on one of this pool's Claude logins, the one with the most room, and count towards its usage. When every login is out of room they wait. None turns them off.",
                                    ask: "change this, or which model and effort new threads start on. Your agent edits them in runner.json.") {
                        Picker("Run on", selection: poolBinding(settings)) {
                            ForEach(claudePools) { pool in
                                Label(pool.title, systemImage: Pools.symbol(pool, accounts: app.accounts)).labelStyle(.titleAndIcon).tag(Optional(pool.id))
                            }
                            // A pool deleted since it was picked, so the
                            // picker still says what's set.
                            if let id = settings.models.pool, !claudePools.contains(where: { $0.id == id }) {
                                Label("\(id), which has no Claude login", systemImage: "exclamationmark.triangle").labelStyle(.titleAndIcon).tag(Optional(id))
                            }
                            Divider()
                            Label("None", systemImage: "nosign").labelStyle(.titleAndIcon).tag(String?.none)
                        }
                        .tip("Which pool's Claude logins write titles and do the librarian's work")
                    }
                }

                SettingsSection("Private folders",
                                about: "Nothing from threads in these folders, or in folders inside them, is sent for titles and other small jobs.",
                                ask: "add several at once. Your agent keeps them in runner.json.") {
                    if settings.models.privateFolders.isEmpty {
                        Text("None yet")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(settings.models.privateFolders, id: \.self) { folder in
                        HStack {
                            Image(systemName: "folder")
                                .foregroundStyle(.secondary)
                            Text(folder)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer()
                            Button {
                                app.updateSettings(SettingsUpdate(privateFolders: settings.models.privateFolders.filter { $0 != folder }))
                            } label: {
                                Image(systemName: "minus.circle")
                            }
                            .buttonStyle(.borderless)
                            .tip("Stop keeping this folder private")
                        }
                    }
                    Button("Add Folder…") { addPrivateFolder(settings.models.privateFolders) }
                        .tip("Keep a folder's threads from ever being sent for titles and other small jobs")
                }

                SettingsErrorRow()
            }
            .formStyle(.grouped)
        }
    }

    private var claudePools: [Pool] { app.claudePools }

    private func poolBinding(_ settings: RunnerSettings) -> Binding<String?> {
        Binding(get: { settings.models.pool }, set: { app.updateSettings(SettingsUpdate(pool: .some($0))) })
    }

    private func addPrivateFolder(_ current: [String]) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.prompt = "Add"
        guard panel.runModal() == .OK else { return }
        var folders = current
        for url in panel.urls {
            let folder = SettingsText.folder(url.path)
            if !folders.contains(folder) { folders.append(folder) }
        }
        app.updateSettings(SettingsUpdate(privateFolders: folders))
    }
}

// MARK: Pools

/// Settings › Pools (docs/pools-plan.md): the pools, and for the one picked,
/// its name, which logins are in it and in what order, and the folders
/// whose new threads start on it. A change is saved (`pools.save`, which
/// writes runner.json) as soon as the list is one the runner would take;
/// until then the window says what's missing.
private struct PoolSettings: View {
    @Environment(AppModel.self) private var app
    /// What's shown, which can run ahead of the runner's list while a new
    /// pool has no logins yet.
    @State private var draft: [Pool] = []
    @State private var selected: String?
    @State private var titleDraft = ""
    @FocusState private var naming: Bool

    var body: some View {
        if !app.poolsSupported {
            Form {
                Text(app.connection == .connected ? "This runner is too old for pools. It gets them when it next updates." : "Not connected to the runner.")
                    .foregroundStyle(.secondary)
            }
            .formStyle(.grouped)
        } else {
            form
                .onAppear { reset(to: app.pools) }
                // The runner's list wins, unless Mike is part way through
                // a change it wouldn't take yet.
                .onChange(of: app.pools) { _, pools in
                    if Pools.problem(draft, accounts: app.accounts) == nil { reset(to: pools) }
                }
        }
    }

    private var form: some View {
        Form {
            SettingsSection("Pools",
                            about: "New threads start on their folder's pool, or on the one you picked last. Each turn runs on a login with room, and the thread moves on when that login runs out.",
                            ask: "set up pools for you, such as one for work and one for everything else, or to add another login.") {
                ForEach(draft) { pool in
                    PoolListRow(pool: pool, accounts: app.accounts, selected: pool.id == current?.id) { choose(pool.id) }
                }
                HStack(spacing: 2) {
                    Button(action: add) { Image(systemName: "plus") }
                        .tip("Add a pool")
                    Button {
                        if let pool = current { delete(pool) }
                    } label: {
                        Image(systemName: "minus")
                    }
                    .disabled(draft.count < 2 || current == nil)
                    .tip(draft.count < 2 ? "There has to be at least one pool" : "Delete \(current?.title ?? "this pool")")
                }
                .buttonStyle(.borderless)
            }

            if let pool = current {
                SettingsSection(pool.title,
                                about: "Tick the logins this pool can use. A mixed pool tries providers in this order; within one provider, the login with the most room goes first.",
                                ask: "change its logins or their order, or add a login that isn't listed.") {
                    // A boxed field with the text from its left edge: the
                    // grouped form's own right-aligned one reads as a label,
                    // so renaming was easy to miss.
                    LabeledContent("Name") {
                        TextField("Name", text: $titleDraft, prompt: Text("Pool name"))
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .multilineTextAlignment(.leading)
                            .frame(width: 220)
                            .focused($naming)
                            .onSubmit(commitTitle)
                            .tip("Rename this pool. It saves when you press Return or click away.")
                    }
                    LabeledContent("Icon") {
                        PoolIconPicker(symbol: Pools.symbol(pool, accounts: app.accounts)) { icon in
                            var changed = pool
                            changed.icon = icon
                            change(changed)
                        }
                    }

                    let list = Pools.loginList(pool, accounts: app.accounts)
                    ForEach(list, id: \.account.id) { entry in
                        LoginToggleRow(account: entry.account, accounts: app.accounts, included: entry.included,
                                       canMoveUp: entry.included && pool.accountIds.first != entry.account.id,
                                       canMoveDown: entry.included && pool.accountIds.last != entry.account.id,
                                       toggle: { change(Pools.setting(pool, login: entry.account.id, included: $0)) },
                                       move: { change(Pools.moving(pool, login: entry.account.id, by: $0)) },
                                       drop: { change(Pools.moving(pool, login: $0, onto: entry.account.id)) })
                    }
                }

                SettingsSection("Folders",
                                about: "New threads in these folders, or a folder inside one, start on \(pool.title).",
                                ask: "add folders for you, or move a project's folders to another pool.") {
                    if pool.folders.isEmpty {
                        Text("None yet")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(pool.folders, id: \.self) { folder in
                        HStack {
                            Image(systemName: "folder")
                                .foregroundStyle(.secondary)
                            Text(folder)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer()
                            Button {
                                var changed = pool
                                changed.folders.removeAll { $0 == folder }
                                change(changed)
                            } label: {
                                Image(systemName: "minus.circle")
                            }
                            .buttonStyle(.borderless)
                            .tip("Stop starting threads in this folder on \(pool.title)")
                        }
                    }
                    Button("Add Folder…") { addFolders(to: pool) }
                        .tip("Pick folders whose new threads start on \(pool.title)")
                }
            }

            if let problem = Pools.problem(draft, accounts: app.accounts) {
                Section {
                    Label(problem, systemImage: "info.circle")
                        .foregroundStyle(.secondary)
                }
            } else if let error = app.poolsError {
                Section {
                    Label(error, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                }
            }
        }
        .formStyle(.grouped)
        // A name typed and then left (another field, a click elsewhere, or
        // the window closing) is saved, not just one ended with Return.
        .onChange(of: naming) { _, focused in if !focused { commitTitle() } }
        .onDisappear(perform: commitTitle)
    }

    private var current: Pool? { draft.first { $0.id == selected } ?? draft.first }

    private func reset(to pools: [Pool]) {
        draft = pools
        if selected == nil || !pools.contains(where: { $0.id == selected }) { selected = pools.first?.id }
        titleDraft = current?.title ?? ""
    }

    private func choose(_ id: String) {
        commitTitle()
        selected = id
        titleDraft = current?.title ?? ""
    }

    /// Replaces one pool in the draft, and saves the draft if the runner
    /// would take it.
    private func change(_ pool: Pool) {
        guard let index = draft.firstIndex(where: { $0.id == pool.id }) else { return }
        draft[index] = pool
        save()
    }

    private func save() {
        guard Pools.problem(draft, accounts: app.accounts) == nil else { return }
        let list = draft
        Task { await app.savePools(list) }
    }

    private func commitTitle() {
        guard let pool = current else { return }
        guard let renamed = Pools.renaming(pool, to: titleDraft) else {
            // Blank: back to the name it has.
            titleDraft = pool.title
            return
        }
        titleDraft = renamed.title
        change(renamed)
    }

    private func add() {
        commitTitle()
        let pool = Pool(id: Pools.newId(title: "New pool", taken: draft.map(\.id)), title: "New pool", accountIds: [])
        draft.append(pool)
        selected = pool.id
        titleDraft = pool.title
    }

    private func delete(_ pool: Pool) {
        let alert = NSAlert()
        alert.messageText = "Delete \(pool.title)?"
        alert.informativeText = "Threads on it carry on with the login they're on, pinned to it."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        alert.buttons.first?.hasDestructiveAction = true
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        draft.removeAll { $0.id == pool.id }
        selected = draft.first?.id
        titleDraft = current?.title ?? ""
        save()
    }

    private func addFolders(to pool: Pool) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.prompt = "Add"
        guard panel.runModal() == .OK else { return }
        var changed = pool
        for url in panel.urls {
            let folder = SettingsText.folder(url.path)
            if !changed.folders.contains(folder) { changed.folders.append(folder) }
        }
        change(changed)
    }
}

/// A pool in Settings' list: its name, and its logins and folders in grey.
private struct PoolListRow: View {
    let pool: Pool
    let accounts: [Account]
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                PoolSymbol(symbol: Pools.symbol(pool, accounts: accounts))
                    .foregroundStyle(selected ? Color.accentColor : .secondary)
                Text(pool.title)
                    .fontWeight(selected ? .semibold : .regular)
                    .foregroundStyle(.primary)
                    .layoutPriority(1)
                Spacer(minLength: 12)
                Text(detail)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            .contentShape(Rectangle())
            // The pool being edited below is the one tinted, like a
            // sidebar's selection, rather than a faint tick at the end of
            // its row. Drawn here as the grouped form on macOS ignores
            // listRowBackground.
            .background {
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color.accentColor.opacity(selected ? 0.2 : 0))
                    .padding(.horizontal, -8)
                    .padding(.vertical, -5)
            }
        }
        .buttonStyle(.plain)
        .tip(selected ? "Editing \(pool.title) below" : "Edit \(pool.title)")
        .accessibilityLabel(pool.title + (selected ? ", showing" : ""))
    }

    /// "acme, gmail, bearworks · ~/dev/convex".
    private var detail: String {
        let logins = pool.accountIds.map { AccountName.login($0, accounts: accounts) }
        var text = logins.isEmpty ? "No logins yet" : logins.joined(separator: ", ")
        if !pool.folders.isEmpty { text += " · " + pool.folders.joined(separator: ", ") }
        return text
    }
}

/// A pool's icon at a fixed width, so titles line up whichever symbol it
/// is. One that this Mac doesn't have shows as a stack.
private struct PoolSymbol: View {
    let symbol: String

    var body: some View {
        Image(systemName: ProjectIconImages.symbolExists(symbol) ? symbol : "square.stack")
            .frame(width: 20)
            .accessibilityHidden(true)
    }
}

/// The Icon row: the pool's icon as a button that opens a grid of the
/// choices (`Pools.iconChoices`).
private struct PoolIconPicker: View {
    let symbol: String
    let pick: (String) -> Void
    @State private var open = false

    var body: some View {
        Button { open = true } label: {
            HStack(spacing: 4) {
                PoolSymbol(symbol: symbol)
                Image(systemName: "chevron.down")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(.secondary)
            }
        }
        .tip("Pick this pool's icon. It shows in front of the model chip under the composer.")
        .popover(isPresented: $open, arrowEdge: .bottom) {
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(30), spacing: 4), count: 7), spacing: 4) {
                ForEach(Pools.iconChoices.filter(ProjectIconImages.symbolExists), id: \.self) { choice in
                    Button {
                        pick(choice)
                        open = false
                    } label: {
                        Image(systemName: choice)
                            .font(.system(size: 14))
                            .frame(width: 30, height: 30)
                            .background(
                                RoundedRectangle(cornerRadius: 6, style: .continuous)
                                    .fill(choice == symbol ? Color.accentColor.opacity(0.25) : .clear)
                            )
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .tip(choice)
                }
            }
            .padding(10)
        }
    }
}

/// A login in a pool: ticked if it's in, dragged (or moved with its
/// arrows) into order.
private struct LoginToggleRow: View {
    let account: Account
    let accounts: [Account]
    let included: Bool
    let canMoveUp: Bool
    let canMoveDown: Bool
    let toggle: (Bool) -> Void
    let move: (Int) -> Void
    /// Another ticked login dropped here takes this one's place.
    let drop: (String) -> Void

    var body: some View {
        row
            // Ticked logins drag into order; the arrows do the same for
            // anyone who'd rather click.
            .draggable(account.id)
            .dropDestination(for: String.self) { ids, _ in
                guard included, let id = ids.first, id != account.id else { return false }
                drop(id)
                return true
            }
    }

    private var row: some View {
        HStack(spacing: 6) {
            Toggle(isOn: Binding(get: { included }, set: toggle)) {
                Text(AccountName.title(account)) + Text("  " + detail).foregroundStyle(.secondary)
            }
            .toggleStyle(.checkbox)
            .lineLimit(1)
            Spacer()
            if included {
                Button { move(-1) } label: { Image(systemName: "chevron.up") }
                    .buttonStyle(.borderless)
                    .disabled(!canMoveUp)
                    .tip("Earlier in the order")
                Button { move(1) } label: { Image(systemName: "chevron.down") }
                    .buttonStyle(.borderless)
                    .disabled(!canMoveDown)
                    .tip("Later in the order")
            }
        }
    }

    /// "Claude · Convex work only".
    private var detail: String {
        [ModelPicker.providerTitle(account.provider), account.note].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// One experiment's switch in Settings > General, with what it does behind
/// its ⓘ.
private struct ExperimentRow: View {
    let experiment: RunnerSettings.Experiment
    let set: (Bool) -> Void

    var body: some View {
        Toggle(isOn: Binding(get: { experiment.on }, set: set)) {
            HStack(spacing: 6) {
                Text(experiment.name)
                if !experiment.does.isEmpty { InfoTip(SettingsText.experimentAbout(experiment.does)) }
            }
        }
        .tip(experiment.on ? "Turn \(experiment.name) off" : "Try \(experiment.name)")
    }
}

/// A section of settings: its title, and behind the ⓘ what it is and what to
/// ask your agent. Agents can edit runner.json and the rest, so the window
/// shows what's set with few controls, and every section says to ask your
/// agent first (Mike, 5 Nov). Taking `ask` as the end of a sentence keeps
/// that line in every one. Sections with only a message or an error in them
/// stay plain `Section`s (SettingsInfoTests checks).
struct SettingsSection<Content: View>: View {
    let title: String
    let about: String
    let ask: String
    @ViewBuilder var content: () -> Content

    init(_ title: String, about: String, ask: String, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.about = about
        self.ask = ask
        self.content = content
    }

    var body: some View {
        Section {
            content()
        } header: {
            HStack(spacing: 6) {
                Text(title)
                InfoTip("\(about)\n\nAsk your agent to \(ask)")
            }
        }
    }
}

/// Remembers the Settings window, so the automation hook can bring it up,
/// keep it behind and capture it like the main window.
private struct SettingsWindowFinder: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            SettingsWindow.current = view.window
            if let window = view.window { SettingsWindow.makeResizable(window) }
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        if let window = nsView.window {
            SettingsWindow.current = window
            SettingsWindow.makeResizable(window)
        }
    }
}

@MainActor
enum SettingsWindow {
    static weak var current: NSWindow?

    /// A Settings scene's window comes fixed size; this one can be dragged
    /// wider and taller. It also opens at the panes' smallest height (the
    /// scene ignores an ideal height, and `.defaultSize` on it stopped the
    /// window opening at all), so the first time it's grown to the ideal,
    /// as far as the screen allows.
    static func makeResizable(_ window: NSWindow) {
        guard !window.styleMask.contains(.resizable) else { return }
        window.styleMask.insert(.resizable)
        window.contentMinSize = NSSize(width: SettingsLayout.minWidth, height: SettingsLayout.minHeight)
        let room = (window.screen ?? NSScreen.main)?.visibleFrame.height ?? SettingsLayout.idealHeight
        let chrome = window.frame.height - window.contentLayoutRect.height
        let height = min(SettingsLayout.idealHeight, room - chrome - 40)
        guard window.contentLayoutRect.height < height else { return }
        var frame = window.frame
        // Grow downwards from where the top already is.
        frame.origin.y -= height - window.contentLayoutRect.height
        frame.size.height = height + chrome
        window.setFrame(frame, display: true)
    }
}
