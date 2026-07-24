import AVFoundation
import Speech
import SwiftUI

/// 麦克风采集 (AVAudioEngine) + 中英识别。
///
/// 关键设计:**任何时刻只跑一个识别器**,避免两个端侧识别器并发互相抢占(会随机只有一个出结果)。
/// - 录音时:只跑中文(zh-CN)实时识别(用户以中文为主),同时把音频录进 `captured`。
/// - 停止后:中文结果含汉字 → 用中文;否则拿录好的整段音频**单独再跑一次英文(en-US)**。
final class SpeechRecognizer: ObservableObject {
    @Published var isRecording = false
    /// 实时(部分)识别结果,录音过程中持续更新
    @Published var liveTranscript = ""
    @Published var errorMessage: String?

    /// 最终识别结果回调(停止录音后触发一次)
    var onFinalResult: ((String) -> Void)?

    private let audioEngine = AVAudioEngine()
    private let zhRecognizer = SFSpeechRecognizer(locale: Locale(identifier: "zh-CN"))
    private let enRecognizer = SFSpeechRecognizer(locale: Locale(identifier: "en-US"))

    // 主(中文)实时识别
    private var liveRequest: SFSpeechAudioBufferRecognitionRequest?
    private var liveTask: SFSpeechRecognitionTask?
    private var liveText = ""
    // 英文兜底:录音期间捕获的整段音频
    private var captured: [AVAudioPCMBuffer] = []
    private var enTask: SFSpeechRecognitionTask?

    // VAD:检测到开口才建识别任务、才开始录音,避免开头静默触发 1110
    private var speechStarted = false
    private var preRoll: [AVAudioPCMBuffer] = []
    private let preRollCount = 6
    private let rmsThreshold: Float = 0.01

    private var hasDelivered = false
    /// 代际计数:stop() 时 +1,取消权限流程中尚未真正启动的录音
    private var generation = 0
    private var userStopped = false
    private var finishTimeout: DispatchWorkItem?

    func toggle() {
        isRecording ? stop() : startRecording()
    }

    /// 开始录音(含权限申请);已在录音则忽略
    func startRecording() {
        guard !isRecording else { return }
        requestPermissionsAndStart()
    }

    // MARK: - 权限

    private func requestPermissionsAndStart() {
        errorMessage = nil
        let gen = generation
        SFSpeechRecognizer.requestAuthorization { [weak self] status in
            DispatchQueue.main.async {
                guard let self, self.generation == gen else { return }
                guard status == .authorized else {
                    self.errorMessage = "语音识别权限被拒绝(系统设置 → 隐私与安全性 → 语音识别)"
                    return
                }
                AVCaptureDevice.requestAccess(for: .audio) { granted in
                    DispatchQueue.main.async {
                        guard self.generation == gen else { return }
                        guard granted else {
                            self.errorMessage = "麦克风权限被拒绝(系统设置 → 隐私与安全性 → 麦克风)"
                            return
                        }
                        self.start()
                    }
                }
            }
        }
    }

    // MARK: - 录音

    private func start() {
        guard (zhRecognizer?.isAvailable ?? false) || (enRecognizer?.isAvailable ?? false) else {
            errorMessage = "语音识别不可用(zh-CN / en-US 均不可用,检查系统听写设置)"
            return
        }
        liveText = ""
        liveTranscript = ""
        captured = []
        preRoll = []
        speechStarted = false
        hasDelivered = false
        userStopped = false
        liveRequest = nil
        liveTask = nil
        enTask = nil

        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let self else { return }

            if self.speechStarted {
                // 每处独立副本:同一 buffer 对象被 append 后可能失效
                self.liveRequest?.append(Self.copy(buffer))
                self.captured.append(Self.copy(buffer))
                return
            }

            // 尚未开口:滚动保留前置缓冲
            self.preRoll.append(buffer)
            if self.preRoll.count > self.preRollCount { self.preRoll.removeFirst() }

            if Self.rms(buffer) > self.rmsThreshold {
                self.speechStarted = true
                self.startLiveChinese()
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
            errorMessage = "音频引擎启动失败: \(error.localizedDescription)"
            cleanup()
        }
    }

    /// 开口瞬间创建中文实时识别任务
    private func startLiveChinese() {
        guard let zh = zhRecognizer, zh.isAvailable else { return }
        let req = SFSpeechAudioBufferRecognitionRequest()
        req.shouldReportPartialResults = true
        liveRequest = req
        liveTask = zh.recognitionTask(with: req) { [weak self] result, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let result {
                    self.liveText = result.bestTranscription.formattedString
                    self.liveTranscript = self.liveText
                    if result.isFinal { self.finish() }
                }
                if error != nil { self.finish() }   // 中文识别结束/失败 → 进裁决(可能转英文兜底)
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

        // 兜底:2 秒内中文没出最终结果就强制裁决
        guard !hasDelivered else { return }
        let work = DispatchWorkItem { [weak self] in self?.finish() }
        finishTimeout = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0, execute: work)
    }

    // MARK: - 裁决:中文含汉字直接用,否则跑英文兜底

    private func finish() {
        guard !hasDelivered else { return }
        if liveText.containsCJK {
            deliver(liveText)
        } else {
            runEnglishFallback()
        }
    }

    /// 在录好的整段音频上单独跑一次英文识别(此刻中文任务已结束,无并发抢占)
    private func runEnglishFallback() {
        guard let en = enRecognizer, en.isAvailable, !captured.isEmpty else {
            deliver(liveText)   // 没英文识别器或没录到音频 → 用中文结果(可能为空)
            return
        }
        let req = SFSpeechAudioBufferRecognitionRequest()
        req.shouldReportPartialResults = false
        let buffers = captured
        enTask = en.recognitionTask(with: req) { [weak self] result, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let result, result.isFinal {
                    let enText = result.bestTranscription.formattedString
                    self.deliver(enText.isEmpty ? self.liveText : enText)
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
        enTask?.cancel(); enTask = nil
        liveRequest = nil
        captured = []
        preRoll = []
    }

    // MARK: - 音频工具

    /// 均方根音量(判断是否有人声)
    private static func rms(_ buffer: AVAudioPCMBuffer) -> Float {
        guard let ch = buffer.floatChannelData, buffer.frameLength > 0 else { return 0 }
        let n = Int(buffer.frameLength)
        var sum: Float = 0
        for i in 0..<n { let v = ch[0][i]; sum += v * v }
        return (sum / Float(n)).squareRoot()
    }

    /// 深拷贝一帧音频
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
    /// 是否包含中日韩统一表意文字(用于判定"说的是中文")
    var containsCJK: Bool {
        unicodeScalars.contains { (0x4E00...0x9FFF).contains($0.value) }
    }
}
