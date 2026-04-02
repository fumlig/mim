mod border;
mod context;
mod editor;
mod format;
mod message;
mod screen;
mod spinner;
mod tool;
mod widget;

use anyhow::Result;
use clap::Parser;
use context::Context;
use std::path::PathBuf;
use tracing::debug;
use tracing_subscriber::EnvFilter;

use agent::{
    provider::{openai::OpenAIProvider, Provider, ResponseEvent},
    session::Session,
    Agent, Cancel,
};
use tokio::sync::mpsc;

use crate::border::Border;
use crate::editor::{Editor, EditorAction};
use crate::message::Message;
use crate::screen::Screen;
use crate::spinner::Spinner;
use crate::widget::WidgetExt;

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

enum AgentOutput {
    /// The agent is about to process this input.
    Starting { text: String, cancel: Cancel },
    /// A streaming event from the agent.
    Event(ResponseEvent),
    /// The current turn completed.
    TurnDone,
    /// An error occurred during the turn.
    Error(String),
}

async fn agent_task<P>(
    mut agent: Agent<P>,
    mut input_rx: mpsc::Receiver<String>,
    output_tx: mpsc::UnboundedSender<AgentOutput>,
) where
    P: Provider + Send + 'static,
    P::Error: std::fmt::Display + Send + Sync + 'static,
{
    while let Some(text) = input_rx.recv().await {
        let cancel = Cancel::new();
        if output_tx
            .send(AgentOutput::Starting {
                text: text.clone(),
                cancel: cancel.clone(),
            })
            .is_err()
        {
            break;
        }

        let tx = output_tx.clone();
        let result = agent
            .run(&text, cancel, move |event| {
                let _ = tx.send(AgentOutput::Event(event));
            })
            .await;

        let msg = match result {
            Ok(()) => AgentOutput::TurnDone,
            Err(e) => AgentOutput::Error(e.to_string()),
        };
        if output_tx.send(msg).is_err() {
            break;
        }
    }
}

async fn run(args: Args) -> Result<()> {
    let ctx = Context::new(args.path)?;
    debug!(root=?ctx.root, cwd=?ctx.cwd, "mim context");

    let provider = OpenAIProvider::new();
    let tools = tool::make_tools()?;
    let session = Session::open(ctx.session_path)?;
    let agent = Agent::new(provider, args.model, tools, session);

    let (input_tx, input_rx) = mpsc::channel::<String>(16);
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<AgentOutput>();

    tokio::spawn(agent_task(agent, input_rx, output_tx));

    let mut screen = Screen::new()?;
    let mut prompt = Editor::new();
    let mut blocks: Vec<Message> = Vec::new();
    let mut current_cancel: Option<Cancel> = None;
    let mut spinner = Spinner::new(Spinner::ASCII);

    loop {
        let mut frame = screen.begin()?;
        for (i, block) in blocks.iter_mut().enumerate() {
            if i > 0 {
                frame.add_line(String::new());
            }
            frame.add(block);
        }
        if current_cancel.is_some() {
            frame.add(&mut spinner);
        }
        {
            frame.add_focused(
                &mut prompt
                    .pad(0, 0, 0, 1)
                    .line_numbers(2)
                    .ascii()
                    .pad(1, 0, 0, 0),
            );
        }
        screen.end(frame)?;

        tokio::select! {
            event = screen.event() => {
                let Some(action) = prompt.handle(event?) else {
                    continue;
                };
                match action {
                    EditorAction::Submit(text) => {
                        let _ = input_tx.send(text).await;
                    }
                    EditorAction::Interrupt => {
                        if let Some(cancel) = current_cancel.take() {
                            cancel.cancel();
                        } else {
                            break;
                        }
                    }
                    EditorAction::Suspend => screen.suspend()?,
                    EditorAction::Quit => screen.quit()?,
                    EditorAction::Eof => break,
                }
            }
            Some(output) = output_rx.recv() => {
                match output {
                    AgentOutput::Starting { text, cancel } => {
                        blocks.push(Message::user(&text));
                        blocks.push(Message::assistant());
                        current_cancel = Some(cancel);
                    }
                    AgentOutput::Event(event) => {
                        spinner.step();
                        if let Some(block) = blocks.last_mut() {
                            block.push_event(&event);
                        }
                    }
                    AgentOutput::TurnDone => {
                        current_cancel = None;
                    }
                    AgentOutput::Error(e) => {
                        current_cancel = None;
                        if let Some(block) = blocks.last_mut() {
                            block.push_error(&e);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
