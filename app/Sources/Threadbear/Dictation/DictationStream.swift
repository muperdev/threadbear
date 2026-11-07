import AVFoundation
import Foundation
import ThreadbearCore

/// One dictation: the microphone, turned into 24 kHz 16-bit mono PCM and
/// streamed to Collabute's speech-to-text (AI Gateway's transcription
/// socket, with the token CollabuteAccount got for it), and the transcript
/// coming back. Nothing is recorded or kept: the audio only goes to the
/// socket, and the words only to the composer.
@MainActor
final class DictationStream {
    /// The whole transcript so far, each time it changes.
    var onText: ((String) -> Void)?
    /// How loud the microphone is, 0...1, a few times a second.
    var onLevel: ((Float) -> Void)?

    private let engine = AVAudioEngine()
    private var socket: URLSessionWebSocketTask?
    private var transcript = DictationTranscript()
    /// What streams that dropped heard, carried on in a new one.
    private var heardBefore = ""
    private var finished: CheckedContinuation<String, Error>?
    private var finishText: String?
    private var stopping = false
    private var reconnects = 0
    private var language: String?
    /// A fresh voice token, for a stream that dropped.
    private let newToken: () async throws -> CollabuteAuth.VoiceToken

    init(newToken: @escaping () async throws -> CollabuteAuth.VoiceToken) {
        self.newToken = newToken
    }
    /// Audio from the audio thread, sent in ~100 ms frames.
    private let chunker = PCMChunker(bytes: DictationFrames.sampleRate / 10 * 2)

    var text: String { DictationTranscript.join(heardBefore, transcript.text) }

    /// Asks for the microphone, gets a token, opens the socket and starts
    /// listening.
    func start(token: CollabuteAuth.VoiceToken, language: String?) async throws {
        try await VoiceAudio.askForAccess()
        self.language = language
        open(token)
        try startEngine()
    }

    /// Stops listening, waits for the last words (8 seconds at most) and
    /// returns the whole transcript.
    func finish() async -> String {
        guard !stopping else { return text }
        stopping = true
        stopEngine()
        if let tail = chunker.flush() { send(tail) }
        guard let socket, socket.state == .running else { return text }
        socket.send(.string(DictationFrames.audioDone)) { _ in }
        let words = (try? await withThrowingTaskGroup(of: String.self) { group in
            group.addTask { @MainActor in try await self.waitForFinish() }
            group.addTask {
                try await Task.sleep(for: .seconds(8))
                throw CancellationError()
            }
            let first = try await group.next() ?? ""
            group.cancelAll()
            return first
        }) ?? text
        close()
        return words
    }

    /// Stops without waiting for anything.
    func cancel() {
        stopping = true
        stopEngine()
        close()
    }

    // MARK: Socket

    private func open(_ token: CollabuteAuth.VoiceToken) {
        transcript = DictationTranscript()
        let socket = URLSession.shared.webSocketTask(with: token.url, protocols: DictationFrames.protocols(token: token.token))
        self.socket = socket
        socket.resume()
        socket.send(.string(DictationFrames.start(language: language))) { [weak self] error in
            if let error { Task { @MainActor in self?.dropped(error) } }
        }
        receive(on: socket)
    }

    private func receive(on socket: URLSessionWebSocketTask) {
        socket.receive { [weak self] result in
            Task { @MainActor in
                guard let self, self.socket === socket else { return }
                switch result {
                case .success(.string(let text)):
                    self.handle(text)
                    if self.socket === socket { self.receive(on: socket) }
                case .success(.data(let data)):
                    self.handle(String(decoding: data, as: UTF8.self))
                    if self.socket === socket { self.receive(on: socket) }
                case .success:
                    self.receive(on: socket)
                case .failure(let error):
                    self.dropped(error)
                }
            }
        }
    }

