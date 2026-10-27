//! 进程内 whisper.cpp 引擎(`voice-engine = whisper`,编译时需开启 `whisper` feature)。
//! 本地离线、不依赖外部程序。模型是 whisper.cpp 的 ggml 格式(`ggml-base.bin` 等)。
//!
//! 载入模型要几秒、占几百 MB,所以全进程只载一次:按模型路径缓存,换了路径才重载。
//! 推理用的 `WhisperState`(约 460 MB 的 KV cache 与 Metal 计算缓冲)同样只建一次、反复复用。
//!
//! 延迟几乎全在编码器:Whisper 不管说了多长都把音频补到 30 s 再算一遍。按住说话一句话通常只有
//! 几秒,所以按录音长度缩小 `audio_ctx`,编码器就只算实际有声的那段(M3 上 4 s 语音约快 3 倍)。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use anyhow::{anyhow, Context};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::clip::AudioClip;
use crate::engine::{clean_transcript, SpeechEngine, TranscribeOptions};

pub struct WhisperEngine {
    pub model: PathBuf,
}

/// 已载入的模型(推理状态持有模型的引用)及其路径
type LoadedModel = Mutex<Option<(PathBuf, WhisperState)>>;

/// Whisper 编码器的完整上下文:30 s 音频对应 1500 帧,即每秒 50 帧
const FULL_AUDIO_CTX: f32 = 1500.0;
const AUDIO_CTX_PER_SEC: f32 = FULL_AUDIO_CTX / 30.0;
/// 太小的上下文会让解码器幻听、重复;whisper.cpp 的流式示例默认 768,这里下限取 512
const MIN_AUDIO_CTX: i32 = 512;
/// 在实际时长之外多留的帧数(约 5 s),给解码器一点"句子已结束"的静音余量
const AUDIO_CTX_PAD: f32 = 256.0;
/// 解码器自己给出的"这段没有语音"概率超过它就丢弃该段(与 whisper.cpp / OpenAI 的默认值相同)。
/// 只是兜底:实测 large-v3-turbo 配上缩小的 audio_ctx,纯噪音时这个概率也常是 0,照样吐出
/// "谢谢观看" / 复读提示词之类的幻听。真正挡住噪音的是录音端的 `AudioClip::trim_silence`
/// 和 `engine::is_hallucination` / `engine::echoes_prompt`
const NO_SPEECH_THRESHOLD: f32 = 0.6;

/// 按录音时长算编码器上下文:实际帧数 + 余量,向上取到 64 的倍数,夹在 [512, 1500]
pub fn audio_ctx_for(duration_secs: f32) -> i32 {
    let frames = duration_secs.max(0.0) * AUDIO_CTX_PER_SEC + AUDIO_CTX_PAD;
    let rounded = ((frames / 64.0).ceil() * 64.0) as i32;
    rounded.clamp(MIN_AUDIO_CTX, FULL_AUDIO_CTX as i32)
}

fn cache() -> &'static LoadedModel {
    static CACHE: OnceLock<LoadedModel> = OnceLock::new();
    CACHE.get_or_init(|| {
        // 把 whisper.cpp / ggml 的日志导进 whisper-rs(没开日志后端时即丢弃),别刷满 stderr
        whisper_rs::install_logging_hooks();
        Mutex::new(None)
    })
}

/// 拿到(必要时先载入)模型的推理状态。锁在整个识别期间持有:同一时刻只会有一次识别。
fn state(model: &Path) -> anyhow::Result<MutexGuard<'static, Option<(PathBuf, WhisperState)>>> {
    let mut slot = cache().lock().unwrap();
    if !matches!(slot.as_ref(), Some((path, _)) if path == model) {
        let started = Instant::now();
        let mut params = WhisperContextParameters::default();
        // Metal 上快约 15%,无精度代价(whisper-cli 自己默认就开)
        params.flash_attn(true);
        let ctx = WhisperContext::new_with_params(model, params)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| format!("failed to load whisper model {}", model.display()))?;
        let st = ctx.create_state().map_err(|e| anyhow!("{e}"))?;
        log::info!("whisper: loaded {} in {:.2}s", model.display(), started.elapsed().as_secs_f32());
        *slot = Some((model.to_path_buf(), st));
    }
    Ok(slot)
}

/// 预先载入模型,把几秒的冷启动挪到用户开口之前
pub fn preload(model: &Path) {
    let model = model.to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = state(&model) {
            log::warn!("{e:#}");
        }
    });
}

/// 释放缓存的模型。进程退出前必须调:否则 ggml 的 Metal 设备在 exit 时先析构,
/// 发现模型的显存还没释放会直接 abort(`GGML_ASSERT([rsets->data count] == 0)`)
pub fn unload() {
    let loaded = cache().lock().unwrap().take();
    drop(loaded);
}

impl SpeechEngine for WhisperEngine {
    fn name(&self) -> String {
        let file = self.model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        format!("whisper · {file}")
    }

    fn transcribe(&self, clip: &AudioClip, opts: &TranscribeOptions) -> anyhow::Result<String> {
        let mut slot = state(&self.model)?;
        let (_, state) = slot.as_mut().expect("state() always fills the slot");
        let started = Instant::now();

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let language = (opts.language != "auto").then_some(opts.language.as_str());
        params.set_language(language.or(Some("auto")));
        if let Some(prompt) = opts.prompt() {
            params.set_initial_prompt(&prompt);
        }
        let threads = std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4);
        params.set_n_threads(threads as i32);
        // 一句话的听写:不需要跨段上下文,也别往 stdout 打进度
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        // 只让编码器算实际录音那一段,而不是补齐到 30 s
        let audio_ctx = audio_ctx_for(clip.duration_secs());
        params.set_audio_ctx(audio_ctx);

        state.full(params, &clip.samples).map_err(|e| anyhow!("whisper failed: {e}"))?;
        let mut text = Vec::new();
        for seg in state.as_iter() {
            let Ok(s) = seg.to_str_lossy() else { continue };
            let no_speech = seg.no_speech_probability();
            if no_speech > NO_SPEECH_THRESHOLD {
                log::info!("whisper: dropped segment, no_speech_prob {no_speech:.2}: {:?}", s.trim());
                continue;
            }
            log::debug!("whisper: segment no_speech_prob {no_speech:.2}: {:?}", s.trim());
            text.push(s.into_owned());
        }
        log::info!(
            "whisper: {:.1}s audio, audio_ctx {audio_ctx}, transcribed in {:.0}ms",
            clip.duration_secs(),
            started.elapsed().as_millis()
        );
        Ok(clean_transcript(&text.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::audio_ctx_for;

    #[test]
    fn audio_ctx_scales_with_duration() {
        assert_eq!(audio_ctx_for(0.0), 512);
        assert_eq!(audio_ctx_for(4.0), 512); // 200 + 256 = 456 → 512
        assert_eq!(audio_ctx_for(10.0), 768); // 500 + 256 = 756 → 768
        assert_eq!(audio_ctx_for(20.0), 1280); // 1000 + 256 = 1256 → 1280
        assert_eq!(audio_ctx_for(30.0), 1500);
        assert_eq!(audio_ctx_for(90.0), 1500);
    }
}
