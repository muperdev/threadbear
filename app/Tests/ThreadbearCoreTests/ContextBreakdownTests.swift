import Foundation
import Testing
@testable import ThreadbearCore

@Suite struct ContextBreakdownTests {
    private func answer(_ json: String) throws -> ContextUsageAnswer {
        try JSONDecoder().decode(ContextUsageAnswer.self, from: Data(json.utf8))
    }

    /// The rows Ghostex's panel showed on 9 Nov, as the CLI sends them
    /// (its own order, a colour name we ignore, deferred rows mixed in).
    private let ghostex = """
    { "live": true, "somethingNew": 1, "usage": { "at": "2026-11-10T01:00:00Z", "model": "claude-opus-5-5",
      "totalTokens": 323000, "maxTokens": 1000000, "categories": [
        { "id": "system_prompt", "name": "System prompt", "tokens": 4500, "kind": "used" },
        { "id": "system_tools", "name": "System tools", "tokens": 2300, "kind": "used" },
        { "id": "mcp_instructions", "name": "MCP server instructions", "tokens": 596, "kind": "used" },
        { "id": "mcp_tools", "name": "MCP tools", "tokens": 1700, "kind": "used" },
        { "id": "mcp_tools_deferred", "name": "MCP tools (deferred)", "tokens": 173000, "kind": "deferred" },
        { "id": "system_tools_deferred", "name": "System tools (deferred)", "tokens": 16000, "kind": "deferred" },
        { "id": "memory_files", "name": "Memory files", "tokens": 15000, "kind": "used" },
        { "id": "skills", "name": "Skills", "tokens": 8100, "kind": "used" },
        { "id": "custom_agents", "name": "Custom agents", "tokens": 0, "kind": "used" },
        { "id": "messages", "name": "Messages", "tokens": 291000, "kind": "used" },
        { "id": "autocompact_buffer", "name": "Autocompact buffer", "tokens": 33000, "kind": "buffer" },
        { "id": "free_space", "name": "Free space", "tokens": 644000, "kind": "free" }
      ] } }
    """

