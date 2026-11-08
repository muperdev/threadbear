import Foundation

/// What the app works out about pools (docs/pools-plan.md): which pool a new
/// thread starts on, the picker's rail and status line, the model chip's
/// title and badge, and the sidebar's waiting line. The runner decides which
/// login a turn really runs on; `pickLogin` here only guesses it, so the
/// picker can list the right models before a thread exists.
public enum Pools {
    // MARK: Folders

    /// The pool whose `folders` hold `cwd` (the folder itself or one inside
    /// it), or nil. The most specific folder wins; between equal folders,
    /// the first pool. `~` in either is `home`. The runner has the same rule
    /// and the same cases (`runner/test/fixtures/pool-folder-cases.json`).
    public static func forFolder(_ pools: [Pool], cwd: String, home: String) -> String? {
        let folder = normalised(cwd, home: home)
        guard !folder.isEmpty else { return nil }
        var best: (id: String, length: Int)?
        for pool in pools {
            for entry in pool.folders {
                let root = normalised(entry, home: home)
                guard !root.isEmpty else { continue }
                let inside = folder == root || folder.hasPrefix(root == "/" ? "/" : root + "/")
                if inside, root.count > (best?.length ?? -1) { best = (pool.id, root.count) }
            }
        }
        return best?.id
    }

    /// "~" and "~/x" made absolute, and any trailing slashes dropped.
    static func normalised(_ path: String, home: String) -> String {
        var text = path.trimmingCharacters(in: .whitespaces)
        if text == "~" { text = home } else if text.hasPrefix("~/") { text = home + text.dropFirst(1) }
        while text.count > 1, text.hasSuffix("/") { text.removeLast() }
        return text
    }

    /// The pool a new thread starts on: the one Mike picked for this draft,
    /// else its folder's, else the one he last picked, else the runner's
    /// default, else the first. A pool that's gone is skipped. Nil only with
    /// no pools at all.
    public static func newThreadPool(_ pools: [Pool], picked: String?, cwd: String, home: String, last: String?, runnerDefault: String?) -> String? {
        let ids = Set(pools.map(\.id))
        if let picked, ids.contains(picked) { return picked }
        if let folder = forFolder(pools, cwd: cwd, home: home) { return folder }
        if let last, ids.contains(last) { return last }
        if let runnerDefault, ids.contains(runnerDefault) { return runnerDefault }
        return pools.first?.id
    }

    /// Where Unpin puts a thread pinned to `accountId`: a pool that has that
    /// login (the folder's, then the last picked, then the first), so the
    /// thread stays on the login it's on. With none that has it, the
    /// folder's or the last one.
    public static func unpinTarget(_ pools: [Pool], accountId: String, cwd: String, home: String, last: String?) -> String? {
        let holding = pools.filter { $0.accountIds.contains(accountId) }
        let folder = forFolder(pools, cwd: cwd, home: home)
        for candidate in [folder, last] {
            if let candidate, holding.contains(where: { $0.id == candidate }) { return candidate }
        }
        if let first = holding.first { return first.id }
        return newThreadPool(pools, picked: nil, cwd: cwd, home: home, last: last, runnerDefault: nil)
    }

    // MARK: Logins

    /// The pool's logins the runner has, in the pool's order, optionally
    /// only one provider's.
    public static func logins(_ pool: Pool, accounts: [Account], provider: String? = nil) -> [Account] {
        pool.accountIds.compactMap { id in accounts.first { $0.id == id } }
            .filter { provider == nil || providerId($0) == provider }
    }

    /// The providers in the pool, in the order its logins first name them.
    public static func providers(_ pool: Pool, accounts: [Account]) -> [String] {
        var seen: [String] = []
        for account in logins(pool, accounts: accounts) where !seen.contains(providerId(account)) {
            seen.append(providerId(account))
        }
        return seen
    }

    static func providerId(_ account: Account) -> String { account.provider.isEmpty ? "claude" : account.provider }

