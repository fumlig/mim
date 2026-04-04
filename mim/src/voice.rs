use std::collections::VecDeque;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;

use crate::capture::AudioChunk;
use crate::silero::Silero;

const CHUNK_SIZE: usize = 512;
const SAMPLE_RATE: usize = 16_000;
/// Pre-speech padding kept in ring buffer (ms).
const SPEECH_PAD_MS: usize = 60;
/// Discard speech segments shorter than this (ms).
const MIN_SPEECH_MS: usize = 250;

/// Events emitted by the voice detector.
pub enum VoiceEvent {
    /// Speech onset detected.
    SpeechStart,
    /// Speech offset confirmed. Contains the complete speech segment.
    SpeechEnd(Vec<i16>),
}

/// Streams audio through Silero VAD and emits [`VoiceEvent`]s at speech
/// boundaries.
///
/// Once speech starts, the segment continues until no chunk has exceeded the
/// threshold for longer than `silence_duration`.
pub struct VoiceDetector {
    vad: Silero,
    threshold: f32,
    silence_samples: usize,
    min_speech_samples: usize,
    speech_pad_chunks: usize,

    pending: Vec<i16>,
    current_sample: usize,
    last_speech_at: usize,
    triggered: bool,
    segment: Vec<i16>,
    ring: VecDeque<Vec<i16>>,
}

impl VoiceDetector {
    /// Create a new detector.
    ///
    /// * `threshold` — probability at or above which a chunk counts as speech
    ///   (e.g. 0.5).
    /// * `silence_secs` — seconds without any speech chunk before ending a
    ///   segment (e.g. 0.5).
    /// * `model_path` — path to the Silero VAD ONNX file (see
    ///   `scripts/download-silero-vad.sh`).
    pub fn new(threshold: f32, silence_secs: f32, model_path: &Path) -> anyhow::Result<Self> {
        let vad = Silero::new(model_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to load silero model from {}: {e}\n\
                 hint: run scripts/download-silero-vad.sh, or pass --vad-model <path>",
                model_path.display()
            )
        })?;
        let silence_samples = ((silence_secs * SAMPLE_RATE as f32) as usize).max(CHUNK_SIZE);
        let min_speech_samples = MIN_SPEECH_MS * SAMPLE_RATE / 1000;
        let speech_pad_chunks = (SPEECH_PAD_MS * SAMPLE_RATE / 1000 + CHUNK_SIZE - 1) / CHUNK_SIZE;

        Ok(Self {
            vad,
            threshold,
            silence_samples,
            min_speech_samples,
            speech_pad_chunks,
            pending: Vec::with_capacity(CHUNK_SIZE),
            current_sample: 0,
            last_speech_at: 0,
            triggered: false,
            segment: Vec::new(),
            ring: VecDeque::with_capacity(speech_pad_chunks + 1),
        })
    }

    /// Consume the detector and a stream of [`AudioChunk`]s; return a stream
    /// of [`VoiceEvent`]s. The returned stream ends once the audio stream
    /// ends, after emitting any in-progress speech segment via `flush`.
    pub fn detect<S>(self, audio: S) -> VoiceStream<S>
    where
        S: Stream<Item = AudioChunk> + Unpin,
    {
        VoiceStream {
            detector: self,
            audio,
            pending: VecDeque::new(),
            audio_done: false,
        }
    }

    /// Feed samples from a capture chunk. Returns any events produced.
    fn feed(&mut self, samples: &[i16]) -> Vec<VoiceEvent> {
        let mut events = Vec::new();
        self.pending.extend_from_slice(samples);

        while self.pending.len() >= CHUNK_SIZE {
            let window: Vec<i16> = self.pending.drain(..CHUNK_SIZE).collect();
            let prob = self.vad.predict(&window).unwrap_or(0.0);
            self.current_sample += CHUNK_SIZE;

            let is_speech = prob >= self.threshold;
            if is_speech {
                self.last_speech_at = self.current_sample;
            }

            if !self.triggered {
                if is_speech {
                    self.triggered = true;
                    for chunk in self.ring.drain(..) {
                        self.segment.extend_from_slice(&chunk);
                    }
                    self.segment.extend_from_slice(&window);
                    events.push(VoiceEvent::SpeechStart);
                } else {
                    if self.ring.len() >= self.speech_pad_chunks {
                        self.ring.pop_front();
                    }
                    self.ring.push_back(window);
                }
            } else {
                self.segment.extend_from_slice(&window);
                if self.current_sample - self.last_speech_at >= self.silence_samples {
                    self.emit_end(&mut events);
                }
            }
        }

        events
    }

    /// Flush any in-progress speech segment (call when capture stops).
    fn flush(&mut self) -> Option<VoiceEvent> {
        if !self.pending.is_empty() {
            let mut window = std::mem::take(&mut self.pending);
            window.resize(CHUNK_SIZE, 0);
            let prob = self.vad.predict(&window).unwrap_or(0.0);
            self.current_sample += CHUNK_SIZE;

            if prob >= self.threshold {
                self.last_speech_at = self.current_sample;
                if !self.triggered {
                    self.triggered = true;
                    for chunk in self.ring.drain(..) {
                        self.segment.extend_from_slice(&chunk);
                    }
                }
            }
            if self.triggered {
                self.segment.extend_from_slice(&window);
            }
        }

        if !self.triggered || self.segment.len() < self.min_speech_samples {
            self.reset();
            return None;
        }

        let samples = std::mem::take(&mut self.segment);
        self.reset();
        Some(VoiceEvent::SpeechEnd(samples))
    }

    /// Reset all state.
    fn reset(&mut self) {
        self.vad.reset();
        self.pending.clear();
        self.segment.clear();
        self.ring.clear();
        self.triggered = false;
        self.last_speech_at = 0;
        self.current_sample = 0;
    }

    fn emit_end(&mut self, events: &mut Vec<VoiceEvent>) {
        if self.segment.len() >= self.min_speech_samples {
            events.push(VoiceEvent::SpeechEnd(std::mem::take(&mut self.segment)));
        } else {
            self.segment.clear();
        }
        self.triggered = false;
        self.last_speech_at = 0;
        self.ring.clear();
        self.vad.reset();
    }
}

