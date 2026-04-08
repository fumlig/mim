use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show},
    event::{Event, EventStream},
    queue,
    style::{Attribute, SetAttribute},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
};
use futures::StreamExt;

use crate::format::{extract_cursor, truncate_to_width};
use crate::widget::Widget;

/// Per-render frame. Accumulates lines from widgets and raw text.
/// Created by [`Screen::begin`], consumed by [`Screen::end`].
///
/// The hardware cursor position is not stored on the frame: focused widgets
/// embed [`crate::format::CURSOR_MARKER`] in their rendered lines, and
/// [`Screen::end`] extracts it once all widgets have been added.
pub struct Frame {
    /// Rendered lines.
    lines: Vec<String>,
    /// Terminal width at frame creation.
    width: usize,
    /// Terminal height at frame creation.
    height: usize,
    /// When set, all reachable lines are rewritten regardless of diff.
    reset: bool,
}

impl Frame {
    /// Terminal width for this frame.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Terminal height for this frame.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Force a full redraw of all reachable lines for this frame.
    pub fn reset(&mut self) {
        self.reset = true;
    }

    /// Append a widget's rendered lines.
    ///
    /// If the widget is focused, it will have embedded
    /// [`crate::format::CURSOR_MARKER`] in its output; [`Screen::end`] finds
    /// the marker and places the hardware cursor accordingly.
    pub fn add(&mut self, widget: &mut impl Widget) {
        self.lines.extend(widget.render(self.width));
    }

    /// Append a single pre-formatted line.
    pub fn add_line(&mut self, line: String) {
        self.lines.push(line);
    }
}

/// Immediate mode scrolling terminal rendering
pub struct Screen {
    /// Last rendered frame. `None` before first render.
    last_frame: Option<Frame>,
    /// Row in our frame where the terminal cursor sits.
    cursor_row: usize,
    /// Whether raw mode is active.
    active: bool,
    /// Async stream of crossterm events.
    events: EventStream,
}

impl Screen {
    pub fn new() -> io::Result<Self> {
        Self::enter()
    }

    /// Enter raw mode, hide the cursor, and return a new renderer.
    pub fn enter() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(io::stdout(), Hide)?;

