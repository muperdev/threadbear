import Foundation
import os

/// What a provider is called and can do, from `hello.providers`
/// (docs/protocol.md, "Types"). A provider id alone never decides what a
/// control offers, so a provider this app has never heard of still gets the
/// right menus.
public struct ProviderInfo: Sendable, Hashable, Decodable {
    public var id: String
    public var name: String
    public var cliName: String
    public var loginCommand: String
    /// "claude", "codex" (with ultra) or "none".
    public var effort: String
    /// "classifier", or "asks" where Auto behaves like Ask.
    public var autoMode: String
    public var sandbox: Bool
    public var rewind: Bool
    public var smallJobs: Bool
    public var usageReadings: Bool

    public init(id: String, name: String, cliName: String, loginCommand: String = "", effort: String = "none", autoMode: String = "asks",
                sandbox: Bool = false, rewind: Bool = false, smallJobs: Bool = false, usageReadings: Bool = false) {
        self.id = id
        self.name = name
        self.cliName = cliName
        self.loginCommand = loginCommand
        self.effort = effort
        self.autoMode = autoMode
        self.sandbox = sandbox
        self.rewind = rewind
        self.smallJobs = smallJobs
        self.usageReadings = usageReadings
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        func text(_ key: CodingKeys, _ fallback: String) -> String { ((try? c.decodeIfPresent(String.self, forKey: key)) ?? nil) ?? fallback }
        func flag(_ key: CodingKeys) -> Bool { ((try? c.decodeIfPresent(Bool.self, forKey: key)) ?? nil) ?? false }
        name = text(.name, id)
        cliName = text(.cliName, id)
        loginCommand = text(.loginCommand, "")
        effort = text(.effort, "none")
        autoMode = text(.autoMode, "asks")
        sandbox = flag(.sandbox)
        rewind = flag(.rewind)
        smallJobs = flag(.smallJobs)
        usageReadings = flag(.usageReadings)
    }

    enum CodingKeys: String, CodingKey {
        case id, name, cliName, loginCommand, effort, autoMode, sandbox, rewind, smallJobs, usageReadings
    }
}

/// The one place the app asks what a provider is. It holds what connected
/// runners have said, over what's built in for Claude and Codex, which is
/// what an older runner's threads get.
public enum Providers {
    static let builtIn: [String: ProviderInfo] = [
        "claude": ProviderInfo(id: "claude", name: "Claude", cliName: "Claude Code", loginCommand: "claude auth login", effort: "claude",
                               autoMode: "classifier", sandbox: true, rewind: true, smallJobs: true, usageReadings: true),
        "codex": ProviderInfo(id: "codex", name: "Codex", cliName: "Codex", loginCommand: "codex login", effort: "codex",
                              autoMode: "asks", sandbox: false, rewind: true, smallJobs: false, usageReadings: true),
        // The grok experiment's, as a runner from before hello's providers had it too.
        "grok": ProviderInfo(id: "grok", name: "Grok", cliName: "Grok Build", loginCommand: "grok login", effort: "codex",
                             autoMode: "asks", sandbox: false, rewind: false, smallJobs: false, usageReadings: false),
    ]

    private static let told = OSAllocatedUnfairLock(initialState: [String: ProviderInfo]())

    /// From each runner's hello. Added to, as a thread can be another runner's.
    public static func update(_ list: [ProviderInfo]) {
        told.withLock { state in
            for info in list where !info.id.isEmpty { state[info.id] = info }
        }
    }

    /// Nil when nothing has said what this provider is.
    public static func known(_ id: String) -> ProviderInfo? {
        let key = id.isEmpty ? "claude" : id
        return told.withLock { $0[key] } ?? builtIn[key]
    }

    /// Never fails. One nothing has said anything about gets its id for a
    /// name and can do nothing, so no control offers what it might not have.
    public static func info(_ id: String) -> ProviderInfo {
        if let found = known(id) { return found }
        let name = id.prefix(1).uppercased() + id.dropFirst()
        return ProviderInfo(id: id, name: name, cliName: name)
    }
}
