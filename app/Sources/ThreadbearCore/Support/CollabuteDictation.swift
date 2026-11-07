import Foundation

// Dictation through Collabute's speech-to-text (the composer's mic button).
// The pieces with no audio, sockets or UI, so they can be tested alone:
// the stream's frames and parts, the live transcript built from them, and
// what the sign-in's token endpoint answered.
//
// Collabute's gateway mints a short-lived AI Gateway transcription token
// (POST /api/mcp/voice-token, for a sign-in granted `voice:transcribe`);
// the app streams 24 kHz 16-bit mono PCM to the URL it gives, over a web
// socket that carries the token in its subprotocols, the same protocol
// the AI SDK's `experimental_streamTranscribe` speaks.

/// One part of AI Gateway's transcription stream.
public enum DictationStreamPart: Equatable, Sendable {
    /// Words the model has locked in for a segment; append-only.
    case delta(id: String?, text: String)
    /// The still-changing words after the locked-in ones.
    case partial(id: String?, text: String)
    /// The whole segment, settled.
    case final(id: String?, text: String)
    /// The stream is done; its whole text.
    case finish(text: String)
    case error(message: String)
    /// Something else (stream-start, metadata), or a part this doesn't know.
    case other

    public static func parse(_ text: String) -> DictationStreamPart? {
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = object["type"] as? String else { return nil }
        let id = object["id"] as? String
        switch type {
        case "transcript-delta":
            guard let delta = object["delta"] as? String else { return nil }
            return .delta(id: id, text: delta)
        case "transcript-partial":
            guard let text = object["text"] as? String else { return nil }
            return .partial(id: id, text: text)
        case "transcript-final":
            guard let text = object["text"] as? String else { return nil }
            return .final(id: id, text: text)
        case "finish":
            return .finish(text: object["text"] as? String ?? "")
        case "error":
            return .error(message: errorMessage(object["error"]))
        default:
            return .other
        }
    }

    private static func errorMessage(_ error: Any?) -> String {
        if let error = error as? [String: Any], let message = error["message"] as? String { return message }
        if let error = error as? String { return error }
        return "Collabute's speech-to-text stopped."
    }
}

/// The live transcript from a stream's parts, per segment (`id`): a
/// segment's final text once it has one, else its locked-in words and the
/// changing tail after them. Treating a partial as the whole text, then
/// adding the next delta to it, repeats words (Collabute's desktop learned
/// this: "...for tomorrow? Then create a Can you check").
public struct DictationTranscript: Equatable, Sendable {
    private struct Segment: Equatable, Sendable {
        var committed = ""
        var pending = ""
        var final: String?
    }

    private var order: [String] = []
    private var segments: [String: Segment] = [:]

    public init() {}

    /// Applies a part; true when the text changed.
    @discardableResult
    public mutating func apply(_ part: DictationStreamPart) -> Bool {
        let before = text
        switch part {
        case .delta(let id, let delta):
            update(id) {
                $0.committed += delta
                // The locked-in words came out of the pending tail: keep the
                // rest, so the words don't blink until the next partial.
                $0.pending = Self.dropPrefix($0.pending, delta)
            }
        case .partial(let id, let text):
            update(id) { $0.pending = text }
        case .final(let id, let text):
            update(id) {
                $0.final = text
                $0.pending = ""
            }
        case .finish, .error, .other:
            break
        }
        return text != before
    }

    public var text: String {
        let pieces = order.compactMap { key -> String? in
            guard let segment = segments[key] else { return nil }
            let value = (segment.final ?? segment.committed + segment.pending).trimmingCharacters(in: .whitespacesAndNewlines)
            return value.isEmpty ? nil : value
        }
        return pieces.joined(separator: " ").replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
    }

    private mutating func update(_ id: String?, _ change: (inout Segment) -> Void) {
        let key = id ?? ""
        if segments[key] == nil {
            segments[key] = Segment()
            order.append(key)
        }
        change(&segments[key]!)
    }

    static func dropPrefix(_ pending: String, _ delta: String) -> String {
        let head = delta.trimmingCharacters(in: .whitespacesAndNewlines)
        let rest = String(pending.drop(while: { $0.isWhitespace }))
        guard !head.isEmpty, rest.hasPrefix(head) else { return "" }
        return String(rest.dropFirst(head.count))
    }

    /// Words heard before and after a dropped stream, as one.
    public static func join(_ before: String, _ after: String) -> String {
        [before, after]
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .joined(separator: " ")
    }
}

/// What the app sends on the transcription socket.
public enum DictationFrames {
    public static let sampleRate = 24_000
    public static let subprotocol = "ai-gateway-transcription.v1"
    static let authPrefix = "ai-gateway-auth."

    /// The token goes in the subprotocols, as the AI SDK sends it from a
    /// browser, where a web socket can't carry an Authorization header.
    public static func protocols(token: String) -> [String] {
        [subprotocol, authPrefix + token]
    }

