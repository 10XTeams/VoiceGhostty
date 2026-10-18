//! 用内置 whisper 引擎识别一个 WAV 文件,并打印每次识别的耗时。量延迟用:
//!
//! ```sh
//! cargo run --release -p vg-voice --features whisper-metal --example transcribe -- <model.bin> <audio.wav> [次数]
//! ```

#[cfg(feature = "whisper")]
fn main() -> anyhow::Result<()> {
    use std::time::Instant;
    use vg_voice::{AudioClip, TranscribeOptions};

    let mut args = std::env::args().skip(1);
    let model = std::path::PathBuf::from(args.next().expect("usage: transcribe <model.bin> <audio.wav> [runs]"));
    let wav = args.next().expect("usage: transcribe <model.bin> <audio.wav> [runs]");
    let runs: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);

    let reader = hound::WavReader::open(&wav)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader.into_samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect(),
        hound::SampleFormat::Float => reader.into_samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    let clip = AudioClip { samples, sample_rate: spec.sample_rate };
    let engine = vg_voice::whisper::WhisperEngine { model: model.clone() };
    let opts = TranscribeOptions { language: "zh".into(), punctuation: true };

    let t = Instant::now();
    vg_voice::whisper::preload(&model);
    // preload 在后台线程;第一次 transcribe 会等它完成
    for i in 1..=runs {
        let t = Instant::now();
        let text = vg_voice::transcribe(&engine, &clip, &opts)?;
        println!("#{i}: {:>5} ms  {}", t.elapsed().as_millis(), text.unwrap_or_default());
    }
    println!("total incl. model load: {:.2}s", t.elapsed().as_secs_f32());
    vg_voice::whisper::unload();
    Ok(())
}

#[cfg(not(feature = "whisper"))]
fn main() {
    eprintln!("build with --features whisper (or whisper-metal)");
}
