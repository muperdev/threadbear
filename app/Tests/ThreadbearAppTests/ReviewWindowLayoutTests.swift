import AppKit
import XCTest
@testable import Threadbear
import ThreadbearCore

/// However much a review holds, everything stays in its own column. A
/// seven-shot option once stretched the whole right-hand column past its
/// edges, over the stage and off the window (Mike, 9 Nov), and only a look
/// at a review of that shape would have shown it, so these go through the
/// real window at the sizes it can be, with reviews of awkward shapes.
///
/// `REVIEW_LAYOUT_SHOTS=<folder>` also writes a picture of every one, with
/// real images in, to look over by eye.
@MainActor
extension ReviewWindowTests {
    private static let sizes = [
        "smallest": NSSize(width: 780, height: 650),
        "laptop": NSSize(width: 1361, height: 850),
        "biggest": ReviewWindowController.maxSize,
    ]

    func testNothingSpillsOutOfItsColumnWhateverTheReviewHolds() throws {
        for (shape, review) in try shapes() {
            for (sizeName, size) in Self.sizes {
                let (session, host, window) = make(review, size: size)
                defer { window.close() }
                let states: [(String, () -> Void)] = review.isPending
                    ? [("first", { session.enlarge(0, shot: 0) }),
                       ("last", { session.enlarge(review.options.count - 1, shot: max(0, review.options.last!.allShots.count - 1)) }),
                       ("all", { session.showAll() })]
                    : [("all", { session.showAll() })]
                for (state, go) in states {
                    go()
                    pump()
                    let name = "\(shape) \(sizeName) \(state)"
                    assertEverythingInItsColumn(host, name)
                    if review.note != nil { try assertRailMarginIsEmpty(host, name) }
                    try picture(host, named: name)
                }
            }
        }
    }

    /// The shape Mike hit: the second row of thumbnails has to work.
    func testEveryThumbnailInTheRailCanBeClickedEvenPastTheFirstRow() throws {
        let review = try XCTUnwrap(shapes()["flow, seven shots"])
        let (session, host, window) = make(review, size: NSSize(width: 1361, height: 850))
        defer { window.close() }
        for shot in 0..<7 {
            session.enlarge(0, shot: shot == 0 ? 6 : 0)
            pump()
            let tip = "flow. Welcome box, then a guide alongside, Step \(shot + 1)"
            // The rail's copy of the shot: the filmstrip's has the same tip.
            let thumb = try XCTUnwrap(anchors(in: host).first { $0.text == tip && inRail($0, host) }, "no thumbnail for shot \(shot)")
            press(thumb, window)
            pump()
            XCTAssertEqual(session.shot, shot)
        }
    }

    /// Stepping along a strip too long to show keeps the one you're on in view.
    func testTheFilmstripFollowsTheShotYoureOn() throws {
        let review = try XCTUnwrap(shapes()["thirty shots"])
        let (session, host, window) = make(review, size: NSSize(width: 780, height: 650))
        defer { window.close() }
        for shot in [0, 15, 29, 3] {
            session.enlarge(0, shot: shot)
            pump()
            let tip = "A. Thirty, Step \(shot + 1)"
            let thumb = try XCTUnwrap(anchors(in: host).first { $0.text == tip && !inRail($0, host) && !isMark($0) }, "no thumbnail for shot \(shot)")
            XCTAssertEqual(shown(thumb, host).width, thumb.bounds.width, accuracy: 1, "shot \(shot) is cut off or out of view")
        }
    }

    /// Narrowing the window mustn't lose the one you're on.
    func testResizingKeepsTheCurrentThumbnailAndDotInView() throws {
        let review = try XCTUnwrap(shapes()["thirty shots"])
        let (session, host, window) = make(review, size: ReviewWindowController.maxSize)
        defer { window.close() }
        session.enlarge(0, shot: 25)
        pump()
        window.setContentSize(NSSize(width: 780, height: 650))
        pump()
        let tip = "A. Thirty, Step 26"
        let thumb = try XCTUnwrap(anchors(in: host).first { $0.text == tip && !inRail($0, host) && !isMark($0) })
        XCTAssertEqual(shown(thumb, host).width, thumb.bounds.width, accuracy: 1, "the thumbnail fell out of view")
        let dot = try XCTUnwrap(anchors(in: host).first { $0.text == tip && !inRail($0, host) && isMark($0) })
        XCTAssertEqual(shown(dot, host).width, dot.bounds.width, accuracy: 1, "the dot fell out of view")
    }

