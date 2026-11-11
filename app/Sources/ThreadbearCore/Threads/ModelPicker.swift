import Foundation

/// What the composer's model picker shows, worked out here so the popover,
/// the automation's text form of it and the tests all read the same rows.
public enum ModelPicker {
    /// A column in the picker's rail: one provider (Claude, Codex, ...)
    /// and the accounts that run on it. Threads run on an account, so
    /// picking a model under another provider means moving to one of its
    /// accounts; `accountIds` keeps the runner's order, so the first is
    /// the one that gets used.
    public struct Provider: Sendable, Equatable, Identifiable {
        public var id: String
        public var title: String
        public var accountIds: [String]

        public init(id: String, title: String, accountIds: [String]) {
            self.id = id
            self.title = title
            self.accountIds = accountIds
        }
    }

    /// A button in the picker's rail: a pool (its providers' logos, its
    /// title, and its logins and best room under it) or, from a runner
    /// older than pools, a provider (its logo).
    public struct Column: Sendable, Equatable, Identifiable {
        public var id: String
        public var title: String
        /// "3 · 61%" under a pool's title.
        public var subtitle: String?
        /// An SF Symbol for a provider's icon, where the app has no logo
        /// for it.
        public var symbol: String?
        /// A pool's logins.
        public var logins: Int?
        /// The providers whose logos head a pool's button ("claude",
        /// "codex"), in the order its logins first name them.
        public var providers: [String]

        public init(id: String, title: String, subtitle: String? = nil, symbol: String? = nil, logins: Int? = nil, providers: [String] = []) {
            self.id = id
            self.title = title
            self.subtitle = subtitle
            self.symbol = symbol
            self.logins = logins
            self.providers = providers
        }
    }

    /// One provider's models in a column. A mixed pool has one per
    /// provider, each under its `title`; otherwise there's one, untitled.
    public struct Section: Sendable, Equatable, Identifiable {
        /// The provider.
        public var id: String
        public var title: String?
        public var rows: [Row]

        public init(id: String, title: String? = nil, rows: [Row]) {
            self.id = id
            self.title = title
            self.rows = rows
        }
    }

    /// One model in the list.
    public struct Row: Sendable, Equatable, Identifiable {
        public var id: String
        public var title: String
        public var subtitle: String?
        /// The account's (or the runner's) default: picking it leaves the
        /// thread on the default rather than pinning this model.
        public var isDefault: Bool
        public var isSelected: Bool
        /// A dated or superseded id, listed under "Older" rather than with
        /// the main rows.
        public var isOlder: Bool

        public init(id: String, title: String, subtitle: String? = nil, isDefault: Bool = false, isSelected: Bool = false, isOlder: Bool = false) {
            self.id = id
            self.title = title
            self.subtitle = subtitle
            self.isDefault = isDefault
            self.isSelected = isSelected
            self.isOlder = isOlder
        }
    }

    /// The providers in the rail, in the order the runner first lists an
    /// account for each. No accounts yet means a rail with just Claude, so
    /// the picker has somewhere to show the alias rows.
    public static func providers(_ accounts: [Account]) -> [Provider] {
        var providers: [Provider] = []
        for account in accounts {
            let id = account.provider.isEmpty ? "claude" : account.provider
            if let index = providers.firstIndex(where: { $0.id == id }) {
                providers[index].accountIds.append(account.id)
            } else {
                providers.append(Provider(id: id, title: providerTitle(id), accountIds: [account.id]))
            }
        }
        return providers.isEmpty ? [Provider(id: "claude", title: "Claude", accountIds: [])] : providers
    }

    /// How a provider id reads in the rail and its tooltip.
    public static func providerTitle(_ id: String) -> String {
        Providers.info(id).name
    }

    /// The SF Symbol that stands for a provider where the app has no logo
    /// for it (the picker draws Claude's and Codex's own, ProviderLogo),
    /// and for a pool's icon in the sidebar and Settings.
    public static func providerSymbol(_ id: String) -> String {
        switch id {
        case "claude": return "sparkle"
        case "codex": return "chevron.left.forwardslash.chevron.right"
        case "grok": return "bolt"
        default: return "cpu"
        }
    }

    /// The three aliases the CLI always takes, offered until the runner has
    /// asked the account what it really lists.
    public static let aliases = ["opus", "sonnet", "haiku"]

