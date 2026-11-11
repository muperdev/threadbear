import Foundation

/// A project's part of the sidebar's Projects view: its header, and its
/// threads under it.
public struct ProjectSection: Sendable, Equatable, Identifiable {
    public var project: Project
    /// Its threads in the inbox (not put away), newest activity first.
    public var threads: [ThreadSummary]
    /// Its archived threads, newest activity first: folded away under the
    /// open ones in the sidebar, a few at a time.
    public var archived: [ThreadSummary] = []
    /// Its newest thread's activity (put away ones too), else the runner's.
    public var lastActivityAt: Date

    public var id: String { project.id }
}

/// The Projects view: a section per project, newest activity first, then
/// the threads in no project.
public struct ProjectSections: Sendable, Equatable {
    public var sections: [ProjectSection] = []
    /// Inbox threads in no project the view shows (none, or only archived
    /// or unknown ones), newest first.
    public var unfiled: [ThreadSummary] = []

    public init(sections: [ProjectSection] = [], unfiled: [ThreadSummary] = []) {
        self.sections = sections
        self.unfiled = unfiled
    }

    /// Archived projects are left out, and so are archived and hibernating
    /// threads, as in the inbox (archived ones go in `archived`). A thread
    /// in two projects is under both.
    public static func build(projects: [Project], threads: some Sequence<ThreadSummary>, now: Date) -> ProjectSections {
        let shown = projects.filter { !$0.archived }
        let shownIds = Set(shown.map(\.id))
        var members: [String: [ThreadSummary]] = [:]
        var archived: [String: [ThreadSummary]] = [:]
        var newest: [String: Date] = [:]
        var unfiled: [ThreadSummary] = []
        for thread in threads {
            let inbox = !thread.isArchived && !thread.isSnoozed(at: now)
            var filed = false
            for membership in thread.projects where shownIds.contains(membership.id) {
                filed = true
                newest[membership.id] = max(newest[membership.id] ?? .distantPast, thread.lastActivityAt)
                if inbox { members[membership.id, default: []].append(thread) }
                if thread.isArchived { archived[membership.id, default: []].append(thread) }
            }
            if inbox && !filed { unfiled.append(thread) }
        }
        let newestFirst: (ThreadSummary, ThreadSummary) -> Bool = {
            $0.lastActivityAt != $1.lastActivityAt ? $0.lastActivityAt > $1.lastActivityAt : $0.id < $1.id
        }
        var sections = shown.map { project in
            ProjectSection(
                project: project,
                threads: (members[project.id] ?? []).sorted(by: newestFirst),
                archived: (archived[project.id] ?? []).sorted(by: newestFirst),
                lastActivityAt: max(newest[project.id] ?? .distantPast, project.lastActivityAt ?? project.updatedAt)
            )
        }
        sections.sort {
            $0.lastActivityAt != $1.lastActivityAt ? $0.lastActivityAt > $1.lastActivityAt
                : $0.project.title.localizedStandardCompare($1.project.title) == .orderedAscending
        }
        return ProjectSections(sections: sections, unfiled: unfiled.sorted(by: newestFirst))
    }

    /// Thread ids top to bottom, for the arrow keys. A thread in two
    /// projects is in it twice; the first is where the arrows land.
    public var order: [String] {
        var seen = Set<String>()
        return (sections.flatMap(\.threads) + unfiled).map(\.id).filter { seen.insert($0).inserted }
    }

    /// A project's threads (any, put away ones too), newest activity first,
    /// for its page.
    public static func threads(in projectId: String, from threads: some Sequence<ThreadSummary>) -> [ThreadSummary] {
        threads
            .filter { $0.projects.contains { $0.id == projectId } }
            .sorted { $0.lastActivityAt != $1.lastActivityAt ? $0.lastActivityAt > $1.lastActivityAt : $0.id < $1.id }
    }
}

