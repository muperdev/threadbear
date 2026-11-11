import Foundation

/// What fills a thread's context, in Claude's own words
/// (`threads.contextUsage`, docs/protocol.md "Context breakdown"), as the
/// runner kept it. Read leniently: a row that isn't a name and a number is
/// left out rather than losing the lot.
///
/// The numbers come from whichever runner has the thread, which may be
/// another Mac's, so nothing in them may crash this one: a count outside
/// 0...`countLimit` is read as no count (a row is left out), and the
/// breakdown below adds and divides without trapping (Daybreak, 10 Nov
/// 2026: two rows of 5e18 tokens overflowed the sum, and a total of 1e18
/// in a window of 1 overflowed the percentage).
public struct ContextUsage: Sendable, Equatable, Decodable {
    /// Far above any model's window (a few million today), and far below
    /// where adding a few hundred rows of it could overflow.
    public static let countLimit = 1 << 40

    /// A count as sent, if it's one.
    static func count(_ n: Int?) -> Int? {
        guard let n, (0...countLimit).contains(n) else { return nil }
        return n
    }

    public struct Category: Sendable, Equatable, Decodable {
        /// "messages", "memory_files"...; absent from an older CLI, and a
        /// newer one may add ids this app doesn't know.
        public var id: String?
        public var name: String
        public var tokens: Int
        /// "used", "buffer", "free" or "deferred"; another is left out.
        public var kind: String

        public init(id: String? = nil, name: String, tokens: Int, kind: String) {
            self.id = id
            self.name = name
            self.tokens = tokens
            self.kind = kind
        }

        private enum CodingKeys: String, CodingKey { case id, name, tokens, kind }

        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            id = try c.decodeIfPresent(String.self, forKey: .id)
            name = try c.decode(String.self, forKey: .name)
            kind = try c.decode(String.self, forKey: .kind)
            guard let tokens = ContextUsage.count(try c.decode(Int.self, forKey: .tokens)) else {
                throw DecodingError.dataCorruptedError(forKey: .tokens, in: c, debugDescription: "not a token count")
            }
            self.tokens = tokens
        }
    }

    public var at: Date?
    public var model: String?
    public var totalTokens: Int
    public var maxTokens: Int
    public var categories: [Category]

    private enum CodingKeys: String, CodingKey { case at, model, totalTokens, maxTokens, categories }

    /// Skips a row it can't read instead of failing the whole answer.
    private struct Lenient: Decodable {
        let category: Category?
        init(from decoder: Decoder) { category = try? Category(from: decoder) }
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        at = ((try? c.decodeIfPresent(String.self, forKey: .at)) ?? nil).flatMap(ISO8601.parse)
        model = (try? c.decodeIfPresent(String.self, forKey: .model)) ?? nil
        totalTokens = Self.count(try? c.decode(Int.self, forKey: .totalTokens)) ?? 0
        maxTokens = Self.count(try? c.decode(Int.self, forKey: .maxTokens)) ?? 0
        categories = ((try? c.decode([Lenient].self, forKey: .categories)) ?? []).compactMap(\.category)
    }

    public init(at: Date?, model: String? = nil, totalTokens: Int, maxTokens: Int, categories: [Category]) {
        self.at = at
        self.model = model
        self.totalTokens = totalTokens
        self.maxTokens = maxTokens
        self.categories = categories
    }
}

/// The runner's answer: the breakdown, if there is one; whether the thread
/// has a running CLI to ask again; and why it's an old one, when asking failed.
public struct ContextUsageAnswer: Sendable, Equatable, Decodable {
    public var usage: ContextUsage?
    public var live: Bool
    public var note: String?

    private enum CodingKeys: String, CodingKey { case usage, live, note }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        usage = (try? c.decodeIfPresent(ContextUsage.self, forKey: .usage)) ?? nil
        live = (try? c.decode(Bool.self, forKey: .live)) ?? false
        note = (try? c.decodeIfPresent(String.self, forKey: .note)) ?? nil
    }

    public init(usage: ContextUsage?, live: Bool, note: String? = nil) {
        self.usage = usage
        self.live = live
        self.note = note
    }
}

/// The breakdown as the composer ring's popover draws it (after Ghostex's
/// panel, which Mike liked on 9 Nov): "32% used", "323k / 1M", a stacked
/// bar, then a row each for what's in the window, the buffer kept for
/// compacting, the free space, and the tool schemas kept out of it.
public struct ContextBreakdown: Equatable, Sendable {
    /// Colour groups, so the view picks the colours and like rows share one.
    public enum Swatch: String, Sendable {
        case messages, memory, skills, agents, system, mcp, other, buffer, free
    }

