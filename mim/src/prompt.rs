use std::cell::Cell;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::widget::Widget;
use crate::format::visible_width;

/// Action returned by [`Prompt::handle`].
pub enum PromptAction {
    /// User pressed Enter — contains the submitted text.
    Submit(String),
    /// Ctrl+D on empty prompt — end of input.
    Eof,
    /// Ctrl+C — interrupt.
    Interrupt,
    /// Ctrl+Z — suspend process.
    Suspend,
    /// Ctrl+\ — quit with core dump.
    Quit,
}

/// Single-line input prompt with cursor and horizontal scrolling.
pub struct Prompt {
    prefix: String,
    prefix_width: usize,
    buf: Vec<char>,
    cursor: usize,
    /// Horizontal scroll offset (chars hidden on the left).
    /// Interior mutability so `render` (which is `&self`) can adjust it.
    scroll: Cell<usize>,
}

impl Prompt {
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            prefix_width: visible_width(prefix),
            buf: Vec::new(),
            cursor: 0,
            scroll: Cell::new(0),
        }
    }

    /// The current input text.
    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// Whether the input buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Clear the input buffer and reset cursor.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.scroll.set(0);
    }

    /// Process a crossterm event. Returns an action if one was triggered.
    pub fn handle(&mut self, event: Event) -> Option<PromptAction> {
        let key = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => key,
            _ => return None,
        };

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('d') if ctrl => {
                if self.buf.is_empty() {
                    return Some(PromptAction::Eof);
                }
            }
            KeyCode::Char('c') if ctrl => {
                return Some(PromptAction::Interrupt);
            }
            KeyCode::Char('z') if ctrl => {
                return Some(PromptAction::Suspend);
            }
            KeyCode::Char('\\') if ctrl => {
                return Some(PromptAction::Quit);
            }
            KeyCode::Enter if !ctrl => {
                let text: String = self.buf.drain(..).collect();
                self.cursor = 0;
                self.scroll.set(0);
                return Some(PromptAction::Submit(text));
            }

            // Navigation
            KeyCode::Left if !ctrl => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Right if !ctrl => {
                self.cursor = (self.cursor + 1).min(self.buf.len());
            }
            KeyCode::Home | KeyCode::Char('a') if ctrl || matches!(key.code, KeyCode::Home) => {
                self.cursor = 0;
            }
            KeyCode::End | KeyCode::Char('e') if ctrl || matches!(key.code, KeyCode::End) => {
                self.cursor = self.buf.len();
            }

            // Deletion
            KeyCode::Backspace if !ctrl => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buf.remove(self.cursor);
                }
            }
            KeyCode::Delete if !ctrl => {
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                }
            }
            KeyCode::Char('u') if ctrl => {
                self.buf.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char('k') if ctrl => {
                self.buf.truncate(self.cursor);
            }
            KeyCode::Char('w') if ctrl => {
                self.delete_word_back();
            }

            // Character input
            KeyCode::Char(c) if !ctrl => {
                self.buf.insert(self.cursor, c);
                self.cursor += 1;
            }

            _ => {}
        }
        None
    }

    /// Delete the word before the cursor (Ctrl+W).
    fn delete_word_back(&mut self) {
        // Skip trailing whitespace
        while self.cursor > 0 && self.buf[self.cursor - 1] == ' ' {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
        // Skip word characters
        while self.cursor > 0 && self.buf[self.cursor - 1] != ' ' {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
    }

    /// Adjust scroll so the cursor is visible within `avail` columns.
    fn adjust_scroll(&self, avail: usize) {
        if avail == 0 {
            self.scroll.set(0);
            return;
        }
        let mut scroll = self.scroll.get();
        if self.cursor < scroll {
            scroll = self.cursor;
        }
        if self.cursor >= scroll + avail {
            scroll = self.cursor - avail + 1;
        }
        self.scroll.set(scroll);
    }
}

impl Widget for Prompt {
    fn render(&self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return vec![String::new()];
        }

        let avail = width.saturating_sub(self.prefix_width);
        if avail == 0 {
            return vec![self.prefix.clone()];
        }

        self.adjust_scroll(avail);
        let scroll = self.scroll.get();
        let visible_end = (scroll + avail).min(self.buf.len());

        // Text before cursor
        let before: String = self.buf[scroll..self.cursor].iter().collect();

        // Character under cursor (reversed video), or a space if at end
        let (cursor_ch, after_start) = if self.cursor < self.buf.len() {
            (self.buf[self.cursor], self.cursor + 1)
        } else {
            (' ', self.buf.len())
        };

        // Text after cursor
        let after: String = self.buf[after_start..visible_end].iter().collect();

        let line = format!(
            "{}{}\x1b[7m{}\x1b[27m{}",
            self.prefix, before, cursor_ch, after,
        );

        vec![line]
    }

    fn cursor(&self, width: u16) -> Option<(usize, usize)> {
        let avail = (width as usize).saturating_sub(self.prefix_width);
        self.adjust_scroll(avail);
        let scroll = self.scroll.get();
        let col = self.prefix_width + (self.cursor - scroll);
        Some((0, col))
    }
}
