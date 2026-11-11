import AppKit
import XCTest
import SwiftTerm
@testable import Threadbear

/// Links clicked in the Terminal panel (Daybreak, 10 Nov). SwiftTerm hands
/// a clicked OSC 8 hyperlink, or a path it spots in the text, straight to
/// NSWorkspace, any scheme, so a repo's README printed with `cat` could
/// hold a `file:` link to its own `.command` and a click ran it in Terminal
/// without OpenGate's question. The terminal's view now opens every link
/// through OpenGate. No shell starts here, and nothing really opens: the
/// gate's seams record what would have.
@MainActor
final class TerminalLinkTests: XCTestCase {
    private var folder: URL!
    private var opened: [URL] = []
    private var asked: [OpenGate.Question] = []
    private var refused: [String] = []
    private var saved: (open: (URL) -> Void, openWith: (URL, URL) -> Void, ask: (OpenGate.Question) -> Bool, refuse: (String) -> Void, appFor: (URL) -> URL?)!

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("tb-terminal-links-\(UUID().uuidString)", isDirectory: true)
            .resolvingSymlinksInPath()
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        saved = (OpenGate.workspaceOpen, OpenGate.workspaceOpenWith, OpenGate.ask, OpenGate.refuse, OpenGate.appFor)
        opened = []; asked = []; refused = []
        OpenGate.workspaceOpen = { [unowned self] in self.opened.append($0) }
        OpenGate.workspaceOpenWith = { [unowned self] url, _ in self.opened.append(url) }
        OpenGate.ask = { [unowned self] in self.asked.append($0); return false }
        OpenGate.refuse = { [unowned self] in self.refused.append($0) }
        OpenGate.appFor = { url in
            switch url.pathExtension {
            case "command": URL(fileURLWithPath: "/System/Applications/Utilities/Terminal.app")
            case "": url.scheme == "file" ? nil : URL(fileURLWithPath: "/Applications/Slack.app")
            default: URL(fileURLWithPath: "/System/Applications/TextEdit.app")
            }
        }
    }

    override func tearDown() async throws {
        (OpenGate.workspaceOpen, OpenGate.workspaceOpenWith, OpenGate.ask, OpenGate.refuse, OpenGate.appFor) = saved
        try? FileManager.default.removeItem(at: folder)
    }

    /// The panel's own view, never started: a click on a link calls this.
    private func terminal() -> PromptWatchingTerminalView {
        let view = PromptWatchingTerminalView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        view.linkFolder = folder.path
        return view
    }

    /// A harmless `.command` set to run, as a repo can hold one.
    private func payload() throws -> URL {
        let file = folder.appendingPathComponent("payload.command")
        try "#!/bin/sh\nexit 0\n".write(to: file, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
        return file
    }

    func testAFileHyperlinkToAProgramAsksAndNoMeansItDoesntRun() throws {
        let file = try payload()
        let view = terminal()
        view.requestOpenLink(source: view, link: file.absoluteString, params: ["id": "x"])
        XCTAssertEqual(asked.count, 1, "a program asks first")
        XCTAssertTrue(opened.isEmpty)
    }

    /// The same file as a bare path SwiftTerm spotted, absolute, relative
    /// to the shell's folder, or with a `:12:4` line and column after it.
    func testAPathToAProgramAsksToo() throws {
        let file = try payload()
        let view = terminal()
        for link in [file.path, "payload.command", "./payload.command", "payload.command:12:4"] {
            asked = []
            view.requestOpenLink(source: view, link: link, params: [:])
            XCTAssertEqual(asked.count, 1, link)
        }
        XCTAssertTrue(opened.isEmpty)
    }

    func testAnotherSchemeAsksByName() {
        let view = terminal()
        view.requestOpenLink(source: view, link: "slack://open?team=T1", params: [:])
        XCTAssertEqual(asked.count, 1)
        XCTAssertEqual(asked.first?.detail.contains("slack://open?team=T1"), true)
        XCTAssertTrue(opened.isEmpty)
    }

    func testAWebPageOpens() throws {
        let view = terminal()
        view.requestOpenLink(source: view, link: "https://example.com/docs", params: [:])
        XCTAssertEqual(opened, [try XCTUnwrap(URL(string: "https://example.com/docs"))])
        XCTAssertTrue(asked.isEmpty)
    }

    func testALinkToNothingOpensNothing() {
        let view = terminal()
        view.requestOpenLink(source: view, link: "no-such-file-\(UUID().uuidString)", params: [:])
        view.requestOpenLink(source: view, link: "", params: [:])
        XCTAssertTrue(opened.isEmpty)
        XCTAssertTrue(asked.isEmpty)
        XCTAssertEqual(refused.count, 2)
    }

    func testLinkURLReadsRelativePathsFromTheShellsFolder() throws {
        let file = try payload()
        XCTAssertEqual(PromptWatchingTerminalView.linkURL("payload.command", folder: folder.path)?.path, file.path)
        XCTAssertEqual(PromptWatchingTerminalView.linkURL("payload.command:3", folder: folder.path)?.path, file.path)
        XCTAssertNil(PromptWatchingTerminalView.linkURL("payload.command", folder: "/"))
        XCTAssertEqual(PromptWatchingTerminalView.linkURL("https://x.test/a", folder: "/")?.scheme, "https")
    }

    /// Every terminal view the app makes is one that opens its links
    /// through OpenGate: a new one made straight from SwiftTerm's would
    /// open them itself again.
    func testEveryTerminalViewOpensItsLinksThroughTheGate() throws {
        let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let root = repo.appendingPathComponent("app/Sources")
        let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil)!
        var made: [String] = []
        var overrides = false
        for case let file as URL in files where file.pathExtension == "swift" {
            let source = try String(contentsOf: file, encoding: .utf8)
            for (i, line) in source.components(separatedBy: "\n").enumerated() {
                let code = line.components(separatedBy: "//").first ?? line
                if code.range(of: #"\b(LocalProcessTerminalView|TerminalView)\s*\("#, options: .regularExpression) != nil {
                    made.append("\(file.lastPathComponent):\(i + 1)")
                }
                if code.contains("override func requestOpenLink"), file.lastPathComponent == "TerminalPanel.swift" { overrides = true }
            }
        }
        XCTAssertEqual(made, [], "make PromptWatchingTerminalView, which opens links through OpenGate")
        XCTAssertTrue(overrides, "PromptWatchingTerminalView must override requestOpenLink")
    }
}
