use crossterm::{
    cursor,
    terminal::{self, Clear, ClearType},
    QueueableCommand,
};
use std::io::{self, stdout, Write};
use termimad::{FmtText, MadSkin};

pub struct MarkdownRenderer {
    skin: MadSkin,
    buffer: String,
    prev_lines: u16,
}

impl MarkdownRenderer {
    pub fn new(skin: MadSkin) -> Self {
        Self {
            skin,
            buffer: String::new(),
            prev_lines: 0,
        }
    }

    pub fn push(&mut self, delta: &str) -> io::Result<()> {
        self.buffer.push_str(delta);
        self.redraw()
    }

    pub fn finish(&mut self) -> io::Result<()> {
        if self.prev_lines == 0 && self.buffer.is_empty() {
            return Ok(());
        }
        self.redraw()?;
        let mut out = stdout().lock();
        out.queue(cursor::Show)?;
        out.flush()?;
        self.buffer.clear();
        self.prev_lines = 0;
        Ok(())
    }

    fn redraw(&mut self) -> io::Result<()> {
        let (width, _) = terminal::size()?;
        // Use width-1 to avoid terminal autowrap: lines padded to exactly
        // the terminal width (e.g. centered headers) cause the cursor to
        // wrap, producing a phantom blank line that breaks our line count.
        let render_width = (width as usize).saturating_sub(1).max(1);
        let fmt_text = FmtText::from(&self.skin, &self.buffer, Some(render_width));
        let rendered = format!("{}", fmt_text);
        let line_count = rendered.matches('\n').count() as u16;

        let mut out = stdout().lock();

        if self.prev_lines == 0 {
            out.queue(cursor::Hide)?;
        } else {
            out.queue(cursor::MoveUp(self.prev_lines))?;
        }
        out.queue(cursor::MoveToColumn(0))?;
        out.queue(Clear(ClearType::FromCursorDown))?;
        out.write_all(rendered.as_bytes())?;
        out.flush()?;

        self.prev_lines = line_count;
        Ok(())
    }
}
