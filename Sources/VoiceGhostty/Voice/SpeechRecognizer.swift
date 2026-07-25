import AVFoundation
import Speech
import SwiftUI

/// Microphone capture (AVAudioEngine) + dual-locale (Chinese/English) recognition.
///
/// Key design: **only one recognizer runs at any time**, to avoid two on-device recognizers running concurrently and preempting each other (which randomly leaves only one producing results).
/// - The *primary* locale comes from `AppSettings.defaultLanguage` (default zh-CN); the other locale is the fallback.
/// - While recording: run only the primary locale's live recognition, while also recording the audio into `captured`.
/// - After stopping: if the primary result looks good (Chinese-primary → contains Han characters; English-primary → non-empty) use it; otherwise take the full recorded audio and **run the fallback locale once separately**.
final class SpeechRecognizer: ObservableObject {
    @Published var isRecording = false
    /// Live (partial) recognition result, continuously updated during recording
    @Published var liveTranscript = ""
    @Published var errorMessage: String?

    /// Final recognition result callback (fired once after recording stops)
    var onFinalResult: ((String) -> Void)?

    private let audioEngine = AVAudioEngine()
    private let zhRecognizer = SFSpeechRecognizer(locale: Locale(identifier: "zh-CN"))
    private let enRecognizer = SFSpeechRecognizer(locale: Locale(identifier: "en-US"))

    /// Primary recognition locale, refreshed from `AppSettings` at the start of each recording.
    private var primaryLocale = "zh-CN"
    private var primaryRecognizer: SFSpeechRecognizer? {
        primaryLocale == "en-US" ? enRecognizer : zhRecognizer
    }
    private var fallbackRecognizer: SFSpeechRecognizer? {
        primaryLocale == "en-US" ? zhRecognizer : enRecognizer
    }

    // Primary live recognition
    private var liveRequest: SFSpeechAudioBufferRecognitionRequest?
    private var liveTask: SFSpeechRecognitionTask?
    private var liveText = ""
    // Fallback: the full audio captured during recording
    private var captured: [AVAudioPCMBuffer] = []
    private var fallbackTask: SFSpeechRecognitionTask?

    // VAD: only build the recognition task and start recording once speech is detected, to avoid leading silence triggering error 1110
    private var speechStarted = false
    private var preRoll: [AVAudioPCMBuffer] = []
    private let preRollCount = 6
    private let rmsThreshold: Float = 0.01

    private var hasDelivered = false
    /// Generation counter: +1 on stop(), cancelling a recording not yet actually started during the permission flow
    private var generation = 0
    private var userStopped = false
    private var finishTimeout: DispatchWorkItem?

    func toggle() {
        isRecording ? stop() : startRecording()
    }

    /// Start recording (including permission requests); ignored if already recording
    func startRecording() {
        guard !isRecording else { return }
        requestPermissionsAndStart()
    }

    // MARK: - Permissions

    private func requestPermissionsAndStart() {
        errorMessage = nil
        let gen = generation
        SFSpeechRecognizer.requestAuthorization { [weak self] status in
            DispatchQueue.main.async {
                guard let self, self.generation == gen else { return }
                guard status == .authorized else {
                    self.errorMessage = Loc.shared("Speech recognition permission denied (System Settings → Privacy & Security → Speech Recognition)",
                                                   "语音识别权限被拒绝(系统设置 → 隐私与安全性 → 语音识别)")
                    return
                }
                AVCaptureDevice.requestAccess(for: .audio) { granted in
                    DispatchQueue.main.async {
                        guard self.generation == gen else { return }
                        guard granted else {
                            self.errorMessage = Loc.shared("Microphone permission denied (System Settings → Privacy & Security → Microphone)",
                                                           "麦克风权限被拒绝(系统设置 → 隐私与安全性 → 麦克风)")
                            return
                        }
                        self.start()
                    }
                }
            }
        }
    }

    // MARK: - Recording

    private func start() {
        guard (zhRecognizer?.isAvailable ?? false) || (enRecognizer?.isAvailable ?? false) else {
            errorMessage = Loc.shared("Speech recognition unavailable (neither zh-CN nor en-US available; check system dictation settings)",
                                      "语音识别不可用(zh-CN 与 en-US 均不可用;请检查系统听写设置)")
            return
        }
        primaryLocale = AppSettings.defaultLanguage
        liveText = ""
        liveTranscript = ""
        captured = []
        preRoll = []
        speechStarted = false
        hasDelivered = false
        userStopped = false
        liveRequest = nil
        liveTask = nil
        fallbackTask = nil

        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let self else { return }

            if self.speechStarted {
                // Independent copy each place: the same buffer object may become invalid after being appended
                self.liveRequest?.append(Self.copy(buffer))
                self.captured.append(Self.copy(buffer))
                return
            }

            // Not speaking yet: keep a rolling pre-roll buffer
            self.preRoll.append(buffer)
            if self.preRoll.count > self.preRollCount { self.preRoll.removeFirst() }

            if Self.rms(buffer) > self.rmsThreshold {
                self.speechStarted = true
                self.startLive()
                for b in self.preRoll {
                    self.liveRequest?.append(Self.copy(b))
                    self.captured.append(Self.copy(b))
                }
            }
        }

