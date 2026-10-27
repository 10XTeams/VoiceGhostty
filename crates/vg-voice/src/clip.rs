//! 一段录音及其预处理:转 16 kHz 单声道、裁掉首尾静音、编码 WAV。
//! 所有识别引擎都吃 16 kHz 单声道(Whisper 系的原生采样率)。

pub const TARGET_RATE: u32 = 16_000;
/// 静音 ≈ 0.002,说话 ≈ 0.05+(沿用 Swift 版实测阈值)。这是绝对下限,实际阈值还会随本底噪声抬高
pub const RMS_THRESHOLD: f32 = 0.01;
/// 电平分析的窗长(ms)
const WINDOW_MS: usize = 20;
/// 有声窗口至少要比本底噪声响这么多倍(约 6 dB),否则只是噪声自身的起伏
const ABOVE_FLOOR: f32 = 2.0;
/// 判定"有人说话"所需的有声累计时长(ms)。一次键盘敲击、碰一下麦克风只有 20–50 ms
const MIN_VOICED_MS: usize = 100;
/// 最响的窗口与本底(20 分位)之比低于此值,说明整段电平是平的:风扇、空调、白噪音这类
/// 稳态噪声,而不是有起伏的语音。白噪音 20 ms 窗的 RMS 起伏只有百分之几,语音通常 10 倍以上
const MIN_DYNAMIC_RANGE: f32 = 2.0;

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

    /// 以 20 ms 为窗,裁掉首尾没有人声的部分,两端各留 200 ms 余量(避免吞掉起音)。
    /// 全程静音、只有稳态噪声、或有声部分短得像一次误触时返回 None——调用方据此直接提示
    /// "没听到声音",不必白跑一次识别(Whisper 拿到纯噪音会"幻听"出字幕套话)。
    pub fn trim_silence(&self) -> Option<AudioClip> {
        let window = (self.sample_rate as usize * WINDOW_MS / 1000).max(1);
        let levels: Vec<f32> = self.samples.chunks(window).map(rms).collect();
        let voiced = speech_mask(&levels)?;
        let first = voiced.iter().position(|v| *v)?;
        let last = voiced.iter().rposition(|v| *v)?;
        let pad = 200 / WINDOW_MS; // 200 ms
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

/// 逐窗判断哪些窗口有人声(`levels` 是每个 20 ms 窗的 RMS)。
/// 判不出人声时返回 None:全程静音;整段电平平直(稳态噪声);有声窗口累计不足 100 ms。
/// 阈值不是写死的:以 20 分位电平当本底噪声,有声窗口要高出本底一截,这样风扇声、
/// 白噪音把整体电平抬过绝对阈值时也不会被当成语音。
pub(crate) fn speech_mask(levels: &[f32]) -> Option<Vec<bool>> {
    if levels.is_empty() {
        return None;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[(sorted.len() - 1) / 5];
    let peak = sorted[sorted.len() - 1];
    if peak < RMS_THRESHOLD {
        log::debug!("voice: all silence (peak rms {peak:.4})");
        return None;
    }
    if peak < floor * MIN_DYNAMIC_RANGE {
        log::info!("voice: steady noise, not speech (floor {floor:.4}, peak {peak:.4})");
        return None;
    }
    let threshold = RMS_THRESHOLD.max(floor * ABOVE_FLOOR);
    let mask: Vec<bool> = levels.iter().map(|l| *l >= threshold).collect();
    let voiced_ms = mask.iter().filter(|v| **v).count() * WINDOW_MS;
    if voiced_ms < MIN_VOICED_MS {
        log::info!("voice: only {voiced_ms} ms above {threshold:.4}, treating as a click");
        return None;
    }
    Some(mask)
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

    /// 确定性的伪随机白噪音(LCG),幅度 ±amp
    fn white_noise(len: usize, amp: f32) -> Vec<f32> {
        let mut x: u32 = 0x1234_5678;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    #[test]
    fn steady_noise_above_threshold_is_none() {
        // 白噪音 RMS ≈ amp/√3 ≈ 0.035,远高于绝对阈值 0.01,但电平是平的
        let clip = AudioClip { samples: white_noise(3 * 16_000, 0.06), sample_rate: 16_000 };
        assert!(rms(&clip.samples) > RMS_THRESHOLD);
        assert!(clip.trim_silence().is_none());
    }

    #[test]
    fn speech_over_noise_is_kept() {
        // 1 s 噪音 + 0.5 s 噪音上叠语音 + 1 s 噪音
        let noise = white_noise(40_000, 0.03);
        let mut samples = noise[..16_000].to_vec();
        samples.extend(noise[16_000..24_000].iter().zip(tone(8_000, 0.3)).map(|(n, t)| n + t));
        samples.extend(&noise[24_000..]);
        let clip = AudioClip { samples, sample_rate: 16_000 };
        let trimmed = clip.trim_silence().expect("speech should survive background noise");
        assert!((trimmed.duration_secs() - 0.9).abs() < 0.05, "{}", trimmed.duration_secs());
    }

    #[test]
    fn short_word_in_long_hold_is_kept() {
        // 按住 6 s 只说了 0.3 s:有声部分占比很小,也不能当成噪声丢掉
        let mut samples = vec![0.0; 3 * 16_000];
        samples.extend(tone(4_800, 0.2));
        samples.extend(vec![0.0; 3 * 16_000]);
        let clip = AudioClip { samples, sample_rate: 16_000 };
        assert!(clip.trim_silence().is_some());
    }

    #[test]
    fn single_click_is_none() {
        // 30 ms 的敲击声
        let mut samples = vec![0.0; 16_000];
        samples.extend(tone(480, 0.5));
        samples.extend(vec![0.0; 16_000]);
        let clip = AudioClip { samples, sample_rate: 16_000 };
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