/// The Patches tab's groups: the patches from the thread's own folder (and
/// home and the built-ins) first, then each project's, so it's clear why a
/// chip or panel is there.
public struct PatchGroup: Sendable, Equatable, Identifiable {
    /// Nil for the thread's own folder.
    public var projectId: String?
    /// The project's title; nil for the thread's own folder.
    public var title: String?
    public var patches: [PatchInfo]

    public init(projectId: String?, title: String?, patches: [PatchInfo]) {
        self.projectId = projectId
        self.title = title
        self.patches = patches
    }

    public var id: String { projectId ?? "" }
}

public enum PatchGroups {
    /// One group with no title when nothing came from a project, as the
    /// tab was before projects.
    public static func group(_ thread: ThreadSummary, projectTitle: (String) -> String? = { _ in nil }) -> [PatchGroup] {
        let own = thread.patches.filter { $0.projectId == nil }
        let fromProjects = thread.patches.filter { $0.projectId != nil }
        guard !fromProjects.isEmpty else { return own.isEmpty ? [] : [PatchGroup(projectId: nil, title: nil, patches: own)] }
        var groups: [PatchGroup] = []
        if !own.isEmpty { groups.append(PatchGroup(projectId: nil, title: nil, patches: own)) }
        // The thread's projects in its order, then any it isn't in any more
        // (a summary a moment behind).
        var ids = thread.projects.map(\.id)
        for patch in fromProjects where !ids.contains(patch.projectId!) { ids.append(patch.projectId!) }
        for id in ids {
            let patches = fromProjects.filter { $0.projectId == id }
            guard !patches.isEmpty else { continue }
            let title = thread.projects.first { $0.id == id }?.title ?? projectTitle(id) ?? "A project"
            groups.append(PatchGroup(projectId: id, title: title, patches: patches))
        }
        return groups
    }

    /// A project's patches across its threads, each once (by id and
    /// folder), for its page: what the project gives a thread put in it.
    public static func patches(of project: Project, in threads: [ThreadSummary]) -> [(patch: PatchInfo, thread: ThreadSummary)] {
        var seen = Set<String>()
        var out: [(PatchInfo, ThreadSummary)] = []
        for thread in threads {
            for patch in ProjectPane.patches(of: project, in: thread) {
                if seen.insert(patch.id + "|" + (patch.folder ?? "")).inserted { out.append((patch, thread)) }
            }
        }
        return out
    }
}

/// Where a panel came from, for the small badge in the corner of its rail
/// button: two Convex buttons, say, one from the thread's folder and one
/// from a project's, look alike without it.
public enum PanelOrigin: Sendable, Equatable {
    /// One of the thread's projects' folders: the badge is that project's icon.
    case project(id: String)
    /// A tool patch (`appliesTo`), here for these paths.
    case tool(matched: [String])
    /// `~/.threadbear/patches`.
    case home
    /// A `.threadbear/patches` in the thread's folder or one above it.
    case folder
    case builtin

    /// A project's folder wins over the rest, as that's what Mike would
    /// least expect to see in another folder's thread; then a tool patch,
    /// which is there because of one file.
    public static func of(_ panel: Panel, in thread: ThreadSummary) -> PanelOrigin {
        of(patch: panel.patch, projectId: panel.projectId, in: thread)
    }

    /// Where an action's chip came from, worked out as a panel's is, for
    /// its shelf over the composer (ActionShelves).
    public static func of(_ action: Action, in thread: ThreadSummary) -> PanelOrigin {
        of(patch: action.patch, projectId: action.projectId, in: thread)
    }

    private static func of(patch id: String, projectId: String?, in thread: ThreadSummary) -> PanelOrigin {
        let patch = thread.patches.first { $0.id == id }
        if let projectId = projectId ?? patch?.projectId { return .project(id: projectId) }
        if let matched = patch?.matched, !matched.isEmpty { return .tool(matched: matched) }
        switch patch?.source {
        case "home": return .home
        case "builtin": return .builtin
        default: return .folder
        }
    }

