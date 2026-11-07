import AppKit
import Foundation
import Security
import ThreadbearCore

/// This Mac's sign-in to Collabute, for dictation (Dictation): Collabute's
/// OAuth device flow asking for `voice:transcribe` only, its tokens kept in
/// the login Keychain, refreshed as they run out. Nothing else of
/// Collabute's is reached with it.
@MainActor
@Observable
final class CollabuteAccount {
    static let shared = CollabuteAccount()

    /// Settings > Dictation's server; empty is Collabute's own.
    static let serverKey = "collabuteServer"
    static var server: String { CollabuteAuth.server(UserDefaults.standard.string(forKey: serverKey)) }

    enum State: Equatable {
        case signedOut
        /// Waiting for him to approve Threadbear in the browser.
        case signingIn(code: String, url: URL)
        case signedIn
    }

    private(set) var state: State = .signedOut
    /// Why the last sign-in didn't work.
    private(set) var problem: String?
    @ObservationIgnored private var tokens: CollabuteAuth.Tokens?
    @ObservationIgnored private var signInTask: Task<Void, Never>?
    @ObservationIgnored private var refreshing: Task<CollabuteAuth.Tokens, Error>?

    private init() {
        tokens = Keychain.read()
        if let tokens, tokens.server == Self.server { state = .signedIn }
    }

    var isSignedIn: Bool { state == .signedIn }

    /// Starts the device flow: opens Collabute's approval page in the
    /// browser and waits for him to approve it there.
    func signIn() {
        if case .signingIn(_, let url) = state {
            NSWorkspace.shared.open(url)
            return
        }
        problem = nil
        let server = Self.server
        signInTask?.cancel()
        signInTask = Task {
            do {
                let start = try await Self.startDevice(server: server)
                state = .signingIn(code: start.userCode, url: start.verificationURL)
                NSWorkspace.shared.open(start.verificationURL)
                let tokens = try await Self.waitForApproval(start, server: server)
                guard !Task.isCancelled else { return }
                save(tokens)
                state = .signedIn
            } catch is CancellationError {
            } catch {
                if !Task.isCancelled {
                    problem = error.localizedDescription
                    state = .signedOut
                }
            }
        }
    }

    func cancelSignIn() {
        signInTask?.cancel()
        signInTask = nil
        if case .signingIn = state { state = .signedOut }
    }

    func signOut() {
        cancelSignIn()
        tokens = nil
        Keychain.delete()
        state = .signedOut
        problem = nil
    }

    /// Settings > Dictation's server changed: a sign-in is for one server.
    func serverChanged() {
        if let tokens, tokens.server != Self.server { signOut() }
        else if tokens != nil { state = .signedIn }
    }

    /// A voice token for one dictation, refreshing the sign-in first if it
    /// has run out.
    func voiceToken() async throws -> CollabuteAuth.VoiceToken {
        var access = try await accessToken()
        var (status, body) = try await Self.post("\(Self.server)/api/mcp/voice-token", json: [:], bearer: access)
        if status == 401, tokens?.refreshToken != nil {
            // Revoked or expired early: one refresh, then give up.
            access = try await accessToken(forceRefresh: true)
            (status, body) = try await Self.post("\(Self.server)/api/mcp/voice-token", json: [:], bearer: access)
        }
        if status == 401 { signOut() }
        guard (200..<300).contains(status), let token = CollabuteAuth.voiceToken(body) else {
            throw DictationError(CollabuteAuth.voiceTokenProblem(status: status, body: body))
        }
        return token
    }

    private func accessToken(forceRefresh: Bool = false) async throws -> String {
        guard let tokens, tokens.server == Self.server else {
            throw DictationError("Sign in to Collabute to dictate.")
        }
        if tokens.fresh(), !forceRefresh { return tokens.accessToken }
        guard let refreshToken = tokens.refreshToken else {
            signOut()
            throw DictationError("Collabute signed Threadbear out. Sign in again in Settings > Dictation.")
        }
        // One refresh at a time: a refresh token works once.
        let task = refreshing ?? Task { try await Self.refresh(refreshToken, server: tokens.server) }
        refreshing = task
        defer { refreshing = nil }
        do {
            let fresh = try await task.value
            save(fresh)
            return fresh.accessToken
        } catch {
            signOut()
            throw DictationError("Collabute signed Threadbear out. Sign in again in Settings > Dictation.")
        }
    }

