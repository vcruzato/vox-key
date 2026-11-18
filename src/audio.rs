/// Average interleaved frames of `channels` into mono. `channels <= 1` is a passthrough.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let p = std::f64::consts::PI * x;
        p.sin() / p
    }
}

/// Resample mono `input` from `in_rate` to 16 kHz using a windowed-sinc (Lanczos)
/// kernel. When downsampling, the cutoff is lowered to the output Nyquist to
/// suppress aliasing (the kernel widens accordingly). Output gain is normalized
/// by the sum of kernel weights, so DC and low frequencies pass at unity.
pub fn resample_to_16k(input: &[f32], in_rate: u32) -> Vec<f32> {
    const OUT_RATE: u32 = 16_000;
    const A: f64 = 16.0; // Lanczos lobes (kernel taps per side, before cutoff widening)
    if input.is_empty() {
        return Vec::new();
    }
    if in_rate == OUT_RATE {
        return input.to_vec();
    }
    let ratio = OUT_RATE as f64 / in_rate as f64; // < 1.0 when downsampling
    let cutoff = ratio.min(1.0);
    let half = A / cutoff; // kernel radius in input samples
    let n = input.len() as i64;
    let out_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let t = i as f64 / ratio; // position in input-sample space
        let left = (t - half).ceil() as i64;
        let right = (t + half).floor() as i64;
        let mut acc = 0.0f64;
        let mut wsum = 0.0f64;
        for j in left..=right {
            if j < 0 || j >= n {
                continue;
            }
            let u = cutoff * (j as f64 - t);
            if u.abs() > A {
                continue;
            }
            let w = sinc(u) * sinc(u / A); // Lanczos = sinc(u) * sinc(u/A)
            acc += input[j as usize] as f64 * w;
            wsum += w;
        }
        out.push(if wsum.abs() > 1e-12 {
            (acc / wsum) as f32
        } else {
            0.0
        });
    }
    out
}

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct Recorder {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>, // mono f32 at the device's native rate
    stream: Option<cpal::Stream>,
    sample_rate: u32,
    channels: u16,
    max_samples: usize,
}

impl Recorder {
    pub fn new(device_name: &str, max_seconds: u32) -> Result<Self> {
        let host = cpal::default_host();
        let device = pick_device(&host, device_name)?;
        let config = device
            .default_input_config()
            .context("querying default input config")?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        tracing::info!(
            device = %device,
            sample_rate,
            channels,
            format = ?config.sample_format(),
            "audio input"
        );
        Ok(Self {
            device,
            config,
            buffer: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            sample_rate,
            channels,
            max_samples: (max_seconds as usize) * (sample_rate as usize),
        })
    }

    pub fn start(&mut self) -> Result<()> {
        self.buffer.lock().unwrap().clear();
        let buf = self.buffer.clone();
        let channels = self.channels;
        let max = self.max_samples;
        let cfg: cpal::StreamConfig = self.config.config();
        let err_fn = |e| tracing::error!(error = %e, "audio stream error");

        let stream = match self.config.sample_format() {
            cpal::SampleFormat::F32 => self.device.build_input_stream(
                cfg,
                move |d: &[f32], _: &cpal::InputCallbackInfo| push(&buf, d, channels, max, |s| s),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => self.device.build_input_stream(
                cfg,
                move |d: &[i16], _: &cpal::InputCallbackInfo| {
                    push(&buf, d, channels, max, |s| s as f32 / 32768.0)
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I32 => self.device.build_input_stream(
                cfg,
                move |d: &[i32], _: &cpal::InputCallbackInfo| {
                    push(&buf, d, channels, max, |s| s as f32 / 2_147_483_648.0)
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => self.device.build_input_stream(
                cfg,
                move |d: &[u16], _: &cpal::InputCallbackInfo| {
                    push(&buf, d, channels, max, |s| (s as f32 - 32768.0) / 32768.0)
                },
                err_fn,
                None,
            )?,
            other => anyhow::bail!("unsupported sample format: {other:?}"),
        };
        stream.play().context("starting input stream")?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Stops capture and returns the recording resampled to 16 kHz mono f32.
    pub fn stop(&mut self) -> Vec<f32> {
        self.stream.take(); // dropping the stream stops capture
        let mono = std::mem::take(&mut *self.buffer.lock().unwrap());
        resample_to_16k(&mono, self.sample_rate)
    }
}

/// Downmix one callback's interleaved frames to mono f32 and append, capped at `max`.
fn push<T: Copy>(
    buf: &Arc<Mutex<Vec<f32>>>,
    data: &[T],
    channels: u16,
    max: usize,
    conv: impl Fn(T) -> f32,
) {
    let ch = channels.max(1) as usize;
    let mut b = buf.lock().unwrap();
    for frame in data.chunks_exact(ch) {
        if b.len() >= max {
            break;
        }
        let sum: f32 = frame.iter().map(|&s| conv(s)).sum();
        b.push(sum / ch as f32);
    }
}

fn pick_device(host: &cpal::Host, name: &str) -> Result<cpal::Device> {
    if name == "default" || name.is_empty() {
        return host
            .default_input_device()
            .context("no default input device");
    }
    for dev in host.input_devices().context("listing input devices")? {
        if dev.to_string().contains(name) {
            return Ok(dev);
        }
    }
    anyhow::bail!("no input device matching '{name}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_stereo_averages_pairs() {
        assert_eq!(downmix_to_mono(&[1.0, 3.0, 2.0, 4.0], 2), vec![2.0, 3.0]);
    }

    #[test]
    fn downmix_mono_passthrough() {
        assert_eq!(downmix_to_mono(&[1.0, 2.0, 3.0], 1), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let x = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_to_16k(&x, 16000), x);
    }

    #[test]
    fn resample_empty_is_empty() {
        assert!(resample_to_16k(&[], 48000).is_empty());
    }

    #[test]
    fn resample_48k_to_16k_thirds_the_length() {
        let x = vec![0.0f32; 4800]; // 0.1 s at 48 kHz
        let y = resample_to_16k(&x, 48000);
        assert!((y.len() as i64 - 1600).abs() <= 2, "len was {}", y.len());
    }

    #[test]
    fn resample_preserves_dc_level() {
        let x = vec![0.5f32; 4800];
        let y = resample_to_16k(&x, 48000);
        // interior samples should be ~0.5 (ignore the first/last few edge samples)
        let mid = &y[10..y.len() - 10];
        for &v in mid {
            assert!((v - 0.5).abs() < 0.02, "got {v}");
        }
    }
}
