//! "轮到你了"提示音:状态灯变绿时响。用 cpal 输出流现场合成(两个泛音 + 指数衰减,近似玻璃声),
//! 各平台都不需要音频文件或额外依赖。播放在独立线程里,调用方立即返回。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const LENGTH_SECS: f32 = 0.9;

/// 第 `i` 个采样点(`rate` Hz)的振幅
pub fn sample(i: usize, rate: u32) -> f32 {
    let t = i as f32 / rate as f32;
    if t >= LENGTH_SECS {
        return 0.0;
    }
    let tau = std::f32::consts::TAU;
    // 5 ms 起音,避免"咔哒"声
    let attack = (t / 0.005).min(1.0);
    let body = (tau * 1318.5 * t).sin() * (-t * 5.0).exp() // E6
        + 0.5 * (tau * 1975.5 * t).sin() * (-t * 7.0).exp() // B6
        + 0.25 * (tau * 2637.0 * t).sin() * (-t * 10.0).exp(); // E7
    0.18 * attack * body
}

pub fn play() {
    std::thread::spawn(|| {
        if let Err(e) = play_blocking() {
            log::warn!("chime failed: {e:#}");
        }
    });
}

fn play_blocking() -> anyhow::Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no audio output device"))?;
    let supported = device.default_output_config()?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let cursor = Arc::new(AtomicUsize::new(0));

    macro_rules! stream {
        ($t:ty, $conv:expr) => {{
            let cursor = cursor.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [$t], _| {
                    for frame in data.chunks_mut(channels) {
                        let i = cursor.fetch_add(1, Ordering::Relaxed);
                        let v: $t = $conv(sample(i, rate));
                        frame.fill(v);
                    }
                },
                |e| log::warn!("chime stream error: {e}"),
                None,
            )?
        }};
    }
    let stream = match format {
        cpal::SampleFormat::F32 => stream!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => stream!(i16, |s: f32| (s * i16::MAX as f32) as i16),
        cpal::SampleFormat::U16 => stream!(u16, |s: f32| ((s + 1.0) * 32767.5) as u16),
        other => anyhow::bail!("unsupported output format {other:?}"),
    };
    stream.play()?;
    std::thread::sleep(Duration::from_secs_f32(LENGTH_SECS + 0.1));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_is_bounded_and_ends() {
        let rate = 48_000;
        let peak = (0..rate as usize).map(|i| sample(i, rate).abs()).fold(0.0, f32::max);
        assert!(peak > 0.05 && peak < 0.5, "peak {peak}");
        assert_eq!(sample(0, rate), 0.0);
        assert_eq!(sample(rate as usize, rate), 0.0);
    }
}
