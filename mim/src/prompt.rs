//! The bottom-of-screen prompt: owns the editor and the current input
//! mode (text or audio), and renders the horizontal rule + padding that
//! visually separates the prompt from the scrollback above it.
//!
//! Before `Prompt` existed, `main.rs` reached directly into the editor
//! and hand-assembled a `Block` with a `HorizontalBorder` on every frame.
//! Now the presentation lives here and main only knows "there is a
//! prompt, forward events to it, render it".

use clap::ValueEnum;
use crossterm::event::Event;

use crate::widget::{Editor, HorizontalBorder, Paragraph, Widget, WidgetExt};

// Re-exported so `main` only has to import from `crate::prompt` for the
// whole prompt API surface.
pub use crate::widget::EditorAction;

/// Prompt input mode. Also used by the CLI to pick the starting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum PromptMode {
    /// Typed text input via the [`Editor`].
    Text,
    /// Audio capture mode — the editor is still present but the visible
    /// status line indicates we're listening.
    Audio,
}

impl PromptMode {
    /// Next mode in the cycle; used by the Tab key in main.
    pub fn next(self) -> Self {
        let all = Self::value_variants();
        let i = all.iter().position(|m| *m == self).unwrap();
        all[(i + 1) % all.len()]
    }
}

/// The bottom prompt. Wraps an [`Editor`] plus a current [`PromptMode`]
/// and renders them as a bordered input area.
pub struct Prompt {
    editor: Editor,
    mode: PromptMode,
}

impl Prompt {
    pub fn new(mode: PromptMode) -> Self {
        Self {
            editor: Editor::new(),
            mode,
        }
    }

    pub fn mode(&self) -> PromptMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: PromptMode) {
        self.mode = mode;
    }

    /// Cycle to the next mode (bound to Tab by main).
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.next();
    }

    // ── Editor proxies ──────────────────────────────────────────────

    /// Whether the input buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.editor.is_empty()
    }

    /// Clear the input buffer.
    pub fn clear(&mut self) {
        self.editor.clear();
    }

    /// Feed a terminal event into the underlying editor.
    pub fn handle(&mut self, event: Event) -> Option<EditorAction> {
        self.editor.handle(event)
    }
}

impl Widget for Prompt {
    fn render(&mut self, width: usize) -> Vec<String> {
        // The visual layout is: horizontal rule, one blank row, content.
        // That's two bands on the top edge, so we nest two blocks: the
        // inner one owns the blank pad, the outer one owns the rule.
        match self.mode {
            PromptMode::Text => self
                .editor
                .block()
                .top(HorizontalBorder::line())
                .block()
                .pad_top(1)
                .render(width),
            PromptMode::Audio => Paragraph::new("audio")
                .block()
                .top(HorizontalBorder::line())
                .block()
                .pad_top(1)
                .render(width),
        }
    }
}
