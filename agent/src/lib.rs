pub mod entry;
pub mod provider;
pub mod session;
pub mod tool;

use entry::{Entry, Message, MessageContent, Role, ToolResult};
use futures::StreamExt;
use provider::{ResponseEvent, ResponseProvider, TranscriptionEvent, TranscriptionProvider};
use session::Session;
use std::collections::HashMap;
use tool::Tool;

pub use tokio_util::sync::CancellationToken as Cancel;

/// A single turn's input to [`Agent::run`].
///
/// Text inputs are used as-is. Audio inputs are transcribed first (with
/// partial transcription events surfaced through `on_event`) and the
/// final transcription is then fed into the model exactly like a text
/// input would be.
pub enum Input {
    Text(String),
    /// Mono PCM samples at 16 kHz.
    Audio(Vec<i16>),
}

impl From<String> for Input {
    fn from(text: String) -> Self {
        Input::Text(text)
    }
}

impl From<&str> for Input {
    fn from(text: &str) -> Self {
        Input::Text(text.to_string())
    }
}

/// Anything the agent emits while a turn is in flight.
///
/// Each turn produces a sequence of [`OutputEvent`]s. Audio turns may emit
/// [`OutputEvent::Transcription`] events first, followed by
/// [`OutputEvent::Response`] events from the model. Text turns only emit
/// the latter.
#[derive(Debug, Clone)]
pub enum OutputEvent {
    Response(ResponseEvent),
    Transcription(TranscriptionEvent),
}

impl From<ResponseEvent> for OutputEvent {
    fn from(event: ResponseEvent) -> Self {
        OutputEvent::Response(event)
    }
}

impl From<TranscriptionEvent> for OutputEvent {
    fn from(event: TranscriptionEvent) -> Self {
        OutputEvent::Transcription(event)
    }
}

/// Sample rate expected by [`Input::Audio`] and passed through to the
/// [`TranscriptionProvider`].
const AUDIO_SAMPLE_RATE: u32 = 16_000;

/// An agent backed by independent providers for each modality.
///
/// `R` drives chat-style response turns; `T` drives audio transcription.
/// They are deliberately separate type parameters so a deployment can
/// pair, say, a hosted chat model with a local Whisper instance. A single
/// type that implements both traits (e.g. [`provider::openai::OpenAIProvider`])
/// can be used for both slots.
pub struct Agent<R, T> {
    response_provider: R,
    transcription_provider: T,
    model: String,
    transcription_model: String,
    tools: HashMap<String, Tool>,
    session: Session,
}

