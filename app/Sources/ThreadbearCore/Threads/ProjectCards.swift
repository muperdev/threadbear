import Foundation

/// What a project's card on the Projects page says about its threads: how
/// many are working, waiting on Mike, failed or finished and not looked at.
/// Counted over the inbox threads `ProjectSections` lists under it, so the
/// card and the sidebar agree.
public struct ProjectCardStatus: Sendable, Equatable {
    public var working = 0
    public var needsYou = 0
    public var failed = 0
    public var unread = 0
    /// Open threads (not archived or hibernating).
    public var open = 0
    /// Open asks on its threads' cards, counted only with the
    /// projectDecisions experiment on (ProjectDecisions). A thread counted
    /// here isn't counted as new as well.
    public var decisions = 0

    public init(working: Int = 0, needsYou: Int = 0, failed: Int = 0, unread: Int = 0, open: Int = 0, decisions: Int = 0) {
        self.decisions = decisions
        self.working = working
        self.needsYou = needsYou
        self.failed = failed
        self.unread = unread
        self.open = open
    }

    public init(threads: [ThreadSummary], decisions counting: Bool = false) {
        open = threads.count
        for thread in threads {
            let asks = counting ? ProjectDecisions.openCount(thread) : 0
            decisions += asks
            let look = ThreadRowLook(thread)
            switch look.status {
            // Sub-agents or jobs still going count as working too: the
            // project isn't quiet.
            case .working, .background: working += 1
            case .needsYou: needsYou += 1
            case .failed: failed += 1
            case .time, .stopped: break
            }
            // Its dot is an unread dot now, but it's already counted as needing him.
            if look.unread && look.status != .needsYou && asks == 0 { unread += 1 }
        }
    }

    /// Nothing running, waiting or new.
    public var isQuiet: Bool { working == 0 && needsYou == 0 && failed == 0 && unread == 0 && decisions == 0 }

    /// One piece of the card's status line.
    public enum Part: Sendable, Equatable {
        case needsYou(Int), working(Int), failed(Int), unread(Int), decisions(Int)

        public var text: String {
            switch self {
            case .decisions(let n): ProjectDecisions.label(n)
            case .needsYou(let n): "\(n) needs you"
            case .working(let n): "\(n) working"
            case .failed(let n): "\(n) failed"
            case .unread(let n): "\(n) new"
            }
        }
    }

    /// The card's status line, most pressing first: "1 needs you · 2
    /// working". Empty when it's quiet; the card says when it was last
    /// active instead.
    public var parts: [Part] {
        var parts: [Part] = []
        if decisions > 0 { parts.append(.decisions(decisions)) }
        if needsYou > 0 { parts.append(.needsYou(needsYou)) }
        if working > 0 { parts.append(.working(working)) }
        if failed > 0 { parts.append(.failed(failed)) }
        if unread > 0 { parts.append(.unread(unread)) }
        return parts
    }
}

/// The Projects page's cards: one per live project, in the sidebar's order
/// (newest activity first), with what its threads are doing.
public struct ProjectCard: Sendable, Equatable, Identifiable {
    public var project: Project
    public var status: ProjectCardStatus
    public var lastActivityAt: Date
    /// Its newest few open threads' titles, for the card's foot.
    public var recent: [String]

    public var id: String { project.id }

    /// Across every project, each thread once: one in two projects is
    /// on both cards but is still one thread working.
    public static func overall(_ sections: ProjectSections, decisions: Bool = false) -> ProjectCardStatus {
        var seen = Set<String>()
        return ProjectCardStatus(threads: sections.sections.flatMap(\.threads).filter { seen.insert($0.id).inserted }, decisions: decisions)
    }

    /// `decisions`: count each project's open asks (the projectDecisions
    /// experiment).
    public static func build(_ sections: ProjectSections, recent count: Int = 3, decisions: Bool = false) -> [ProjectCard] {
        sections.sections.map { section in
            ProjectCard(
                project: section.project,
                status: ProjectCardStatus(threads: section.threads, decisions: decisions),
                lastActivityAt: section.lastActivityAt,
                recent: section.threads.prefix(count).map { $0.title.isEmpty ? "Untitled" : $0.title }
            )
        }
    }

    /// The cards a search on the Projects page keeps. Every word has to be
    /// somewhere in the project, case and accents aside, so "port lights"
    /// finds Port Geo Lights Cruise. Goals and notes aren't on the card but
    /// count, as you often remember what a project was for and not its name.
    /// That casts a wide net, so cards with every word in the title go first;
    /// each group stays newest activity first.
    public static func matching(_ cards: [ProjectCard], _ query: String) -> [ProjectCard] {
        let words = query.split(whereSeparator: \.isWhitespace).map(String.init)
        guard !words.isEmpty else { return cards }
        let hit = { (text: String) in
            words.allSatisfy { text.range(of: $0, options: [.caseInsensitive, .diacriticInsensitive]) != nil }
        }
        let found = cards.filter { hit($0.project.searchText) }
        let inTitle = found.filter { hit($0.project.title) }
        return inTitle + found.filter { !hit($0.project.title) }
    }

    /// The Projects page's count while its search has text.
    public static func matchCount(_ shown: Int, of total: Int) -> String {
        "\(shown) of \(total) \(total == 1 ? "project" : "projects")"
    }
}

private extension Project {
    /// Everything a search looks through. Folders by name only: the path
    /// before it is the same for most projects, so "dev" would find them all.
    var searchText: String {
        ([title, state, goals, notes]
            + folders.map { ($0 as NSString).lastPathComponent }
            + links.map(\.title))
            .joined(separator: "\n")
    }
}
