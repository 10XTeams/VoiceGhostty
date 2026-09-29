//! 一段录音及其预处理:转 16 kHz 单声道、裁掉首尾静音、编码 WAV。
//! 所有识别引擎都吃 16 kHz 单声道(Whisper 系的原生采样率)。

pub const TARGET_RATE: u32 = 16_000;
/// 静音 ≈ 0.002,说话 ≈ 0.05+(沿用 Swift 版实测阈值)
pub const RMS_THRESHOLD: f32 = 0.01;

#[derive(Debug, Clone, Default)]
pub struct AudioClip {
    /// 单声道 f32,[-1, 1]
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioClip {
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// 线性插值重采样到 16 kHz。语音识别对这点失真不敏感,不值得引入重型重采样库。
    pub fn to_16k(&self) -> AudioClip {
        if self.sample_rate == TARGET_RATE || self.samples.is_empty() || self.sample_rate == 0 {
            return self.clone();
        }
        let ratio = self.sample_rate as f64 / TARGET_RATE as f64;
        let out_len = (self.samples.len() as f64 / ratio).floor() as usize;
        let last = self.samples.len() - 1;
        let samples = (0..out_len)
            .map(|i| {
                let pos = i as f64 * ratio;
                let idx = pos.floor() as usize;
                let frac = (pos - idx as f64) as f32;
                let a = self.samples[idx.min(last)];
                let b = self.samples[(idx + 1).min(last)];
                a + (b - a) * frac
            })
            .collect();
        AudioClip { samples, sample_rate: TARGET_RATE }
    }

    /// 以 20 ms 为窗,裁掉首尾 RMS 低于阈值的部分,两端各留 200 ms 余量(避免吞掉起音)。
    /// 全程静音返回 None——调用方据此直接提示"没听到声音",不必白跑一次识别。
    pub fn trim_silence(&self) -> Option<AudioClip> {
        let window = (self.sample_rate as usize / 50).max(1);
        let voiced: Vec<bool> = self
            .samples
            .chunks(window)
            .map(|w| rms(w) >= RMS_THRESHOLD)
            .collect();
        let first = voiced.iter().position(|v| *v)?;
        let last = voiced.iter().rposition(|v| *v)?;
        let pad = 10; // 10 个窗 = 200 ms
        let start = first.saturating_sub(pad) * window;
        let end = ((last + 1 + pad) * window).min(self.samples.len());
        Some(AudioClip {
            samples: self.samples[start..end].to_vec(),
            sample_rate: self.sample_rate,
        })
    }

    /// 16-bit PCM WAV(HTTP 上传 / 外部命令都用它)
    pub fn to_wav_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
            for s in &self.samples {
                writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
            }
            writer.finalize()?;
        }
        Ok(cursor.into_inner())
    }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(len: usize, amp: f32) -> Vec<f32> {
        (0..len).map(|i| amp * (i as f32 * 0.3).sin()).collect()
    }

    #[test]
    fn resamples_48k_to_16k() {
        let clip = AudioClip { samples: vec![0.5; 48_000], sample_rate: 48_000 };
        let out = clip.to_16k();
        assert_eq!(out.sample_rate, 16_000);
        assert_eq!(out.samples.len(), 16_000);
        assert!((out.duration_secs() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn trims_leading_and_trailing_silence() {
        let mut samples = vec![0.0; 16_000];
        samples.extend(tone(8_000, 0.3));
        samples.extend(vec![0.0; 16_000]);
        let clip = AudioClip { samples, sample_rate: 16_000 };
        let trimmed = clip.trim_silence().unwrap();
        // 0.5 s 语音 + 两端各 0.2 s 余量
        assert!((trimmed.duration_secs() - 0.9).abs() < 0.05, "{}", trimmed.duration_secs());
    }

    #[test]
    fn all_silence_is_none() {
        let clip = AudioClip { samples: vec![0.001; 16_000], sample_rate: 16_000 };
        assert!(clip.trim_silence().is_none());
    }

    #[test]
    fn encodes_wav() {
        let clip = AudioClip { samples: tone(1600, 0.5), sample_rate: 16_000 };
        let bytes = clip.to_wav_bytes().unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 1600 * 2);
    }
}
