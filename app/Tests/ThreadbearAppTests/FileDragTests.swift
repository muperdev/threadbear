import AppKit
import XCTest
@testable import Threadbear

/// What a drag out of the app puts on the pasteboard (FileDrag): a file
/// as the file, so X's upload box in Chrome takes the clip itself.
@MainActor
final class FileDragTests: XCTestCase {
    private func dragged(_ url: URL) throws -> NSPasteboard {
        let writer = try XCTUnwrap(FileDrag.writer(for: url))
        let board = NSPasteboard(name: .init("FileDragTests-\(UUID().uuidString)"))
        board.clearContents()
        XCTAssertTrue(board.writeObjects([writer]))
        addTeardownBlock { board.releaseGlobally() }
        return board
    }

    func testAFileGoesAsItsFileURL() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: folder) }
        let clip = folder.appendingPathComponent("plugboard B.mp4")
        try Data([0]).write(to: clip)

        let board = try dragged(clip)
        let files = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
        XCTAssertEqual(files?.map(\.path), [clip.path])
    }

    /// A page's ?query is for its own script, not part of the file.
    func testAFileLinksQueryIsLeftOff() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: folder) }
        let page = folder.appendingPathComponent("grow.html")
        try Data("<p>hi</p>".utf8).write(to: page)

        let board = try dragged(try XCTUnwrap(URL(string: page.absoluteString + "?clean&s=45")))
        let files = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
        XCTAssertEqual(files?.map(\.path), [page.path])
        XCTAssertNil(files?.first?.query)
    }

    func testAMissingFileHasNothingToDrag() {
        XCTAssertNil(FileDrag.writer(for: URL(fileURLWithPath: "/tmp/\(UUID().uuidString).mp4")))
    }

    func testAWebLinkGoesAsItsAddress() throws {
        let board = try dragged(try XCTUnwrap(URL(string: "https://github.com/mikecann/plugboard")))
        XCTAssertEqual(board.string(forType: .URL), "https://github.com/mikecann/plugboard")
        XCTAssertEqual(board.string(forType: .string), "https://github.com/mikecann/plugboard")
    }

    // MARK: Press or drag

    private func mouse(_ type: NSEvent.EventType, at point: NSPoint, in window: NSWindow) -> NSEvent {
        NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                           windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
    }

    /// The press as the queue hands it back: posting moves an event's
    /// location (the window isn't on screen), so the press goes through
    /// the same trip as the moves after it.
    private func press(at point: NSPoint, in window: NSWindow) throws -> NSEvent {
        NSApp.postEvent(mouse(.leftMouseDown, at: point, in: window), atStart: false)
        return try XCTUnwrap(window.nextEvent(matching: [.leftMouseDown], until: Date(), inMode: .eventTracking, dequeue: true))
    }

    private func window() -> NSWindow {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 200), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        addTeardownBlock { window.close() }
        return window
    }

    /// Let go without moving far: a click, with the mouse-up left for the
    /// view's own mouseDown, so a link still opens.
    func testAPressLetGoIsAClickAndKeepsItsMouseUp() throws {
        let window = window()
        let down = try press(at: NSPoint(x: 50, y: 50), in: window)
        NSApp.postEvent(mouse(.leftMouseDragged, at: NSPoint(x: 51, y: 51), in: window), atStart: false)
        NSApp.postEvent(mouse(.leftMouseUp, at: NSPoint(x: 51, y: 51), in: window), atStart: false)
        XCTAssertFalse(FileDrag.isDrag(from: down))
        let left = window.nextEvent(matching: [.leftMouseUp], until: Date(), inMode: .eventTracking, dequeue: true)
        XCTAssertEqual(left?.type, .leftMouseUp)
    }

    func testMovingFarEnoughIsADrag() throws {
        let window = window()
        let down = try press(at: NSPoint(x: 50, y: 50), in: window)
        NSApp.postEvent(mouse(.leftMouseDragged, at: NSPoint(x: 51, y: 50), in: window), atStart: false)
        NSApp.postEvent(mouse(.leftMouseDragged, at: NSPoint(x: 58, y: 50), in: window), atStart: false)
        XCTAssertTrue(FileDrag.isDrag(from: down))
        // Clear the queue for the next test.
        while NSApp.nextEvent(matching: .any, until: Date(), inMode: .default, dequeue: true) != nil {}
    }
}

/// The pane's preview takes a press on a picture or a video's picture, so
/// it can drag out, and leaves the rest to the view under it.
@MainActor
final class FilePreviewPressTests: XCTestCase {
    private func shown(_ name: String, _ data: Data) throws -> FilePreviewView {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent(name)
        try data.write(to: file)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let view = FilePreviewView(frame: window.contentView!.bounds)
        view.autoresizingMask = [.width, .height]
        window.contentView!.addSubview(view)
        view.show(file)
        window.layoutIfNeeded()
        addTeardownBlock { view.close(); window.close() }
        return view
    }

    private var middle: NSPoint { NSPoint(x: 200, y: 150) }