    /// Has room: not blocked, and at or over the floor (or its usage isn't
    /// known), as the runner's `MIN_ROOM_PERCENT`.
    public static func hasRoom(_ account: Account) -> Bool {
        if account.usage?.blocked == true { return false }
        if let room = AccountUsageText.room(account.usage), room < AccountUsageText.pickFloor { return false }
        return true
    }

    /// The login a turn of `provider` would run on: `current` while it has
    /// room (each move costs a cold prompt cache), else, of those with
    /// room, the one whose weekly window resets soonest (what's left in it
    /// then is lost), the most room breaking a tie, as the runner's
    /// `pickInPool`; else one whose usage isn't known, else the first.
    /// Notes don't count here: a pool lists only logins it may use.
    public static func pickLogin(_ pool: Pool, accounts: [Account], provider: String, current: String? = nil, now: Date = Date()) -> String? {
        let own = logins(pool, accounts: accounts, provider: provider)
        if let current, let account = own.first(where: { $0.id == current }), hasRoom(account) { return current }
        let open = own.filter(hasRoom)
        let known = open.compactMap { account in
            AccountUsageText.room(account.usage).map { (id: account.id, room: $0, reset: weeklyReset(account, now: now)) }
        }
        var best: (id: String, room: Double, reset: Double)?
        for login in known {
            if let have = best, !(login.reset < have.reset || (login.reset == have.reset && login.room > have.room)) { continue }
            best = login
        }
        if let best { return best.id }
        return open.first?.id ?? own.first?.id
    }

    /// When the login's weekly window resets, in whole minutes so readings
    /// a few seconds apart tie; infinity with no weekly reset known or one
    /// already past, as the runner's `weeklyReset`.
    static func weeklyReset(_ account: Account, now: Date) -> Double {
        guard let reset = account.usage?.windows.first(where: { $0.kind == "weekly" })?.resetsAt, reset > now else { return .infinity }
        return (reset.timeIntervalSince1970 / 60).rounded()
    }

    /// The best room among the pool's logins; a blocked one counts as none.
    /// Nil when no login's usage is known.
    public static func bestRoom(_ pool: Pool, accounts: [Account]) -> Double? {
        logins(pool, accounts: accounts).compactMap { account -> Double? in
            if account.usage?.blocked == true { return 0 }
            return AccountUsageText.room(account.usage)
        }.max()
    }

    /// Under the pool's title in the rail: how many logins, and the best
    /// room among them ("3 · 61%"), or just the count with nothing known.
    public static func railSubtitle(_ pool: Pool, accounts: [Account]) -> String {
        let count = "\(logins(pool, accounts: accounts).count)"
        guard let best = bestRoom(pool, accounts: accounts) else { return count }
        return count + " · " + AccountUsageText.percent(best)
    }

    /// The model chip's badge for a pool thread: only when every login in
    /// the pool is low or blocked, since otherwise the runner moves it to
    /// one with room. Red when all are blocked, else orange with the best
    /// room left.
    public static func chipBadge(_ pool: Pool, accounts: [Account]) -> AccountUsageText.ChipBadge? {
        let all = logins(pool, accounts: accounts)
        guard !all.isEmpty else { return nil }
        if all.allSatisfy({ $0.usage?.blocked == true }) { return .blocked }
        let low = all.allSatisfy { account in
            if account.usage?.blocked == true { return true }
            guard let room = AccountUsageText.room(account.usage) else { return false }
            return room < AccountUsageText.chipWarning
        }
        guard low, let best = bestRoom(pool, accounts: accounts) else { return nil }
        return .low(AccountUsageText.percent(best))
    }

