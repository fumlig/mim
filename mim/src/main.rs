#[cfg(feature = "audio")]
mod capture;
mod context;
mod format;
mod message;
#[cfg(feature = "audio")]
mod playback;
mod prompt;
mod screen;
#[cfg(feature = "audio")]
mod silero;
mod tool;
#[cfg(feature = "audio")]
mod voice;
mod widget;

use anyhow::{anyhow, Result};
use clap::Parser;
use context::Context;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use std::path::PathBuf;
use tokio::sync::mpsc::{Sender, UnboundedReceiver};
use tracing::debug;
use tracing_subscriber::EnvFilter;

use agent::{
    entry::{Entry, MessageContent, Role as EntryRole},
    provider::{openai::OpenAIProvider, ResponseProvider, TranscriptionProvider},
    session::Session,
    Agent, Cancel, Input, OutputEvent,
};
use tokio::sync::mpsc;

use crate::{
    message::Message,
    prompt::{EditorAction, Prompt, PromptMode, VoiceStatus},
    screen::{EventStream, Screen, ScreenEvent, Signal},
    widget::{Spinner, VStack},
};

#[cfg(feature = "audio")]
fn hosts_help() -> String {
    use cpal::traits::HostTrait as _;
    use std::fmt::Write as _;
    let mut s = String::from("Audio host backend");
    let hosts = cpal::available_hosts();
    if !hosts.is_empty() {
        let default_name = cpal::default_host().id().name();
        s.push_str("\n\nAvailable:");
        for id in &hosts {
            let tag = if id.name() == default_name {
                " (default)"
            } else {
                ""
            };
            let _ = write!(s, "\n  {}{tag}", id.name());
        }
    }
    s
}

#[cfg(feature = "audio")]
fn input_devices_help() -> String {
    use cpal::traits::{DeviceTrait as _, HostTrait as _};
    use std::fmt::Write as _;
    let mut s = String::from("Audio input device (substring match)");
    let host = cpal::default_host();
    if let Ok(devices) = host.input_devices() {
        let default_id = host.default_input_device().and_then(|d| d.id().ok());
        s.push_str("\n\nDevices on default host:");
        let mut any = false;
        for device in devices {
            any = true;
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "<unknown>".into());
            let is_default = default_id
                .as_ref()
                .and_then(|did| device.id().ok().map(|id| id == *did))
                .unwrap_or(false);
            let tag = if is_default { " (default)" } else { "" };
            let _ = write!(s, "\n  {name}{tag}");
        }
        if !any {
            s.push_str("\n  (none)");
        }
    }
    s
}

#[cfg(feature = "audio")]
fn output_devices_help() -> String {
    use cpal::traits::{DeviceTrait as _, HostTrait as _};
    use std::fmt::Write as _;
    let mut s = String::from("Audio output device (substring match)");
    let host = cpal::default_host();
    if let Ok(devices) = host.output_devices() {
        let default_id = host.default_output_device().and_then(|d| d.id().ok());
        s.push_str("\n\nDevices on default host:");
        let mut any = false;
        for device in devices {
            any = true;
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "<unknown>".into());
            let is_default = default_id
                .as_ref()
                .and_then(|did| device.id().ok().map(|id| id == *did))
                .unwrap_or(false);
            let tag = if is_default { " (default)" } else { "" };
            let _ = write!(s, "\n  {name}{tag}");
        }
        if !any {
            s.push_str("\n  (none)");
        }
    }
    s
}

#[cfg(feature = "audio")]
#[derive(clap::Args, Debug)]
struct AudioArgs {
    /// Audio host backend
    #[arg(long, long_help = hosts_help())]
    pub audio_host: Option<String>,

    /// Audio input device (substring match)
    #[arg(long, env = "MIM_AUDIO_INPUT", long_help = input_devices_help())]
    pub audio_input: Option<String>,

    /// Audio output device (substring match)
    #[arg(long, env = "MIM_AUDIO_OUTPUT", long_help = output_devices_help())]
    pub audio_output: Option<String>,

    /// VAD speech probability threshold (0.0–1.0)
    #[arg(long, default_value_t = 0.5)]
    pub vad_threshold: f32,

    /// Seconds without speech before ending a segment
    #[arg(long, default_value_t = 1.0)]
    pub vad_silence: f32,

    /// Path to the Silero VAD ONNX model.
    ///
    /// Use scripts/download-silero-vad.sh to fetch it.
    #[arg(long, env = "MIM_VAD_MODEL", default_value = "models/silero_vad.onnx")]
    pub vad_model: PathBuf,
}

#[derive(Parser, Debug)]
#[command(name = "mim", version)]
struct Args {
    /// Prompt mode
    #[arg(short, long, env = "MIM_MODE", default_value = "text")]
    mode: PromptMode,

    /// Root .mim directory. Defaults to nearest .mim in an ancestor, or ./.mim
    #[arg(short, long, env = "MIM_PATH")]
    path: Option<PathBuf>,

    /// Model to use
    #[arg(long, env = "MIM_MODEL")]
    model: String,

