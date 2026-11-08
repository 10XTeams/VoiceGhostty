//! 语音流水线:录音 → 识别 →(自然语言模式:纠错 → 生成命令)→ 结果回主线程上屏。
//! 两种模式共用一条流水线,只在中间处理环节分叉。识别和 LLM 都是阻塞调用,放后台线程,
//! 结果经通道回到 UI 线程,UI 再写进当前终端(去掉换行、不回车——执行永远由用户按回车)。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vg_core::nl2command::TerminalContext;
use vg_core::{nl2command, ollama, Config};
use vg_voice::{LiveAudio, Recorder, TranscribeOptions};

/// 实时字幕:两次识别之间至少隔这么久
const LIVE_INTERVAL: Duration = Duration::from_millis(700);
/// 录满这么多秒才开始出字幕(太短 Whisper 只会幻听)
const LIVE_MIN_SECS: f32 = 1.0;
/// 字幕只看最近这段录音:再长编码器就慢到跟不上说话,而且前面的内容早已在屏幕上
const LIVE_WINDOW_SECS: f32 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceMode {
    /// 转写原样上屏
    Dictation,
    /// 说人话,LLM 生成 shell 命令
    NaturalLanguage,
}

impl VoiceMode {
    pub fn label(self) -> &'static str {
        match self {
            VoiceMode::Dictation => "听写",
            VoiceMode::NaturalLanguage => "自然语言",
        }
    }
}