        Ok(Self {
            last_frame: None,
            cursor_row: 0,
            active: true,
            events: EventStream::new(),
        })
    }

    /// Leave raw mode and move the cursor below rendered content.
    pub fn leave(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;

        let mut stdout = io::stdout();
        if let Some(last) = self.last_frame.as_ref() {
            // Move the hardware cursor down to the last rendered line so the
            // shell prompt doesn't overwrite content below the focused
            // widget's cursor position.
            let last_row = last.lines.len().saturating_sub(1);
            let target = last_row.max(self.cursor_row);
            self.move_cursor(&mut stdout, target, 0)?;
            write!(stdout, "\r\n")?;
        }
        crossterm::execute!(stdout, Show)?;
        crossterm::terminal::disable_raw_mode()?;
        stdout.flush()
    }

    /// Suspend the process (Ctrl+Z).
    ///
    /// Restores the terminal, sends `SIGTSTP` to ourselves, and re-enters
    /// raw mode when the shell resumes us with `fg`.
    pub fn suspend(&mut self) -> io::Result<()> {
        self.leave()?;

        #[cfg(unix)]
        unsafe {
            libc::raise(libc::SIGTSTP);
        }

        // Resumed — re-enter raw mode and force full redraw.
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(io::stdout(), Hide)?;
        self.active = true;
        self.last_frame = None;
        self.cursor_row = 0;
        Ok(())
    }

    /// Quit the process with `SIGQUIT` (Ctrl+\).
    ///
    /// Restores the terminal first so the shell isn't left in a broken state.
    pub fn quit(&mut self) -> io::Result<()> {
        self.leave()?;

        #[cfg(unix)]
        unsafe {
            libc::raise(libc::SIGQUIT);
        }

        Ok(())
    }

    /// Wait for the next crossterm event asynchronously.
    pub async fn event(&mut self) -> io::Result<Event> {
        self.events.next().await.unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "event stream closed",
            ))
        })
    }

    /// Begin a new render pass.
    pub fn begin(&self) -> io::Result<Frame> {
        let (width, height) = crossterm::terminal::size()?;

        Ok(Frame {
            lines: Vec::new(),
            width: width as usize,
            height: height as usize,
            reset: false,
        })
    }

    /// End render pass and show frame in terminal.
    pub fn end(&mut self, mut frame: Frame) -> io::Result<()> {
        let mut stdout = io::stdout();

        queue!(stdout, BeginSynchronizedUpdate)?;

        // Extract the cursor marker once all widgets have rendered and
        // strip it from the lines in place. This must happen before the
        // diff so `last_frame` and `next` compare stripped-to-stripped.
        let cursor = extract_cursor(&mut frame.lines);
        let next = frame;

        if let Some(last) = self.last_frame.take() {
            if last.width != next.width || next.reset {
                self.full_render(&mut stdout, &next, true)
            } else {
                self.delta_render(&mut stdout, &last, &next)
            }
        } else {
            self.full_render(&mut stdout, &next, false)
        }?;

        if let Some((row, col)) = cursor {
            self.move_cursor(&mut stdout, row, col)?;
            queue!(&mut stdout, Show)?;
        } else {
            queue!(&mut stdout, Hide)?;
        }

        queue!(stdout, EndSynchronizedUpdate)?;

        self.last_frame = Some(next);

        stdout.flush()
    }

    /// Render everything from the top
    fn full_render(&mut self, out: &mut impl Write, next: &Frame, clear: bool) -> io::Result<()> {
        if clear {
            queue!(
                out,
                Clear(ClearType::All), // ESC[2J — clear visible screen
                crossterm::cursor::MoveTo(0, 0), // ESC[H  — cursor home
                Clear(ClearType::Purge), // ESC[3J — clear scrollback
            )?;
        }
        Self::write_lines(out, &next.lines, next.width)?;
        self.cursor_row = next.lines.len().saturating_sub(1);
        Ok(())
    }

    /// Move to first changed line, clear to end, render changed lines.
    fn delta_render(&mut self, out: &mut impl Write, last: &Frame, next: &Frame) -> io::Result<()> {
        let old_lines = &last.lines;
        let new_lines = &next.lines;

        // Find first changed line.
        let max_len = old_lines.len().max(new_lines.len());
        let first = match (0..max_len).find(|&i| {
            old_lines.get(i).map(String::as_str).unwrap_or("")
                != new_lines.get(i).map(String::as_str).unwrap_or("")
        }) {
            Some(f) => f,
            None => return Ok(()), // nothing changed
        };

        // We can only move the cursor up within the visible area.
        // If the change is above the viewport, fall back to full render.
        let height = next.height as usize;
        let reachable_top = self.cursor_row.saturating_sub(height.saturating_sub(1));
        if first < reachable_top {
            return self.full_render(out, next, true);
        }

        // Move cursor to the first changed line, clear to end of screen,
        // then render all lines from here.
        self.move_cursor(out, first, 0)?;
        queue!(out, Clear(ClearType::FromCursorDown))?;
        Self::write_lines(out, &next.lines[first..], next.width)?;

        self.cursor_row = next.lines.len().saturating_sub(1);
        Ok(())
    }

    /// Write lines, truncating to `width` and appending a style reset after each.
    fn write_lines(out: &mut impl Write, lines: &[String], width: usize) -> io::Result<()> {
        let w = width;
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                write!(out, "\r\n")?;
            }
            write!(out, "{}", truncate_to_width(line, w, ""))?;
            queue!(out, SetAttribute(Attribute::Reset))?;
        }

        Ok(())
    }

    /// Move the hardware cursor
    fn move_cursor(&mut self, out: &mut impl Write, row: usize, col: usize) -> io::Result<()> {
        let delta = row as isize - self.cursor_row as isize;
        let abs = u16::try_from(delta.unsigned_abs())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if delta > 0 {
            queue!(out, MoveDown(abs))?;
        } else if delta < 0 {
            queue!(out, MoveUp(abs))?;
        }

        queue!(out, MoveToColumn(col as u16))?;

        self.cursor_row = row;
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}
