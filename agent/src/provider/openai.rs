use crate::entry;
use crate::tool::Tool;

use super::{Provider, ResponseEvent, ResponseResult};
use async_openai::{
    config::OpenAIConfig,
    error::OpenAIError,
    traits::EventType,
    types::responses::{
        CompactionSummaryItemParam, CreateResponseArgs, FunctionCallOutput,
        FunctionCallOutputItemParam, FunctionTool, FunctionToolCall, InputContent, InputItem,
        InputMessage, InputParam, InputRole, InputTextContent, Item, MessageItem, OutputItem,
        OutputMessage, OutputMessageContent, OutputStatus, OutputTextContent, ReasoningItem,
        ReasoningTextContent, RefusalContent, ResponseStreamEvent, SummaryPart, SummaryTextContent,
        Tool as OpenAITool,
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
                            entry::MessageContent::Text(text) => {
                                OutputMessageContent::OutputText(OutputTextContent {
                                    annotations: vec![],
                                    logprobs: None,
                                    text: text.clone(),
                                })
                            }
                            entry::MessageContent::Refusal(text) => {
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
                            entry::MessageContent::Text(text) => {
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
        let config =
            OpenAIConfig::new().with_api_key(std::env::var("OPENAI_API_KEY").unwrap_or_default());
        let client = Client::with_config(config);
        Self { client }
    }
}

impl Provider for OpenAIProvider {
    type Error = OpenAIError;

    async fn create_response(
        &self,
        history: &[entry::Entry],
        model: &str,
        tools: impl IntoIterator<Item = &Tool> + Send,
    ) -> ResponseResult<Self::Error> {
        let input = {
            let items = history.iter().map(InputItem::from).collect();
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
                // Reasoning delta
                Ok(ResponseStreamEvent::ResponseReasoningSummaryTextDelta(e)) => {
                    Some(Ok(ResponseEvent::ReasoningDelta(e.delta)))
                }

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
                                    entry::MessageContent::Text(t.text.clone())
                                }
                                OutputMessageContent::Refusal(r) => {
                                    entry::MessageContent::Refusal(r.refusal.clone())
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

        Ok(Box::pin(mapped))
    }
}