    /// The rows for one account's list. For Claude the main rows are the
    /// aliases (a word) plus whatever the default and the chosen model are;
    /// dated and old full ids are `isOlder`. The CLI lists "opus" and the
    /// default both as the same model, so each model appears once. Codex
    /// ids are all dashed ("gpt-6.1-sol"), so for Codex every model is a
    /// main row, in `model/list`'s order. A chosen model the list doesn't
    /// know (set some other way) still gets a row, ticked. With a `query`,
    /// every row that mentions it is kept and the main/older split goes: a
    /// search is a search.
    ///
    /// Before the runner has the list, Claude offers the three aliases the
    /// CLI always takes; Codex offers just its default, since a Claude
    /// alias would be the wrong provider's model.
    public static func rows(choices: [ModelChoice], defaultId: String?, selected: String?, query: String = "", provider: String = "claude") -> [Row] {
        // Only Claude's CLI takes these aliases: anyone else lists its own.
        let codex = provider != "claude" && !provider.isEmpty
        let fallback: [ModelChoice] = codex
            ? [defaultId].compactMap { $0 }.filter { !$0.isEmpty }.map { ModelChoice(id: $0, model: $0, title: ComposerChips.model($0)) }
            : aliases.map { ModelChoice(id: $0, model: $0, title: ComposerChips.model($0)) }
        let known = choices.isEmpty ? fallback : choices
        var rows: [Row] = []
        var seenModels = Set<String>()
        func add(_ c: ModelChoice, older: Bool) {
            guard seenModels.insert(c.model).inserted else { return }
            let isDefault = matches(c, defaultId)
            rows.append(Row(id: c.id, title: c.title, subtitle: c.description, isDefault: isDefault, isSelected: matches(c, selected), isOlder: older))
        }
        let isMain = { (c: ModelChoice) -> Bool in
            codex || c.id.allSatisfy(\.isLetter) || matches(c, defaultId) || matches(c, selected)
        }
        for c in known where isMain(c) { add(c, older: false) }
        if let selected, !selected.isEmpty, !known.contains(where: { matches($0, selected) }) {
            rows.append(Row(id: selected, title: ComposerChips.model(selected), isSelected: true))
        }
        for c in known where !isMain(c) { add(c, older: true) }
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !needle.isEmpty else { return rows }
        return rows.filter { row in
            [row.title, row.id, row.subtitle ?? ""].contains { $0.lowercased().contains(needle) }
        }.map { row in
            var row = row
            row.isOlder = false
            return row
        }
    }

    private static func matches(_ c: ModelChoice, _ id: String?) -> Bool {
        guard let id, !id.isEmpty else { return false }
        return c.id == id || c.model == id
    }

    /// The effort levels the picker offers for a model: the CLI's list for
    /// it when known (Codex's go up to "ultra"), else the provider's usual
    /// ones. There's no "default" level any more: the picker remembers the
    /// last one chosen instead.
    public static func effortLevels(for model: String?, in choices: [ModelChoice], provider: String = "claude") -> [String] {
        if Providers.info(provider).effort == "none" { return [] }
        guard let model, let levels = ComposerChips.choice(for: model, in: choices)?.effortLevels, !levels.isEmpty else {
            return ComposerChips.effortLevels(provider: provider)
        }
        return levels
    }

    /// Whether the picker offers fast mode for a model: Claude only, and
    /// only where the CLI's list says the model has it, so nothing shows
    /// before the list is known (protocol "Fast mode").
    public static func offersFastMode(model: String?, in choices: [ModelChoice], provider: String = "claude") -> Bool {
        guard provider == "claude", let model else { return false }
        return ComposerChips.choice(for: model, in: choices)?.fastMode == true
    }

    /// The level a thread with none chosen runs at, when it's known: the
    /// runner's default for the provider (new threads), else the account's
    /// own (Codex's `model_reasoning_effort`), else the model's.
    public static func impliedEffort(model: String?, choices: [ModelChoice], account: Account?, runnerDefault: String?) -> String? {
        if let runnerDefault, !runnerDefault.isEmpty { return runnerDefault }
        if let own = account?.defaultEffort, !own.isEmpty { return own }
        guard let model else { return nil }
        return ComposerChips.choice(for: model, in: choices)?.defaultEffort
    }
}
