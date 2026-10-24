//! Microphone capture with cpal. Samples are downmixed to mono f32 and later
//! resampled to the configured 16 kHz working rate.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};

pub fn input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    for device in host.input_devices().context("enumerating input devices")? {
        if let Ok(name) = device.name() {
            names.push(name);
        }
    }
    Ok(names)
}

fn pick_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if name.trim().is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device"));
    }
    let needle = name.to_ascii_lowercase();
    let mut fallback = None;
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            let Ok(dev_name) = device.name() else {
                continue;
            };
            if dev_name.to_ascii_lowercase().contains(&needle) {
                return Ok(device);
            }
            fallback.get_or_insert(device);
        }
    }
    fallback.ok_or_else(|| anyhow!("no input device matched {name:?}"))
}

pub struct Recorder {
    /// Kept alive for the lifetime of the recorder.
    _stream: Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    active: Arc<AtomicBool>,
    pub sample_rate: u32,
    pub device_name: String,
}

impl Recorder {
    /// Open the device and start the stream immediately. Keeping the stream
    /// warm removes the 0.4–1.2 s CoreAudio startup delay from the first word
    /// of a dictation; samples are discarded until [`Recorder::begin`].
    pub fn open(device_name: &str, level: Arc<AtomicU32>) -> Result<Self> {
        let device = pick_device(device_name)?;
        let resolved_name = device.name().unwrap_or_default();
        let supported = device
            .default_input_config()
            .context("querying the default input configuration")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let channels = config.channels.max(1) as usize;
        let sample_rate = config.sample_rate.0;
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(sample_rate as usize * 10)));
        let active = Arc::new(AtomicBool::new(false));

        let err_fn = |err| log_line!("[rustype] audio stream error: {err}");
        let stream = match sample_format {
            SampleFormat::F32 => build::<f32>(
                &device,
                &config,
                buffer.clone(),
                active.clone(),
                level.clone(),
                channels,
                err_fn,
            )?,
            SampleFormat::I16 => build::<i16>(
                &device,
                &config,
                buffer.clone(),
                active.clone(),
                level.clone(),
                channels,
                err_fn,
            )?,
            SampleFormat::U16 => build::<u16>(
                &device,
                &config,
                buffer.clone(),
                active.clone(),
                level.clone(),
                channels,
                err_fn,
            )?,
            other => bail!("unsupported sample format {other:?}"),
        };
        stream.play().context("starting the input stream")?;
        Ok(Self {
            _stream: stream,
            buffer,
            active,
            sample_rate,
            device_name: resolved_name,
        })
    }

    /// Begin collecting samples, discarding anything buffered while idle.
    pub fn begin(&self) {
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.clear();
            self.active.store(true, Ordering::SeqCst);
        }
    }

    /// Stop collecting and take the utterance.
    pub fn end(&self) -> (Vec<f32>, u32) {
        self.active.store(false, Ordering::SeqCst);
        let samples = self
            .buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        (samples, self.sample_rate)
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    active: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    channels: usize,
    err_fn: fn(cpal::StreamError),
) -> Result<Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if !active.load(Ordering::Relaxed) {
                    return;
                }
                let mut out = match buffer.lock() {
                    Ok(b) => b,
                    Err(_) => return,
                };
                let mut energy = 0.0f64;
                let mut frames = 0usize;
                for frame in data.chunks(channels.max(1)) {
                    let mut sum = 0.0f32;
                    for sample in frame {
                        sum += Sample::to_sample::<f32>(*sample);
                    }
                    let mono = sum / frame.len() as f32;
                    out.push(mono);
                    energy += (mono as f64) * (mono as f64);
                    frames += 1;
                }
                if frames > 0 {
                    let rms = (energy / frames as f64).sqrt() as f32;
                    // Perceptual mapping so quiet speech still moves the bars.
                    let scaled = (rms * 25.0).sqrt().clamp(0.0, 1.0);
                    level.store((scaled * 1000.0) as u32, Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        )
        .context("building the input stream")?;
    Ok(stream)
}
