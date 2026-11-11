import Foundation

// Shared projects (docs/multiplayer-design.md, runner/src/team.ts): the team
// this Mac is on, as team.get and team.updated send it. Every field is read
// leniently, so a runner that adds one doesn't break an older app.

/// A project's link to the team's copy.
public struct ProjectTeam: Sendable, Hashable, Decodable {
    public var id: String
    /// Teammates' messages to its threads go straight in, rather than waiting.
    public var deliver: Bool

    public init(id: String, deliver: Bool = false) {
        self.id = id
        self.deliver = deliver
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        deliver = ((try? c.decodeIfPresent(String.self, forKey: .delivery)).flatMap { $0 }) == "deliver"
    }

    enum CodingKeys: String, CodingKey { case id, delivery }
}

public struct TeamMember: Sendable, Hashable, Decodable, Identifiable {
    public var memberId: String
    public var name: String
    public var host: Bool
    public var me: Bool
    public var online: Bool
    public var id: String { memberId }

    public init(memberId: String, name: String, host: Bool = false, me: Bool = false, online: Bool = false) {
        self.memberId = memberId
        self.name = name
        self.host = host
        self.me = me
        self.online = online
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func bool(_ k: CodingKeys) -> Bool { ((try? c.decodeIfPresent(Bool.self, forKey: k)).flatMap { $0 }) ?? false }
        memberId = try c.decode(String.self, forKey: .memberId)
        name = ((try? c.decodeIfPresent(String.self, forKey: .name)).flatMap { $0 }) ?? "Someone"
        host = bool(.host)
        me = bool(.me)
        online = bool(.online)
    }

    enum CodingKeys: String, CodingKey { case memberId, name, host, me, online }
}

public struct SharedProject: Sendable, Hashable, Decodable, Identifiable {
    public var id: String
    public var title: String
    public var sharedBy: String
    public var repos: [String]
    /// This Mac's linked copy, if it has one.
    public var localProjectId: String?
    public var mine: Bool

    public init(id: String, title: String, sharedBy: String = "", repos: [String] = [], localProjectId: String? = nil, mine: Bool = false) {
        self.id = id
        self.title = title
        self.sharedBy = sharedBy
        self.repos = repos
        self.localProjectId = localProjectId
        self.mine = mine
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        title = ((try? c.decodeIfPresent(String.self, forKey: .title)).flatMap { $0 }) ?? "Untitled project"
        sharedBy = ((try? c.decodeIfPresent(String.self, forKey: .sharedBy)).flatMap { $0 }) ?? ""
        repos = c.lossyArray(String.self, forKey: .repos)
        localProjectId = (try? c.decodeIfPresent(String.self, forKey: .localProjectId)).flatMap { $0 }
        mine = ((try? c.decodeIfPresent(Bool.self, forKey: .mine)).flatMap { $0 }) ?? false
    }

    enum CodingKeys: String, CodingKey { case id, title, sharedBy, repos, localProjectId, mine }
}

/// One of a member's threads in a shared project, on the board.
public struct TeamCard: Sendable, Hashable, Decodable, Identifiable {
    public enum Status: String, Sendable, Hashable, CaseIterable {
        case working, needsThem, idle, done
        public var title: String {
            switch self {
            case .working: "Working"
            case .needsThem: "Waiting on them"
            case .idle: "Idle"
            case .done: "Done"
            }
        }
    }

    /// team:<member>:<thread8>, what agents call it.
    public var id: String
    public var projectId: String
    public var memberId: String
    public var memberName: String
    public var threadId: String
    public var title: String
    public var status: Status
    public var headline: String?
    public var branch: String?
    public var repo: String?
    public var handedToName: String?
    public var mine: Bool
    public var updatedAt: Date

