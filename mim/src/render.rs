use crossterm::{
    cursor,
    terminal::{self, Clear, ClearType},
    QueueableCommand,
};
use std::io::{self, stdout, Write};
use termimad::{FmtText, MadSkin};

/// A live-updating region of terminal output.
///
/// Tracks how many lines were previously written and erases + redraws on each
/// call to [`update`](Self::update). The cursor is hidden while the region is
/// active and restored on [`finish`](Self::finish) (or [`Drop`]).
///
/// `Live` is content-agnostic — it accepts a pre-rendered string and handles
/// only the terminal mechanics. Pair it with [`render_markdown`] or any other
/// formatting function.
pub struct Live {
    prev_lines: u16,
    cursor_hidden: bool,
}

impl Live {
    pub fn new() -> Self {
        Self {
            prev_lines: 0,
            cursor_hidden: false,
        }
    }

    /// Replace the displayed content with `content`.
    ///
    /// On the first call the cursor is hidden. Each subsequent call erases the
    /// previous frame before writing the new one.
    pub fn update(&mut self, content: &str) -> io::Result<()> {
        let line_count = content.matches('\n').count() as u16;

        let mut out = stdout().lock();

        if !self.cursor_hidden {
            out.queue(cursor::Hide)?;
            self.cursor_hidden = true;
        }

        if self.prev_lines > 0 {
            out.queue(cursor::MoveUp(self.prev_lines))?;
        }
        out.queue(cursor::MoveToColumn(0))?;
        out.queue(Clear(ClearType::FromCursorDown))?;
        out.write_all(content.as_bytes())?;
        out.flush()?;

        self.prev_lines = line_count;
        Ok(())
    }

    /// Finalize the live region.
    ///
    /// Restores cursor visibility and resets internal state. The last frame
    /// remains on screen. This method is idempotent.
    pub fn finish(&mut self) -> io::Result<()> {
        if self.cursor_hidden {
            let mut out = stdout().lock();
            out.queue(cursor::Show)?;
            out.flush()?;
            self.cursor_hidden = false;
        }
        self.prev_lines = 0;
        Ok(())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.finish().ok();
    }
}

/// Render markdown `text` into a styled terminal string using `skin`.
///
/// Uses the current terminal width (minus one column to avoid autowrap
/// artifacts). This is a pure formatting function with no cursor side-effects.
pub fn render_markdown(skin: &MadSkin, text: &str) -> String {
    let (width, _) = terminal::size().unwrap_or((80, 24));
    let render_width = (width as usize).saturating_sub(1).max(1);
    let fmt_text = FmtText::from(skin, text, Some(render_width));
    format!("{}", fmt_text)
}
