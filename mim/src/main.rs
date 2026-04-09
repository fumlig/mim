#[cfg(feature = "capture")]
mod capture;
mod context;
mod format;
mod screen;
#[cfg(feature = "capture")]
mod silero;
mod tool;
#[cfg(feature = "capture")]
mod voice;
mod widget;

use anyhow::{anyhow, Result};
use clap::{Parser, ValueEnum};
use context::Context;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use std::path::PathBuf;
use tokio::sync::mpsc::{Sender, UnboundedReceiver};
use tracing::debug;
use tracing_subscriber::EnvFilter;

use agent::{
    provider::{openai::OpenAIProvider, Provider, ResponseEvent},
    session::Session,
    Agent, Cancel,
};
use tokio::sync::mpsc;

use crate::{
    screen::{EventStream, Screen, ScreenEvent},
    widget::{Block, Editor, EditorAction, Message, Paragraph, Spinner, Widget, WidgetExt},
};

#[derive(Parser, Debug)]
#[command(name = "mim", version)]
struct Args {
    /// Prompt mode
    #[arg(short, long, env = "MIM_MODE", default_value = "text")]
    mode: Mode,

    /// Root .mim directory. Defaults to nearest .mim in an ancestor, or ./.mim
    #[arg(short, long, env = "MIM_PATH")]
    path: Option<PathBuf>,

    /// Model to use
    #[arg(long, env = "MIM_MODEL")]
    model: String,