    private func handle(_ frame: String) {
        guard let part = DictationStreamPart.parse(frame) else { return }
        switch part {
        case .finish(let words):
            let whole = DictationTranscript.join(heardBefore, words.isEmpty ? transcript.text : words)
            finishText = whole
            onText?(whole)
            finished?.resume(returning: whole)
            finished = nil
        case .error(let message):
            DictationLog.write("stream error: \(message)")
            fail(DictationError(message))
        default:
            if transcript.apply(part) { onText?(text) }
        }
    }

    /// The provider dropped the stream: keep what was heard and carry on in
    /// a new one (twice at most), unless he's finishing.
    private func dropped(_ error: Error) {
        guard socket != nil else { return }
        if stopping {
            finished?.resume(returning: text)
            finished = nil
            return
        }
        DictationLog.write("stream dropped: \(error.localizedDescription)")
        guard reconnects < 2 else {
            fail(DictationError("Collabute's speech-to-text stopped."))
            return
        }
        reconnects += 1
        heardBefore = text
        transcript = DictationTranscript()
        socket?.cancel(with: .goingAway, reason: nil)
        socket = nil
        Task {
            do {
                let token = try await newToken()
                guard !stopping else { return }
                open(token)
            } catch {
                fail(error)
            }
        }
    }

    private func fail(_ error: Error) {
        finished?.resume(throwing: error)
        finished = nil
        if !stopping { onFailure?(error) }
        cancel()
    }

    /// Something went wrong while listening.
    var onFailure: ((Error) -> Void)?

    private func waitForFinish() async throws -> String {
        if let finishText { return finishText }
        return try await withCheckedThrowingContinuation { finished = $0 }
    }

    private func send(_ data: Data) {
        guard let socket, socket.state == .running else { return }
        socket.send(.data(data)) { _ in }
    }

    private func close() {
        finished?.resume(returning: text)
        finished = nil
        socket?.cancel(with: .normalClosure, reason: nil)
        socket = nil
    }

    // MARK: Microphone

    private func startEngine() throws {
        let input = engine.inputNode
        let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: Double(DictationFrames.sampleRate), channels: 1, interleaved: false)!
        let converter = ConverterBox(to: format)
        let chunker = chunker
        var lastLevel = Date.distantPast
        // The microphone's own format; converted on the audio thread.
        input.installTap(onBus: 0, bufferSize: 2048, format: nil) { [weak self] buffer, _ in
            let level = VoiceAudio.level(buffer)
            if let out = converter.convert(buffer), let samples = out.floatChannelData?[0] {
                let pcm = DictationFrames.pcm16(UnsafeBufferPointer(start: samples, count: Int(out.frameLength)))
                for frame in chunker.add(pcm) {
                    Task { @MainActor in self?.send(frame) }
                }
            }
            if Date().timeIntervalSince(lastLevel) > 0.08 {
                lastLevel = Date()
                Task { @MainActor in self?.onLevel?(level) }
            }
        }
        engine.prepare()
        do {
            try engine.start()
        } catch {
            input.removeTap(onBus: 0)
            DictationLog.write("the microphone wouldn't start: \(error)")
            throw DictationError("The microphone wouldn't start (Core Audio error \((error as NSError).code)).")
        }
    }

    private func stopEngine() {
        guard engine.isRunning else { return }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }
}

/// Gathers PCM from the audio thread into frames of a fixed size.
final class PCMChunker: @unchecked Sendable {
    private let bytes: Int
    private var pending = Data()
    private let lock = NSLock()

    init(bytes: Int) { self.bytes = bytes }

    func add(_ data: Data) -> [Data] {
        lock.lock()
        defer { lock.unlock() }
        pending.append(data)
        var frames: [Data] = []
        while pending.count >= bytes {
            frames.append(pending.prefix(bytes))
            pending.removeFirst(bytes)
        }
        return frames
    }

    func flush() -> Data? {
        lock.lock()
        defer { lock.unlock() }
        guard !pending.isEmpty else { return nil }
        defer { pending = Data() }
        return pending
    }
}

/// Dictation's lines in the app's log. Never what was said.
enum DictationLog {
    static func write(_ line: String) { AppLog.write("dictation", line) }
}
