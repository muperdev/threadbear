import Foundation

/// How the Usage panel groups the logins: by pool, as threads run on pools
/// (Mike, 28 Oct: "pools and then the providers within them"), with each
/// pool's logins split by provider, as a Claude and a Codex login have
/// different windows. Logins in no pool get a group of their own at the
/// end. A runner without pools gets one group per provider, as before.
public enum UsageGroups {
    public struct Group: Sendable, Equatable, Identifiable {
        public var id: String
        public var title: String
        /// The pool, for its icon. Nil for a provider's group and for
        /// "Not in a pool".
        public var pool: Pool?
        public var providers: [Provider]

        public var accounts: [Account] { providers.flatMap(\.accounts) }
    }

    public struct Provider: Sendable, Equatable, Identifiable {
        public var provider: String
        public var accounts: [Account]
        public var id: String { provider }
    }

    public static let unpooledId = "threadbear.unpooled"

    public static func build(pools: [Pool], accounts: [Account]) -> [Group] {
        guard !pools.isEmpty else {
            return byProvider(accounts).map { Group(id: $0.provider, title: providerTitle($0.provider), pool: nil, providers: [$0]) }
        }
        let byId = Dictionary(accounts.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        var groups = pools.compactMap { pool -> Group? in
            // In the pool's order: it decides which provider comes first.
            let members = pool.accountIds.compactMap { byId[$0] }
            if members.isEmpty { return nil }
            return Group(id: pool.id, title: pool.title, pool: pool, providers: byProvider(members, keepOrder: true))
        }
        let pooled = Set(pools.flatMap(\.accountIds))
        let rest = accounts.filter { !pooled.contains($0.id) }
        if !rest.isEmpty {
            groups.append(Group(id: unpooledId, title: "Not in a pool", pool: nil, providers: byProvider(rest)))
        }
        return groups
    }

    /// Claude, then Codex, then any other; or, with `keepOrder`, in the
    /// order the providers first turn up.
    public static func byProvider(_ accounts: [Account], keepOrder: Bool = false) -> [Provider] {
        var order: [String] = []
        for account in accounts where !order.contains(account.provider) { order.append(account.provider) }
        if !keepOrder {
            let rank = ["claude": 0, "codex": 1, "grok": 2]
            order = order.enumerated().sorted { (rank[$0.element] ?? 3, $0.offset) < (rank[$1.element] ?? 3, $1.offset) }.map(\.element)
        }
        return order.map { provider in Provider(provider: provider, accounts: accounts.filter { $0.provider == provider }) }
    }

    static func providerTitle(_ provider: String) -> String {
        switch provider {
        case "claude": return "Claude"
        case "codex": return "Codex"
        case "grok": return "Grok"
        default: return provider.prefix(1).uppercased() + provider.dropFirst()
        }
    }

    // MARK: The icon's tooltip

    /// The Usage icon's tooltip: one line per group with the most room any
    /// of its logins has (what its next thread would get), or when it's
    /// back if every login is at its limit. The panel has the rest.
    public static func tooltip(_ groups: [Group], now: Date, calendar: Calendar = .current) -> String {
        let lines = groups.compactMap { line($0, now: now, calendar: calendar) }
        return (["Usage"] + lines).joined(separator: "\n")
    }

    static func line(_ group: Group, now: Date, calendar: Calendar) -> String? {
        let known = group.accounts.filter { AccountUsageText.room($0.usage) != nil || $0.usage?.blocked == true }
        if known.isEmpty { return nil }
        let open = known.filter { $0.usage?.blocked != true }
        if let best = open.compactMap({ AccountUsageText.room($0.usage) }).max() {
            return "\(group.title): \(AccountUsageText.percent(best)) left"
        }
        // All at their limit: the soonest any of them comes back.
        let back = known.compactMap { AccountUsageText.tightest($0.usage)?.resetsAt }.filter { $0 > now }.min()
        guard let back else { return "\(group.title): at its limit" }
        let at = calendar.isDate(back, inSameDayAs: now) ? "at " : ""
        return "\(group.title): at its limit, back \(at)" + AccountUsageText.resetTime(back, now: now, calendar: calendar)
    }
}