    #[cfg(feature = "capture")]
    #[command(flatten)]
    audio: capture::AudioArgs,
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

enum AgentEvent {
    /// The agent is about to process this input.
    Starting { text: String, cancel: Cancel },
    /// A streaming event from the agent.
    Response(ResponseEvent),
    /// The current turn completed.
    TurnDone,
    /// An error occurred during the turn.
    Error(String),
}

async fn agent_task<P>(
    mut agent: Agent<P>,
    mut input_rx: mpsc::Receiver<String>,
    output_tx: mpsc::UnboundedSender<AgentEvent>,
) where
    P: Provider + Send + 'static,
    P::Error: std::fmt::Display + Send + Sync + 'static,
{
    while let Some(text) = input_rx.recv().await {
        let cancel = Cancel::new();
        if output_tx
            .send(AgentEvent::Starting {
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
                let _ = tx.send(AgentEvent::Response(event));
            })
            .await;

        let msg = match result {
            Ok(()) => AgentEvent::TurnDone,
            Err(e) => AgentEvent::Error(e.to_string()),
        };
        if output_tx.send(msg).is_err() {
            break;
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum Mode {
    /// Text mode
    Text,
    /// Audio mode
    Audio,
}

impl Mode {
    pub fn next(&self) -> Self {
        let all = Mode::value_variants();
        let i = all
            .iter()
            .position(|m| std::mem::discriminant(m) == std::mem::discriminant(self))
            .unwrap();
        all[(i + 1) % all.len()].clone()
    }
}

struct State {
    mode: Mode,
    screen: Screen,
    events: EventStream,
    input_tx: Sender<String>,
    output_rx: UnboundedReceiver<AgentEvent>,

    messages: Vec<Message>,
    spinner: Spinner,
    cancel: Option<Cancel>,
    prompt: Editor,
}

async fn run(args: Args) -> Result<()> {
    /*
    #[cfg(feature = "capture")]
    {
        use crate::capture::{self, AudioCapture};
        use crate::voice::{VoiceDetector, VoiceEvent};

        let host = capture::resolve_host(args.audio.audio_host.as_deref())?;
        let device = capture::resolve_device(&host, args.audio.audio_device.as_deref())?;
        use futures::StreamExt;

        let (audio, _guard) = AudioCapture::new(device)?.stream()?;
        let mut events = VoiceDetector::new(
            args.audio.vad_threshold,
            args.audio.vad_silence,
            &args.audio.vad_model,
        )?
        .detect(audio);

        while let Some(event) = events.next().await {
            match event {
                VoiceEvent::SpeechStart => println!("[speech start]"),
                VoiceEvent::SpeechEnd(samples) => {
                    let ms = samples.len() as u64 * 1000 / 16_000;
                    println!("[speech end] {ms} ms, {} samples", samples.len());
                }
            }
        }

        println!("ending listen");
    }
    */

    let mut state = {
        let ctx = Context::new(args.path)?;
        debug!(root=?ctx.root, cwd=?ctx.cwd, "mim context");

        let agent = {
            let provider = OpenAIProvider::new();
            let tools = tool::make_tools()?;
            let session = Session::open(ctx.session_path)?;

            Agent::new(provider, args.model, tools, session)
        };

        let (input_tx, output_rx) = {
            let (input_tx, input_rx) = mpsc::channel::<String>(16);
            let (output_tx, mut output_rx) = mpsc::unbounded_channel::<AgentEvent>();

            tokio::spawn(agent_task(agent, input_rx, output_tx));

            (input_tx, output_rx)
        };

        let mode = args.mode;

        let mut screen = Screen::new()?;
        let events = screen.take_events().ok_or(anyhow!("no event stream"))?;

        let prompt = Editor::new();
        let messages: Vec<Message> = Vec::new();
        let cancel: Option<Cancel> = None;
        let spinner = Spinner::cycle(Spinner::ASCII.iter().copied());

        State {
            mode,
            screen,
            events,
            input_tx,
            output_rx,
            messages,
            spinner,
            cancel,
            prompt,
        }
    };

    loop {
        // render
        let mut frame = state.screen.begin()?;
        for (i, message) in state.messages.iter_mut().enumerate() {
            if i > 0 {
                frame.add_line(String::new());
            }
            frame.add(message);
        }

        if state.cancel.is_some() {
            frame.add(&mut state.spinner);
        }

        match state.mode {
            Mode::Text => {
                frame.add(
                    &mut state
                        .prompt
                        .pad(0, 0, 0, 1)
                        .line_numbers(2)
                        .ascii()
                        .pad(1, 0, 0, 0),
                );
            }

            Mode::Audio => {
                frame.add(&mut Paragraph::new("audio"));
            }
        };

        state.screen.end(frame)?;

        // events
        tokio::select! {
            Some(result) = state.events.next(&mut state.screen) => {
                match result? {
                    ScreenEvent::Interrupt => {
                        // Preserve legacy behaviour: while the prompt still
                        // holds text, Ctrl+C clears it; otherwise it cancels
                        // any in-flight work or exits the app.
                        if !state.prompt.is_empty() {
                            state.prompt.clear();
                        } else if let Some(cancel) = state.cancel.take() {
                            cancel.cancel();
                        } else {
                            break;
                        }
                    }
                    ScreenEvent::Suspend => {
                        continue;
                    }
                    ScreenEvent::Quit => {
                        break;
                    }
                    ScreenEvent::Event(Event::Key(KeyEvent {
                        code: KeyCode::Tab,
                        kind: KeyEventKind::Press,
                        ..
                    })) => {
                        state.mode = state.mode.next();
                        continue;
                    }
                    ScreenEvent::Event(event) => {
                        // Pass other events to the prompt. In audio mode we
                        // mutate the editor and submit manually from here.
                        let Some(action) = state.prompt.handle(event) else {
                            continue;
                        };
                        match action {
                            EditorAction::Submit(text) => {
                                let _ = state.input_tx.send(text).await;
                            }
                            EditorAction::Eof => break,
                        }
                    }
                }
            }
            Some(output) = state.output_rx.recv() => {
                match output {
                    AgentEvent::Starting { text, cancel } => {
                        state.messages.push(Message::user(&text));
                        state.messages.push(Message::assistant());
                        state.cancel = Some(cancel);
                    }
                    AgentEvent::Response(event) => {
                        state.spinner.step();
                        if let Some(message) = state.messages.last_mut() {
                            message.push_event(&event);
                        }
                    }
                    AgentEvent::TurnDone => {
                        state.cancel = None;
                    }
                    AgentEvent::Error(e) => {
                        state.cancel = None;
                        if let Some(message) = state.messages.last_mut() {
                            message.push_error(&e);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
