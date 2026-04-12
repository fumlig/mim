use crate::entry;
use crate::tool::Tool;

use std::future::Future;
use std::pin::Pin;

use super::{
    ResponseEvent, ResponseProvider, ResponseResult, SpeechEvent, SpeechProvider, SpeechResult,
    SpeechStream, TranscriptionEvent, TranscriptionProvider, TranscriptionResult,
    TranscriptionStream,
};
use async_openai::{
    config::OpenAIConfig,
    error::OpenAIError,
    traits::EventType,
    types::{
        audio::{
            AudioInput, AudioResponseFormat, CreateSpeechRequestArgs,
            CreateSpeechResponseStreamEvent, CreateTranscriptionRequestArgs,
            CreateTranscriptionResponseStreamEvent, SpeechModel, SpeechResponseFormat,
            Voice,
        },
        responses::{
            CompactionSummaryItemParam, CreateResponseArgs, FunctionCallOutput,
            FunctionCallOutputItemParam, FunctionTool, FunctionToolCall, InputContent, InputItem,
            InputMessage, InputParam, InputRole, InputTextContent, Item, MessageItem, OutputItem,
            OutputMessage, OutputMessageContent, OutputStatus, OutputTextContent, ReasoningItem,
            ReasoningTextContent, RefusalContent, ResponseStreamEvent, SummaryPart,
            SummaryTextContent, Tool as OpenAITool,
        },
    },
    Client,
};
use futures::StreamExt;
use tracing::debug;

impl From<&entry::Entry> for InputItem {
    fn from(entry: &entry::Entry) -> InputItem {
        match entry {
            entry::Entry::Message(msg) => match msg.role {
                entry::Role::Assistant => {
                    let content = msg
                        .content
                        .iter()
                        .map(|c| match c {
                            entry::MessageContent::Text { text } => {
                                OutputMessageContent::OutputText(OutputTextContent {
                                    annotations: vec![],
                                    logprobs: None,
                                    text: text.clone(),
                                })
                            }
                            entry::MessageContent::Refusal { text } => {
                                OutputMessageContent::Refusal(RefusalContent {
                                    refusal: text.clone(),
                                })
                            }
                            // Images and files are input-only; skip for assistant output
                            _ => OutputMessageContent::OutputText(OutputTextContent {
                                annotations: vec![],
                                logprobs: None,
                                text: String::new(),
                            }),
                        })
                        .collect();
                    InputItem::Item(Item::Message(MessageItem::Output(OutputMessage {
                        content,
                        id: String::new(),
                        role: Default::default(),
                        phase: None,
                        status: OutputStatus::Completed,
                    })))
                }
                ref role => {
                    let oai_role = match role {
                        entry::Role::System => InputRole::System,
                        entry::Role::Developer => InputRole::Developer,
                        _ => InputRole::User,
                    };
                    let content = msg
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            entry::MessageContent::Text { text } => {
                                Some(InputContent::InputText(InputTextContent {
                                    text: text.clone(),
                                }))
                            }
                            // TODO: handle Image, File content types
                            _ => None,
                        })
                        .collect();
                    InputItem::Item(Item::Message(MessageItem::Input(InputMessage {
                        content,
                        role: oai_role,
                        status: None,
                    })))
                }
            },
            entry::Entry::ToolCall(tc) => InputItem::Item(Item::FunctionCall(FunctionToolCall {
                call_id: tc.call_id.clone(),
                name: tc.name.clone(),
                arguments: tc.arguments.to_string(),
                namespace: None,
                id: None,
                status: Some(OutputStatus::Completed),
            })),
            entry::Entry::ToolResult(tr) => {
                InputItem::Item(Item::FunctionCallOutput(FunctionCallOutputItemParam {
                    call_id: tr.call_id.clone(),
                    output: FunctionCallOutput::Text(tr.output.to_string()),
                    id: None,
                    status: None,
                }))
            }
            entry::Entry::Reasoning(r) => {
                let summary = r
                    .summary
                    .iter()
                    .map(|text| SummaryPart::SummaryText(SummaryTextContent { text: text.clone() }))
                    .collect();
                let content = r.content.as_ref().map(|parts| {
                    parts
                        .iter()
                        .map(|c| ReasoningTextContent {
                            text: c.text.clone(),
                        })
                        .collect()
                });
                InputItem::Item(Item::Reasoning(ReasoningItem {
                    id: r.id.clone(),
                    summary,
                    content,
                    encrypted_content: r.encrypted_content.clone(),
                    status: Some(OutputStatus::Completed),
                }))
            }
            entry::Entry::Compaction(c) => {
                InputItem::Item(Item::Compaction(CompactionSummaryItemParam {
                    id: None,
                    encrypted_content: c.encrypted_content.clone(),
                }))
            }
        }
    }
}