    public init(id: String, projectId: String, memberId: String, memberName: String, threadId: String, title: String, status: Status, headline: String? = nil, branch: String? = nil, repo: String? = nil, handedToName: String? = nil, mine: Bool = false, updatedAt: Date = Date()) {
        self.id = id
        self.projectId = projectId
        self.memberId = memberId
        self.memberName = memberName
        self.threadId = threadId
        self.title = title
        self.status = status
        self.headline = headline
        self.branch = branch
        self.repo = repo
        self.handedToName = handedToName
        self.mine = mine
        self.updatedAt = updatedAt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func string(_ k: CodingKeys) -> String? { (try? c.decodeIfPresent(String.self, forKey: k)).flatMap { $0 }.flatMap { $0.isEmpty ? nil : $0 } }
        id = try c.decode(String.self, forKey: .id)
        projectId = string(.projectId) ?? ""
        memberId = string(.memberId) ?? ""
        memberName = string(.memberName) ?? "Someone"
        threadId = string(.threadId) ?? ""
        title = string(.title) ?? "Untitled"
        status = string(.status).flatMap(Status.init(rawValue:)) ?? .idle
        headline = string(.headline)
        branch = string(.branch)
        repo = string(.repo)
        handedToName = string(.handedToName)
        mine = ((try? c.decodeIfPresent(Bool.self, forKey: .mine)).flatMap { $0 }) ?? false
        let ms = (try? c.decodeIfPresent(Double.self, forKey: .updatedAt)).flatMap { $0 } ?? 0
        updatedAt = Date(timeIntervalSince1970: ms / 1000)
    }

    enum CodingKeys: String, CodingKey { case id, projectId, memberId, memberName, threadId, title, status, headline, branch, repo, handedToName, mine, updatedAt }
}

/// A teammate's message or handover, waiting for Mike.
public struct TeamWaiting: Sendable, Hashable, Decodable, Identifiable {
    public enum Kind: String, Sendable, Hashable { case message, handover }
    public var id: String
    public var kind: Kind
    public var projectId: String
    public var localProjectId: String?
    public var fromName: String
    public var fromTitle: String
    public var toThread: String?
    public var toThreadTitle: String?
    public var text: String
    public var repo: String?
    public var branch: String?
    public var links: [ProjectLink]
    public var createdAt: Date

    public init(id: String, kind: Kind, projectId: String, localProjectId: String? = nil, fromName: String, fromTitle: String, toThread: String? = nil, toThreadTitle: String? = nil, text: String, repo: String? = nil, branch: String? = nil, links: [ProjectLink] = [], createdAt: Date = Date()) {
        self.id = id
        self.kind = kind
        self.projectId = projectId
        self.localProjectId = localProjectId
        self.fromName = fromName
        self.fromTitle = fromTitle
        self.toThread = toThread
        self.toThreadTitle = toThreadTitle
        self.text = text
        self.repo = repo
        self.branch = branch
        self.links = links
        self.createdAt = createdAt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func string(_ k: CodingKeys) -> String? { (try? c.decodeIfPresent(String.self, forKey: k)).flatMap { $0 }.flatMap { $0.isEmpty ? nil : $0 } }
        id = try c.decode(String.self, forKey: .id)
        kind = string(.kind).flatMap(Kind.init(rawValue:)) ?? .message
        projectId = string(.projectId) ?? ""
        localProjectId = string(.localProjectId)
        fromName = string(.fromName) ?? "Someone"
        fromTitle = string(.fromTitle) ?? "Untitled"
        toThread = string(.toThread)
        toThreadTitle = string(.toThreadTitle)
        text = string(.text) ?? ""
        repo = string(.repo)
        branch = string(.branch)
        links = c.lossyArray(ProjectLink.self, forKey: .links)
        createdAt = string(.createdAt).flatMap(ISO8601.parse) ?? Date()
    }

    enum CodingKeys: String, CodingKey { case id, kind, projectId, localProjectId, fromName, fromTitle, toThread, toThreadTitle, text, repo, branch, links, createdAt }
}

public struct TeamLogEntry: Sendable, Hashable, Decodable {
    public var memberName: String
    public var text: String
    public var threadTitle: String?
    public var at: Date