#[derive(Debug)]
pub enum PipelineEvent {
    /// 流水线进入某个阶段(状态栏显示)
    Stage(&'static str),
    /// 录音还没结束,到目前为止识别出的内容(实时字幕,只是预览,最终以 Insert 为准)
    Partial(String),
    /// 要写入终端光标处的文本 + 可选说明
    Insert { text: String, explanation: Option<String> },
    /// 没听到有效语音
    Nothing,
    Error(String),
}

pub struct Pipeline {
    recorder: Option<Recorder>,
    tx: Sender<PipelineEvent>,
    pub rx: Receiver<PipelineEvent>,
    busy: bool,
    /// 置位后实时字幕线程在当前这轮识别结束后退出
    live_stop: Option<Arc<AtomicBool>>,
}

impl Pipeline {
    pub fn new() -> Pipeline {
        let (tx, rx) = mpsc::channel();
        Pipeline { recorder: None, tx, rx, busy: false, live_stop: None }
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn level(&self) -> f32 {
        self.recorder.as_ref().map(Recorder::level).unwrap_or(0.0)
    }

    /// UI 收到终态事件后调用,解除忙碌
    pub fn finished(&mut self) {
        self.busy = false;
    }

    pub fn start(&mut self, config: &Config) -> Result<(), String> {
        if self.recorder.is_some() || self.busy {
            return Ok(());
        }
        // 先确认引擎可用,免得录完才发现没配置
        let engine = vg_voice::build_engine(&config.voice).map_err(|e| e.to_string())?;
        let recorder = Recorder::start(&config.voice.device).map_err(|e| format!("{e:#}"))?;
        if config.voice.live_caption && engine.supports_live() {
            let stop = Arc::new(AtomicBool::new(false));
            let live = recorder.live();
            let opts = transcribe_options(config);
            let tx = self.tx.clone();
            let flag = stop.clone();
            std::thread::Builder::new()
                .name("vg-live-caption".into())
                .spawn(move || live_captions(engine, live, opts, flag, tx))
                .map_err(|e| e.to_string())?;
            self.live_stop = Some(stop);
        }
        self.recorder = Some(recorder);
        Ok(())
    }

    /// 松开:停止录音,后台识别。`context` 是活动窗格的终端现场,喂给命令生成。
    pub fn stop(&mut self, config: &Config, mode: VoiceMode, context: TerminalContext) {
        let Some(recorder) = self.recorder.take() else { return };
        if let Some(stop) = self.live_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.busy = true;
        let tx = self.tx.clone();
        let config = config.clone();
        std::thread::spawn(move || {
            let event = run(recorder, &config, mode, &context, &tx);
            let _ = tx.send(event);
        });
    }
}

fn run(
    recorder: Recorder,
    config: &Config,
    mode: VoiceMode,
    context: &TerminalContext,
    tx: &Sender<PipelineEvent>,
) -> PipelineEvent {
    let clip = match recorder.stop() {
        Ok(c) => c,
        Err(e) => return PipelineEvent::Error(format!("{e:#}")),
    };
    let engine = match vg_voice::build_engine(&config.voice) {
        Ok(e) => e,
        Err(e) => return PipelineEvent::Error(e.to_string()),
    };
    let _ = tx.send(PipelineEvent::Stage("识别中…"));
    let opts = transcribe_options(config);
    let text = match vg_voice::transcribe(engine.as_ref(), &clip, &opts) {
        Ok(Some(t)) => t,
        Ok(None) => return PipelineEvent::Nothing,
        Err(e) => return PipelineEvent::Error(format!("{} 识别失败:{e:#}", engine.name())),
    };

    match mode {
        VoiceMode::Dictation => PipelineEvent::Insert {
            text: nl2command::strip_newlines(&text),
            explanation: None,
        },
        VoiceMode::NaturalLanguage => {
            // 纠错只能锦上添花:服务没开/输出异常一律用原文
            let intent = if config.correction_enabled {
                let _ = tx.send(PipelineEvent::Stage("整理语句…"));
                ollama::correct(&text, &config.llm_local_model, &config.llm_local_url)
                    .unwrap_or_else(|_| text.clone())
            } else {
                text.clone()
            };
            let _ = tx.send(PipelineEvent::Stage("生成命令…"));
            match nl2command::translate(&intent, context, config) {
                Ok(t) => PipelineEvent::Insert {
                    text: nl2command::strip_newlines(&t.command),
                    explanation: Some(format!("「{intent}」→ {}", t.explanation)),
                },
                Err(e) => PipelineEvent::Error(e.to_string()),
            }
        }
    }
}

fn transcribe_options(config: &Config) -> TranscribeOptions {
    TranscribeOptions {
        language: config.voice.language.clone(),
        punctuation: config.voice.punctuation,
    }
}

/// 实时字幕线程:录音期间每隔一会儿把最近一段录音整体再识别一遍,把结果作为预览发给 UI。
/// 每次都是对"到目前为止"的整段重识别而不是增量拼接,所以前面说错的词会在后续轮次被纠正。
/// 与松开后的最终识别共用同一个引擎实例(whisper 内部有锁,两者串行),线程在 `stop` 置位后
/// 跑完当前这轮就退出。
fn live_captions(
    engine: Box<dyn vg_voice::SpeechEngine>,
    live: LiveAudio,
    opts: TranscribeOptions,
    stop: Arc<AtomicBool>,
    tx: Sender<PipelineEvent>,
) {
    let rate = live.sample_rate().max(1) as f32;
    let mut last_len = 0usize;
    let mut last_text = String::new();
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let len = live.len();
        let secs = len as f32 / rate;
        // 至少录了 1 s,且比上次多了 0.5 s 的新内容才值得再跑一遍
        if secs >= LIVE_MIN_SECS && (len - last_len) as f32 / rate >= 0.5 {
            last_len = len;
            let clip = live.tail(LIVE_WINDOW_SECS);
            match vg_voice::transcribe(engine.as_ref(), &clip, &opts) {
                Ok(Some(text)) if text != last_text => {
                    last_text = text.clone();
                    if tx.send(PipelineEvent::Partial(text)).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    log::debug!("live caption: {e:#}");
                    return; // 引擎出问题就别再每秒重试了,最终识别会报告错误
                }
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if let Some(rest) = LIVE_INTERVAL.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}