impl From<&Tool> for OpenAITool {
    fn from(tool: &Tool) -> OpenAITool {
        OpenAITool::Function(FunctionTool {
            name: tool.name.clone(),
            description: Some(tool.description.clone()),
            parameters: Some(tool.parameters.clone()),
            strict: None,
            defer_loading: None,
        })
    }
}

pub struct OpenAIProvider {
    client: Client<OpenAIConfig>,
}

impl OpenAIProvider {
    pub fn new() -> Self {
        // Client reads OPENAI_API_KEY and OPENAI_BASE_URL
        let client = Client::new();
        Self { client }
    }
}

impl ResponseProvider for OpenAIProvider {
    type Error = OpenAIError;

    fn create_response<'a>(
        &'a self,
        input: &'a [entry::Entry],
        model: &'a str,
        tools: impl IntoIterator<Item = &'a Tool> + Send + 'a,
    ) -> Pin<Box<dyn Future<Output = ResponseResult<Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let input = {
                let items = input.iter().map(InputItem::from).collect();
                InputParam::Items(items)
            };
            let tools: Vec<OpenAITool> = tools.into_iter().map(|t| t.into()).collect();

            let mut builder = CreateResponseArgs::default();
            builder.model(model).stream(true).input(input);

            if !tools.is_empty() {
                builder.tools(tools);
            }

            let request = builder.build()?;

            let stream = self.client.responses().create_stream(request).await?;

            let mapped = stream.filter_map(|result| async {
                match result {
                    // Reasoning deltas (content = full thinking, summary = condensed)
                    Ok(ResponseStreamEvent::ResponseReasoningTextDelta(e)) => {
                        Some(Ok(ResponseEvent::ReasoningDelta(e.delta)))
                    }
                    Ok(ResponseStreamEvent::ResponseReasoningSummaryTextDelta(_)) => None,

                    // Text output delta
                    Ok(ResponseStreamEvent::ResponseOutputTextDelta(e)) => {
                        Some(Ok(ResponseEvent::TextDelta(e.delta)))
                    }

                    // Completed output items
                    Ok(ResponseStreamEvent::ResponseOutputItemDone(e)) => match e.item {
                        OutputItem::FunctionCall(fc) => match serde_json::from_str(&fc.arguments) {
                            Ok(arguments) => Some(Ok(ResponseEvent::ToolCall(entry::ToolCall {
                                call_id: fc.call_id,
                                name: fc.name,
                                arguments,
                            }))),
                            Err(err) => Some(Err(OpenAIError::JSONDeserialize(err, fc.arguments))),
                        },
                        OutputItem::Reasoning(ref r) => {
                            let summary = r
                                .summary
                                .iter()
                                .map(|part| match part {
                                    SummaryPart::SummaryText(t) => t.text.clone(),
                                })
                                .collect();
                            let content = r.content.as_ref().map(|parts| {
                                parts
                                    .iter()
                                    .map(|c| entry::ReasoningContent {
                                        text: c.text.clone(),
                                    })
                                    .collect()
                            });
                            Some(Ok(ResponseEvent::ReasoningDone(entry::Reasoning {
                                id: r.id.clone(),
                                summary,
                                content,
                                encrypted_content: r.encrypted_content.clone(),
                            })))
                        }
                        OutputItem::Message(ref msg) => {
                            let content = msg
                                .content
                                .iter()
                                .map(|c| match c {
                                    OutputMessageContent::OutputText(t) => {
                                        entry::MessageContent::Text {
                                            text: t.text.clone(),
                                        }
                                    }
                                    OutputMessageContent::Refusal(r) => {
                                        entry::MessageContent::Refusal {
                                            text: r.refusal.clone(),
                                        }
                                    }
                                })
                                .collect();
                            Some(Ok(ResponseEvent::TextDone(entry::Message {
                                role: entry::Role::Assistant,
                                content,
                            })))
                        }
                        _ => None,
                    },

                    // Ignore everything else
                    Ok(event) => {
                        debug!(r#type = ?event.event_type(), "ignoring event");
                        None
                    }
                    Err(err) => Some(Err(err)),
                }
            });

            let stream: super::ResponseStream<Self::Error> = Box::pin(mapped);
            Ok(stream)
        })
    }
}

