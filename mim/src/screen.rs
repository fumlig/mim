use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show},
    event::{Event, EventStream},
    queue,
    style::{Attribute, SetAttribute},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
};
use futures::StreamExt;

use crate::format::truncate_to_width;
use crate::widget::Widget;

/// Per-render frame. Accumulates lines from widgets and raw text.
/// Created by [`Renderer::begin`], consumed by [`Renderer::end`].
pub struct Frame {
    /// Rendered lines.
    lines: Vec<String>,
    /// Terminal width at frame creation.
    width: u16,
    /// Terminal height at frame creation.
    height: u16,
    /// Hardware cursor position requested by the focused widget (absolute row, col).
    cursor: Option<(usize, usize)>,
    /// When set, all reachable lines are rewritten regardless of diff.
    reset: bool,
}

impl Frame {
    /// Terminal width for this frame.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Terminal height for this frame.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Force a full redraw of all reachable lines for this frame.
    pub fn reset(&mut self) {
        self.reset = true;
    }

    /// Append a widget's rendered lines.
    pub fn add(&mut self, widget: &mut impl Widget) {
        self.lines.extend(widget.render(self.width));
    }

    /// Append a widget's rendered lines and track its cursor position.
    ///
    /// Only one widget per frame should be focused. The terminal's hardware
    /// cursor will be placed at the position reported by [`Widget::cursor`].
    pub fn add_focused(&mut self, widget: &mut impl Widget) {
        let base_row = self.lines.len();
        // Render first so the widget can update internal layout state
        // (e.g. scroll offset) before we query cursor position.
        self.lines.extend(widget.render(self.width));
        if let Some((row, col)) = widget.cursor(self.width) {
            self.cursor = Some((base_row + row, col));
        }
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
        if self.last_frame.is_some() {
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
            width,
            height,
            cursor: None,
            reset: false,
        })
    }

    /// End render pass and show frame in terminal.
    pub fn end(&mut self, frame: Frame) -> io::Result<()> {
        let mut stdout = io::stdout();

        queue!(stdout, BeginSynchronizedUpdate)?;

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

        self.update_cursor(&mut stdout, &next)?;

        queue!(stdout, EndSynchronizedUpdate)?;

        self.last_frame = Some(next);

        stdout.flush()
    }

    /// Render everything from the top
    fn full_render(&mut self, out: &mut impl Write, next: &Frame, clear: bool) -> io::Result<()> {
        if clear {
            queue!(
                out,
                Clear(ClearType::All),           // ESC[2J — clear visible screen
                crossterm::cursor::MoveTo(0, 0), // ESC[H  — cursor home
                Clear(ClearType::Purge),         // ESC[3J — clear scrollback
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

        // Move cursor to the first changed line.
        if first < self.cursor_row {
            queue!(out, MoveUp((self.cursor_row - first) as u16))?;
        } else if first > self.cursor_row {
            queue!(out, MoveDown((first - self.cursor_row) as u16))?;
        }

        // Clear from cursor to end of screen, then render all lines from here.
        queue!(out, MoveToColumn(0), Clear(ClearType::FromCursorDown))?;
        Self::write_lines(out, &next.lines[first..], next.width)?;

        self.cursor_row = next.lines.len().saturating_sub(1);
        Ok(())
    }

    /// Write lines, truncating to `width` and appending a style reset after each.
    fn write_lines(out: &mut impl Write, lines: &[String], width: u16) -> io::Result<()> {
        let w = width as usize;
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                write!(out, "\r\n")?;
            }
            write!(out, "{}", truncate_to_width(line, w, ""))?;
            queue!(out, SetAttribute(Attribute::Reset))?;
        }

        Ok(())
    }

    /// Position the hardware cursor at the frame's requested position,
    /// or hide it if no widget requested focus.
    fn update_cursor(&mut self, out: &mut impl Write, next: &Frame) -> io::Result<()> {
        if let Some((row, col)) = next.cursor {
            let delta = row as isize - self.cursor_row as isize;
            if delta > 0 {
                queue!(out, MoveDown(delta as u16))?;
            } else if delta < 0 {
                queue!(out, MoveUp((-delta) as u16))?;
            }
            queue!(out, MoveToColumn(col as u16), Show)?;
            self.cursor_row = row;
        } else {
            queue!(out, Hide)?;
        }
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}