    /// Transcription model to use for audio input
    #[arg(long, env = "MIM_STT_MODEL", default_value = "turbo")]
    stt_model: String,

    #[cfg(feature = "audio")]
    #[command(flatten)]
    audio: AudioArgs,
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
    Starting { cancel: Cancel },
    /// A streaming event from the agent.
    Output(OutputEvent),
    /// The current turn completed.
    TurnDone,
    /// An error occurred during the turn.
    Error(String),
}

async fn agent_task<R, T>(
    mut agent: Agent<R, T>,
    mut input_rx: mpsc::Receiver<Input>,
    output_tx: mpsc::UnboundedSender<AgentEvent>,
) where
    R: ResponseProvider + Send + 'static,
    T: TranscriptionProvider + Send + 'static,
    R::Error: std::fmt::Display + Send + Sync + 'static,
    T::Error: std::fmt::Display + Send + Sync + 'static,
{
    while let Some(input) = input_rx.recv().await {
        let cancel = Cancel::new();
        if output_tx
            .send(AgentEvent::Starting {
                cancel: cancel.clone(),
            })
            .is_err()
        {
            break;
        }

        let tx = output_tx.clone();
        let result = agent
            .run(input, cancel, move |event| {
                let _ = tx.send(AgentEvent::Output(event));
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

#[cfg(feature = "audio")]
struct CaptureHandle {
    _guard: capture::AudioGuard,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "audio")]
impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(feature = "audio")]
fn start_capture(
    audio_args: &AudioArgs,
    input_tx: Sender<Input>,
    voice_tx: mpsc::UnboundedSender<VoiceStatus>,
) -> Result<CaptureHandle> {
    use crate::capture::AudioCapture;
    use crate::voice::{VoiceDetector, VoiceEvent};
    use futures::StreamExt;

    let host = capture::resolve_host(audio_args.audio_host.as_deref())?;
    let device = capture::resolve_device(&host, audio_args.audio_input.as_deref())?;
    let (audio, guard) = AudioCapture::new(device)?.stream()?;
    let detector = VoiceDetector::new(
        audio_args.vad_threshold,
        audio_args.vad_silence,
        &audio_args.vad_model,
    )?;

    let task = tokio::spawn(async move {
        let mut events = detector.detect(audio);
        while let Some(event) = events.next().await {
            match event {
                VoiceEvent::SpeechStart => {
                    let _ = voice_tx.send(VoiceStatus::Listening);
                }
                VoiceEvent::SpeechEnd(samples) => {
                    let _ = voice_tx.send(VoiceStatus::Processing);
                    if input_tx.send(Input::Audio(samples)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    Ok(CaptureHandle {
        _guard: guard,
        task,
    })
}

#[cfg(feature = "audio")]
struct PlaybackHandle {
    #[allow(dead_code)]
    sender: playback::PlaybackSender,
    _guard: playback::PlaybackGuard,
}

#[cfg(feature = "audio")]
fn start_playback(audio_args: &AudioArgs) -> Result<PlaybackHandle> {
    let host = capture::resolve_host(audio_args.audio_host.as_deref())?;
    let device = playback::resolve_output_device(&host, audio_args.audio_output.as_deref())?;
    let pb = playback::AudioPlayback::new(device)?;
    tracing::debug!(device = pb.device_name(), "playback device");
    let (sender, guard) = pb.start()?;
    Ok(PlaybackHandle {
        sender,
        _guard: guard,
    })
}

struct State {
    screen: Screen,
    events: EventStream,
    input_tx: Sender<Input>,
    output_rx: UnboundedReceiver<AgentEvent>,

    messages: Vec<Message>,
    pending: Option<Message>,
    spinner: Spinner,
    cancel: Option<Cancel>,
    prompt: Prompt,

    voice_rx: mpsc::UnboundedReceiver<VoiceStatus>,
    #[allow(dead_code)]
    voice_tx: mpsc::UnboundedSender<VoiceStatus>,

    #[cfg(feature = "audio")]
    audio_args: AudioArgs,
    #[cfg(feature = "audio")]
    capture: Option<CaptureHandle>,
    #[cfg(feature = "audio")]
    playback: Option<PlaybackHandle>,
}

async fn run(args: Args) -> Result<()> {
    #[cfg(feature = "audio")]
    let audio_args = args.audio;

    let mut state = {
        let ctx = Context::new(args.path)?;
        debug!(root=?ctx.root, cwd=?ctx.cwd, "mim context");

        let agent = {
            let response_provider = OpenAIProvider::new();
            let transcription_provider = OpenAIProvider::new();
            let tools = tool::make_tools()?;
            let session = Session::open(ctx.session_path)?;

            Agent::new(
                response_provider,
                transcription_provider,
                args.model,
                args.stt_model,
                tools,
                session,
            )
        };

        let (input_tx, output_rx) = {
            let (input_tx, input_rx) = mpsc::channel::<Input>(16);
            let (output_tx, output_rx) = mpsc::unbounded_channel::<AgentEvent>();

            tokio::spawn(agent_task(agent, input_rx, output_tx));

            (input_tx, output_rx)
        };

        let mut screen = Screen::new()?;
        let events = screen.take_events().ok_or(anyhow!("no event stream"))?;

        let prompt = Prompt::new(args.mode);
        let messages: Vec<Message> = Vec::new();
        let cancel: Option<Cancel> = None;
        let spinner = Spinner::cycle(Spinner::ASCII.iter().copied());

        let (voice_tx, voice_rx) = mpsc::unbounded_channel::<VoiceStatus>();

        #[cfg(feature = "audio")]
        let capture = if prompt.mode() == PromptMode::Audio {
            Some(start_capture(&audio_args, input_tx.clone(), voice_tx.clone())?)
        } else {
            None
        };

        #[cfg(feature = "audio")]
        let playback = if prompt.mode() == PromptMode::Audio {
            Some(start_playback(&audio_args)?)
        } else {
            None
        };

        State {
            screen,
            events,
            input_tx,
            output_rx,
            messages,
            pending: None,
            spinner,
            cancel,
            prompt,
            voice_rx,
            voice_tx,
            #[cfg(feature = "audio")]
            audio_args,
            #[cfg(feature = "audio")]
            capture,
            #[cfg(feature = "audio")]
            playback,
        }
    };

    loop {
        // render
        let mut frame = state.screen.begin()?;

        let mut messages = VStack::new().spacing(1);
        for m in &mut state.messages {
            messages = messages.add(m);
        }
        if let Some(p) = &mut state.pending {
            messages = messages.add(p);
        }
        frame.add(&mut messages);

        if state.cancel.is_some() {
            frame.add(&mut state.spinner);
        }

        frame.add(&mut state.prompt);

        state.screen.end(frame)?;

        // events
        tokio::select! {
            Some(result) = state.events.next(&mut state.screen) => {
                match result? {
                    ScreenEvent::Signal(signal) => match signal {
                        Signal::Interrupt => {
                            if !state.prompt.is_empty() {
                                state.prompt.clear();
                            } else if let Some(cancel) = state.cancel.take() {
                                cancel.cancel();
                            } else {
                                break;
                            }
                        }
                        Signal::Suspend => {
                            continue;
                        }
                        Signal::Quit => {
                            break;
                        }
                    }
                    ScreenEvent::Event(Event::Key(KeyEvent {
                        code: KeyCode::Tab,
                        kind: KeyEventKind::Press,
                        ..
                    })) => {
                        state.prompt.toggle_mode();
                        #[cfg(feature = "audio")]
                        match state.prompt.mode() {
                            PromptMode::Audio => {
                                state.prompt.set_voice_status(VoiceStatus::Silence);
                                state.capture = Some(start_capture(
                                    &state.audio_args,
                                    state.input_tx.clone(),
                                    state.voice_tx.clone(),
                                )?);
                                state.playback = Some(start_playback(&state.audio_args)?);
                            }
                            PromptMode::Text => {
                                state.capture = None;
                                state.playback = None;
                                state.prompt.clear_transcription();
                            }
                        }
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
                                let _ = state.input_tx.send(Input::Text(text)).await;
                            }
                            EditorAction::Eof => break,
                        }
                    }
                }
            }
            Some(output) = state.output_rx.recv() => {
                match output {
                    AgentEvent::Starting { cancel } => {
                        state.cancel = Some(cancel);
                    }
                    AgentEvent::Output(OutputEvent::Delta(ref entry)) => {
                        state.spinner.step();
                        match entry {
                            Entry::Message(m)
                                if matches!(m.role, EntryRole::User) =>
                            {
                                // Transcription in progress — show in prompt
                                if let Some(MessageContent::Text { text }) =
                                    m.content.first()
                                {
                                    state.prompt.set_transcription(text);
                                }
                            }
                            _ => {
                                // Assistant content streaming
                                state.pending =
                                    Some(Message::from_entry(entry));
                            }
                        }
                    }
                    AgentEvent::Output(OutputEvent::Entry(ref entry)) => {
                        state.spinner.step();
                        if let Entry::Message(m) = entry {
                            if matches!(m.role, EntryRole::User) {
                                state.prompt.clear_transcription();
                            }
                        }
                        state.pending = None;
                        state.messages.push(Message::from_entry(entry));
                    }
                    AgentEvent::TurnDone => {
                        state.cancel = None;
                        state.pending = None;
                        state.prompt.clear_transcription();
                        state.prompt.set_voice_status(VoiceStatus::Silence);
                    }
                    AgentEvent::Error(e) => {
                        state.cancel = None;
                        state.prompt.clear_transcription();
                        state.prompt.set_voice_status(VoiceStatus::Silence);
                        if let Some(pending) = state.pending.take() {
                            state.messages.push(pending);
                        }
                        if let Some(message) = state.messages.last_mut() {
                            message.push_error(&e);
                        } else {
                            state.messages.push(Message::error(&e));
                        }
                    }
                }
            }
            Some(status) = state.voice_rx.recv() => {
                state.prompt.set_voice_status(status);
            }
        }
    }

    Ok(())
}