impl TranscriptionProvider for OpenAIProvider {
    type Error = OpenAIError;

    fn create_transcription<'a>(
        &'a self,
        samples: &'a [i16],
        sample_rate: u32,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = TranscriptionResult<Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            // Encode PCM synchronously so the future owns the bytes and
            // doesn't need to borrow `samples` across awaits.
            let wav = encode_wav_mono_i16(samples, sample_rate);

            let request = CreateTranscriptionRequestArgs::default()
                .file(AudioInput::from_bytes("speech.wav".to_string(), wav.into()))
                .model(model.to_string())
                .response_format(AudioResponseFormat::Json)
                .stream(true)
                .build()?;

            let raw = self
                .client
                .audio()
                .transcription()
                .create_stream(request)
                .await?;

            let mapped = raw.filter_map(|result| async {
                match result {
                    Ok(CreateTranscriptionResponseStreamEvent::TranscriptTextDelta(e)) => {
                        Some(Ok(TranscriptionEvent::Delta(e.delta)))
                    }
                    Ok(CreateTranscriptionResponseStreamEvent::TranscriptTextDone(e)) => {
                        Some(Ok(TranscriptionEvent::Done(e.text)))
                    }
                    Ok(event) => {
                        debug!(r#type = ?event.event_type(), "ignoring transcription event");
                        None
                    }
                    Err(err) => Some(Err(err)),
                }
            });

            let stream: TranscriptionStream<Self::Error> = Box::pin(mapped);
            Ok(stream)
        })
    }
}

impl SpeechProvider for OpenAIProvider {
    type Error = OpenAIError;

    fn create_speech<'a>(
        &'a self,
        text: &'a str,
        model: &'a str,
        voice: &'a str,
        instructions: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = SpeechResult<Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let speech_model = SpeechModel::Other(model.to_string());
            let speech_voice = Voice::Other(voice.to_string());

            let mut builder = CreateSpeechRequestArgs::default();
            builder
                .input(text)
                .model(speech_model)
                .voice(speech_voice)
                .response_format(SpeechResponseFormat::Pcm);

            if let Some(inst) = instructions {
                builder.instructions(inst);
            }

            let request = builder.build()?;

            let raw = self.client.audio().speech().create_stream(request).await?;

            let mapped = raw.filter_map(|result| async {
                match result {
                    Ok(CreateSpeechResponseStreamEvent::SpeechAudioDelta(e)) => {
                        use base64::Engine as _;
                        match base64::engine::general_purpose::STANDARD.decode(&e.audio) {
                            Ok(bytes) => Some(Ok(SpeechEvent::Delta(bytes))),
                            Err(err) => Some(Err(OpenAIError::InvalidArgument(format!(
                                "invalid base64 in speech delta: {err}"
                            )))),
                        }
                    }
                    Ok(CreateSpeechResponseStreamEvent::SpeechAudioDone(_)) => {
                        Some(Ok(SpeechEvent::Done))
                    }
                    Err(err) => Some(Err(err)),
                }
            });

            let stream: SpeechStream<Self::Error> = Box::pin(mapped);
            Ok(stream)
        })
    }
}

/// Encode mono PCM `i16` samples at `sample_rate` Hz into an in-memory WAV
/// (RIFF/WAVE) byte buffer. 44-byte header + raw little-endian samples.
fn encode_wav_mono_i16(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate: u32 = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align: u16 = channels * bits_per_sample / 8;
    let data_len: u32 = (samples.len() * 2) as u32;
    let riff_len: u32 = 36 + data_len;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_len.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt  chunk (PCM)
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }

    buf
}
