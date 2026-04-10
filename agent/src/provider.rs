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

#[cfg(feature = "openai")]
pub mod openai;
