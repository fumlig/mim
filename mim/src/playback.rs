use anyhow::{Context as _, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Required sample rate (16 kHz to match capture).
const SAMPLE_RATE: u32 = 16_000;
/// Required channel count.
const CHANNELS: u16 = 1;
/// Required sample format.
const SAMPLE_FORMAT: SampleFormat = SampleFormat::I16;

/// Resolve a [`cpal::Device`] output device on `host` by substring match,
/// or return the default.
pub fn resolve_output_device(host: &cpal::Host, query: Option<&str>) -> Result<Device> {
    match query {
        None => host
            .default_output_device()
            .context("no default output device"),
        Some(query) => {
            let q = query.to_lowercase();
            let devices = host
                .output_devices()
                .context("failed to enumerate output devices")?;
            for device in devices {
                let name = device
                    .description()
                    .map(|d| d.name().to_string())
                    .unwrap_or_default();
                if name.to_lowercase().contains(&q) {
                    return Ok(device);
                }
            }
            anyhow::bail!("no output device matching \"{query}\"")
        }
    }
}

/// Plays back 16 kHz mono i16 audio through an output device.
///
/// Audio is submitted via [`PlaybackSender`] and queued for immediate
/// playback. When the queue is empty, silence is output.
pub struct AudioPlayback {
    device: Device,
    device_name: String,
}

impl AudioPlayback {
    /// Build a playback handle for `device`.
    ///
    /// The device must support 16 kHz mono i16. No audio is played until
    /// [`start`](Self::start) is called.
    pub fn new(device: Device) -> Result<Self> {
        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "<unknown>".into());
        Ok(Self {
            device,
            device_name,
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Start the output stream and return a [`PlaybackSender`] for queuing
    /// audio together with a [`PlaybackGuard`]. Audio flows for as long as
    /// the guard is alive; **dropping the guard stops playback**.
    ///
    /// Submit samples via [`PlaybackSender::send`]; they are appended to an
    /// internal queue and played back in order. When the queue is empty, the
    /// stream outputs silence.
    ///
    /// ```ignore
    /// let (sender, _guard) = AudioPlayback::new(device)?.start()?;
    /// sender.send(&my_samples);
    /// // audio plays immediately
    /// // drop(_guard) stops playback
    /// ```
    pub fn start(self) -> Result<(PlaybackSender, PlaybackGuard)> {
        let config_range = self
            .device
            .supported_output_configs()
            .context("failed to query supported output configs")?
            .find(|r| {
                r.channels() == CHANNELS
                    && r.sample_format() == SAMPLE_FORMAT
                    && r.min_sample_rate() <= SAMPLE_RATE
                    && SAMPLE_RATE <= r.max_sample_rate()
            })
            .with_context(|| {
                format!(
                    "device \"{}\" does not support {SAMPLE_RATE} Hz mono i16 output",
                    self.device_name
                )
            })?;

        let supported = config_range
            .try_with_sample_rate(SAMPLE_RATE)
            .context("sample rate not in range (bug)")?;

        let config: StreamConfig = supported.into();
        let queue: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
        let playback_queue = Arc::clone(&queue);

        let stream = self
            .device
            .build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut q = playback_queue.lock().unwrap();
                    for sample in data.iter_mut() {
                        *sample = q.pop_front().unwrap_or(0);
                    }
                },
                |err| tracing::error!("audio playback error: {err}"),
                None,
            )
            .context("failed to build output stream")?;

        stream.play().context("failed to start playback stream")?;

        Ok((PlaybackSender { queue }, PlaybackGuard { _stream: stream }))
    }
}

/// Handle for submitting audio samples to the playback queue.
///
/// Cloneable — multiple producers can queue audio concurrently.
/// Samples are played back in the order they are queued (FIFO).
#[derive(Clone)]
pub struct PlaybackSender {
    queue: Arc<Mutex<VecDeque<i16>>>,
}

impl PlaybackSender {
    /// Append `samples` to the playback queue. They will be played
    /// immediately after any previously queued samples.
    pub fn send(&self, samples: &[i16]) {
        let mut q = self.queue.lock().unwrap();
        q.extend(samples.iter().copied());
    }

    /// Discard all queued samples, effectively silencing pending playback.
    pub fn clear(&self) {
        let mut q = self.queue.lock().unwrap();
        q.clear();
    }

    /// Returns the number of samples currently queued.
    pub fn queued(&self) -> usize {
        self.queue.lock().unwrap().len()
    }
}

/// RAII handle that keeps the underlying cpal output stream alive. Drop it to
/// stop playback.
///
/// Dropping the guard drops the cpal stream, which stops the output callback.
pub struct PlaybackGuard {
    _stream: cpal::Stream,
}