    /// The quiet line under Effort: each login's short name and room, the
    /// most room first, and the ones at their limit last, saying when they
    /// come back: "acme 61% · gmail 16% · bearworks resting until
    /// 5 am". Nil for a pool with no logins.
    public static func statusLine(_ pool: Pool, accounts: [Account], now: Date, calendar: Calendar = .current) -> String? {
        let all = logins(pool, accounts: accounts)
        guard !all.isEmpty else { return nil }
        // 0: room known, 1: not known, 2: resting. Stable within each.
        let ranked = all.enumerated().map { index, account -> (rank: Int, room: Double, index: Int, text: String) in
            let name = AccountName.login(account.id, accounts: accounts)
            if account.usage?.blocked == true {
                let reset = restingUntil(account.usage)
                return (2, 0, index, name + " resting" + (reset.map { " until " + AccountUsageText.resetTime($0, now: now, calendar: calendar) } ?? ""))
            }
            if let room = AccountUsageText.room(account.usage) { return (0, room, index, name + " " + AccountUsageText.percent(room)) }
            return (1, 0, index, name)
        }
        return ranked.sorted { a, b in
            if a.rank != b.rank { return a.rank < b.rank }
            if a.room != b.room { return a.room > b.room }
            return a.index < b.index
        }.map(\.text).joined(separator: " · ")
    }

    /// When a blocked login has room again: the reset of the window that
    /// ran out, else the soonest reset.
    static func restingUntil(_ usage: AccountUsage?) -> Date? {
        let main = AccountUsageText.mainWindows(usage)
        return main.filter { $0.remainingPercent < 1 }.compactMap(\.resetsAt).min() ?? main.compactMap(\.resetsAt).min()
    }

    // MARK: Words

    /// The model chip: who runs it (the pool's title, or a pinned login's
    /// short name), the model and the effort: "Claude · Opus 5.5 · High".
    public static func chipTitle(owner: String?, model: String?, effort: String?, fast: Bool = false, choices: [ModelChoice]) -> String {
        let rest = ComposerChips.chipTitle(model: model, effort: effort, fast: fast, choices: choices)
        guard let owner, !owner.isEmpty else { return rest }
        return owner + " · " + rest
    }

    /// The sidebar row's second line for a thread waiting for room:
    /// "Waiting for room · 5:00 am", "Waiting for room · tomorrow 5:00 am".
    public static func waitingLine(_ waiting: WaitingForRoom, now: Date, calendar: Calendar = .current) -> String {
        "Waiting for room · " + waitingTime(waiting.until, now: now, calendar: calendar)
    }

    /// The open thread's line while it waits: "Every login in Evenings is
    /// out of room. It carries on by itself at 5:25 pm."
    public static func waitingSentence(poolTitle: String, _ waiting: WaitingForRoom, now: Date, calendar: Calendar = .current) -> String {
        let start = "Every login in \(poolTitle) is out of room. "
        if waiting.until <= now { return start + "It carries on any moment now." }
        let (day, clock) = waitingParts(waiting.until, now: now, calendar: calendar)
        return start + "It carries on by itself " + (day.map { "\($0) at " } ?? "at ") + clock + "."
    }

    /// A time with its minutes always shown ("5:00 am"), with the day in
    /// front when it isn't today. Past times read "any moment now": the
    /// runner is about to carry on.
    static func waitingTime(_ date: Date, now: Date, calendar: Calendar) -> String {
        if date <= now { return "any moment now" }
        let (day, clock) = waitingParts(date, now: now, calendar: calendar)
        return day.map { $0 + " " + clock } ?? clock
    }

    /// "5:00 am", and the day when it isn't today ("tomorrow", "Thu").
    static func waitingParts(_ date: Date, now: Date, calendar: Calendar) -> (day: String?, clock: String) {
        let parts = calendar.dateComponents([.hour, .minute], from: date)
        let hour = parts.hour ?? 0
        let twelve = hour % 12 == 0 ? 12 : hour % 12
        let clock = String(format: "%d:%02d %@", twelve, parts.minute ?? 0, hour < 12 ? "am" : "pm")
        if calendar.isDate(date, inSameDayAs: now) { return (nil, clock) }
        return (AccountUsageText.dayName(date, now: now, calendar: calendar), clock)
    }

    // MARK: Icons

    /// The symbols Settings › Pools offers. Plain, one-colour SF Symbols
    /// that read at the composer chip's size.
    public static let iconChoices = [
        "sparkle", "chevron.left.forwardslash.chevron.right", "briefcase", "house",
        "hammer", "wrench.and.screwdriver", "paintbrush", "film",
        "book", "graduationcap", "leaf", "flame",
        "bolt", "moon", "sun.max", "star",
        "heart", "flag", "bookmark", "tag",
        "globe", "building.2", "cart", "gamecontroller",
        "music.note", "camera", "airplane", "cup.and.saucer",
    ]

