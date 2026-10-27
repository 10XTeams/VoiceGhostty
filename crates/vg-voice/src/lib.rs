//! VoiceGhostty 语音层:录音 + 可配置的识别引擎。
//!
//! 引擎由配置 `voice-engine` 选择(见 vg-core::config::VoiceEngineKind),`auto` 时按
//! 外部命令(配了 voice-command)→ HTTP(有 API Key 或指向本机服务)的顺序挑第一个可用的。

pub mod chime;
pub mod clip;
pub mod engine;
pub mod recorder;
#[cfg(feature = "whisper")]
pub mod whisper;

use anyhow::bail;
use vg_core::{VoiceConfig, VoiceEngineKind};

pub use clip::AudioClip;
pub use engine::{echoes_prompt, is_hallucination, CommandEngine, HttpEngine, SpeechEngine, TranscribeOptions};
pub use recorder::{input_devices, Recorder};

/// 录音短于这个时长直接丢弃(误触空格)
pub const MIN_SPEECH_SECS: f32 = 0.3;

/// 按配置构造识别引擎。返回的错误是给用户看的配置提示。
pub fn build_engine(cfg: &VoiceConfig) -> anyhow::Result<Box<dyn SpeechEngine>> {
    match cfg.engine {
        VoiceEngineKind::Off => bail!("Voice is off (voice-engine = off)"),
        VoiceEngineKind::Command => command_engine(cfg),
        VoiceEngineKind::Http => Ok(Box::new(http_engine(cfg))),
        VoiceEngineKind::Whisper => whisper_engine(cfg),
        VoiceEngineKind::Auto => {
            // 编进来了且有模型:本地离线、零配置,优先
            if cfg!(feature = "whisper") && cfg.resolved_model().is_some() {
                return whisper_engine(cfg);
            }
            if !cfg.command.is_empty() {
                return command_engine(cfg);
            }
            let http = http_engine(cfg);
            if cfg.resolved_api_key().is_some() || http.is_local() {
                return Ok(Box::new(http));
            }
            bail!(
                "No speech engine configured. Set voice-command (e.g. whisper.cpp), \
                 or voice-api-key / OPENAI_API_KEY, or point voice-api-url at a local server"
            )
        }
    }
}

#[cfg(feature = "whisper")]
fn whisper_engine(cfg: &VoiceConfig) -> anyhow::Result<Box<dyn SpeechEngine>> {
    let model = cfg.resolved_model().ok_or_else(|| {
        anyhow::anyhow!(
            "No whisper model found: set voice-model, or put a ggml-*.bin into {}",
            vg_core::config::models_dir().display()
        )
    })?;
    Ok(Box::new(whisper::WhisperEngine { model }))
}

#[cfg(not(feature = "whisper"))]
fn whisper_engine(_cfg: &VoiceConfig) -> anyhow::Result<Box<dyn SpeechEngine>> {
    bail!(
        "This build does not include the built-in whisper engine (build with --features whisper), \
         or use voice-engine = command with whisper.cpp's whisper-cli"
    )
}

/// 选了内置 whisper 时,启动就在后台载入模型
pub fn preload(cfg: &VoiceConfig) {
    #[cfg(feature = "whisper")]
    if matches!(cfg.engine, VoiceEngineKind::Whisper | VoiceEngineKind::Auto) {
        if let Some(model) = cfg.resolved_model() {
            whisper::preload(&model);
        }
    }
    #[cfg(not(feature = "whisper"))]
    let _ = cfg;
}

/// 退出前释放内置 whisper 的模型(没编进来时什么都不做)
pub fn shutdown() {
    #[cfg(feature = "whisper")]
    whisper::unload();
}

fn command_engine(cfg: &VoiceConfig) -> anyhow::Result<Box<dyn SpeechEngine>> {
    if cfg.command.is_empty() {
        bail!("voice-engine = command but voice-command is empty");
    }
    Ok(Box::new(CommandEngine { template: cfg.command.clone() }))
}

fn http_engine(cfg: &VoiceConfig) -> HttpEngine {
    HttpEngine {
        url: cfg.api_url.clone(),
        api_key: cfg.resolved_api_key(),
        model: cfg.api_model.clone(),
    }
}

/// 一次完整识别:转 16 kHz → 裁静音 → 交给引擎。静音 / 只有噪音 / 过短 / 识别出的是幻听套话,
/// 都返回 Ok(None)。
pub fn transcribe(
    engine: &dyn SpeechEngine,
    clip: &AudioClip,
    opts: &TranscribeOptions,
) -> anyhow::Result<Option<String>> {
    let Some(speech) = clip.to_16k().trim_silence() else {
        return Ok(None);
    };
    if speech.duration_secs() < MIN_SPEECH_SECS {
        return Ok(None);
    }
    let text = engine.transcribe(&speech, opts)?;
    if is_hallucination(&text) || opts.prompt().is_some_and(|p| echoes_prompt(&text, &p)) {
        log::info!("voice: discarded hallucinated transcript {text:?}");
        return Ok(None);
    }
    Ok(Some(text))
}