    /// The badge's SF Symbol; nil for a project, whose own icon is drawn.
    public var symbol: String? {
        switch self {
        case .project: return nil
        case .tool: return "wrench.and.screwdriver.fill"
        case .home: return "house.fill"
        case .folder: return "folder.fill"
        case .builtin: return "shippingbox.fill"
        }
    }

    /// The rail button's tooltip: "Convex, from the Threadbear project".
    public func tip(_ title: String, projectTitle: String?) -> String {
        switch self {
        case .project: return "\(title), " + (projectTitle.map { "from the \($0) project" } ?? "from a project")
        case .tool(let matched):
            return "\(title), for " + matched.map { ($0 as NSString).lastPathComponent }.joined(separator: ", ")
        case .home: return "\(title), from ~/.threadbear"
        case .folder: return "\(title), from this folder's .threadbear"
        case .builtin: return "\(title), built in"
        }
    }
}

/// A thread's action chips as shelves over its composer, one for each
/// place they came from (a project, the folder, ~/.threadbear, a tool,
/// built in), so a project's buttons read as the project's rather than
/// as part of writing a message (Mike, 30 Oct). Each shelf holds its
/// chips by patch, so a hairline can sit between two patches from the
/// same place. Shelves and patches keep the order their first chip had.
public struct ActionShelf: Sendable, Equatable {
    public var origin: PanelOrigin
    public var patches: [[Action]]

    public static func shelves(_ actions: [Action], in thread: ThreadSummary) -> [ActionShelf] {
        var shelves: [ActionShelf] = []
        for action in actions {
            let origin = PanelOrigin.of(action, in: thread)
            let index = shelves.firstIndex { $0.origin == origin } ?? {
                shelves.append(ActionShelf(origin: origin, patches: []))
                return shelves.count - 1
            }()
            if let group = shelves[index].patches.firstIndex(where: { $0.first?.patch == action.patch }) {
                shelves[index].patches[group].append(action)
            } else {
                shelves[index].patches.append([action])
            }
        }
        return shelves
    }
}

/// The pane on a project's page: its panels and patches, as one of its
/// threads has them. Only threads have them (the runner works them out
/// per thread, from its folder and its projects' folders), so the page
/// borrows a thread's and keeps only what came from the project.
public enum ProjectPane {
    /// What a thread has from the project: patches tagged with it, and the
    /// thread's own ones from inside one of its folders, which the runner
    /// doesn't tag (a thread already in a project's folder gets that
    /// folder's patches as its own). Home patches go everywhere, so they
    /// aren't the project's.
    public static func patches(of project: Project, in thread: ThreadSummary) -> [PatchInfo] {
        thread.patches.filter { patch in
            if let id = patch.projectId { return id == project.id }
            guard patch.source != "home" else { return false }
            // A built-in from the thread's own folder says no folder.
            let from = patch.folder ?? patch.file ?? thread.cwd
            return !from.isEmpty && project.folders.contains { isInside(from, $0) }
        }
    }

    /// The panels those patches give.
    public static func panels(of project: Project, in thread: ThreadSummary) -> [Panel] {
        let own = Set(patches(of: project, in: thread).filter { $0.projectId == nil }.map(\.id))
        return thread.panels.filter { $0.projectId == project.id || ($0.projectId == nil && own.contains($0.patch)) }
    }

    /// The thread the page borrows from, with only the project's panels
    /// and patches: the newest open one that has any, else the newest
    /// archived one. Nil when none of its threads has anything from it.
    /// It keeps the thread's id, which the panels' requests to the runner
    /// go by.
    public static func standIn(for project: Project, threads: some Sequence<ThreadSummary>) -> ThreadSummary? {
        var best: ThreadSummary?
        for thread in threads where thread.projects.contains(where: { $0.id == project.id }) {
            if let best, !better(thread, than: best) { continue }
            guard !patches(of: project, in: thread).isEmpty else { continue }
            best = thread
        }
        guard var out = best else { return nil }
        out.panels = panels(of: project, in: out)
        out.patches = patches(of: project, in: out)
        return out
    }

