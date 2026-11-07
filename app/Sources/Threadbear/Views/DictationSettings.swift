import SwiftUI
import ThreadbearCore

/// Settings > Dictation: the composer's mic (Dictation), and this Mac's
/// sign-in to Collabute for it (CollabuteAccount). Kept on this Mac, in
/// UserDefaults and the login Keychain, not the runner.
struct DictationSettings: View {
    @AppStorage(Dictation.enabledKey) private var enabled = true
    @AppStorage(CollabuteAccount.serverKey) private var server = ""
    @State private var account = CollabuteAccount.shared

    var body: some View {
        Form {
            SettingsSection("Dictation",
                            about: "Press the mic beside Send and talk: what you say is written into the message box as you say it, by Collabute's speech-to-text. Press it again to stop. Nothing is sent until you send it, and nothing you say is recorded or kept on this Mac. It needs a Collabute account whose workspace can use AI.",
                            ask: "find out why dictation isn't working.") {
                Toggle("Show the mic", isOn: $enabled)
                    .tip("Show the mic button beside Send")
                LabeledContent("Collabute") {
                    HStack(spacing: 8) {
                        switch account.state {
                        case .signedIn:
                            Text("Signed in").foregroundStyle(.secondary)
                            Button("Sign Out") { account.signOut() }
                                .tip("Forget this Mac's Collabute sign-in")
                        case .signingIn(let code, _):
                            Text("Approve code \(code) in your browser").foregroundStyle(.secondary).textSelection(.enabled)
                            Button("Open Page") { account.signIn() }
                                .tip("Open Collabute's approval page again")
                            Button("Cancel") { account.cancelSignIn() }
                                .tip("Stop waiting for the approval")
                        case .signedOut:
                            Button("Sign In…") { account.signIn() }
                                .tip("Sign in to Collabute in your browser, for dictation only")
                        }
                    }
                }
                if let problem = account.problem {
                    Text(problem).foregroundStyle(.red).font(.callout)
                }
            }

            SettingsSection("Server",
                            about: "Where Collabute is. Leave it empty for Collabute's own (\(CollabuteAuth.defaultServer)). Changing it signs you out.",
                            ask: "point dictation at another Collabute server.") {
                TextField("Server", text: $server, prompt: Text(CollabuteAuth.defaultServer))
                    .tip("The Collabute server dictation uses")
                    .onSubmit { account.serverChanged() }
                    .onChange(of: server) { account.serverChanged() }
            }
        }
        .formStyle(.grouped)
        .fixedSize(horizontal: false, vertical: true)
        .scrollIndicators(.never)
        .scrollDisabled(true)
    }
}