    private func save(_ tokens: CollabuteAuth.Tokens) {
        self.tokens = tokens
        Keychain.write(tokens)
    }

    // MARK: Requests

    private static func startDevice(server: String) async throws -> CollabuteAuth.DeviceStart {
        let (status, body) = try await post("\(server)/api/mcp/oauth/device/start", json: [
            "client_name": CollabuteAuth.clientName,
            "scope": CollabuteAuth.scope,
        ])
        guard (200..<300).contains(status), let start = CollabuteAuth.deviceStart(body) else {
            throw DictationError(status == 400 && String(decoding: body, as: UTF8.self).contains(CollabuteAuth.scope)
                ? "This Collabute server doesn't have dictation for other apps yet."
                : "Couldn't start signing in to Collabute (\(status)).")
        }
        return start
    }

    private static func waitForApproval(_ start: CollabuteAuth.DeviceStart, server: String) async throws -> CollabuteAuth.Tokens {
        var interval = start.interval
        let deadline = Date().addingTimeInterval(start.expiresIn)
        while Date() < deadline {
            try await Task.sleep(for: .seconds(interval))
            let (status, body) = try await post("\(server)/api/mcp/oauth/token", json: [
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                "device_code": start.deviceCode,
            ])
            switch CollabuteAuth.tokenAnswer(status: status, body: body, server: server) {
            case .tokens(let tokens): return tokens
            case .pending: continue
            case .slowDown: interval += 5
            case .failed(let message): throw DictationError(message)
            }
        }
        throw DictationError("The sign-in ran out before it was approved. Sign in again.")
    }

    private static func refresh(_ refreshToken: String, server: String) async throws -> CollabuteAuth.Tokens {
        let (status, body) = try await post("\(server)/api/mcp/oauth/token", json: [
            "grant_type": "refresh_token",
            "refresh_token": refreshToken,
        ])
        guard case .tokens(let tokens) = CollabuteAuth.tokenAnswer(status: status, body: body, server: server) else {
            throw DictationError("Refresh refused (\(status))")
        }
        return tokens
    }

    private static func post(_ link: String, json: [String: String], bearer: String? = nil) async throws -> (Int, Data) {
        guard let url = URL(string: link) else { throw DictationError("Settings > Dictation's server isn't a web address.") }
        var request = URLRequest(url: url, timeoutInterval: 20)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let bearer { request.setValue("Bearer \(bearer)", forHTTPHeaderField: "Authorization") }
        request.httpBody = try JSONSerialization.data(withJSONObject: json)
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            return ((response as? HTTPURLResponse)?.statusCode ?? 0, data)
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            throw DictationError("Couldn't reach Collabute: \(error.localizedDescription)")
        }
    }

    // MARK: Keychain

    /// One login Keychain item holding the tokens as JSON.
    private enum Keychain {
        static let service = "Threadbear Collabute dictation"
        static let account = "collabute"

        static var query: [String: Any] {
            [kSecClass as String: kSecClassGenericPassword,
             kSecAttrService as String: service,
             kSecAttrAccount as String: account]
        }

        static func read() -> CollabuteAuth.Tokens? {
            var item: CFTypeRef?
            var search = query
            search[kSecReturnData as String] = true
            search[kSecMatchLimit as String] = kSecMatchLimitOne
            guard SecItemCopyMatching(search as CFDictionary, &item) == errSecSuccess, let data = item as? Data else { return nil }
            return try? JSONDecoder().decode(CollabuteAuth.Tokens.self, from: data)
        }

        static func write(_ tokens: CollabuteAuth.Tokens) {
            guard let data = try? JSONEncoder().encode(tokens) else { return }
            let update = [kSecValueData as String: data]
            if SecItemUpdate(query as CFDictionary, update as CFDictionary) == errSecItemNotFound {
                var add = query
                add[kSecValueData as String] = data
                add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
                SecItemAdd(add as CFDictionary, nil)
            }
        }

        static func delete() {
            SecItemDelete(query as CFDictionary)
        }
    }
}

struct DictationError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