        do {
            audioEngine.prepare()
            try audioEngine.start()
            isRecording = true
        } catch {
            errorMessage = Loc.shared("Audio engine failed to start: ", "音频引擎启动失败:") + error.localizedDescription
            cleanup()
        }
    }

    /// Create the primary-locale live recognition task the moment speech starts
    private func startLive() {
        guard let primary = primaryRecognizer, primary.isAvailable else { return }
        let req = SFSpeechAudioBufferRecognitionRequest()
        req.shouldReportPartialResults = true
        liveRequest = req
        liveTask = primary.recognitionTask(with: req) { [weak self] result, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let result {
                    self.liveText = result.bestTranscription.formattedString
                    self.liveTranscript = self.liveText
                    if result.isFinal { self.finish() }
                }
                if error != nil { self.finish() }   // Primary recognition ended/failed → go to arbitration (may switch to the fallback locale)
            }
        }
    }

    func stop() {
        generation += 1
        userStopped = true
        liveRequest?.endAudio()
        audioEngine.stop()
        audioEngine.inputNode.removeTap(onBus: 0)
        isRecording = false

        // Fallback: force arbitration if Chinese produces no final result within 2 seconds
        guard !hasDelivered else { return }
        let work = DispatchWorkItem { [weak self] in self?.finish() }
        finishTimeout = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0, execute: work)
    }

    // MARK: - Arbitration: use the primary result if it looks good, otherwise run the fallback locale

    private func finish() {
        guard !hasDelivered else { return }
        if primaryResultIsGood {
            deliver(liveText)
        } else {
            runFallback()
        }
    }

    /// Whether the primary live result is trustworthy.
    /// - Chinese-primary: the audio is Chinese only if the transcript contains Han characters (the zh recognizer
    ///   otherwise transliterates English into wrong Han, so fall back to English).
    /// - English-primary: the en recognizer never emits Han, so any non-empty Latin transcript is trusted.
    private var primaryResultIsGood: Bool {
        if primaryLocale == "en-US" {
            return !liveText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        } else {
            return liveText.containsCJK
        }
    }

    /// Run the fallback locale once on the full recorded audio (by now the primary task has ended, so no concurrent preemption)
    private func runFallback() {
        guard let fallback = fallbackRecognizer, fallback.isAvailable, !captured.isEmpty else {
            deliver(liveText)   // No fallback recognizer or no audio recorded → use the primary result (may be empty)
            return
        }
        let req = SFSpeechAudioBufferRecognitionRequest()
        req.shouldReportPartialResults = false
        let buffers = captured
        fallbackTask = fallback.recognitionTask(with: req) { [weak self] result, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let result, result.isFinal {
                    let fallbackText = result.bestTranscription.formattedString
                    self.deliver(fallbackText.isEmpty ? self.liveText : fallbackText)
                } else if error != nil {
                    self.deliver(self.liveText)
                }
            }
        }
        for b in buffers { req.append(Self.copy(b)) }
        req.endAudio()

        let work = DispatchWorkItem { [weak self] in
            guard let self, !self.hasDelivered else { return }
            self.deliver(self.liveText)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0, execute: work)
    }

    private func deliver(_ raw: String) {
        guard !hasDelivered else { return }
        hasDelivered = true
        finishTimeout?.cancel()
        finishTimeout = nil
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        cleanup()
        if !text.isEmpty { onFinalResult?(text) }
    }

    private func cleanup() {
        liveTask?.cancel(); liveTask = nil
        fallbackTask?.cancel(); fallbackTask = nil
        liveRequest = nil
        captured = []
        preRoll = []
    }

    // MARK: - Audio utilities

    /// Root-mean-square volume (to detect whether there is speech)
    private static func rms(_ buffer: AVAudioPCMBuffer) -> Float {
        guard let ch = buffer.floatChannelData, buffer.frameLength > 0 else { return 0 }
        let n = Int(buffer.frameLength)
        var sum: Float = 0
        for i in 0..<n { let v = ch[0][i]; sum += v * v }
        return (sum / Float(n)).squareRoot()
    }

    /// Deep-copy a single audio frame
    private static func copy(_ buffer: AVAudioPCMBuffer) -> AVAudioPCMBuffer {
        let out = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: buffer.frameCapacity)!
        out.frameLength = buffer.frameLength
        let n = Int(buffer.frameLength)
        let channels = Int(buffer.format.channelCount)
        if let src = buffer.floatChannelData, let dst = out.floatChannelData {
            for ch in 0..<channels { memcpy(dst[ch], src[ch], n * MemoryLayout<Float>.size) }
        }
        return out
    }
}

private extension String {
    /// Whether it contains CJK Unified Ideographs (used to decide "the speech is Chinese")
    var containsCJK: Bool {
        unicodeScalars.contains { (0x4E00...0x9FFF).contains($0.value) }
    }
}