    private static func better(_ a: ThreadSummary, than b: ThreadSummary) -> Bool {
        if a.isArchived != b.isArchived { return !a.isArchived }
        return a.lastActivityAt != b.lastActivityAt ? a.lastActivityAt > b.lastActivityAt : a.id < b.id
    }

    /// `path` is `folder` or inside it, as the runner's isInside.
    static func isInside(_ path: String, _ folder: String) -> Bool {
        let p = (path as NSString).standardizingPath, f = (folder as NSString).standardizingPath
        return p == f || p.hasPrefix(f.hasSuffix("/") ? f : f + "/")
    }
}

/// The words the Librarian view uses.
public enum LibrarianText {
    /// A line for each project the look filed the thread in, in the row's
    /// order: "Walkies: Plans the dog walk route". Nothing on other looks.
    public static func filingReasons(_ entry: LibrarianEntry, title: (String) -> String?) -> [String] {
        entry.projectIds.compactMap { id in
            guard let why = entry.reasons[id], !why.isEmpty else { return nil }
            return title(id).map { "\($0): \(why)" } ?? why
        }
    }

    /// "Idle", "Looking at Rename the Settings tabs", "Paused", "Off: Needs
    /// a Claude pool to run on".
    public static func state(_ status: LibrarianStatus) -> String {
        switch status.state {
        case .idle: return status.paused ? "Paused" : "Idle"
        case .working:
            guard let current = status.current else { return "Working" }
            let title = current.threadTitle.isEmpty ? "a thread" : "\u{201C}\(current.threadTitle)\u{201D}"
            return "Looking at \(title)"
        case .paused: return "Paused"
        case .off:
            guard let reason = status.reason, !reason.isEmpty else { return "Off" }
            return "Off: \(reason)"
        }
    }

    /// The reason a look is queued, when the runner gave no words for it.
    public static func reason(_ reason: LibrarianReason) -> String {
        switch reason {
        case .firstMessage: return "Its first message came in"
        case .firstTurn: return "Its first turn ended"
        case .newFolder: return "Edited files in a new folder"
        case .moved: return "Moved to another folder"
        case .catchUp: return "More turns since the last look"
        case .asked: return "Asked for"
        case .notes: return "Its notes got long"
        }
    }

    public static func why(_ job: LibrarianJob) -> String {
        job.why.isEmpty ? reason(job.reason) : job.why
    }

    /// "14 looks today · 3 skipped · 52k tokens". No money: looks run on
    /// a Claude login, which uses its room, and what
    /// they'd have cost on the API read as a bill Mike wasn't paying.
    public static func today(_ status: LibrarianStatus) -> String {
        var parts = [status.looksToday == 1 ? "1 look today" : "\(status.looksToday) looks today"]
        if status.skippedToday > 0 { parts.append("\(status.skippedToday) skipped") }
        if status.tokensToday > 0 { parts.append("\(tokens(status.tokensToday)) tokens") }
        return parts.joined(separator: " · ")
    }

    /// 820, 52k, 1.2M.
    public static func tokens(_ n: Int) -> String {
        if n < 1000 { return "\(n)" }
        if n < 1_000_000 { return "\(Int((Double(n) / 1000).rounded()))k" }
        return String(format: "%.1fM", Double(n) / 1_000_000)
    }

    /// "in 4m", "now", or "waiting" for one more than a minute overdue:
    /// looks wait while it's paused, off or busy with another.
    public static func due(_ date: Date?, now: Date) -> String {
        guard let date else { return "" }
        if date.timeIntervalSince(now) > -60 { return ScheduleTime.relative(date, now: now) }
        return "waiting"
    }
}
