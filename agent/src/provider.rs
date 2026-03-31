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

pub trait Provider {
    type Error;

    fn create_response<'a>(
        &self,
        history: &[Entry],
        model: &str,
        tools: impl IntoIterator<Item = &'a Tool> + Send,
    ) -> impl Future<Output = ResponseResult<Self::Error>> + Send;
}

#[cfg(feature = "openai")]
pub mod openai;