    public struct Row: Equatable, Sendable, Identifiable {
        public var id: String
        public var name: String
        public var tokens: Int
        public var swatch: Swatch
        /// Tool schemas outside the window until loaded: listed, not counted.
        public var deferred: Bool
        /// Its share of the window, nil for a deferred row.
        public var fraction: Double?

        public var tokenText: String { ContextMeter.tokens(tokens) }
        public var percentText: String? { fraction.map(ContextBreakdown.share) }
    }

    public let used: Int
    public let window: Int
    public let at: Date?
    public let rows: [Row]

    /// The order Ghostex lists them in: the conversation first, then what
    /// each session starts with. Ids this app doesn't know come after, in
    /// the CLI's order.
    private static let order: [String: (Int, Swatch)] = [
        "messages": (0, .messages),
        "memory_files": (1, .memory),
        "skills": (2, .skills),
        "custom_agents": (3, .agents),
        "system_tools": (4, .system),
        "system_prompt": (5, .system),
        "mcp_instructions": (6, .mcp),
        "mcp_tools": (7, .mcp),
        "autocompact_buffer": (0, .buffer),
        "compact_buffer": (0, .buffer),
        "free_space": (0, .free),
        "mcp_tools_deferred": (0, .mcp),
        "system_tools_deferred": (1, .system),
    ]

    public init?(_ usage: ContextUsage?) {
        guard let usage else { return nil }
        // Saturating, as the memberwise init takes any Int: the decoder's
        // limit keeps a runner's answer far from here, but nothing traps.
        let counted = usage.categories.filter { $0.kind != "deferred" }.reduce(0) { sum, c in
            let (next, over) = sum.addingReportingOverflow(max(0, c.tokens))
            return over ? Int.max : next
        }
        let window = usage.maxTokens > 0 ? usage.maxTokens : counted
        guard window > 0 else { return nil }
        func rows(_ kind: String) -> [Row] {
            usage.categories.enumerated()
                .filter { $0.element.kind == kind && ($0.element.tokens > 0 || kind == "free") }
                .sorted { a, b in
                    let x = a.element.id.flatMap { Self.order[$0]?.0 } ?? 100 + a.offset
                    let y = b.element.id.flatMap { Self.order[$0]?.0 } ?? 100 + b.offset
                    return x < y
                }
                .map { i, c in
                    let swatch = kind == "buffer" ? .buffer : kind == "free" ? .free : c.id.flatMap { Self.order[$0]?.1 } ?? .other
                    return Row(id: c.id ?? "\(kind)-\(i)", name: c.name, tokens: c.tokens, swatch: swatch, deferred: kind == "deferred",
                               fraction: kind == "deferred" ? nil : Double(c.tokens) / Double(window))
                }
        }
        self.used = usage.totalTokens
        self.window = window
        self.at = usage.at
        self.rows = rows("used") + rows("buffer") + rows("free") + rows("deferred")
    }

    /// "32% used".
    public var headline: String { "\(Self.wholePercent(Double(used) / Double(window)))% used" }
    /// "323k / 1M".
    public var amount: String { "\(ContextMeter.tokens(used)) / \(ContextMeter.tokens(window))" }
    /// The bar's pieces, left to right: what's used, then the buffer.
    public var bar: [Row] { rows.filter { !$0.deferred && $0.swatch != .free } }

    /// "29%", "1.5%", "0.1%", "<0.1%": whole numbers from 10%, one place
    /// below, so the small rows don't all read 0%.
    public static func share(_ fraction: Double) -> String {
        let p = fraction * 100
        // Not `p <= 0`: NaN is neither.
        if !(p > 0) { return "0%" }
        if p >= 10 { return "\(wholePercent(fraction))%" }
        let tenth = (p * 10).rounded() / 10
        if tenth < 0.1 { return "<0.1%" }
        let s = String(format: "%.1f", tenth)
        return (s.hasSuffix(".0") ? String(s.dropLast(2)) : s) + "%"
    }

    /// A fraction as a whole percentage, held to 0...9999 first: a Double
    /// past Int's range (or NaN) traps when it's made an Int.
    static func wholePercent(_ fraction: Double) -> Int {
        guard !fraction.isNaN else { return 0 }
        return Int((min(max(fraction, 0), 99.99) * 100).rounded())
    }

    /// "just now", "5 min ago", "3 hours ago", "2 days ago".
    public static func age(_ at: Date, now: Date) -> String {
        let seconds = now.timeIntervalSince(at)
        if seconds < 60 { return "just now" }
        let minutes = Int(seconds / 60)
        if minutes < 60 { return "\(minutes) min ago" }
        let hours = minutes / 60
        if hours < 24 { return hours == 1 ? "1 hour ago" : "\(hours) hours ago" }
        let days = hours / 24
        return days == 1 ? "1 day ago" : "\(days) days ago"
    }
}
