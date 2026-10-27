//! 真实识别:用 whisper 模型把一段 WAV 走完整条识别路径(重采样 → 裁静音 → whisper)。
//! 需要模型和音频,默认跳过。运行方式:
//!   VG_TEST_MODEL=ggml-base.bin VG_TEST_WAV_EN=en.wav VG_TEST_WAV_ZH=zh.wav \
//!     cargo test -p vg-voice --features whisper --test whisper_real -- --nocapture
#![cfg(feature = "whisper")]

use std::path::PathBuf;

use vg_voice::whisper::WhisperEngine;
use vg_voice::{AudioClip, TranscribeOptions};

fn read_wav(path: &str) -> AudioClip {
    let mut reader = hound::WavReader::open(path).unwrap();
    let spec = reader.spec();
    let samples: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect();
    AudioClip { samples, sample_rate: spec.sample_rate }
}

fn engine() -> Option<WhisperEngine> {
    let Ok(model) = std::env::var("VG_TEST_MODEL") else {
        eprintln!("skipped: set VG_TEST_MODEL");
        return None;
    };
    Some(WhisperEngine { model: PathBuf::from(model) })
}

fn transcribe(engine: &WhisperEngine, clip: &AudioClip, language: &str) -> Option<String> {
    let opts = TranscribeOptions { language: language.into(), punctuation: true };
    let started = std::time::Instant::now();
    let text = vg_voice::transcribe(engine, clip, &opts).unwrap();
    // 不释放的话进程退出时 Metal 后端会 abort
    vg_voice::shutdown();
    eprintln!("[{language}] {:?} ({:.2}s)", text, started.elapsed().as_secs_f32());
    text
}

fn run(wav_var: &str, language: &str) -> Option<String> {
    let engine = engine()?;
    let Ok(wav) = std::env::var(wav_var) else {
        eprintln!("skipped: set {wav_var}");
        return None;
    };
    Some(transcribe(&engine, &read_wav(&wav), language).unwrap())
}

/// 确定性的伪随机白噪音(LCG),幅度 ±amp
fn white_noise(len: usize, amp: f32) -> Vec<f32> {
    let mut x: u32 = 0x9e37_79b9;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

/// 没有人声只有噪音 / 静音时必须返回 None,而不是把 Whisper 的幻听
/// ("请不吝点赞…"、复读提示词"再用简体中文"、"继续。")当成识别结果
#[test]
fn noise_and_silence_transcribe_to_nothing() {
    let Some(engine) = engine() else { return };
    let cases = [
        ("silence", vec![0.0; 48_000]),
        ("white noise 0.06", white_noise(48_000, 0.06)),
        ("white noise 0.02", white_noise(48_000, 0.02)),
    ];
    for (name, samples) in cases {
        let clip = AudioClip { samples, sample_rate: 16_000 };
        for language in ["zh", "en", "auto"] {
            let text = transcribe(&engine, &clip, language);
            assert!(text.is_none(), "{name} [{language}] produced {text:?}");
        }
    }
}

/// 反过来,叠了背景噪音的语音不能被当成噪音丢掉
#[test]
fn speech_over_noise_still_transcribes() {
    let Some(engine) = engine() else { return };
    let Ok(wav) = std::env::var("VG_TEST_WAV_ZH") else { return };
    let clean = read_wav(&wav).to_16k();
    // 前后各 1 s 噪音,语音段叠同样的噪音
    let mut samples = white_noise(16_000, 0.03);
    samples.extend(clean.samples.iter().zip(white_noise(clean.samples.len(), 0.03)).map(|(s, n)| s + n));
    samples.extend(white_noise(16_000, 0.03));
    let clip = AudioClip { samples, sample_rate: 16_000 };
    let text = transcribe(&engine, &clip, "zh").expect("speech should survive background noise");
    assert!(text.contains("看看") && text.contains("空间"), "{text}");
}

#[test]
fn transcribes_english() {
    if let Some(text) = run("VG_TEST_WAV_EN", "en") {
        let t = text.to_lowercase();
        assert!(t.contains("list") && t.contains("files"), "{text}");
    }
}

#[test]
fn transcribes_chinese_in_simplified() {
    // 只检查代码能左右的部分:出中文、是简体、句子主干在。同音字准不准取决于模型和音频,不归代码管——
    // 合成语音的「磁盘」:base 听成「随盘」,large-v3-turbo 听成「瓷盘」
    if let Some(text) = run("VG_TEST_WAV_ZH", "zh") {
        assert!(text.contains("看看") && text.contains("空间"), "{text}");
        assert!(!text.contains('盤') && !text.contains('間'), "should be simplified: {text}");
    }
}
