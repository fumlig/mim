use agent::{
    provider::{openai::OpenAIProvider, ResponseEvent},
    session::Session,
    tool::{function_tool, Tool},
    Agent,
};
use anyhow::Result;
use chrono::Utc;
use chrono_tz::Tz;
use clap::Parser;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::Deserialize;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "mim", version)]
struct Args {
    /// The input to send to the model
    #[arg(required = true)]
    input: String,

    /// Model to use
    #[arg(short, long, env = "MIM_MODEL")]
    model: String,
}

fn make_tools() -> Result<Vec<Tool>> {
    #[derive(Deserialize, JsonSchema)]
    struct GetCurrentTimeParams {
        /// IANA time zone name, e.g. "America/New_York", "Europe/Berlin", "UTC"
        timezone: String,
    }

    let get_current_time = function_tool::<GetCurrentTimeParams, _, _>(
        "get_current_time".into(),
        "Get the current date and time in a given IANA time zone (e.g. \"America/New_York\", \"Europe/Berlin\", \"UTC\").".into(),
        |params| {
            let tz: Tz = params
                .timezone
                .parse()
                .map_err(|_| serde_json::Error::custom(format!("unknown timezone: {}", params.timezone)))?;
            let now = Utc::now().with_timezone(&tz);
            Ok(serde_json::json!({
                "timezone": params.timezone,
                "datetime": now.to_rfc3339(),
            }))
        },
    )?;

    Ok(vec![get_current_time])
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MIM_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let Args { input, model } = Args::parse();

    let provider = OpenAIProvider::new();

    let tools = make_tools()?;
    let session = Session::new(PathBuf::from("session.json"));

    let mut agent = Agent::new(provider, model, tools, session);

    agent
        .run(&input, |event| match event {
            ResponseEvent::TextDelta(text) => print!("{text}"),
            ResponseEvent::ReasoningDelta(text) => eprint!("{text}"),
            ResponseEvent::ToolCall(tc) => {
                eprintln!("[tool call: {} args={}]", tc.name, tc.arguments);
            }
            ResponseEvent::ToolResult(result) => {
                eprintln!("[tool result for {}: {}]", result.call_id, result.output);
            }
            _ => {}
        })
        .await?;

    println!();

    Ok(())
}