    public init(memberName: String, text: String, threadTitle: String? = nil, at: Date = Date()) {
        self.memberName = memberName
        self.text = text
        self.threadTitle = threadTitle
        self.at = at
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        memberName = ((try? c.decodeIfPresent(String.self, forKey: .memberName)).flatMap { $0 }) ?? "Someone"
        text = ((try? c.decodeIfPresent(String.self, forKey: .text)).flatMap { $0 }) ?? ""
        threadTitle = (try? c.decodeIfPresent(String.self, forKey: .threadTitle)).flatMap { $0 }
        at = Date(timeIntervalSince1970: ((try? c.decodeIfPresent(Double.self, forKey: .at)).flatMap { $0 } ?? 0) / 1000)
    }

    enum CodingKeys: String, CodingKey { case memberName, text, threadTitle, at }
}

public struct TeamState: Sendable, Hashable, Decodable {
    /// The multiplayer experiment is on.
    public var on: Bool
    public var joined: Bool
    public var connected: Bool
    public var error: String?
    public var url: String?
    public var name: String?
    public var me: TeamMember?
    public var members: [TeamMember]
    public var projects: [SharedProject]
    public var cards: [TeamCard]
    public var waiting: [TeamWaiting]
    /// Each shared project's notes from agents, newest last, by shared id.
    public var log: [String: [TeamLogEntry]]

    public init(on: Bool = false, joined: Bool = false, connected: Bool = false, error: String? = nil, url: String? = nil, name: String? = nil, me: TeamMember? = nil, members: [TeamMember] = [], projects: [SharedProject] = [], cards: [TeamCard] = [], waiting: [TeamWaiting] = [], log: [String: [TeamLogEntry]] = [:]) {
        self.on = on
        self.joined = joined
        self.connected = connected
        self.error = error
        self.url = url
        self.name = name
        self.me = me
        self.members = members
        self.projects = projects
        self.cards = cards
        self.waiting = waiting
        self.log = log
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func bool(_ k: CodingKeys) -> Bool { ((try? c.decodeIfPresent(Bool.self, forKey: k)).flatMap { $0 }) ?? false }
        on = bool(.on)
        joined = bool(.joined)
        connected = bool(.connected)
        error = (try? c.decodeIfPresent(String.self, forKey: .error)).flatMap { $0 }
        url = (try? c.decodeIfPresent(String.self, forKey: .url)).flatMap { $0 }
        name = (try? c.decodeIfPresent(String.self, forKey: .name)).flatMap { $0 }
        me = (try? c.decodeIfPresent(TeamMember.self, forKey: .me)).flatMap { $0 }
        members = c.lossyArray(TeamMember.self, forKey: .members)
        projects = c.lossyArray(SharedProject.self, forKey: .projects)
        cards = c.lossyArray(TeamCard.self, forKey: .cards)
        waiting = c.lossyArray(TeamWaiting.self, forKey: .waiting)
        log = ((try? c.decodeIfPresent([String: [TeamLogEntry]].self, forKey: .log)).flatMap { $0 }) ?? [:]
    }

    enum CodingKeys: String, CodingKey { case on, joined, connected, error, url, name, me, members, projects, cards, waiting, log }

    /// The shared project a local one is linked to.
    public func shared(_ project: Project) -> SharedProject? {
        guard let id = project.team?.id else { return nil }
        return projects.first { $0.id == id }
    }

    /// Teammates' cards in a shared project (Mike's own threads are in its Threads section).
    public func board(_ sharedId: String) -> [TeamCard] {
        cards.filter { $0.projectId == sharedId && !$0.mine }.sorted { $0.updatedAt > $1.updatedAt }
    }

    public func waiting(for sharedId: String) -> [TeamWaiting] {
        waiting.filter { $0.projectId == sharedId }.sorted { $0.createdAt < $1.createdAt }
    }
}
