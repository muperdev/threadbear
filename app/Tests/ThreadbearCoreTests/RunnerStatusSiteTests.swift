import XCTest
@testable import ThreadbearCore

/// `runner.status`'s `sync`: where the website is and whether the backend can
/// send phone notifications. A self-hosted backend (docs/self-hosting.md)
/// names its site; a cloud one leaves it to the .convex.site twin.
final class RunnerStatusSiteTests: XCTestCase {
    private func status(_ sync: String) throws -> RunnerStatus {
        try JSONDecoder().decode(RunnerStatus.self, from: Data(#"{"version":"1","running":[],"sync":\#(sync)}"#.utf8))
    }

    func testACloudDeploymentsSiteIsItsConvexSiteTwin() throws {
        let s = try status(#"{"url":"https://quick-fox-77.convex.cloud"}"#)
        XCTAssertEqual(s.websiteURL?.absoluteString, "https://quick-fox-77.convex.site")
        XCTAssertFalse(s.pushOff)
    }

    func testASelfHostedBackendNamesItsSiteAndHasNoPush() throws {
        let s = try status(#"{"url":"https://mac.tail1.ts.net:3210","site":"https://mac.tail1.ts.net:3211","pushOff":true}"#)
        XCTAssertEqual(s.websiteURL?.absoluteString, "https://mac.tail1.ts.net:3211")
        XCTAssertTrue(s.pushOff)
    }

    func testANonHTTPSSiteIsIgnored() throws {
        let s = try status(#"{"url":"https://mac.tail1.ts.net:3210","site":"http://mac.tail1.ts.net:3211"}"#)
        XCTAssertNil(s.websiteURL)
    }

    func testABackendOnThisMacOnlyHasAnHTTPSiteOnLoopback() throws {
        let s = try status(#"{"url":"http://127.0.0.1:13210","site":"http://127.0.0.1:13211","pushOff":true}"#)
        XCTAssertEqual(s.websiteURL?.absoluteString, "http://127.0.0.1:13211")
        XCTAssertTrue(s.localOnly)
        XCTAssertFalse(try status(#"{"url":"https://mac.tail1.ts.net:3210"}"#).localOnly)
        XCTAssertFalse(try status(#"{"url":"https://quick-fox-77.convex.cloud"}"#).localOnly)
        // A name that only starts like loopback is someone else's host.
        XCTAssertFalse(try status(#"{"url":"http://127.0.0.1.evil.com:13210"}"#).localOnly)
        XCTAssertNil(try status(#"{"url":"http://127.0.0.1:13210","site":"http://127.0.0.1.evil.com:13211"}"#).websiteURL)
    }

    func testAnOlderRunnerWithNoSyncStillDecodes() throws {
        let s = try JSONDecoder().decode(RunnerStatus.self, from: Data(#"{"version":"1","running":[]}"#.utf8))
        XCTAssertNil(s.websiteURL)
        XCTAssertFalse(s.pushOff)
        XCTAssertFalse(s.localOnly)
    }
}
