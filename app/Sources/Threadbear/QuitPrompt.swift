import AppKit

/// The app menu's Quit Threadbear (⌘Q) asks first, with a box to stop asking
/// (Wayne, 8 Nov). Settings > General's "Ask before quitting" turns it back on.
///
/// Only that menu item asks. The quits the app makes itself (Restart to
/// Update, a handover, switching profile, scripted runs) call
/// `NSApp.terminate` directly and never see this, and nor does the Dock's Quit.
@MainActor
enum QuitPrompt {
    static let askKey = "askBeforeQuitting"

    struct Answer: Equatable {
        var quit: Bool
        var dontAskAgain: Bool
    }

    /// The alert. Tests swap it out: a real one ends their run (ModalGuard).
    static var ask: () -> Answer = { askWithAlert() }
    static var terminate: () -> Void = { NSApp.terminate(nil) }

    static func asks(_ defaults: UserDefaults = .standard) -> Bool {
        defaults.object(forKey: askKey) as? Bool ?? true
    }

    static func quit(defaults: UserDefaults = .standard) {
        if asks(defaults) {
            let answer = ask()
            guard answer.quit else { return }
            if answer.dontAskAgain { defaults.set(false, forKey: askKey) }
        }
        terminate()
    }

    private static func askWithAlert() -> Answer {
        let alert = NSAlert()
        alert.messageText = "Quit Threadbear?"
        alert.informativeText = "Your threads keep running on the runner while Threadbear is closed."
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        alert.showsSuppressionButton = true
        alert.suppressionButton?.title = "Don't ask again"
        NSApp.activate()
        let quit = alert.runModal() == .alertFirstButtonReturn
        return Answer(quit: quit, dontAskAgain: alert.suppressionButton?.state == .on)
    }
}
