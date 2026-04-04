use anyhow::{Context as _, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};
use futures::Stream;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::sync::mpsc;

/// Required sample rate (16 kHz for VAD + transcription).
const SAMPLE_RATE: u32 = 16_000;
/// Required channel count.
const CHANNELS: u16 = 1;
/// Required sample format.
const SAMPLE_FORMAT: SampleFormat = SampleFormat::I16;
/// Maximum number of unread chunks buffered before new chunks are dropped.
const CHANNEL_CAPACITY: usize = 64;

fn hosts_help() -> String {
    let mut s = String::from("Audio host backend");
    let hosts = cpal::available_hosts();
    if !hosts.is_empty() {
        let default_name = cpal::default_host().id().name();
        s.push_str("\n\nAvailable:");
        for id in &hosts {
            let tag = if id.name() == default_name {
                " (default)"
            } else {
                ""
            };
            let _ = write!(s, "\n  {}{tag}", id.name());
        }
    }
    s
}

fn devices_help() -> String {
    let mut s = String::from("Audio input device (substring match)");
    let host = cpal::default_host();
    if let Ok(devices) = host.input_devices() {
        let default_id = host.default_input_device().and_then(|d| d.id().ok());
        s.push_str("\n\nDevices on default host:");
        let mut any = false;
        for device in devices {
            any = true;
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "<unknown>".into());
            let is_default = default_id
                .as_ref()
                .and_then(|did| device.id().ok().map(|id| id == *did))
                .unwrap_or(false);
            let tag = if is_default { " (default)" } else { "" };
            let _ = write!(s, "\n  {name}{tag}");
        }
        if !any {
            s.push_str("\n  (none)");
        }
    }
    s
}

#[derive(clap::Args, Debug)]
pub struct AudioArgs {
    /// Audio host backend
    #[arg(long, long_help = hosts_help())]
    pub audio_host: Option<String>,

    /// Audio input device (substring match)
    #[arg(long, env = "MIM_DEVICE", long_help = devices_help())]
    pub audio_device: Option<String>,

    /// VAD speech probability threshold (0.0–1.0)
    #[arg(long, default_value_t = 0.5)]
    pub vad_threshold: f32,

    /// Seconds without speech before ending a segment
    #[arg(long, default_value_t = 1.0)]
    pub vad_silence: f32,

    /// Path to the Silero VAD ONNX model.
    ///
    /// Use scripts/download-silero-vad.sh to fetch it.
    #[arg(long, env = "MIM_VAD_MODEL", default_value = "models/silero_vad.onnx")]
    pub vad_model: PathBuf,
}

/// Resolve a [`cpal::Host`] by name, or return the default.
pub fn resolve_host(name: Option<&str>) -> Result<cpal::Host> {
    match name {
        None => Ok(cpal::default_host()),
        Some(name) => {
            let id: cpal::HostId = name
                .parse()
                .map_err(|_| anyhow::anyhow!("unknown audio host \"{name}\""))?;
            cpal::host_from_id(id).context("audio host unavailable")
        }
    }
}

/// Resolve a [`cpal::Device`] on `host` by substring match, or return the default.
pub fn resolve_device(host: &cpal::Host, query: Option<&str>) -> Result<Device> {
    match query {
        None => host
            .default_input_device()
            .context("no default input device"),
        Some(query) => {
            let q = query.to_lowercase();
            let devices = host
                .input_devices()
                .context("failed to enumerate input devices")?;
            for device in devices {
                let name = device
                    .description()
                    .map(|d| d.name().to_string())
                    .unwrap_or_default();
                if name.to_lowercase().contains(&q) {
                    return Ok(device);
                }
            }
            anyhow::bail!("no input device matching \"{query}\"")
        }
    }
}

/// Mono i16 audio samples from a single cpal callback.
#[derive(Clone)]
pub struct AudioChunk {
    pub samples: Vec<i16>,
}

/// Captures 16 kHz mono i16 audio from an input device.
pub struct AudioCapture {
    device: Device,
    device_name: String,
}

impl AudioCapture {
    /// Build a capture for `device`.
    ///
    /// The device must support 16 kHz mono i16. No audio is captured until
    /// [`stream`](Self::stream) is called.
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

    /// Start capturing and return an async stream of [`AudioChunk`]s together
    /// with an [`AudioGuard`]. Audio flows for as long as the guard is alive;
    /// **dropping the guard stops capture and ends the stream** (the next
    /// `next().await` returns `None` after any buffered chunks have been
    /// drained).
    ///
    /// If the consumer falls too far behind, new chunks are dropped (logged
    /// at `debug` level) rather than buffered indefinitely.
    ///
    /// ```ignore
    /// use futures::StreamExt;
    /// let (mut audio, _guard) = AudioCapture::new(device)?.stream()?;
    /// while let Some(chunk) = audio.next().await {
    ///     // process chunk.samples ...
    /// }
    /// // drop(_guard) here -> capture stops, stream ends.
    /// ```
    pub fn stream(self) -> Result<(AudioStream, AudioGuard)> {
        let config_range = self
            .device
            .supported_input_configs()
            .context("failed to query supported input configs")?
            .find(|r| {
                r.channels() == CHANNELS
                    && r.sample_format() == SAMPLE_FORMAT
                    && r.min_sample_rate() <= SAMPLE_RATE
                    && SAMPLE_RATE <= r.max_sample_rate()
            })
            .with_context(|| {
                format!(
                    "device \"{}\" does not support {SAMPLE_RATE} Hz mono i16",
                    self.device_name
                )
            })?;

        let supported = config_range
            .try_with_sample_rate(SAMPLE_RATE)
            .context("sample rate not in range (bug)")?;

        let config: StreamConfig = supported.into();
        let (tx, rx) = mpsc::channel::<AudioChunk>(CHANNEL_CAPACITY);

        let stream = self
            .device
            .build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if tx
                        .try_send(AudioChunk {
                            samples: data.to_vec(),
                        })
                        .is_err()
                    {
                        tracing::debug!("audio channel full: dropped chunk");
                    }
                },
                |err| tracing::error!("audio capture error: {err}"),
                None,
            )
            .context("failed to build input stream")?;

        stream.play().context("failed to start audio stream")?;

        Ok((AudioStream { rx }, AudioGuard { _stream: stream }))
    }
}

/// Async stream of [`AudioChunk`]s produced by [`AudioCapture::stream`]. The
/// stream ends (yields `None`) once the matching [`AudioGuard`] is dropped
/// and any buffered chunks have been delivered.
pub struct AudioStream {
    rx: mpsc::Receiver<AudioChunk>,
}

impl Stream for AudioStream {
    type Item = AudioChunk;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// RAII handle that keeps the underlying cpal input stream alive. Drop it to
/// stop capture; the paired [`AudioStream`] will then yield `None` after any
/// remaining buffered chunks.
///
/// Dropping the guard drops the cpal stream, which drops the callback's
/// `Sender`, which causes [`AudioStream`] to end naturally — no manual
/// cancellation plumbing required.
pub struct AudioGuard {
    _stream: cpal::Stream,
}
