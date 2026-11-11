import AppKit
import SwiftUI
import ThreadbearCore

/// The sheet that makes or edits a schedule: the message, when it runs
/// (once, or a repeat preset that's only a cron string underneath), and
/// where it goes. What the runner makes of the time shows under it as it
/// changes, so a cron typo is plain before saving.
struct ScheduleEditor: View {
    @Environment(AppModel.self) private var app
    @State private var state: ScheduleEditorState
    @State private var preview: Result<SchedulePreview, AppModel.PreviewError>?
    @State private var saving = false
    @State private var saveError: String?
    /// This Mac's folder, kept while another runner is picked so picking
    /// this one again puts it back.
    @State private var localCwd: String

    init(state: ScheduleEditorState) {
        _state = State(initialValue: state)
        _localCwd = State(initialValue: state.isAway ? "" : state.cwd)
    }

    private var canSave: Bool {
        guard !saving, !state.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return false }
        if case .failure? = preview { return false }
        return !state.repeatChoice.cron.isEmpty || state.once
    }

    /// Why the button is greyed out, when it is.
    private var saveTip: String {
        if canSave { return "Save the schedule (Return)" }
        if saving { return "Saving…" }
        if state.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "Type a message first" }
        return "Fix when it sends first"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: zoomed(16)) {
            Text(state.isNew ? "Schedule a message" : "Edit schedule")
                .font(.system(size: zoomed(15), weight: .semibold))
            Form {
                message
                TextField("Title", text: $state.title, prompt: Text(titlePrompt))
                    .tip("Its name in the Scheduled list. Left empty, it's named after the message's first line.")
                when
                target
            }
            .formStyle(.columns)
            if let saveError {
                Text(saveError)
                    .font(.system(size: TypeScale.label))
                    .foregroundStyle(Color(nsColor: Theme.failed))
                    .fixedSize(horizontal: false, vertical: true)
            }
            buttons
        }
        .padding(zoomed(20))
        .frame(width: zoomed(520))
        // Asked again whenever the time changes; the pause lets a cron
        // being typed settle first, and a newer change cancels the ask.
        .task(id: state.when) {
            if case .cron(let cron) = state.when, cron.isEmpty {
                preview = .failure(AppModel.PreviewError(message: "Type a cron, such as 0 9 * * 1-5."))
                return
            }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let result = await app.previewSchedule(state.when)
            guard !Task.isCancelled else { return }
            preview = result
        }
    }

    /// The runner names it after the message's first line when there's no
    /// title. Showing that line here read as a title already typed in.
    private let titlePrompt = "Optional"

    @ViewBuilder
    private var message: some View {
        LabeledContent("Message") {
            VStack(alignment: .leading, spacing: zoomed(4)) {
                TextEditor(text: $state.text)
                    .font(.system(size: zoomed(13)))
                    .scrollContentBackground(.hidden)
                    .padding(.horizontal, zoomed(4))
                    .padding(.vertical, zoomed(5))
                    .frame(height: zoomed(84))
                    .background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: zoomed(6)))
                    .overlay(RoundedRectangle(cornerRadius: zoomed(6)).strokeBorder(.separator, lineWidth: 0.5))
                if state.draftImages > 0 {
                    Text(state.draftImages == 1 ? "The image stays in the box: a schedule sends only text." : "The images stay in the box: a schedule sends only text.")
                        .font(.system(size: TypeScale.sidebarMeta))
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder
    private var when: some View {
        Picker("When", selection: $state.once) {
            Text("Once").tag(true)
            Text("Repeat").tag(false)
        }
        .pickerStyle(.segmented)
        .fixedSize()
        .tip("Send it once, or again and again")
        if state.once {
            DatePicker("At", selection: $state.at, displayedComponents: [.date, .hourAndMinute])
                .datePickerStyle(.compact)
                .fixedSize()
                .tip("When to send it")
        } else {
            LabeledContent("Repeat") {
                HStack(spacing: zoomed(6)) {
                    Picker("Repeat", selection: $state.repeatChoice.kind) {
                        ForEach(RepeatChoice.Kind.allCases) { kind in
                            if kind == .custom { Divider() }
                            Label(kind.title, systemImage: kind.symbol).labelStyle(.titleAndIcon).tag(kind)
                        }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .tip("How often to send it")
                    repeatDetail
                }
            }
            if state.repeatChoice.kind == .custom {
                TextField("Cron", text: $state.repeatChoice.custom, prompt: Text("0 9 * * 1-5"))
                    .font(.system(size: zoomed(13), design: .monospaced))
                    .tip("Minute, hour, day of the month, month and day of the week")
            }
        }
        LabeledContent("") { previewText }
    }

    /// The parts of a preset that change: on which day, at what time.
    @ViewBuilder
    private var repeatDetail: some View {
        switch state.repeatChoice.kind {
        case .hourly, .custom:
            EmptyView()
        case .daily, .weekdays:
            timePicker
        case .weekly:
            Text("on").foregroundStyle(.secondary)
            Picker("Day", selection: $state.repeatChoice.weekday) {
                // Monday first; cron counts Sunday as 0.
                ForEach([1, 2, 3, 4, 5, 6, 0], id: \.self) { day in
                    Text(Calendar.current.weekdaySymbols[day]).tag(day)
                }
            }
            .labelsHidden()
            .fixedSize()
            .tip("Which day of the week")
            timePicker
        case .monthly:
            Text("on day").foregroundStyle(.secondary)
            Picker("Day", selection: $state.repeatChoice.day) {
                ForEach(1...31, id: \.self) { Text("\($0)").tag($0) }
            }
            .labelsHidden()
            .fixedSize()
            .tip("Which day of the month. Months without it are skipped.")
            timePicker
        }
    }

    private var timePicker: some View {
        HStack(spacing: zoomed(6)) {
            Text("at").foregroundStyle(.secondary)
            DatePicker("Time", selection: time, displayedComponents: .hourAndMinute)
                .labelsHidden()
                .datePickerStyle(.compact)
                .fixedSize()
                .tip("What time to send it")
        }
    }

    /// The preset's hour and minute as a date today, for the time picker.
    private var time: Binding<Date> {
        Binding(
            get: {
                Calendar.current.date(bySettingHour: state.repeatChoice.hour, minute: state.repeatChoice.minute, second: 0, of: Date()) ?? Date()
            },
            set: { date in
                let parts = Calendar.current.dateComponents([.hour, .minute], from: date)
                state.repeatChoice.hour = parts.hour ?? 9
                state.repeatChoice.minute = parts.minute ?? 0
            }
        )
    }

    /// "Weekdays at 9:00", then the next three runs; or why the runner
    /// can't read it, in the same place.
    @ViewBuilder
    private var previewText: some View {
        VStack(alignment: .leading, spacing: zoomed(2)) {
            switch preview {
            case .success(let result)?:
                Text(result.whenText)
                    .foregroundStyle(.secondary)
                if !result.runs.isEmpty {
                    Text("Next " + result.runs.prefix(3).map(Self.runText).joined(separator: " · "))
                        .foregroundStyle(Color(nsColor: Theme.faintText))
                } else {
                    Text(state.once ? "That's in the past, so it runs as soon as it's saved." : "It won't run again.")
                        .foregroundStyle(Color(nsColor: Theme.faintText))
                }
            case .failure(let error)?:
                Text(error.message)
                    .foregroundStyle(Color(nsColor: Theme.failed))
            case nil:
                Text(" ")
            }
        }
        .font(.system(size: TypeScale.sidebarMeta + 1))
        .fixedSize(horizontal: false, vertical: true)
        .frame(minHeight: zoomed(32), alignment: .top)
    }

    /// "Wed 28 Oct 9:00": short enough for three on one line.
    static func runText(_ date: Date) -> String {
        runFormatter.string(from: date).replacingOccurrences(of: ",", with: "").replacingOccurrences(of: " at ", with: " ")
    }

    private static let runFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.setLocalizedDateFormatFromTemplate("EEEdMMMjmm")
        return formatter
    }()

    @ViewBuilder
    private var target: some View {
        if state.isNew, let from = state.fromThreadId {
            Picker("Sends to", selection: $state.threadId) {
                Text("This thread").tag(Optional(from))
                Text("A new thread each time").tag(String?.none)
            }
            .pickerStyle(.radioGroup)
            .tip("Send it into this thread, or start a new thread with it each time")
        } else {
            LabeledContent("Sends to") {
                Text(targetText).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        if state.threadId == nil {
            runsOn
            if let away = awayName {
                // A path on that Mac: nothing here can check or choose it.
                LabeledContent("Folder") {
                    HStack(spacing: zoomed(6)) {
                        TextField("Folder", text: $state.cwd, prompt: Text("Default folder"))
                            .labelsHidden()
                            .tip("A folder on \(away). Left empty, \(away)'s default folder.")
                        InfoTip("A path on \(away), checked there when it runs. Its threads use \(away)'s default account and model.")
                    }
                }
            } else {
                LabeledContent("Folder") { FolderMenu(cwd: $state.cwd) }
                    .tip("The folder each new thread starts in")
                modelRows
            }
        }
    }

    /// The model and effort each new thread starts with, from the list of
    /// the login it runs on. Only for this Mac: another Mac's threads use
    /// its own defaults (the Folder row's tip says so).
    @ViewBuilder
    private var modelRows: some View {
        let models = ScheduleModels(accountId: state.accountId, poolId: state.poolId, model: state.model, logins: logins)
        LabeledContent("Model") {
            HStack(spacing: zoomed(6)) {
                Picker("Model", selection: modelChoice(models)) {
                    Text(models.defaultModelTitle).tag(String?.none)
                    Divider()
                    ForEach(models.rows.filter { !$0.isDefault && !$0.isOlder }) { Text($0.title).tag(Optional($0.id)) }
                    let older = models.rows.filter { !$0.isDefault && $0.isOlder }
                    if !older.isEmpty {
                        Divider()
                        ForEach(older) { Text($0.title).tag(Optional($0.id)) }
                    }
                }
                .labelsHidden()
                .fixedSize()
                .tip("The model each new thread starts on")
                Picker("Effort", selection: $state.effort) {
                    Text(models.defaultEffortTitle).tag(String?.none)
                    Divider()
                    ForEach(models.effortLevels, id: \.self) { level in
                        Text(ComposerChips.effortTitle(level)).tag(Optional(level))
                    }
                }
                .labelsHidden()
                .fixedSize()
                .tip("How hard it thinks")
            }
        }
    }

    /// The model menu's pick. Picking the default row stores nil, so the
    /// schedule follows the default if it changes; a level the new model
    /// hasn't got goes back to its default.
    private func modelChoice(_ models: ScheduleModels) -> Binding<String?> {
        Binding(
            // The default model chosen by name reads as Default: it's the same thread.
            get: { models.rows.first { $0.isSelected && !$0.isDefault }?.id },
            set: { model in
                state.model = model
                let levels = ScheduleModels(accountId: state.accountId, poolId: state.poolId, model: model, logins: logins).effortLevels
                if let effort = state.effort, !levels.contains(effort) { state.effort = nil }
            }
        )
    }

    /// This Mac's logins and pools, as the composer's pickers read them.
    private var logins: RunnerLogins { RunnerLogins(accounts: app.accounts, pools: app.pools, defaults: app.defaults) }

    /// Which Mac runs it, when there's more than one runner. Set when it's
    /// made: an existing schedule shows where it runs but can't move.
    @ViewBuilder
    private var runsOn: some View {
        let others = app.runners.filter { !$0.isSelf }
        if state.isNew, !others.isEmpty {
            Picker("Runs on", selection: $state.runnerId) {
                Label("This Mac", systemImage: app.selfRunnerIcon).labelStyle(.titleAndIcon).tag(String?.none)
                Divider()
                ForEach(others) { runner in
                    // Still a choice: it runs once that Mac is back.
                    Label(runner.online ? runner.name : "\(runner.name) (offline)", systemImage: RunnerIcon.symbol(runner.icon))
                        .labelStyle(.titleAndIcon)
                        .tag(Optional(runner.id))
                }
            }
            .fixedSize()
            .tip("Which Mac runs it and starts its threads")
            .onChange(of: state.runnerId) { old, new in
                if old == nil, new != nil {
                    localCwd = state.cwd
                    state.cwd = ""
                } else if new == nil {
                    state.cwd = localCwd.isEmpty ? app.defaults.cwd : localCwd
                }
            }
        } else if !state.isNew, state.isAway || !others.isEmpty {
            LabeledContent("Runs on") {
                Label(awayName ?? "This Mac", systemImage: awayIcon ?? app.selfRunnerIcon)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .tip("Where it runs. To run it somewhere else, make a new schedule.")
        }
    }

    /// The other runner's icon, when one is picked.
    private var awayIcon: String? {
        guard let id = state.runnerId else { return nil }
        return RunnerIcon.symbol(app.runners.first { $0.id == id }?.icon ?? app.schedule(state.scheduleId ?? "")?.runnerIcon)
    }

    /// The other runner's name, when one is picked.
    private var awayName: String? {
        guard let id = state.runnerId else { return nil }
        return app.runners.first { $0.id == id }?.name ?? state.runnerName ?? "another Mac"
    }

    private var targetText: String {
        if let threadId = state.threadId {
            let title = app.threads[threadId]?.title ?? ""
            return title.isEmpty ? "This thread" : title
        }
        return "A new thread each time"
    }

    private var buttons: some View {
        HStack {
            if let id = state.scheduleId {
                Button("Delete…") { app.deleteSchedule(id) }
                    .tip("Delete this schedule")
            }
            Spacer()
            Button("Cancel") { app.scheduleEditor = nil }
                .keyboardShortcut(.cancelAction)
                .tip("Close without saving (Esc)")
            Button(state.isNew ? "Schedule" : "Save") { save() }
                .keyboardShortcut(.defaultAction)
                .disabled(!canSave)
                .tip(saveTip)
        }
    }

    private func save() {
        saving = true
        saveError = nil
        Task {
            let error = await app.saveSchedule(state)
            saving = false
            if let error {
                saveError = error
            } else {
                app.scheduleEditor = nil
            }
        }
    }
}

/// The folder a new thread starts in: the recent ones, or Choose….
private struct FolderMenu: View {
    @Environment(AppModel.self) private var app
    @Binding var cwd: String

    var body: some View {
        IconMenu {
            ForEach(recent, id: \.self) { folder in
                Button(folder.abbreviatingHome, systemImage: "folder") { cwd = folder }
            }
            Divider()
            Button("Choose…", systemImage: "folder.badge.plus") { choose() }
        } label: {
            Text(cwd.abbreviatingHome)
        }
        .fixedSize()
        .tip(cwd.abbreviatingHome)
    }

    private var recent: [String] {
        var seen = Set<String>()
        var folders: [String] = []
        for folder in [cwd, app.defaults.cwd] + app.threads.values.filter({ !$0.isRemote }).sorted(by: { $0.lastActivityAt > $1.lastActivityAt }).map(\.cwd) {
            guard !folder.isEmpty, seen.insert(folder).inserted else { continue }
            folders.append(folder)
            if folders.count >= 8 { break }
        }
        return folders
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.directoryURL = URL(fileURLWithPath: cwd)
        if panel.runModal() == .OK, let url = panel.url { cwd = url.path }
    }
}