    /// The pool's icon: the one Mike picked, else its first provider's
    /// (Claude's sparkle, Codex's brackets), else a stack.
    public static func symbol(_ pool: Pool, accounts: [Account]) -> String {
        if let icon = pool.icon, !icon.isEmpty { return icon }
        if let first = providers(pool, accounts: accounts).first { return ModelPicker.providerSymbol(first) }
        return "square.stack"
    }

    // MARK: Settings

    /// Why the runner would refuse this list in `pools.save`, in words, or
    /// nil when it's fine. The Settings window says this rather than send a
    /// list it knows will bounce.
    public static func problem(_ pools: [Pool], accounts: [Account]) -> String? {
        if pools.isEmpty { return "Keep at least one pool." }
        var seen = Set<String>()
        for pool in pools {
            let name = pool.title.trimmingCharacters(in: .whitespaces)
            if name.isEmpty { return "Every pool needs a name." }
            if !seen.insert(pool.id).inserted { return "Two pools have the id \(pool.id)." }
            if pool.accountIds.isEmpty { return "\(name) needs at least one login." }
            if !accounts.isEmpty, let unknown = pool.accountIds.first(where: { id in !accounts.contains { $0.id == id } }) {
                return "\(name) has a login the runner doesn't know (\(unknown))."
            }
        }
        return nil
    }

    /// Settings › Pools' logins for a pool: its own, ticked, in its order
    /// (the order that decides which provider comes first), then the rest
    /// in the runner's order.
    public static func loginList(_ pool: Pool, accounts: [Account]) -> [(account: Account, included: Bool)] {
        let own = logins(pool, accounts: accounts)
        return own.map { ($0, true) } + accounts.filter { account in !own.contains { $0.id == account.id } }.map { ($0, false) }
    }

    /// Ticks or unticks a login; a newly ticked one goes last.
    public static func setting(_ pool: Pool, login: String, included: Bool) -> Pool {
        var pool = pool
        pool.accountIds.removeAll { $0 == login }
        if included { pool.accountIds.append(login) }
        return pool
    }

    /// Moves a ticked login one place up (`by: -1`) or down.
    public static func moving(_ pool: Pool, login: String, by step: Int) -> Pool {
        var pool = pool
        guard let index = pool.accountIds.firstIndex(of: login) else { return pool }
        let target = index + step
        guard pool.accountIds.indices.contains(target) else { return pool }
        pool.accountIds.swapAt(index, target)
        return pool
    }

    /// A ticked login dragged onto another: it takes that one's place, and
    /// the rest shift along.
    public static func moving(_ pool: Pool, login: String, onto target: String) -> Pool {
        var pool = pool
        guard login != target, let from = pool.accountIds.firstIndex(of: login), let to = pool.accountIds.firstIndex(of: target) else { return pool }
        pool.accountIds.remove(at: from)
        pool.accountIds.insert(login, at: to)
        return pool
    }

    /// The pool with a new name, trimmed, or nil when there's nothing to
    /// save: a blank name (the runner refuses those) or the same one. The id
    /// stays, so threads and folders on the pool keep it.
    public static func renaming(_ pool: Pool, to title: String) -> Pool? {
        let title = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty, title != pool.title else { return nil }
        var pool = pool
        pool.title = title
        return pool
    }

    /// An id for a new pool from its title: lowercase words joined by
    /// dashes, with "-2", "-3" when it's taken.
    public static func newId(title: String, taken: [String]) -> String {
        let words = title.lowercased().split { !($0.isLetter || $0.isNumber) }.joined(separator: "-")
        let base = words.isEmpty ? "pool" : words
        guard taken.contains(base) else { return base }
        var n = 2
        while taken.contains("\(base)-\(n)") { n += 1 }
        return "\(base)-\(n)"
    }
}