/// Async stream of [`VoiceEvent`]s produced by [`VoiceDetector::detect`].
///
/// Ends once the underlying audio stream ends. The final `flush` is called
/// automatically, so any in-progress speech segment is emitted as a trailing
/// [`VoiceEvent::SpeechEnd`] before the stream terminates.
pub struct VoiceStream<S> {
    detector: VoiceDetector,
    audio: S,
    pending: VecDeque<VoiceEvent>,
    audio_done: bool,
}

impl<S> Stream for VoiceStream<S>
where
    S: Stream<Item = AudioChunk> + Unpin,
{
    type Item = VoiceEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // `VoiceStream<S>` is auto-`Unpin` because every field is `Unpin`
        // (including `S` via the bound), so `get_mut` is safe without any
        // pin projection.
        let this = self.get_mut();
        loop {
            // 1. Drain events already produced by a previous feed/flush.
            if let Some(event) = this.pending.pop_front() {
                return Poll::Ready(Some(event));
            }
            // 2. Audio exhausted and queue drained -> we're done.
            if this.audio_done {
                return Poll::Ready(None);
            }
            // 3. Pull the next audio chunk.
            match Pin::new(&mut this.audio).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(chunk)) => {
                    this.pending.extend(this.detector.feed(&chunk.samples));
                }
                Poll::Ready(None) => {
                    this.audio_done = true;
                    if let Some(event) = this.detector.flush() {
                        this.pending.push_back(event);
                    }
                }
            }
        }
    }
}
