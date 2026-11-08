//! 麦克风采集(cpal,跨 macOS / Windows / Linux)。
//!
//! cpal 的 `Stream` 在 macOS 上不是 `Send`,所以流放在专用线程里创建和持有,主线程只通过
//! 通道发"停止"并拿回录音。录音期间实时更新电平,供界面画音量条。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::clip::{rms, AudioClip};

pub struct Recorder {
    stop_tx: mpsc::Sender<()>,
    handle: Option<JoinHandle<anyhow::Result<AudioClip>>>,
    level: Arc<AtomicU32>,
    /// 录到现在的全部采样(设备原生采样率),实时字幕边录边读
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

impl Recorder {
    /// 开始录音。`device_name` 为空用系统默认输入设备。
    pub fn start(device_name: &str) -> anyhow::Result<Recorder> {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<u32>>();
        let level = Arc::new(AtomicU32::new(0));
        let level_in_thread = level.clone();
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let samples_in_thread = samples.clone();
        let device_name = device_name.to_string();

        let handle = std::thread::Builder::new()
            .name("vg-recorder".into())
            .spawn(move || -> anyhow::Result<AudioClip> {
                let samples = samples_in_thread;
                let setup = build_stream(&device_name, samples.clone(), level_in_thread);
                let (stream, rate) = match setup {
                    Ok(v) => {
                        let _ = ready_tx.send(Ok(v.1));
                        v
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        let _ = ready_tx.send(Err(e));
                        return Err(anyhow!(msg));
                    }
                };
                // 阻塞到收到停止信号(或 Recorder 被丢弃导致通道断开)
                let _ = stop_rx.recv();
                drop(stream);
                let samples = std::mem::take(&mut *samples.lock().unwrap());
                Ok(AudioClip { samples, sample_rate: rate })
            })?;

        let sample_rate = ready_rx
            .recv()
            .map_err(|_| anyhow!("recorder thread exited unexpectedly"))??;
        Ok(Recorder { stop_tx, handle: Some(handle), level, samples, sample_rate })
    }

    /// 当前输入电平(RMS,0..1 左右),界面用
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// 录音还在进行时,拷一份到目前为止的采样。实时字幕用它反复识别"说到哪了"。
    pub fn snapshot(&self) -> AudioClip {
        let samples = self.samples.lock().unwrap().clone();
        AudioClip { samples, sample_rate: self.sample_rate }
    }

    /// 可以在录音期间低成本地共享给其它线程的采样句柄(与 [`Recorder::snapshot`] 读到的是同一份)
    pub fn live(&self) -> LiveAudio {
        LiveAudio { samples: self.samples.clone(), sample_rate: self.sample_rate }
    }

    /// 停止并取回整段录音
    pub fn stop(mut self) -> anyhow::Result<AudioClip> {
        let _ = self.stop_tx.send(());
        self.handle
            .take()
            .expect("stop called once")
            .join()
            .map_err(|_| anyhow!("recorder thread panicked"))?
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

/// 录音进行中可跨线程读取的采样视图(实时字幕线程用,不持有 cpal 流)
#[derive(Clone)]
pub struct LiveAudio {
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

impl LiveAudio {
    /// 目前录到的采样数
    pub fn len(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 最近 `max_secs` 秒的录音(不足则全部)
    pub fn tail(&self, max_secs: f32) -> AudioClip {
        let guard = self.samples.lock().unwrap();
        let keep = ((max_secs.max(0.0)) * self.sample_rate as f32) as usize;
        let start = guard.len().saturating_sub(keep);
        AudioClip { samples: guard[start..].to_vec(), sample_rate: self.sample_rate }
    }
}

/// 列出输入设备名(设置页下拉框用)
pub fn input_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

fn build_stream(
    device_name: &str,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
) -> anyhow::Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_input_device()
    } else {
        host.input_devices()?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
    }
    .context("no microphone found (check the input device and microphone permission)")?;

    let supported = device
        .default_input_config()
        .context("microphone has no usable input format")?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    // 多声道取平均下混成单声道,顺带更新电平
    let push = move |mono: &[f32]| {
        level.store(rms(mono).to_bits(), Ordering::Relaxed);
        samples.lock().unwrap().extend_from_slice(mono);
    };
    let err_fn = |e| log::warn!("audio stream error: {e}");

    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| push(&downmix(data, channels, |s| s)),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| push(&downmix(data, channels, |s| s as f32 / i16::MAX as f32)),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| {
                push(&downmix(data, channels, |s| (s as f32 - 32768.0) / 32768.0))
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported microphone sample format: {other:?}")),
    };
    stream.play().context("failed to start the microphone (permission denied?)")?;
    Ok((stream, rate))
}

fn downmix<T: Copy>(data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) -> Vec<f32> {
    if channels <= 1 {
        return data.iter().map(|s| to_f32(*s)).collect();
    }
    data.chunks(channels)
        .map(|frame| frame.iter().map(|s| to_f32(*s)).sum::<f32>() / frame.len() as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmixes_stereo() {
        let out = downmix(&[1.0f32, 0.0, 0.5, 0.5], 2, |s| s);
        assert_eq!(out, vec![0.5, 0.5]);
    }
}
