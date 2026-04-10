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
/// Every entry the agent creates — user messages, assistant messages,
/// reasoning, tool calls, tool results — flows through this enum.
///
/// [`OutputEvent::Delta`] carries a partial [`Entry`] that is re-emitted
/// with the **full** accumulated content on each streaming chunk. The
/// consumer should *replace* (not append to) whatever it was showing for
/// the in-progress entry.
///
/// [`OutputEvent::Entry`] carries a finalized [`Entry`] that has been
/// committed to the session.
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// Partial entry being streamed. Re-emitted with full accumulated
    /// content on each chunk.
    Delta(Entry),
    /// Finalized entry, committed to the session.
    Entry(Entry),
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
    /// (emitting [`OutputEvent::Delta`] events with partial user messages)
    /// and then proceeds to the response phase. For [`Input::Text`], only
    /// the response phase runs.
    ///
    /// Every entry the agent creates — user message, assistant message,
    /// reasoning, tool calls, tool results — is surfaced as an
    /// [`OutputEvent`].
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
                match self.transcribe(&samples, &cancel, &mut on_event).await? {
                    Some(t) => t,
                    None => return Ok(()),
                }
            }
        };

        if cancel.is_cancelled() {
            return Ok(());
        }

        self.respond(&text, &cancel, &mut on_event).await
    }

    /// Run the response (chat completion) loop for a single user message.
    ///
    /// Emits the user message as [`OutputEvent::Entry`], then streams
    /// model output as [`OutputEvent::Delta`] / [`OutputEvent::Entry`]
    /// pairs. Automatically loops when the model requests tool calls.
    async fn respond(
        &mut self,
        text: &str,
        cancel: &Cancel,
        on_event: &mut impl FnMut(OutputEvent),
    ) -> Result<(), anyhow::Error> {
        let user_entry = Entry::Message(Message {
            role: Role::User,
            content: vec![MessageContent::Text {
                text: text.to_string(),
            }],
        });
        self.session.append(user_entry.clone());
        on_event(OutputEvent::Entry(user_entry));

        loop {
            if cancel.is_cancelled() {
                break;
            }

            let mut stream = self
                .response_provider
                .create_response(self.session.entries(), &self.model, self.tools.values())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let mut pending_tool_calls: Vec<entry::ToolCall> = Vec::new();
            let mut text_acc = String::new();
            let mut reasoning_acc = String::new();

            loop {
                tokio::select! {
                    item = stream.next() => {
                        let Some(result) = item else { break };
                        let event = result.map_err(|e| anyhow::anyhow!("{e}"))?;

                        match event {
                            ResponseEvent::TextDelta(d) => {
                                text_acc.push_str(&d);
                                on_event(OutputEvent::Delta(Entry::Message(Message {
                                    role: Role::Assistant,
                                    content: vec![MessageContent::Text {
                                        text: text_acc.clone(),
                                    }],
                                })));
                            }
                            ResponseEvent::TextDone(msg) => {
                                text_acc.clear();
                                let entry = Entry::Message(msg);
                                self.session.append(entry.clone());
                                on_event(OutputEvent::Entry(entry));
                            }
                            ResponseEvent::ReasoningDelta(d) => {
                                reasoning_acc.push_str(&d);
                                on_event(OutputEvent::Delta(Entry::Reasoning(entry::Reasoning {
                                    id: String::new(),
                                    summary: vec![],
                                    content: Some(vec![entry::ReasoningContent {
                                        text: reasoning_acc.clone(),
                                    }]),
                                    encrypted_content: None,
                                })));
                            }
                            ResponseEvent::ReasoningDone(r) => {
                                reasoning_acc.clear();
                                let entry = Entry::Reasoning(r);
                                self.session.append(entry.clone());
                                on_event(OutputEvent::Entry(entry));
                            }
                            ResponseEvent::ToolCall(tc) => {
                                pending_tool_calls.push(tc.clone());
                                let entry = Entry::ToolCall(tc);
                                self.session.append(entry.clone());
                                on_event(OutputEvent::Entry(entry));
                            }
                            ResponseEvent::ToolResult(_) => {
                                // Not emitted by the provider stream;
                                // tool results are generated below.
                            }
                        }
                    }
                    _ = cancel.cancelled() => {
                        break;
                    }
                }
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

                let entry = Entry::ToolResult(result);
                self.session.append(entry.clone());
                on_event(OutputEvent::Entry(entry));
            }
        }

        Ok(())
    }

    /// Transcribe `samples` through the transcription provider, emitting
    /// [`OutputEvent::Delta`] events with partial user messages as text
    /// arrives. Returns the final text on success, or `None` if cancelled
    /// before a result arrived.
    async fn transcribe(
        &self,
        samples: &[i16],
        cancel: &Cancel,
        on_event: &mut impl FnMut(OutputEvent),
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
                            on_event(OutputEvent::Delta(Entry::Message(Message {
                                role: Role::User,
                                content: vec![MessageContent::Text {
                                    text: accumulated.clone(),
                                }],
                            })));
                        }
                        TranscriptionEvent::Done(text) => {
                            final_text = Some(text.clone());
                        }
                    }
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