impl<R, T> Agent<R, T>
where
    R: ResponseProvider,
    T: TranscriptionProvider,
    R::Error: std::fmt::Display + Send + Sync + 'static,
    T::Error: std::fmt::Display + Send + Sync + 'static,
{
    pub fn new(
        response_provider: R,
        transcription_provider: T,
        model: String,
        transcription_model: String,
        tools: impl IntoIterator<Item = Tool>,
        session: Session,
    ) -> Self {
        let tools = tools
            .into_iter()
            .map(|t| {
                let name = t.name.clone();
                (name, t)
            })
            .collect();

        Self {
            response_provider,
            transcription_provider,
            model,
            transcription_model,
            tools,
            session,
        }
    }

    /// Run a conversation turn, streaming [`OutputEvent`]s via the
    /// callback.
    ///
    /// For [`Input::Audio`], the turn begins with a transcription phase
    /// (emitting [`OutputEvent::Transcription`] events) and then proceeds
    /// to the response phase as if the transcribed text had been typed.
    /// For [`Input::Text`], only the response phase runs.
    pub async fn run<F>(
        &mut self,
        input: Input,
        cancel: Cancel,
        mut on_event: F,
    ) -> Result<(), anyhow::Error>
    where
        F: FnMut(OutputEvent),
    {
        let text = match input {
            Input::Text(t) => t,
            Input::Audio(samples) => {
                match self
                    .transcribe(&samples, &cancel, |e| {
                        on_event(OutputEvent::Transcription(e))
                    })
                    .await?
                {
                    Some(t) => t,
                    None => return Ok(()),
                }
            }
        };

        if cancel.is_cancelled() {
            return Ok(());
        }

        self.respond(&text, &cancel, |e| on_event(OutputEvent::Response(e)))
            .await
    }

    /// Run the response (chat completion) loop for a single user message.
    ///
    /// Appends the user message to the session, streams model output, and
    /// automatically loops when the model requests tool calls. Streams
    /// flat [`ResponseEvent`]s via `on_event`.
    async fn respond(
        &mut self,
        text: &str,
        cancel: &Cancel,
        mut on_event: impl FnMut(ResponseEvent),
    ) -> Result<(), anyhow::Error> {
        self.session.append(Entry::Message(Message {
            role: Role::User,
            content: vec![MessageContent::Text {
                text: text.to_string(),
            }],
        }));

        loop {
            if cancel.is_cancelled() {
                break;
            }

            let mut stream = self
                .response_provider
                .create_response(self.session.entries(), &self.model, self.tools.values())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let mut pending_entries: Vec<Entry> = Vec::new();
            let mut pending_tool_calls: Vec<entry::ToolCall> = Vec::new();

            loop {
                tokio::select! {
                    item = stream.next() => {
                        let Some(result) = item else { break };
                        let event = result.map_err(|e| anyhow::anyhow!("{e}"))?;

                        match &event {
                            ResponseEvent::TextDone(msg) => {
                                pending_entries.push(Entry::Message(msg.clone()));
                            }
                            ResponseEvent::ReasoningDone(r) => {
                                pending_entries.push(Entry::Reasoning(r.clone()));
                            }
                            ResponseEvent::ToolCall(tc) => {
                                pending_entries.push(Entry::ToolCall(tc.clone()));
                                pending_tool_calls.push(tc.clone());
                            }
                            _ => {}
                        }

                        on_event(event);
                    }
                    _ = cancel.cancelled() => {
                        break;
                    }
                }
            }

            for entry in pending_entries {
                self.session.append(entry);
            }

            if cancel.is_cancelled() || pending_tool_calls.is_empty() {
                break;
            }

            for tc in &pending_tool_calls {
                if cancel.is_cancelled() {
                    break;
                }

                let tool = self
                    .tools
                    .get(&tc.name)
                    .ok_or_else(|| anyhow::anyhow!("unknown tool: {}", tc.name))?;

                let output = (tool.handler)(tc.arguments.clone())?;
                let result = ToolResult {
                    call_id: tc.call_id.clone(),
                    output,
                };

                on_event(ResponseEvent::ToolResult(result.clone()));
                self.session.append(Entry::ToolResult(result));
            }
        }

        Ok(())
    }

    /// Transcribe `samples` through the transcription provider, streaming
    /// [`TranscriptionEvent`]s via `on_event`. Returns the final text on
    /// success, or `None` if cancelled before a result arrived.
    async fn transcribe(
        &self,
        samples: &[i16],
        cancel: &Cancel,
        mut on_event: impl FnMut(TranscriptionEvent),
    ) -> Result<Option<String>, anyhow::Error> {
        if cancel.is_cancelled() {
            return Ok(None);
        }

        let mut stream = self
            .transcription_provider
            .create_transcription(samples, AUDIO_SAMPLE_RATE, &self.transcription_model)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut accumulated = String::new();
        let mut final_text: Option<String> = None;

        loop {
            tokio::select! {
                item = stream.next() => {
                    let Some(result) = item else { break };
                    let event = result.map_err(|e| anyhow::anyhow!("{e}"))?;
                    match &event {
                        TranscriptionEvent::Delta(d) => {
                            accumulated.push_str(d);
                        }
                        TranscriptionEvent::Done(text) => {
                            final_text = Some(text.clone());
                        }
                    }
                    on_event(event);
                }
                _ = cancel.cancelled() => {
                    return Ok(None);
                }
            }
        }

        // Some providers may end the stream without an explicit `Done`
        // event; fall back to whatever deltas accumulated.
        Ok(Some(final_text.unwrap_or(accumulated)))
    }
}
