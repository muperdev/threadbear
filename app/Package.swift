// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Threadbear",
    platforms: [
        .macOS(.v26)
    ],
    products: [
        .executable(name: "Threadbear", targets: ["Threadbear"]),
        .library(name: "ThreadbearCore", targets: ["ThreadbearCore"]),
    ],
    dependencies: [
        .package(url: "https://github.com/swiftlang/swift-markdown.git", from: "0.9.0"),
        // highlight.js in JavaScriptCore, for code in the panel (CodeFileView).
        .package(url: "https://github.com/raspu/Highlightr.git", from: "2.3.0"),
        // The window's dockable panels, in this repo (dock/README.md).
        .package(path: "../dock"),
        // Crash reports from the installed app (CrashReporting.swift):
        // Sentry's static framework alone (SentryBinary/Package.swift says why).
        .package(path: "SentryBinary"),
        // The Terminal panel's emulator and pty (TerminalPanel.swift), MIT.
        // Its Metal shader is left out of SwiftPM builds (SwiftTerm's own
        // Package.swift excludes Shaders.metal), so `swift build` doesn't
        // need Xcode's separate Metal Toolchain: checked on a Mac without it.
        // Exactly 1.99.0: from 1.99.1 (SwiftTerm #774) it no longer compiles on Xcode 26.0.
        .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", exact: "1.99.0"),
    ],
    targets: [
        // Protocol models, the JSON-RPC client, the event reducer and other
        // logic with no AppKit or SwiftUI, so it can be tested on its own.
        .target(
            name: "ThreadbearCore",
            path: "Sources/ThreadbearCore"
        ),
        .executableTarget(
            name: "Threadbear",
            dependencies: [
                "ThreadbearCore",
                .product(name: "Markdown", package: "swift-markdown"),
                .product(name: "Highlightr", package: "Highlightr"),
                .product(name: "BearDock", package: "dock"),
                .product(name: "Sentry", package: "SentryBinary"),
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            path: "Sources/Threadbear",
            // Big pictures (the Librarian's page). Looked up through Art,
            // never Bundle.module, which stops the app if the bundle's missing.
            resources: [.copy("Resources/Art"), .copy("Resources/Rich")],
            // The AppKit side renders and measures text off the main thread
            // with non-Sendable TextKit objects confined to one thread each;
            // Swift 5 mode keeps that from drowning in Sendable errors.
            // ThreadbearCore stays in Swift 6 mode.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "ThreadbearCoreTests",
            dependencies: ["ThreadbearCore"],
            path: "Tests/ThreadbearCoreTests"
        ),
        // Ends a test run that reaches a real modal (ModalGuard.m says why).
        // Linked only into the tests below, never the app.
        .target(
            name: "ModalGuard",
            path: "Tests/ModalGuard"
        ),
        // AppKit-side checks (the composer, transcript text, renders,
        // AppModel's drafts) against the app target itself.
        .testTarget(
            name: "ThreadbearAppTests",
            dependencies: ["Threadbear", "ThreadbearCore", "ModalGuard"],
            path: "Tests/ThreadbearAppTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
