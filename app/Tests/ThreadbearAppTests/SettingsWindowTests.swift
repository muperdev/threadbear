import AppKit
import SwiftUI
import XCTest
@testable import Threadbear

/// The Settings window's sizes, as SwiftUI hands them to the window.
@MainActor
final class SettingsWindowTests: XCTestCase {
    /// SwiftUI made the Settings window's largest width infinite, from its
    /// content's unbounded one. A Settings window is never meant to resize,
    /// but this one does (SettingsWindow.makeResizable), and dragging its
    /// edge on macOS 27 worked out a frame of {{-inf, 42}, {inf, 1295}},
    /// which AppKit threw on, crashing the app (THREADBEAR-1S, 10 Nov).
    /// It resizes taller too now, so neither side may be infinite.
    func testTheWindowsLargestSizeIsOneAppKitCanFrame() {
        let host = NSHostingView(rootView: Color.clear.settingsSizes())
        host.sizingOptions = [.minSize, .maxSize]
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 880, height: 620), styleMask: [.titled, .resizable], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))

        XCTAssertEqual(window.contentMinSize.width, SettingsLayout.minWidth)
        XCTAssertEqual(window.contentMinSize.height, SettingsLayout.minHeight)
        // AppKit itself never frames a window bigger than 100,000 points.
        for side in [window.contentMaxSize.width, window.contentMaxSize.height, window.maxSize.width, window.maxSize.height] {
            XCTAssertLessThanOrEqual(side, 100_000)
            XCTAssertGreaterThan(side, 6_000, "still drags as big as any screen")
        }
    }
}
