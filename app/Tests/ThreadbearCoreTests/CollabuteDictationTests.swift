import Foundation
import XCTest
@testable import ThreadbearCore

final class CollabuteDictationTests: XCTestCase {
    // MARK: Stream parts and the live transcript

    func testParsesTheStreamParts() {
        XCTAssertEqual(DictationStreamPart.parse(#"{"type":"transcript-delta","id":"s1","delta":"Hello"}"#), .delta(id: "s1", text: "Hello"))
        XCTAssertEqual(DictationStreamPart.parse(#"{"type":"transcript-partial","text":"Hel"}"#), .partial(id: nil, text: "Hel"))
        XCTAssertEqual(DictationStreamPart.parse(#"{"type":"transcript-final","id":"s1","text":"Hello there."}"#), .final(id: "s1", text: "Hello there."))
        XCTAssertEqual(DictationStreamPart.parse(#"{"type":"finish","text":"Hello there.","segments":[]}"#), .finish(text: "Hello there."))
        XCTAssertEqual(DictationStreamPart.parse(#"{"type":"error","error":{"type":"rate_limit_exceeded","message":"Slow down"}}"#), .error(message: "Slow down"))
        XCTAssertEqual(DictationStreamPart.parse(#"{"type":"stream-start","warnings":[]}"#), .other)
        XCTAssertNil(DictationStreamPart.parse("not json"))
        XCTAssertNil(DictationStreamPart.parse(#"{"type":"transcript-delta"}"#))
    }

    /// A partial is the tail after the locked-in words, not the whole text:
    /// treated as the whole, the next delta repeated words.
    func testDeltaAfterPartialDoesNotRepeatWords() {
        var transcript = DictationTranscript()
        transcript.apply(.partial(id: "a", text: "Can you check"))
        XCTAssertEqual(transcript.text, "Can you check")
        transcript.apply(.delta(id: "a", text: "Can you "))
        XCTAssertEqual(transcript.text, "Can you check")
        transcript.apply(.partial(id: "a", text: "check the build"))
        XCTAssertEqual(transcript.text, "Can you check the build")
        transcript.apply(.final(id: "a", text: "Can you check the build?"))
        XCTAssertEqual(transcript.text, "Can you check the build?")
    }

    func testSegmentsJoinInOrder() {
        var transcript = DictationTranscript()
        transcript.apply(.final(id: "1", text: "First sentence."))
        transcript.apply(.partial(id: "2", text: "Second"))
        XCTAssertEqual(transcript.text, "First sentence. Second")
        XCTAssertFalse(transcript.apply(.other))
    }

    func testJoinKeepsWordsFromADroppedStream() {
        XCTAssertEqual(DictationTranscript.join("Before the drop", " and after "), "Before the drop and after")
        XCTAssertEqual(DictationTranscript.join("", "only"), "only")
    }

    // MARK: Frames

    func testTokenRidesInTheSubprotocols() {
        XCTAssertEqual(DictationFrames.protocols(token: "vcst_abc"), ["ai-gateway-transcription.v1", "ai-gateway-auth.vcst_abc"])
    }

    func testStartFrameNamesTheFormatAndLanguage() throws {
        let frame = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(DictationFrames.start(language: "en").utf8)) as? [String: Any])
        XCTAssertEqual(frame["type"] as? String, "transcription-stream.start")
        let format = try XCTUnwrap(frame["inputAudioFormat"] as? [String: Any])
        XCTAssertEqual(format["type"] as? String, "audio/pcm")
        XCTAssertEqual(format["rate"] as? Int, 24_000)
        XCTAssertEqual(((frame["providerOptions"] as? [String: Any])?["azure"] as? [String: Any])?["language"] as? String, "en")
        let bare = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(DictationFrames.start(language: nil).utf8)) as? [String: Any])
        XCTAssertNil(bare["providerOptions"])
    }

    func testPCM16IsLittleEndianAndClamped() {
        let samples: [Float] = [0, 1, -1, 2]
        let data = samples.withUnsafeBufferPointer { DictationFrames.pcm16($0) }
        XCTAssertEqual(data.count, 8)
        let values = data.withUnsafeBytes { Array($0.bindMemory(to: Int16.self)) }.map { Int16(littleEndian: $0) }
        XCTAssertEqual(values, [0, Int16.max, -Int16.max, Int16.max])
    }

    // MARK: Sign-in and voice token

    func testDeviceStart() throws {
        let body = Data(#"{"device_code":"dc","user_code":"ABCD-EFGH","verification_uri":"https://api.collabute.ai/api/mcp/oauth/device/approve","verification_uri_complete":"https://api.collabute.ai/api/mcp/oauth/device/approve?user_code=ABCD-EFGH","expires_in":600,"interval":5}"#.utf8)
        let start = try XCTUnwrap(CollabuteAuth.deviceStart(body))
        XCTAssertEqual(start.userCode, "ABCD-EFGH")
        XCTAssertEqual(start.verificationURL.absoluteString, "https://api.collabute.ai/api/mcp/oauth/device/approve?user_code=ABCD-EFGH")
        XCTAssertEqual(start.interval, 5)
    }

    func testTokenAnswers() {
        let now = Date(timeIntervalSince1970: 1_000)
        let ok = Data(#"{"access_token":"a","refresh_token":"r","expires_in":900,"token_type":"Bearer","scope":"voice:transcribe"}"#.utf8)
        XCTAssertEqual(CollabuteAuth.tokenAnswer(status: 200, body: ok, server: "https://s", now: now),
                       .tokens(.init(accessToken: "a", refreshToken: "r", expiresAt: now.addingTimeInterval(900), server: "https://s")))
        // The gateway's error filter can wrap the OAuth error.
        let pending = Data(#"{"statusCode":400,"message":{"error":"authorization_pending","error_description":"User has not approved this device code yet"}}"#.utf8)
        XCTAssertEqual(CollabuteAuth.tokenAnswer(status: 400, body: pending, server: "https://s"), .pending)
        XCTAssertEqual(CollabuteAuth.tokenAnswer(status: 400, body: Data(#"{"error":"slow_down"}"#.utf8), server: "https://s"), .slowDown)
        guard case .failed = CollabuteAuth.tokenAnswer(status: 400, body: Data(#"{"error":"invalid_grant"}"#.utf8), server: "https://s") else {
            return XCTFail("an expired code is a failure")
        }
    }

    func testTokensAreFreshUntilAMinuteBeforeTheyRunOut() {
        let now = Date(timeIntervalSince1970: 1_000)
        let tokens = CollabuteAuth.Tokens(accessToken: "a", refreshToken: nil, expiresAt: now.addingTimeInterval(61), server: "s")
        XCTAssertTrue(tokens.fresh(at: now))
        XCTAssertFalse(tokens.fresh(at: now.addingTimeInterval(2)))
    }

    func testVoiceTokenOnlyOverASecureSocket() {
        let good = Data(#"{"token":"vcst_x","url":"wss://ai-gateway.vercel.sh/v4/ai/transcription-model?ai-model-id=m","model":"m"}"#.utf8)
        XCTAssertEqual(CollabuteAuth.voiceToken(good)?.token, "vcst_x")
        XCTAssertNil(CollabuteAuth.voiceToken(Data(#"{"token":"vcst_x","url":"ws://example.test"}"#.utf8)))
    }

    func testVoiceTokenProblemsInPlainWords() {
        XCTAssertTrue(CollabuteAuth.voiceTokenProblem(status: 403, body: Data(#"{"code":"MISSING_SCOPE"}"#.utf8)).contains("Sign in again"))
        XCTAssertTrue(CollabuteAuth.voiceTokenProblem(status: 403, body: Data()).contains("workspace"))
        XCTAssertTrue(CollabuteAuth.voiceTokenProblem(status: 429, body: Data()).contains("Too much"))
    }

    func testServerDefaultsAndTrims() {
        XCTAssertEqual(CollabuteAuth.server(nil), "https://api.collabute.ai")
        XCTAssertEqual(CollabuteAuth.server("  "), "https://api.collabute.ai")
        XCTAssertEqual(CollabuteAuth.server("not a url"), "https://api.collabute.ai")
        XCTAssertEqual(CollabuteAuth.server("https://api-dev.collabute.ai/"), "https://api-dev.collabute.ai")
    }
}