    /// The first frame: the audio's format, and the language to expect
    /// (a hint, not a lock: speech in another language still comes through).
    public static func start(language: String?) -> String {
        var frame: [String: Any] = [
            "type": "transcription-stream.start",
            "inputAudioFormat": ["type": "audio/pcm", "rate": sampleRate],
        ]
        if let language, !language.isEmpty {
            frame["providerOptions"] = ["azure": ["language": language]]
        }
        let data = (try? JSONSerialization.data(withJSONObject: frame, options: [.sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    /// After the last audio: the stream finishes with its final text.
    public static let audioDone = #"{"type":"transcription-stream.audio-done"}"#

    /// Float samples (-1...1) as 16-bit little-endian PCM.
    public static func pcm16(_ samples: UnsafeBufferPointer<Float>) -> Data {
        var data = Data(count: samples.count * 2)
        data.withUnsafeMutableBytes { raw in
            let out = raw.bindMemory(to: Int16.self)
            for (i, sample) in samples.enumerated() {
                let clamped = max(-1, min(1, sample))
                out[i] = Int16(clamped * Float(Int16.max)).littleEndian
            }
        }
        return data
    }
}

/// Collabute's sign-in (its MCP OAuth device flow) and voice token.
public enum CollabuteAuth {
    public static let defaultServer = "https://api.collabute.ai"
    public static let scope = "voice:transcribe"
    public static let clientName = "Threadbear"

    public struct Tokens: Codable, Equatable, Sendable {
        public var accessToken: String
        public var refreshToken: String?
        public var expiresAt: Date
        public var server: String

        public init(accessToken: String, refreshToken: String?, expiresAt: Date, server: String) {
            self.accessToken = accessToken
            self.refreshToken = refreshToken
            self.expiresAt = expiresAt
            self.server = server
        }

        /// A minute early, so a token never runs out on its way.
        public func fresh(at now: Date = Date()) -> Bool { expiresAt.timeIntervalSince(now) > 60 }
    }

    public struct DeviceStart: Equatable, Sendable {
        public var deviceCode: String
        public var userCode: String
        public var verificationURL: URL
        public var interval: TimeInterval
        public var expiresIn: TimeInterval
    }

    public static func deviceStart(_ data: Data) -> DeviceStart? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let deviceCode = object["device_code"] as? String,
              let userCode = object["user_code"] as? String,
              let link = (object["verification_uri_complete"] as? String) ?? (object["verification_uri"] as? String),
              let url = URL(string: link) else { return nil }
        return DeviceStart(
            deviceCode: deviceCode,
            userCode: userCode,
            verificationURL: url,
            interval: max(2, (object["interval"] as? NSNumber)?.doubleValue ?? 5),
            expiresIn: (object["expires_in"] as? NSNumber)?.doubleValue ?? 600
        )
    }

    public enum TokenAnswer: Equatable, Sendable {
        case tokens(Tokens)
        /// Not approved yet: ask again after the interval.
        case pending
        /// Asked too often: wait longer.
        case slowDown
        case failed(String)
    }

    /// What the token endpoint answered, for a device code or a refresh.
    /// The gateway's error filter may wrap the OAuth error, so the error
    /// code is looked for anywhere in the body.
    public static func tokenAnswer(status: Int, body: Data, server: String, now: Date = Date()) -> TokenAnswer {
        let object = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any]
        if (200..<300).contains(status), let object, let access = object["access_token"] as? String {
            let expiresIn = (object["expires_in"] as? NSNumber)?.doubleValue ?? 900
            return .tokens(Tokens(
                accessToken: access,
                refreshToken: object["refresh_token"] as? String,
                expiresAt: now.addingTimeInterval(expiresIn),
                server: server
            ))
        }
        let text = String(decoding: body, as: UTF8.self)
        if text.contains("authorization_pending") { return .pending }
        if text.contains("slow_down") { return .slowDown }
        if text.contains("access_denied") { return .failed("The sign-in was turned down.") }
        if text.contains("invalid_grant") { return .failed("The sign-in ran out or was already used. Sign in again.") }
        return .failed("Collabute's sign-in answered \(status).")
    }

    /// A voice token from POST /api/mcp/voice-token.
    public struct VoiceToken: Equatable, Sendable {
        public var token: String
        public var url: URL
        public var model: String
    }

    public static func voiceToken(_ data: Data) -> VoiceToken? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let token = object["token"] as? String,
              let link = object["url"] as? String, let url = URL(string: link),
              url.scheme == "wss" else { return nil }
        return VoiceToken(token: token, url: url, model: object["model"] as? String ?? "")
    }

    /// Plain words for a voice token refused by the gateway.
    public static func voiceTokenProblem(status: Int, body: Data) -> String {
        let text = String(decoding: body, as: UTF8.self)
        switch status {
        case 401: return "Collabute signed Threadbear out. Sign in again in Settings > Dictation."
        case 403 where text.contains("MISSING_SCOPE"):
            return "This sign-in can't use dictation. Sign in again in Settings > Dictation."
        case 403: return "Your Collabute workspace can't use AI dictation right now."
        case 429: return "Too much dictation for now. Try again in a few minutes."
        case 404: return "This Collabute server doesn't have dictation for other apps yet."
        default: return "Collabute's speech-to-text isn't available (\(status))."
        }
    }

    /// The server as typed in Settings, without a trailing slash; the
    /// default when it's empty or not a web address.
    public static func server(_ typed: String?) -> String {
        let value = (typed ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: value), let scheme = url.scheme, ["https", "http"].contains(scheme), url.host != nil else {
            return defaultServer
        }
        return value.hasSuffix("/") ? String(value.dropLast()) : value
    }
}