    @Test func readsTheAnswerAndOrdersRowsAsGhostexDid() throws {
        let a = try answer(ghostex)
        #expect(a.live)
        let b = try #require(ContextBreakdown(a.usage))
        #expect(b.headline == "32% used")
        #expect(b.amount == "323k / 1M")
        #expect(b.rows.map(\.name) == [
            "Messages", "Memory files", "Skills", "System tools", "System prompt", "MCP server instructions", "MCP tools",
            "Autocompact buffer", "Free space", "MCP tools (deferred)", "System tools (deferred)",
        ])
        // A row with nothing in it is left out.
        #expect(!b.rows.contains { $0.name == "Custom agents" })
        let messages = try #require(b.rows.first)
        #expect(messages.swatch == .messages)
        #expect(messages.tokenText == "291k")
        #expect(messages.percentText == "29%")
        #expect(b.rows.first { $0.name == "Memory files" }?.percentText == "1.5%")
        #expect(b.rows.first { $0.name == "MCP server instructions" }?.percentText == "0.1%")
        // Deferred rows aren't in the window: tokens but no share of it.
        let deferred = b.rows.filter(\.deferred)
        #expect(deferred.map(\.tokenText) == ["173k", "16k"])
        #expect(deferred.allSatisfy { $0.percentText == nil })
        // The bar: what's used, in the rows' order, then the buffer.
        #expect(b.bar.map(\.swatch) == [.messages, .memory, .skills, .system, .system, .mcp, .mcp, .buffer])
    }

    @Test func toleratesWhatItDoesntKnow() throws {
        let a = try answer("""
        { "live": false, "usage": { "at": "2026-11-10T01:00:00Z", "totalTokens": 1200, "maxTokens": 200000, "categories": [
          { "name": "Old CLI row", "tokens": 1000, "kind": "used" },
          { "id": "brand_new", "name": "Brand new", "tokens": 200, "kind": "used" },
          { "id": "messages", "name": "Messages", "tokens": "lots", "kind": "used" },
          { "id": "x", "name": "Some new kind", "tokens": 5, "kind": "sideways" },
          { "id": "free_space", "name": "Free space", "tokens": 198800, "kind": "free" }
        ] } }
        """)
        let b = try #require(ContextBreakdown(a.usage))
        #expect(b.rows.map(\.name) == ["Old CLI row", "Brand new", "Free space"])
        #expect(b.rows.first?.swatch == .other)
        #expect(ContextBreakdown(nil) == nil)
        // No usage at all (Codex, or nothing kept): still an answer.
        #expect(try answer(#"{ "live": false }"#).usage == nil)
    }

    /// Daybreak, 10 Nov 2026: a paired runner's answer crashed the app when
    /// the popover opened. Two rows of 5e18 tokens overflowed the sum, and
    /// a total of 1e18 in a window of 1 overflowed the percentage. Counts
    /// outside 0...countLimit are read as none, and nothing traps.
    @Test func aRunnersHugeCountsAreRefusedNotCrashed() throws {
        let sum = try answer("""
        { "live": true, "usage": { "totalTokens": 100, "maxTokens": 200000, "categories": [
          { "id": "messages", "name": "Messages", "tokens": 5000000000000000000, "kind": "used" },
          { "id": "skills", "name": "Skills", "tokens": 5000000000000000000, "kind": "used" },
          { "id": "system_prompt", "name": "System prompt", "tokens": -9000000000000000000, "kind": "used" },
          { "id": "memory_files", "name": "Memory files", "tokens": 100, "kind": "used" }
        ] } }
        """)
        #expect(sum.usage?.categories.map(\.name) == ["Memory files"])
        let b = try #require(ContextBreakdown(sum.usage))
        #expect(b.headline == "0% used")

        let percent = try answer(#"{ "live": true, "usage": { "totalTokens": 1000000000000000000, "maxTokens": 1, "categories": [] } }"#)
        #expect(percent.usage?.totalTokens == 0)
        #expect(percent.usage?.maxTokens == 1)
        #expect(ContextBreakdown(percent.usage)?.headline == "0% used")
        let max = try answer(#"{ "live": true, "usage": { "totalTokens": 9223372036854775807, "maxTokens": 9223372036854775807, "categories": [] } }"#)
        #expect(ContextBreakdown(max.usage) == nil, "neither count is one, so there's no window")
        #expect(try answer(#"{ "live": true, "usage": { "totalTokens": 5, "maxTokens": 1099511627777, "categories": [] } }"#).usage?.maxTokens == 0)
    }

    /// Made in code with any Int (not through the decoder), the presenter
    /// still saturates and clamps rather than trapping.
    @Test func thePresenterNeverTraps() throws {
        let big = ContextUsage(at: nil, totalTokens: Int.max, maxTokens: 1, categories: [
            .init(id: "messages", name: "Messages", tokens: Int.max, kind: "used"),
            .init(id: "skills", name: "Skills", tokens: Int.max, kind: "used"),
        ])
        let b = try #require(ContextBreakdown(big))
        #expect(b.headline == "9999% used")
        #expect(b.rows.first?.percentText == "9999%")
        let unwindowed = try #require(ContextBreakdown(ContextUsage(at: nil, totalTokens: Int.max, maxTokens: 0, categories: [
            .init(id: "messages", name: "Messages", tokens: Int.max, kind: "used"),
            .init(id: "skills", name: "Skills", tokens: Int.max, kind: "used"),
        ])))
        #expect(unwindowed.window == Int.max, "the sum saturates")
        #expect(unwindowed.headline == "100% used")
        #expect(ContextBreakdown.share(.infinity) == "9999%")
        #expect(ContextBreakdown.share(.nan) == "0%")
        #expect(ContextBreakdown.share(-.infinity) == "0%")
    }

    @Test func shareWords() {
        #expect(ContextBreakdown.share(0.29) == "29%")
        #expect(ContextBreakdown.share(0.015) == "1.5%")
        #expect(ContextBreakdown.share(0.0006) == "0.1%")
        #expect(ContextBreakdown.share(0.00004) == "<0.1%")
        #expect(ContextBreakdown.share(0) == "0%")
    }

    @Test func ageWords() {
        let at = Date(timeIntervalSince1970: 1_000_000)
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(20)) == "just now")
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(-5)) == "just now")
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(65)) == "1 min ago")
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(5 * 60 + 10)) == "5 min ago")
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(3 * 3600)) == "3 hours ago")
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(3600)) == "1 hour ago")
        #expect(ContextBreakdown.age(at, now: at.addingTimeInterval(2 * 86_400)) == "2 days ago")
    }
}
