import AppKit
import XCTest
@testable import Threadbear
@testable import ThreadbearCore

/// What's in each composer outlives the app (10 Nov: a message a restart
/// cut off went back in its box, and the next relaunch threw it away).
@MainActor
final class DraftStoreTests: XCTestCase {
    private var folder: URL!

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("drafts-\(UUID().uuidString)")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: folder)
        super.tearDown()
    }

    func testDraftsComeBackAfterARelaunch() async throws {
        let url = DraftStore.url(dataDir: folder)
        let app = AppModel()
        let store = DraftStore(url: url)
        store.delay = .milliseconds(10)
        app.draftStore = store
        let picture = ImageRef(path: "/tmp/a.png", mediaType: "image/png")
        app.drafts["th1"] = Draft(text: "make the demo")
        app.drafts["new"] = Draft(text: "", images: [picture])
        app.drafts["th2"] = Draft(text: "  \n")
        for _ in 0..<200 where !FileManager.default.fileExists(atPath: url.path) {
            try await Task.sleep(for: .milliseconds(5))
        }
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)

        let relaunched = AppModel()
        relaunched.drafts["new"] = Draft(text: "typed already")
        DraftStore(url: url).restore(into: relaunched)
        XCTAssertEqual(relaunched.drafts["th1"]?.text, "make the demo")
        XCTAssertEqual(relaunched.drafts["new"]?.text, "typed already", "what's in a box already stays")
        XCTAssertNil(relaunched.drafts["th2"], "an empty box isn't kept")

        // Everything sent: the file goes.
        app.drafts = [:]
        store.save(app)
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
    }

    /// An edit of an earlier message isn't kept as a draft (it would go as
    /// a new message): what was in the box before it is.
    func testAnEditKeepsWhatWasInTheBoxBefore() {
        let kept = DraftStore.kept(drafts: ["th1": Draft(text: "fixed wording"), "th2": Draft(text: "other")],
                                   editing: ["th1": MessageEdit(seq: 4, before: Draft(text: "half a thought"))])
        XCTAssertEqual(kept["th1"]?.text, "half a thought")
        XCTAssertEqual(kept["th2"]?.text, "other")
        XCTAssertNil(DraftStore.kept(drafts: ["th1": Draft(text: "fixed")], editing: ["th1": MessageEdit(seq: 4, before: Draft())])["th1"])
    }

    /// Quitting while a message waits for a restarted runner: its words
    /// are in the box next time, before anything typed since.
    func testAHeldMessageGoesInTheBoxWhenTheAppLeaves() throws {
        let url = DraftStore.url(dataDir: folder)
        let app = AppModel()
        let session = app.session(for: "th1")
        session.addPending(id: "p1", text: "make the demo", images: [], answers: [], summarySeq: 3)
        session.addPending(id: "p2", text: "taken already", images: [], answers: [], summarySeq: 3)
        session.holdForResend("p1")
        app.drafts["th1"] = Draft(text: "and a title")
        let store = DraftStore(url: url)
        store.save(app)
        XCTAssertEqual(store.load()["th1"]?.text, "and a title", "only when leaving")
        store.save(app, leaving: true)
        XCTAssertEqual(store.load()["th1"]?.text, "make the demo\n\nand a title")
    }

    func testAnUnreadableFileIsNoDrafts() throws {
        let url = DraftStore.url(dataDir: folder)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("not json".utf8).write(to: url)
        XCTAssertTrue(DraftStore(url: url).load().isEmpty)
    }
}