    func testAPictureTakesThePress() throws {
        let image = NSImage(size: NSSize(width: 20, height: 20), flipped: false) { rect in
            NSColor.systemPink.setFill(); rect.fill(); return true
        }
        let png = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation))?.representation(using: .png, properties: [:]))
        let view = try shown("shot.png", png)
        XCTAssertTrue(view.pressTarget(at: middle) === view)
    }

    func testAVideosPictureTakesThePress() throws {
        // Any bytes do: the player makes its views whether it plays or not.
        let view = try shown("clip.mp4", Data([0, 0, 0, 0]))
        XCTAssertTrue(view.pressTarget(at: middle) === view)
    }

    func testOtherFilesKeepTheirPresses() throws {
        let view = try shown("notes.pdf", Data("%PDF-1.4\n".utf8))
        XCTAssertFalse(view.pressTarget(at: middle) === view)
    }

    /// Right-click, Copy Image: the PNG's own bytes and a TIFF, so it
    /// pastes as a picture, not as the file or its path.
    func testCopyImagePutsThePictureOnThePasteboard() throws {
        let image = NSImage(size: NSSize(width: 20, height: 20), flipped: false) { rect in
            NSColor.systemPink.setFill(); rect.fill(); return true
        }
        let png = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation))?.representation(using: .png, properties: [:]))
        let view = try shown("shot.png", png)
        let click = try XCTUnwrap(NSEvent.mouseEvent(with: .rightMouseDown, location: middle, modifierFlags: [], timestamp: 0,
                                                     windowNumber: view.window!.windowNumber, context: nil,
                                                     eventNumber: 0, clickCount: 1, pressure: 1))
        // Another Mac's copy: nothing in the Finder to show.
        let menu = try XCTUnwrap(view.menu(for: click))
        XCTAssertEqual(menu.items.map(\.title), ["Copy Image"])

        let board = NSPasteboard(name: NSPasteboard.Name(UUID().uuidString))
        addTeardownBlock { board.releaseGlobally() }
        XCTAssertTrue(view.copyImage(to: board))
        XCTAssertEqual(board.data(forType: .png), png)
        XCTAssertNotNil(board.data(forType: .tiff))
        XCTAssertNil(board.string(forType: .fileURL))
        XCTAssertNotNil(NSImage(pasteboard: board))
    }

    /// A click (let go without moving) on a picture: the image view
    /// doesn't take presses, so it hands the click back up the chain to
    /// the preview, which handed it to the image view again, until the
    /// stack ran out (THREADBEAR-N, 5 Nov). It goes on to the pane once.
    func testClickingAPictureGoesOnToThePane() throws {
        let image = NSImage(size: NSSize(width: 20, height: 20), flipped: false) { rect in
            NSColor.systemPink.setFill(); rect.fill(); return true
        }
        let png = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation))?.representation(using: .png, properties: [:]))
        let view = try shown("shot.png", png)
        let window = try XCTUnwrap(view.window)
        let pane = PressRecorder(frame: window.contentView!.bounds)
        window.contentView!.addSubview(pane)
        view.removeFromSuperview()
        pane.addSubview(view)
        addTeardownBlock { while NSApp.nextEvent(matching: .any, until: Date(), inMode: .default, dequeue: true) != nil {} }

        func mouse(_ type: NSEvent.EventType) -> NSEvent {
            NSEvent.mouseEvent(with: type, location: middle, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                               windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        }
        NSApp.postEvent(mouse(.leftMouseDown), atStart: false)
        let down = try XCTUnwrap(window.nextEvent(matching: [.leftMouseDown], until: Date(), inMode: .eventTracking, dequeue: true))
        NSApp.postEvent(mouse(.leftMouseUp), atStart: false)

        XCTAssertTrue(view.pressTarget(at: middle) === view)
        view.mouseDown(with: down)
        XCTAssertEqual(pane.presses, 1)
    }

    /// Right-click, Show in Finder and Copy Path, as a file link in the
    /// transcript has (Behrooz looked there for it, 9 Nov).
    func testAPictureOnThisMacShowsInTheFinder() throws {
        let image = NSImage(size: NSSize(width: 20, height: 20), flipped: false) { rect in
            NSColor.systemPink.setFill(); rect.fill(); return true
        }
        let png = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation))?.representation(using: .png, properties: [:]))
        let view = try shown("shot.png", png)
        var revealed: [URL] = []
        view.reveal = { revealed.append($0) }
        let click = try XCTUnwrap(NSEvent.mouseEvent(with: .rightMouseDown, location: middle, modifierFlags: [], timestamp: 0,
                                                     windowNumber: view.window!.windowNumber, context: nil,
                                                     eventNumber: 0, clickCount: 1, pressure: 1))
        let menu = try XCTUnwrap(view.menu(for: click))
        XCTAssertEqual(menu.items.map(\.title), ["Copy Image", "", "Show in Finder", "Copy Path"])
        let finder = try XCTUnwrap(menu.items.first { $0.title == "Show in Finder" })
        menu.performActionForItem(at: menu.index(of: finder))
        XCTAssertEqual(revealed.map(\.lastPathComponent), ["shot.png"])

        let board = NSPasteboard(name: NSPasteboard.Name(UUID().uuidString))
        addTeardownBlock { board.releaseGlobally() }
        XCTAssertTrue(view.copyPath(to: board))
        XCTAssertEqual(board.string(forType: .string), revealed.first?.path)
    }

    func testOtherFilesHaveNoCopyImage() throws {
        let view = try shown("notes.pdf", Data("%PDF-1.4\n".utf8))
        XCTAssertFalse(view.copyImage(to: NSPasteboard(name: NSPasteboard.Name(UUID().uuidString))))
    }
}

/// Counts the presses that reach it, standing in for the pane around the preview.
private final class PressRecorder: NSView {
    var presses = 0
    override func mouseDown(with event: NSEvent) { presses += 1 }
}
