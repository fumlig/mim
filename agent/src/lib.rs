pub mod entry;
pub mod provider;
pub mod session;
pub mod tool;

use entry::{Entry, Message, MessageContent, Role, ToolResult};
use futures::StreamExt;
use provider::{Provider, ResponseEvent};
use session::Session;
use std::collections::HashMap;
use tool::Tool;

pub use tokio_util::sync::CancellationToken as Cancel;

pub struct Agent<P: Provider> {
    provider: P,
    model: String,
    tools: HashMap<String, Tool>,
    session: Session,
}

impl<P> Agent<P>
where
    P: Provider,
    P::Error: std::fmt::Display + Send + Sync + 'static,
{
    pub fn new(
        provider: P,
        model: String,
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
            provider,
            model,
            tools,
            session,
        }
    }

    /// Run a conversation turn. Streams events back via the callback.
    /// Loops automatically when the model requests tool calls.
    /// If the `cancel` token is triggered, the current stream and any
    /// remaining tool calls are abandoned and the method returns `Ok(())`.
    pub async fn run<F>(
        &mut self,
        input: &str,
        cancel: Cancel,
        mut on_event: F,
    ) -> Result<(), anyhow::Error>
    where
        F: FnMut(ResponseEvent),
    {
        self.session.append(Entry::Message(Message {
            role: Role::User,
            content: vec![MessageContent::Text(input.to_string())],
        }));

        loop {
            let mut stream = self
                .provider
                .create_response(self.session.entries(), &self.model, self.tools.values())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let mut pending_entries: Vec<Entry> = Vec::new();
            let mut pending_tool_calls: Vec<entry::ToolCall> = Vec::new();
            let mut cancelled = false;

            loop {
                tokio::select! {
                    event = stream.next() => {
                        let Some(event) = event else { break };
                        let event = event.map_err(|e| anyhow::anyhow!("{e}"))?;

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
                        cancelled = true;
                        break;
                    }
                }
            }

            // Append all accumulated entries to the session in stream order.
            for entry in pending_entries {
                self.session.append(entry);
            }

            if cancelled {
                return Ok(());
            }

            // If no tool calls were requested, we're done.
            if pending_tool_calls.is_empty() {
                break;
            }

            // Execute all tool calls and append results.
            for tc in &pending_tool_calls {
                if cancel.is_cancelled() {
                    return Ok(());
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
}
