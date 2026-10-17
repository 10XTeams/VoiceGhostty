//! 进程内 whisper.cpp 引擎(`voice-engine = whisper`,编译时需开启 `whisper` feature)。
//! 本地离线、不依赖外部程序。模型是 whisper.cpp 的 ggml 格式(`ggml-base.bin` 等)。
//!
//! 载入模型要几秒、占几百 MB,所以全进程只载一次:按模型路径缓存,换了路径才重载。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::clip::AudioClip;
use crate::engine::{clean_transcript, SpeechEngine, TranscribeOptions};

pub struct WhisperEngine {
    pub model: PathBuf,
}

/// 已载入的模型及其路径
type LoadedModel = Mutex<Option<(PathBuf, Arc<WhisperContext>)>>;

fn cache() -> &'static LoadedModel {
    static CACHE: OnceLock<LoadedModel> = OnceLock::new();
    CACHE.get_or_init(|| {
        // 把 whisper.cpp / ggml 的日志导进 whisper-rs(没开日志后端时即丢弃),别刷满 stderr
        whisper_rs::install_logging_hooks();
        Mutex::new(None)
    })
}

fn context(model: &Path) -> anyhow::Result<Arc<WhisperContext>> {
    let mut slot = cache().lock().unwrap();
    if let Some((path, ctx)) = slot.as_ref() {
        if path == model {
            return Ok(ctx.clone());
        }
    }
    let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("failed to load whisper model {}", model.display()))?;
    let ctx = Arc::new(ctx);
    *slot = Some((model.to_path_buf(), ctx.clone()));
    Ok(ctx)
}

/// 预先载入模型,把几秒的冷启动挪到用户开口之前
pub fn preload(model: &Path) {
    let model = model.to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = context(&model) {
            log::warn!("{e:#}");
        }
    });
}

/// 释放缓存的模型。进程退出前必须调:否则 ggml 的 Metal 设备在 exit 时先析构,
/// 发现模型的显存还没释放会直接 abort(`GGML_ASSERT([rsets->data count] == 0)`)
pub fn unload() {
    let ctx = cache().lock().unwrap().take();
    drop(ctx);
}

impl SpeechEngine for WhisperEngine {
    fn name(&self) -> String {
        let file = self.model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        format!("whisper · {file}")
    }

    fn transcribe(&self, clip: &AudioClip, opts: &TranscribeOptions) -> anyhow::Result<String> {
        let ctx = context(&self.model)?;
        let mut state = ctx.create_state().map_err(|e| anyhow!("{e}"))?;

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

        state.full(params, &clip.samples).map_err(|e| anyhow!("whisper failed: {e}"))?;
        let text: Vec<String> = state
            .as_iter()
            .filter_map(|seg| seg.to_str_lossy().ok().map(|s| s.into_owned()))
            .collect();
        Ok(clean_transcript(&text.join("\n")))
    }
}
