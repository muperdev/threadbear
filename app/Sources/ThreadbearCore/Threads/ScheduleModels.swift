import Foundation

/// The model and effort the schedule editor offers for a schedule that
/// starts a new thread each time: the login whose list it shows, that
/// list, and what "Default" stands for. Mike asked for them on 9 Nov: the
/// editor showed neither, so a schedule kept whatever the composer had when
/// it was made.
public struct ScheduleModels: Sendable, Equatable {
    /// "claude" or "codex": the login's.
    public var provider: String
    /// The login the list comes from. Nil before the runner has said hello.
    public var account: Account?
    /// The model a thread that names none runs on: the runner's default for
    /// the provider, else the login's.
    public var defaultModel: String?
    /// The level "Default" means for `model`, when it's known.
    public var defaultEffort: String?
    /// The model rows, as the composer's picker lists them.
    public var rows: [ModelPicker.Row]
    /// The levels for `model` (or the default model).
    public var effortLevels: [String]

    /// `accountId` pins the login; `poolId` lets the runner pick one each
    /// run, so the list is the login in the pool for the chosen model's
    /// provider (or its first). Neither is what the runner does with a
    /// thread that names none: its default pool, else its default login.
    public init(accountId: String?, poolId: String?, model: String?, logins: RunnerLogins) {
        let account: Account?
        if let accountId, let pinned = logins.account(accountId) {
            account = pinned
        } else if let pool = (poolId ?? (accountId == nil ? logins.defaults.poolId : nil)).flatMap(logins.pool) {
            let members = Pools.logins(pool, accounts: logins.accounts)
            account = members.first { $0.models.contains { Self.matches($0, model) } } ?? members.first
        } else {
            account = logins.account(logins.defaults.accountId) ?? logins.accounts.first
        }
        let provider = account.map(Pools.providerId) ?? "claude"
        let choices = account?.models ?? []
        let defaultModel = ComposerChips.resolvedModel(logins.defaults.model(for: provider), account?.defaultModel)
        let effective = ComposerChips.resolvedModel(model, defaultModel)
        self.provider = provider
        self.account = account
        self.defaultModel = defaultModel
        defaultEffort = ModelPicker.impliedEffort(model: effective, choices: choices, account: account, runnerDefault: logins.defaults.efforts[provider])
        rows = ModelPicker.rows(choices: choices, defaultId: defaultModel, selected: model, provider: provider)
        effortLevels = ModelPicker.effortLevels(for: effective, in: choices, provider: provider)
    }

    /// "Default (Opus 5.5)", or plain "Default" when it isn't known.
    public var defaultModelTitle: String {
        guard let defaultModel else { return "Default" }
        let title = account.flatMap { ComposerChips.choice(for: defaultModel, in: $0.models)?.title } ?? ComposerChips.model(defaultModel)
        return "Default (\(title))"
    }

    /// "Default (High)", or "Default effort" when it isn't known: beside
    /// the model menu a bare "Default" didn't say what it was.
    public var defaultEffortTitle: String {
        defaultEffort.map { "Default (\(ComposerChips.effortTitle($0)))" } ?? "Default effort"
    }

    private static func matches(_ choice: ModelChoice, _ id: String?) -> Bool {
        guard let id, !id.isEmpty else { return false }
        return choice.id == id || choice.model == id
    }
}
