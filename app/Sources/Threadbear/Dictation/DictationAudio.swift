import AVFoundation
import ThreadbearCore

/// The microphone side of dictation: asking for it, how loud it is, and
/// turning its buffers into the format Collabute's speech-to-text takes.
/// These came from Voice mode's VoiceAudio, which Mike took out on 9 Nov.
enum DictationAudio {
    /// Asks macOS for the microphone the first time; says how to turn it
    /// back on when it was refused.
    static func askForAccess() async throws {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: break
        case .notDetermined:
            guard await AVCaptureDevice.requestAccess(for: .audio) else { throw DictationError("Threadbear isn't allowed to use the microphone.") }
        default:
            throw DictationError("Threadbear isn't allowed to use the microphone. Turn it on in System Settings > Privacy & Security > Microphone.")
        }
    }

    /// How loud a buffer is, 0 to 1, for the mic's level.
    nonisolated static func level(_ buffer: AVAudioPCMBuffer) -> Float {
        guard let data = buffer.floatChannelData?[0], buffer.frameLength > 0 else { return 0 }
        var sum: Float = 0
        for i in 0..<Int(buffer.frameLength) { sum += data[i] * data[i] }
        let rms = (sum / Float(buffer.frameLength)).squareRoot()
        return min(1, rms * 12)
    }
}

/// Converts the microphone's buffers to one format on the audio thread,
/// making its converter from the first buffer it sees.
final class ConverterBox: @unchecked Sendable {
    private let format: AVAudioFormat
    private var converter: AVAudioConverter?
    private var failed = false

    init(to format: AVAudioFormat) { self.format = format }

    func convert(_ buffer: AVAudioPCMBuffer) -> AVAudioPCMBuffer? {
        if converter == nil, !failed {
            converter = AVAudioConverter(from: buffer.format, to: format)
            // More channels than one: the first.
            if buffer.format.channelCount > format.channelCount { converter?.channelMap = [0] }
            if converter == nil {
                failed = true
                AppLog.write("dictation", "can't convert \(buffer.format) for speech-to-text")
            }
        }
        guard let converter else { return nil }
        let ratio = format.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio + 16)
        guard let out = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else { return nil }
        var fed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if fed { status.pointee = .noDataNow; return nil }
            fed = true
            status.pointee = .haveData
            return buffer
        }
        return error == nil && out.frameLength > 0 ? out : nil
    }
}