    /// An option with more dots than the row has room for: the one you're on
    /// is what has to show, not the middle of the option.
    func testTheCurrentDotStaysInViewInsideAWideOption() throws {
        let review = try XCTUnwrap(shapes()["thirty shots"])
        let (session, host, window) = make(review, size: NSSize(width: 780, height: 650))
        defer { window.close() }
        session.enlarge(1, shot: 0)
        pump()
        for shot in [0, 29, 12] {
            session.enlarge(0, shot: shot)
            pump()
            let tip = "A. Thirty, Step \(shot + 1)"
            let dot = try XCTUnwrap(anchors(in: host).first { $0.text == tip && !inRail($0, host) && isMark($0) }, "no dot for shot \(shot)")
            XCTAssertEqual(shown(dot, host).width, dot.bounds.width, accuracy: 1, "dot \(shot) is out of view")
        }
    }

    /// A reply longer than the window scrolls in its own box: Done stays on
    /// the window and the options above still have room.
    func testALongSentReplyLeavesRoomForEverythingElse() throws {
        let review = try XCTUnwrap(shapes()["sent, long reply"])
        let (_, host, window) = make(review, size: NSSize(width: 780, height: 650))
        defer { window.close() }
        let done = try XCTUnwrap(anchor("Close it", in: host))
        assertOnWindowTopToBottom(done.convert(done.bounds, to: host), host, "Done", line: #line)
        let row = try XCTUnwrap(anchor("Show B", in: host))
        let options = try XCTUnwrap(row.enclosingScrollView)
        XCTAssertGreaterThan(options.frame.height, 200, "the reply squeezed the options out")
    }

    /// With dozens of options the way to the grid and the arrows stay put.
    func testTheArrowsAndGridStayReachableWithAVeryLongRowOfMarks() throws {
        let review = try XCTUnwrap(shapes()["two dozen options"])
        let (session, host, window) = make(review, size: NSSize(width: 780, height: 650))
        defer { window.close() }
        click("Next (\u{2192})", in: host, window)
        XCTAssertEqual(session.shot, 1)
        click("See them all and choose (Esc)", in: host, window)
        XCTAssertEqual(session.mode, .all)
    }

    func testTheBottomRowSharesItsRoomByWhatEachWants() {
        let fits = ReviewStepperLayout.share(room: 600, strip: 200, marks: 300)
        XCTAssertEqual(fits.strip, 200); XCTAssertEqual(fits.marks, 300)
        let shortMarks = ReviewStepperLayout.share(room: 600, strip: 700, marks: 100)
        XCTAssertEqual(shortMarks.strip, 500, "short marks don't scroll to make the strip longer")
        XCTAssertEqual(shortMarks.marks, 100)
        let shortStrip = ReviewStepperLayout.share(room: 300, strip: 100, marks: 500)
        XCTAssertEqual(shortStrip.strip, 100); XCTAssertEqual(shortStrip.marks, 200)
        let both = ReviewStepperLayout.share(room: 300, strip: 900, marks: 900)
        XCTAssertEqual(both.strip, 150); XCTAssertEqual(both.marks, 150)
        let none = ReviewStepperLayout.share(room: -40, strip: 900, marks: 900)
        XCTAssertEqual(none.strip, 0); XCTAssertEqual(none.marks, 0)
    }

    // MARK: Checking

    private func assertEverythingInItsColumn(_ host: NSView, _ name: String, line: UInt = #line) {
        let width = host.bounds.width
        let divider = width - ReviewWindow.railWidth
        for anchor in anchors(in: host) {
            let tip = "\"\(anchor.text)\" in \(name)"
            if inRail(anchor, host) {
                // The raw frame, as nothing in the rail scrolls sideways.
                let frame = anchor.convert(anchor.bounds, to: host)
                XCTAssertGreaterThanOrEqual(frame.minX, divider - 1, "\(tip) is out of the left of the rail", line: line)
                XCTAssertLessThanOrEqual(frame.maxX, width + 1, "\(tip) is off the window", line: line)
                // The footer's: Send, None, Done. The rest scroll up and down.
                if anchor.enclosingScrollView == nil { assertOnWindowTopToBottom(frame, host, tip, line: line) }
            } else {
                // What can be seen of it: the strip and the marks scroll.
                let frame = shown(anchor, host)
                if frame.isEmpty { continue }
                if anchor.enclosingScrollView == nil { assertOnWindowTopToBottom(frame, host, tip, line: line) }
                XCTAssertGreaterThanOrEqual(frame.minX, -1, "\(tip) is off the window", line: line)
                XCTAssertLessThanOrEqual(frame.maxX, divider + 1, "\(tip) runs into the rail", line: line)
            }
        }
        // These are how you get anywhere, so they're never scrolled away.
        for tip in ["See them all and choose (Esc)"] {
            guard let anchor = anchor(tip, in: host) else { XCTFail("no \(tip) in \(name)", line: line); continue }
            XCTAssertEqual(shown(anchor, host).width, anchor.bounds.width, accuracy: 1, "\(tip) is cut off in \(name)", line: line)
        }
    }

    private func assertOnWindowTopToBottom(_ frame: NSRect, _ host: NSView, _ tip: String, line: UInt) {
        XCTAssertGreaterThanOrEqual(frame.minY, -1, "\(tip) is off the window, up or down", line: line)
        XCTAssertLessThanOrEqual(frame.maxY, host.bounds.height + 1, "\(tip) is off the window, up or down", line: line)
    }

    /// Words have no views to measure, so this looks at the picture. The
    /// rail's left margin holds nothing, so each line of pixels across it is
    /// one colour unless something has spilled into it, as the note did.
    /// (Clear of the 3-point bar on the row on show.)
    private func assertRailMarginIsEmpty(_ host: NSView, _ name: String, line: UInt = #line) throws {
        let rep = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: rep)
        let scale = CGFloat(rep.pixelsWide) / host.bounds.width
        let left = Int((host.bounds.width - ReviewWindow.railWidth + 4) * scale)
        let right = Int((host.bounds.width - ReviewWindow.railWidth + 13) * scale)
        for y in 0..<rep.pixelsHigh {
            let first = try XCTUnwrap(rep.colorAt(x: left, y: y))
            for x in left..<right {
                let colour = try XCTUnwrap(rep.colorAt(x: x, y: y))
                if abs(colour.redComponent - first.redComponent) > 0.02 || abs(colour.greenComponent - first.greenComponent) > 0.02 || abs(colour.blueComponent - first.blueComponent) > 0.02 {
                    return XCTFail("something is drawn in the rail's left margin in \(name), \(y) pixels down", line: line)
                }
            }
        }
    }

