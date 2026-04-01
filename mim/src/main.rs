mod context;
mod render;

use anyhow::Result;
use chrono::Utc;
use chrono_tz::Tz;
use clap::Parser;
use context::Context;
use reedline::{DefaultPrompt, Reedline, Signal};
use render::MarkdownRenderer;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::Deserialize;
use std::path::PathBuf;
use termimad::MadSkin;
use tracing::debug;
use tracing_subscriber::EnvFilter;

use agent::{
    provider::{openai::OpenAIProvider, ResponseEvent},
    session::Session,
    tool::{function_tool, Tool},
    Agent, Cancel,
};

#[derive(Parser, Debug)]
#[command(name = "mim", version)]
struct Args {
    /// Model to use
    #[arg(short, long, env = "MIM_MODEL")]
    model: String,

    /// Root .mim directory. Defaults to nearest .mim in an ancestor, or ./.mim
    #[arg(short, long, env = "MIM_PATH")]
    path: Option<PathBuf>,
}

fn make_tools() -> Result<Vec<Tool>> {
    #[derive(Deserialize, JsonSchema)]
    struct GetCurrentTimeParams {
        /// IANA time zone name, e.g. "America/New_York", "Europe/Berlin", "UTC"
        timezone: String,
    }

    let get_current_time = function_tool(
        "get_current_time".into(),
        "Get the current date and time in a given IANA time zone (e.g. \"America/New_York\", \"Europe/Berlin\", \"UTC\").".into(),
        |params: GetCurrentTimeParams| {
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

    let args = Args::parse();

    let ctx = Context::new(args.path)?;

    debug!(root=?ctx.root, cwd=?ctx.cwd, "mim context");

    let provider = OpenAIProvider::new();
    let tools = make_tools()?;
    let session = Session::open(ctx.session_path)?;

    let mut agent = Agent::new(provider, args.model, tools, session);

    let mut line_editor = Reedline::create();
    let prompt = DefaultPrompt::default();

    loop {
        let sig = line_editor.read_line(&prompt)?;
        let input = match sig {
            Signal::Success(buffer) => buffer,
            Signal::CtrlD | Signal::CtrlC => {
                println!("aborted");
                break;
            }
        };

        let cancel = Cancel::new();
        let signal_task = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                // 1st Ctrl+C: cancel the current response
                if tokio::signal::ctrl_c().await.is_ok() {
                    cancel.cancel();
                }
                // 2nd Ctrl+C: exit the process
                if tokio::signal::ctrl_c().await.is_ok() {
                    std::process::exit(0);
                }
            }
        });

        let mut md = MarkdownRenderer::new(MadSkin::default());
        let mut got_reasoning_deltas = false;
        agent
            .run(&input, cancel, |event| match event {
                ResponseEvent::TextDelta(text) => {
                    md.push(&text).ok();
                }
                ResponseEvent::TextDone(_) => {
                    md.finish().ok();
                }
                ResponseEvent::ReasoningDelta(text) => {
                    got_reasoning_deltas = true;
                    eprint!("{text}");
                }
                ResponseEvent::ReasoningDone(r) => {
                    // If the server didn't stream reasoning deltas, print
                    // the completed content so reasoning is still visible.
                    if !got_reasoning_deltas {
                        if let Some(parts) = &r.content {
                            for part in parts {
                                eprint!("{}", part.text);
                            }
                        }
                    }
                    got_reasoning_deltas = false;
                }
                ResponseEvent::ToolCall(tc) => {
                    println!("[tool call: {} args={}]", tc.name, tc.arguments);
                }
                ResponseEvent::ToolResult(result) => {
                    println!("[tool result for {}: {}]", result.call_id, result.output);
                }
                _ => {}
            })
            .await?;

        signal_task.abort();

        println!();
    }

    Ok(())
}
