//! 语音流水线:录音 → 识别 →(自然语言模式:纠错 → 生成命令)→ 结果回主线程上屏。
//! 两种模式共用一条流水线,只在中间处理环节分叉。识别和 LLM 都是阻塞调用,放后台线程,
//! 结果经通道回到 UI 线程,UI 再写进当前终端(去掉换行、不回车——执行永远由用户按回车)。

use std::sync::mpsc::{self, Receiver, Sender};

use vg_core::{nl2command, ollama, Config};
use vg_voice::{Recorder, TranscribeOptions};

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
}

impl Pipeline {
    pub fn new() -> Pipeline {
        let (tx, rx) = mpsc::channel();
        Pipeline { recorder: None, tx, rx, busy: false }
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
        vg_voice::build_engine(&config.voice).map_err(|e| e.to_string())?;
        let recorder = Recorder::start(&config.voice.device).map_err(|e| format!("{e:#}"))?;
        self.recorder = Some(recorder);
        Ok(())
    }

    /// 松开:停止录音,后台识别。`cwd` 喂给命令生成。
    pub fn stop(&mut self, config: &Config, mode: VoiceMode, cwd: Option<String>) {
        let Some(recorder) = self.recorder.take() else { return };
        self.busy = true;
        let tx = self.tx.clone();
        let config = config.clone();
        std::thread::spawn(move || {
            let event = run(recorder, &config, mode, cwd.as_deref(), &tx);
            let _ = tx.send(event);
        });
    }
}

fn run(
    recorder: Recorder,
    config: &Config,
    mode: VoiceMode,
    cwd: Option<&str>,
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
    let opts = TranscribeOptions {
        language: config.voice.language.clone(),
        punctuation: config.voice.punctuation,
    };
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
            match nl2command::translate(&intent, cwd, config) {
                Ok(t) => PipelineEvent::Insert {
                    text: nl2command::strip_newlines(&t.command),
                    explanation: Some(format!("「{intent}」→ {}", t.explanation)),
                },
                Err(e) => PipelineEvent::Error(e.to_string()),
            }
        }
    }
}