    private func anchors(in host: NSView) -> [TipAnchorView] {
        func walk(_ view: NSView) -> [TipAnchorView] {
            view.subviews.flatMap { ($0 as? TipAnchorView).map { [$0] } ?? walk($0) }
        }
        return walk(host)
    }

    /// The part of a view its scroll view (if any) leaves on show.
    private func shown(_ view: NSView, _ host: NSView) -> NSRect {
        let frame = view.convert(view.bounds, to: host)
        guard let scroll = view.enclosingScrollView else { return frame }
        return frame.intersection(scroll.convert(scroll.bounds, to: host))
    }

    private func inRail(_ view: NSView, _ host: NSView) -> Bool {
        guard let scroll = view.enclosingScrollView else {
            // The footer isn't scrolled.
            return view.convert(view.bounds, to: host).midX > host.bounds.width - ReviewWindow.railWidth
        }
        return scroll.convert(scroll.bounds, to: host).minX >= host.bounds.width - ReviewWindow.railWidth - 1
    }

    /// The dots under the stage share their tips with the thumbnails.
    private func isMark(_ view: NSView) -> Bool { view.bounds.width < 40 }

    private func press(_ view: NSView, _ window: NSWindow) {
        let point = view.convert(NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            window.sendEvent(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1,
                                                pressure: type == .leftMouseDown ? 1 : 0)!)
        }
    }

    private func picture(_ host: NSView, named name: String) throws {
        guard let folder = ProcessInfo.processInfo.environment["REVIEW_LAYOUT_SHOTS"],
              let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return }
        host.cacheDisplay(in: host.bounds, to: rep)
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        try rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: folder).appendingPathComponent(name.replacingOccurrences(of: ",", with: "") + ".png"))
    }

    // MARK: The reviews

    /// Real files only when pictures are wanted: without them each test's
    /// geometry is just the sizes given here.
    private func image(_ tint: Int) throws -> ImageRef {
        let folder = (ProcessInfo.processInfo.environment["REVIEW_LAYOUT_SHOTS"] ?? "/nonexistent/ReviewLayoutTests") + "/images"
        let path = "\(folder)/\(tint).png"
        if ProcessInfo.processInfo.environment["REVIEW_LAYOUT_SHOTS"] != nil, !FileManager.default.fileExists(atPath: path) {
            try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
            let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 440, pixelsHigh: 300, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                       colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
            NSColor(hue: CGFloat(tint % 12) / 12, saturation: 0.45, brightness: 0.75, alpha: 1).setFill()
            NSRect(x: 0, y: 0, width: 440, height: 300).fill()
            ("\(tint)" as NSString).draw(at: NSPoint(x: 30, y: 90), withAttributes: [.font: NSFont.boldSystemFont(ofSize: 110), .foregroundColor: NSColor.white])
            NSGraphicsContext.restoreGraphicsState()
            try rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
        }
        return ImageRef(path: path, mediaType: "image/png", width: 440, height: 300)
    }

    private func shots(_ count: Int, from tint: Int = 0) throws -> [ReviewShot] {
        try (0..<count).map { ReviewShot(title: "Step \($0 + 1)", image: try image(tint + $0)) }
    }

    private func shapes() throws -> [String: DesignReview] {
        let long = "Clickable prototype: use the Live link to try it. The agent's words and matches are written by hand for now, so any idea you type gets the same D&D answer. The topic filters are left off here to keep it simple."
        let lump = String(repeating: "https://example.com/an-address-with-no-spaces-at-all/", count: 8)

        var flow = ReviewOption(id: "flow", title: "Welcome box, then a guide alongside", image: try image(0), shots: try shots(7))
        flow.url = "http://localhost:5190/"
        var seven = DesignReview(id: "review", title: "The welcome box and agent guide", options: [flow])
        seven.note = long

        let thirty = DesignReview(id: "review", title: "Thirty", options: [
            ReviewOption(id: "A", title: "Thirty", image: try image(0), shots: try shots(30)),
            ReviewOption(id: "B", title: "Two", image: try image(5), shots: try shots(2, from: 5)),
        ])

        var crowd = try (1...24).map { ReviewOption(id: "O\($0)", title: "Option \($0)", note: "Why \($0)", image: try image($0), shots: try shots(3, from: $0)) }
        crowd.append(ReviewOption(id: "Today", title: "Today", reference: true, image: try image(0), shots: try shots(3)))
        let dozens = DesignReview(id: "review", title: "Crowd", options: crowd)

        var wordyA = ReviewOption(id: "a-very-long-option-identifier-for-this-design", title: String(repeating: "A long heading that goes on and on ", count: 6), note: lump, image: try image(1), shots: try shots(3, from: 1))
        wordyA.url = "http://localhost:5190/" + lump
        let wordyToday = ReviewOption(id: "Today", title: String(repeating: "What we have today, described ", count: 5), reference: true, image: try image(2), shots: try shots(3, from: 1))
        var wordy = DesignReview(id: "review", title: "Wordy", options: [wordyA, wordyToday])
        wordy.note = lump

        var sent = DesignReview(id: "review", title: "Sent", options: [flow, ReviewOption(id: "B", title: "Other", image: try image(4))])
        sent.status = "answered"
        sent.response = ReviewDraft(pickedIds: ["flow"], notes: [:], reply: String(repeating: "A reply that went on for a very long time indeed. ", count: 120))

        return ["flow, seven shots": seven, "thirty shots": thirty, "two dozen options": dozens, "wordy": wordy, "sent, long reply": sent]
    }
}
