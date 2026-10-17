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

fn run(wav_var: &str, language: &str) -> Option<String> {
    let (Ok(model), Ok(wav)) = (std::env::var("VG_TEST_MODEL"), std::env::var(wav_var)) else {
        eprintln!("skipped: set VG_TEST_MODEL and {wav_var}");
        return None;
    };
    let engine = WhisperEngine { model: PathBuf::from(model) };
    let opts = TranscribeOptions { language: language.into(), punctuation: true };
    let started = std::time::Instant::now();
    let text = vg_voice::transcribe(&engine, &read_wav(&wav), &opts).unwrap().unwrap();
    // 不释放的话进程退出时 Metal 后端会 abort
    vg_voice::shutdown();
    eprintln!("[{language}] {:?} ({:.2}s)", text, started.elapsed().as_secs_f32());
    Some(text)
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
