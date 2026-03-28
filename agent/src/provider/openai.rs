use super::{Provider, ResponseResult, ResponseStreamEvent};
use async_openai::{
    config::OpenAIConfig,
    error::OpenAIError,
    traits::EventType,
    types::responses::{
        CreateResponseArgs, ReasoningEffort,
        ResponseStreamEvent as OpenAIResponseStreamEvent,
    },
    Client,
};
use futures::StreamExt;
use tracing::debug;

pub struct OpenAIProvider {
    client: Client<OpenAIConfig>,
}

impl OpenAIProvider {
    pub fn new() -> Self {
        let config = OpenAIConfig::new()
            .with_api_key(std::env::var("OPENAI_API_KEY").unwrap_or_default());
        let client = Client::with_config(config);
        Self { client }
    }
}

impl Provider for OpenAIProvider {
    type Error = OpenAIError;

    async fn create_response(&self, prompt: &str, model: &str) -> ResponseResult<Self::Error> {
        let request = CreateResponseArgs::default()
            .model(model)
            .stream(true)
            .reasoning(ReasoningEffort::None)
            .input(prompt.to_string())
            .build()?;

        let stream = self.client.responses().create_stream(request).await?;

        let mapped = stream.filter_map(|result| async {
            match result {
                Ok(OpenAIResponseStreamEvent::ResponseOutputTextDelta(delta)) => {
                    Some(Ok(ResponseStreamEvent::TextDelta(delta.delta)))
                }
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
