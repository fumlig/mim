use agent::provider::{openai::OpenAIProvider, Provider, ResponseStreamEvent};
use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "mim", version, trailing_var_arg = true)]
struct Args {
    /// The prompt to send to the model
    #[arg(required = true, trailing_var_arg = true)]
    prompt: Vec<String>,

    /// Model to use
    #[arg(short, long, default_value = "gpt-4o", env = "MIM_MODEL")]
    model: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MIM_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let Args { prompt, model } = Args::parse();
    let prompt = prompt.join(" ");

    let provider = OpenAIProvider::new();

    let mut stream = provider.create_response(&prompt, &model).await?;

    while let Some(event) = stream.next().await {
        match event? {
            ResponseStreamEvent::TextDelta(text) => {
                print!("{text}");
            }
        }
    }
    print!("\n");

    Ok(())
}
