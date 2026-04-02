mod context;
mod prompt;
mod screen;
mod spinner;
mod tool;
mod widget;
mod width;

use anyhow::Result;
use clap::Parser;
use context::Context;
use std::path::PathBuf;
use tracing::debug;
use tracing_subscriber::EnvFilter;

use agent::{
    provider::{openai::OpenAIProvider, Provider, ResponseEvent},
    session::Session,
    Agent,
};

use crate::screen::Screen;
use crate::{
    prompt::{Prompt, PromptAction},
    spinner::Spinner,
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MIM_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let args = Args::parse();

    run(args).await
}

async fn run(args: Args) -> Result<()> {
    let ctx = Context::new(args.path)?;

    debug!(root=?ctx.root, cwd=?ctx.cwd, "mim context");

    let provider = OpenAIProvider::new();
    let tools = tool::make_tools()?;
    let session = Session::open(ctx.session_path)?;

    let mut agent = Agent::new(provider, args.model, tools, session);

    let mut screen = Screen::new()?;
    let mut prompt = Prompt::new("> ");
    let mut spinner = Spinner::new(spinner::SpinnerVariant::Line);

    let mut lines: Vec<String> = Vec::new();

    loop {
        let mut frame = screen.begin()?;

        lines.iter().for_each(|l| frame.add_line(l.clone()));

        frame.add(&spinner);
        frame.add_focused(&prompt);
        screen.end(frame)?;

        let event = screen.event().await?;

        spinner.step();

        let Some(action) = prompt.handle(event) else {
            continue;
        };

        match action {
            PromptAction::Submit(text) => lines.push(text),
            PromptAction::Suspend => screen.suspend()?,
            PromptAction::Quit => screen.quit()?,
            PromptAction::Interrupt | PromptAction::Eof => break,
        }
    }

    Ok(())
}

async fn interact<P: Provider>(mut agent: Agent<P>) -> Result<()>
where
    P::Error: std::fmt::Display + Send + Sync + 'static,
{
    loop {
        let input = "hello";

        agent
            .run(input, |event| match event {
                ResponseEvent::TextDelta(text) => {
                    print!("{}", text);
                }
                ResponseEvent::TextDone(_) => {
                    eprintln!("[text done]")
                }
                ResponseEvent::ReasoningDelta(text) => {
                    eprintln!("[reasoning delta: {}]", text)
                }
                ResponseEvent::ReasoningDone(_) => {
                    eprintln!("[reasoning done]")
                }
                ResponseEvent::ToolCall(tool_call) => {
                    eprintln!(
                        "[tool call: {} args={}]",
                        tool_call.name, tool_call.arguments
                    );
                }
                ResponseEvent::ToolResult(tool_result) => {
                    eprintln!(
                        "[tool result for {}: {}]",
                        tool_result.call_id, tool_result.output
                    );
                }
                _ => {}
            })
            .await?;

        println!();
    }
}
