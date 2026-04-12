use crate::entry::{Entry, Message, Reasoning, ToolCall, ToolResult};
use crate::tool::Tool;
use std::{future::Future, pin::Pin};

#[derive(Debug, Clone)]
pub enum ResponseEvent {
    TextDelta(String),
    TextDone(Message),
    ReasoningDelta(String),
    ReasoningDone(Reasoning),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
}

pub type ResponseStream<Error> =
    Pin<Box<dyn futures::Stream<Item = Result<ResponseEvent, Error>> + Send>>;

pub type ResponseResult<Error> = Result<ResponseStream<Error>, Error>;

pub trait ResponseProvider {
    type Error;

    fn create_response<'a>(
        &'a self,
        history: &'a [Entry],
        model: &'a str,
        tools: impl IntoIterator<Item = &'a Tool> + Send + 'a,
    ) -> Pin<Box<dyn Future<Output = ResponseResult<Self::Error>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub enum TranscriptionEvent {
    Delta(String),
    Done(String),
}

pub type TranscriptionStream<Error> =
    Pin<Box<dyn futures::Stream<Item = Result<TranscriptionEvent, Error>> + Send>>;

pub type TranscriptionResult<Error> = Result<TranscriptionStream<Error>, Error>;

pub trait TranscriptionProvider {
    type Error;

    /// `samples` are mono PCM `i16` at `sample_rate` Hz. Implementations are
    /// free to encode them into whatever container they need before handing
    /// them to the remote service.
    fn create_transcription<'a>(
        &'a self,
        samples: &'a [i16],
        sample_rate: u32,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = TranscriptionResult<Self::Error>> + Send + 'a>>;
}

/// A chunk of audio produced by a speech synthesis provider.
#[derive(Debug, Clone)]
pub enum SpeechEvent {
    /// Raw audio bytes (encoding depends on the requested format; typically
    /// PCM 16-bit LE at 24 kHz mono when using the OpenAI provider).
    Delta(Vec<u8>),
    /// Synthesis is complete; no more deltas will follow.
    Done,
}

pub type SpeechStream<Error> =
    Pin<Box<dyn futures::Stream<Item = Result<SpeechEvent, Error>> + Send>>;

pub type SpeechResult<Error> = Result<SpeechStream<Error>, Error>;

/// Provider that turns text into a stream of audio chunks.
pub trait SpeechProvider {
    type Error;

    /// Synthesize `text` into audio, streaming chunks as they become
    /// available.
    ///
    /// * `model` — TTS model identifier (e.g. `"gpt-4o-mini-tts"`).
    /// * `voice` — voice name (e.g. `"alloy"`, `"nova"`).
    /// * `instructions` — optional style/persona instructions for the voice.
    fn create_speech<'a>(
        &'a self,
        text: &'a str,
        model: &'a str,
        voice: &'a str,
        instructions: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = SpeechResult<Self::Error>> + Send + 'a>>;
}

#[cfg(feature = "openai")]
pub mod openai;
