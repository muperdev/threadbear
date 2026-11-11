import AppKit
import XCTest
@testable import Threadbear

/// Quit Threadbear asks first, and "Don't ask again" sticks until Settings
/// turns it back on.
@MainActor
final class QuitPromptTests: XCTestCase {
    private var defaults: UserDefaults!
    private var suite: String!
    private var asked = 0
    private var quits = 0
    private var answer = QuitPrompt.Answer(quit: true, dontAskAgain: false)
    private var savedAsk: (() -> QuitPrompt.Answer)!
    private var savedTerminate: (() -> Void)!

    override func setUp() {
        super.setUp()
        suite = "quit-prompt-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        savedAsk = QuitPrompt.ask
        savedTerminate = QuitPrompt.terminate
        QuitPrompt.ask = { [unowned self] in asked += 1; return answer }
        QuitPrompt.terminate = { [unowned self] in quits += 1 }
    }

    override func tearDown() {
        QuitPrompt.ask = savedAsk
        QuitPrompt.terminate = savedTerminate
        defaults.removePersistentDomain(forName: suite)
        super.tearDown()
    }

    func testItAsksByDefaultAndQuitsOnQuit() {
        XCTAssertTrue(QuitPrompt.asks(defaults))
        QuitPrompt.quit(defaults: defaults)
        XCTAssertEqual(asked, 1)
        XCTAssertEqual(quits, 1)
        XCTAssertTrue(QuitPrompt.asks(defaults), "still asks next time")
    }

    func testCancelKeepsTheAppOpen() {
        answer = QuitPrompt.Answer(quit: false, dontAskAgain: false)
        QuitPrompt.quit(defaults: defaults)
        XCTAssertEqual(asked, 1)
        XCTAssertEqual(quits, 0)
    }

    func testDontAskAgainQuitsAndStopsAsking() {
        answer = QuitPrompt.Answer(quit: true, dontAskAgain: true)
        QuitPrompt.quit(defaults: defaults)
        XCTAssertEqual(quits, 1)
        XCTAssertFalse(QuitPrompt.asks(defaults))

        QuitPrompt.quit(defaults: defaults)
        XCTAssertEqual(asked, 1, "no second question")
        XCTAssertEqual(quits, 2)
    }

    func testTickingTheBoxThenCancellingChangesNothing() {
        answer = QuitPrompt.Answer(quit: false, dontAskAgain: true)
        QuitPrompt.quit(defaults: defaults)
        XCTAssertEqual(quits, 0)
        XCTAssertTrue(QuitPrompt.asks(defaults))
    }

    func testTurnedOffInSettingsItQuitsStraightAway() {
        defaults.set(false, forKey: QuitPrompt.askKey)
        QuitPrompt.quit(defaults: defaults)
        XCTAssertEqual(asked, 0)
        XCTAssertEqual(quits, 1)
    }
}
