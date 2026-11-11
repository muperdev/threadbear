import Foundation
import XCTest
@testable import ThreadbearCore

/// The transcript's rules written down where the Rust port can read them:
/// for every mock-runner fixture, the rows the real reducer makes from its
/// events, with every work group closed and then with every group open. The
/// same files are read by desktop/core/tests/golden.rs, so the two reducers
/// can only both pass by agreeing row for row.
///
/// Swift is the definition of right. After a change to how rows are built,
/// run with UPDATE_GOLDEN=1 to write the files, and the Rust port has to
/// follow.
///
/// The thread's status from thread.json doesn't appear here: the Working
/// row comes from the events alone (a `turn.started` with no `turn.completed`,
/// `openTurn` in the reducer), and the app never passes the status to the
/// model (ThreadSession only opens a sub-agent's live turn from it). What
/// does come from thread.json, as it does in ThreadSession.open: the root
/// (`cwd`), the background work and the loose-ends check.
///
/// Nothing in the row text depends on the current time or anything else
/// that varies between runs: `RowDebug.excerpt` doesn't use the home folder
/// (it shows titles raw, not `ToolPresentation.shortTitle`) or today's date,
/// and durations and the gap between turns are worked out from the events'
/// own timestamps. If that ever changes, normalise it out here and in
/// golden.rs and say so.
final class GoldenTranscriptTests: XCTestCase {
    struct GoldenRow: Codable, Equatable {
        var id: String
        var kind: String
        var text: String
        /// Only for rows that have the notion, as in the app's dump (AppDump.opens).
        var expanded: Bool?
        var inGroup: Bool
        var seq: Int
        var turn: String?
    }

    struct GoldenFile: Codable, Equatable {
        var closed: [GoldenRow]
        var open: [GoldenRow]
    }

    private static let repo = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    private static let fixtures = repo.appendingPathComponent("app/Tools/mock-runner/fixtures")
    private static let golden = repo.appendingPathComponent("desktop/core/tests/golden")

    private let updating = ProcessInfo.processInfo.environment["UPDATE_GOLDEN"] == "1"

    func testEveryFixtureMatchesItsGoldenFile() throws {
        let names = try FileManager.default.contentsOfDirectory(atPath: Self.fixtures.path).filter { name in
            FileManager.default.fileExists(atPath: Self.fixtures.appendingPathComponent(name).appendingPathComponent("events.ndjson").path)
        }.sorted()
        XCTAssertFalse(names.isEmpty, "no fixtures found at \(Self.fixtures.path)")
        if updating {
            try FileManager.default.createDirectory(at: Self.golden, withIntermediateDirectories: true)
        }
        // A golden file left behind by a fixture that's gone.
        let files = (try? FileManager.default.contentsOfDirectory(atPath: Self.golden.path)) ?? []
        for file in files where file.hasSuffix(".json") && !names.contains(String(file.dropLast(5))) {
            XCTFail("\(file) has no fixture")
        }
        for name in names {
            try check(name)
        }
    }

    private func check(_ name: String) throws {
        let folder = Self.fixtures.appendingPathComponent(name)
        let lines = try String(contentsOf: folder.appendingPathComponent("events.ndjson"), encoding: .utf8)
            .split(separator: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        let decoder = JSONDecoder()
        let events = try lines.map { try decoder.decode(Event.self, from: Data($0.utf8)) }
        let thread = try decoder.decode(ThreadSummary.self, from: Data(contentsOf: folder.appendingPathComponent("thread.json")))

        // As ThreadSession.open does for a thread that's opened.
        var model = TranscriptModel(root: thread.cwd)
        _ = model.setLooseEnds(thread.looseEnds)
        model.load(events: events)
        _ = model.setBackground(thread.background)
        let closed = model.rows.map(Self.golden)

        // Every group open, so the steps folded in them show. Opening a
        // group only adds rows under it and never makes another group.
        for row in model.rows where row.kind == .group {
            _ = model.setExpanded(row.id, true)
        }
        let open = model.rows.map(Self.golden)

        let produced = GoldenFile(closed: closed, open: open)
        let file = Self.golden.appendingPathComponent(name + ".json")
        if updating {
            try Self.write(produced, to: file)
            return
        }
        guard let data = try? Data(contentsOf: file) else {
            XCTFail("\(name): no golden file; run with UPDATE_GOLDEN=1")
            return
        }
        let expected = try decoder.decode(GoldenFile.self, from: data)
        Self.compare(produced.closed, expected.closed, "\(name) closed")
        Self.compare(produced.open, expected.open, "\(name) open")
    }

    private static func golden(_ row: TranscriptRow) -> GoldenRow {
        GoldenRow(id: row.id, kind: row.kind.rawValue, text: RowDebug.excerpt(row), expanded: opens(row) ? isExpanded(row) : nil,
                  inGroup: row.inGroup, seq: row.seq, turn: row.turnId)
    }

    /// AppDump.opens: the rows that show a chevron.
    private static func opens(_ row: TranscriptRow) -> Bool {
        switch row.content {
        case .group, .tool, .reasoning: return true
        case .approval(let item): return item.resolution != nil && item.opens
        default: return false
        }
    }

    /// A group carries its open state; item rows keep theirs in the app's
    /// session, which is empty here (TranscriptController.isExpanded).
    private static func isExpanded(_ row: TranscriptRow) -> Bool {
        if case .group(let group) = row.content { return group.isExpanded }
        return false
    }

    private static func write(_ file: GoldenFile, to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        var data = try encoder.encode(file)
        data.append(UInt8(ascii: "\n"))
        try data.write(to: url)
    }

    private static func compare(_ produced: [GoldenRow], _ expected: [GoldenRow], _ what: String, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(produced.map(\.id), expected.map(\.id), "\(what): the row ids differ", file: file, line: line)
        for (a, b) in zip(produced, expected) where a != b {
            XCTFail("\(what): \(a.id): produced \(a), expected \(b)", file: file, line: line)
            return
        }
    }
}
