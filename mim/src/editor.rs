use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::widget::Widget;

/// Action returned by [`Editor::handle`].
pub enum EditorAction {
    /// User pressed Enter — contains the submitted text.
    Submit(String),
    /// Ctrl+D on empty editor — end of input.
    Eof,
    /// Ctrl+C — interrupt.
    Interrupt,
    /// Ctrl+Z — suspend process.
    Suspend,
    /// Ctrl+\ — quit with core dump.
    Quit,
}

/// Multiline text editor widget with word wrapping.
pub struct Editor {
    buf: Vec<char>,
    cursor: usize,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            cursor: 0,
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
    }

    /// Process a crossterm event. Returns an action if one was triggered.
    pub fn handle(&mut self, event: Event) -> Option<EditorAction> {
        let key = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => key,
            _ => return None,
        };

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Char('d') if ctrl => {
                if self.buf.is_empty() {
                    return Some(EditorAction::Eof);
                }
            }
            KeyCode::Char('c') if ctrl => {
                if self.buf.is_empty() {
                    return Some(EditorAction::Interrupt);
                }
                self.clear();
            }
            KeyCode::Char('z') if ctrl => {
                return Some(EditorAction::Suspend);
            }
            KeyCode::Char('\\') if ctrl => {
                return Some(EditorAction::Quit);
            }

            // Submit
            KeyCode::Enter if !ctrl && !shift && !alt => {
                let text: String = self.buf.drain(..).collect();
                self.cursor = 0;
                return Some(EditorAction::Submit(text));
            }

            // Insert newline
            KeyCode::Enter if shift || alt => {
                self.buf.insert(self.cursor, '\n');
                self.cursor += 1;
            }
            KeyCode::Char('j') if ctrl => {
                self.buf.insert(self.cursor, '\n');
                self.cursor += 1;
            }

            // Navigation
            KeyCode::Left if !ctrl => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Right if !ctrl => {
                self.cursor = (self.cursor + 1).min(self.buf.len());
            }
            KeyCode::Up if !ctrl => {
                self.move_vertical(-1);
            }
            KeyCode::Down if !ctrl => {
                self.move_vertical(1);
            }
            KeyCode::Home | KeyCode::Char('a') if ctrl || matches!(key.code, KeyCode::Home) => {
                self.cursor = self.line_start(self.cursor);
            }
            KeyCode::End | KeyCode::Char('e') if ctrl || matches!(key.code, KeyCode::End) => {
                self.cursor = self.line_end(self.cursor);
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
                let start = self.line_start(self.cursor);
                self.buf.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Char('k') if ctrl => {
                let end = self.line_end(self.cursor);
                if end == self.cursor && self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                } else {
                    self.buf.drain(self.cursor..end);
                }
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

    /// Index of the start of the logical line containing `pos`.
    fn line_start(&self, pos: usize) -> usize {
        self.buf[..pos]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1)
    }

    /// Index of the end of the logical line containing `pos` (the '\n' or buf.len()).
    fn line_end(&self, pos: usize) -> usize {
        self.buf[pos..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.buf.len(), |i| pos + i)
    }

    /// Column offset of `pos` within its logical line.
    fn column(&self, pos: usize) -> usize {
        pos - self.line_start(pos)
    }

    /// Move the cursor up (delta = -1) or down (delta = 1) by one logical line,
    /// preserving column position as much as possible.
    fn move_vertical(&mut self, delta: isize) {
        let col = self.column(self.cursor);
        if delta < 0 {
            let start = self.line_start(self.cursor);
            if start == 0 {
                return;
            }
            let prev_end = start - 1;
            let prev_start = self.line_start(prev_end);
            let prev_len = prev_end - prev_start;
            self.cursor = prev_start + col.min(prev_len);
        } else {
            let end = self.line_end(self.cursor);
            if end == self.buf.len() {
                return;
            }
            let next_start = end + 1;
            let next_end = self.line_end(next_start);
            let next_len = next_end - next_start;
            self.cursor = next_start + col.min(next_len);
        }
    }

    /// Delete the word before the cursor (Ctrl+W), stopping at line start.
    fn delete_word_back(&mut self) {
        let stop = self.line_start(self.cursor);
        while self.cursor > stop && self.buf[self.cursor - 1] == ' ' {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
        while self.cursor > stop && self.buf[self.cursor - 1] != ' ' {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
    }

    /// Build the display lines and locate the cursor within them.
    /// Returns (lines, cursor_row, cursor_col).
    fn layout(&self, width: usize) -> (Vec<String>, usize, usize) {
        let w = width.max(1);
        let text: String = self.buf.iter().collect();
        let logical_lines: Vec<&str> = text.split('\n').collect();

        let mut display_lines: Vec<String> = Vec::new();
        let mut cursor_row = 0;
        let mut cursor_col = 0;
        let mut buf_offset: usize = 0;

        for (li, &logical_line) in logical_lines.iter().enumerate() {
            let chars: Vec<char> = logical_line.chars().collect();
            let breaks = line_breaks(&chars, w);

            if self.cursor >= buf_offset {
                let cursor_in_line = self.cursor - buf_offset;
                if cursor_in_line <= chars.len() {
                    for (si, &seg_start) in breaks.iter().enumerate() {
                        let seg_end = breaks.get(si + 1).copied().unwrap_or(chars.len());
                        if cursor_in_line < seg_end
                            || (cursor_in_line == seg_end && si == breaks.len() - 1)
                        {
                            cursor_row = display_lines.len() + si;
                            cursor_col = cursor_in_line - seg_start;
                            break;
                        }
                    }
                }
            }

            for (si, &seg_start) in breaks.iter().enumerate() {
                let seg_end = breaks.get(si + 1).copied().unwrap_or(chars.len());

                let mut end = seg_end;
                if si < breaks.len() - 1 {
                    while end > seg_start && chars[end - 1] == ' ' {
                        end -= 1;
                    }
                }

                let content: String = chars[seg_start..end].iter().collect();
                display_lines.push(content);
            }

            buf_offset += chars.len();
            if li < logical_lines.len() - 1 {
                buf_offset += 1;
            }
        }

        if display_lines.is_empty() {
            display_lines.push(String::new());
        }

        (display_lines, cursor_row, cursor_col)
    }
}

/// Compute word-wrap break positions for a line of characters.
/// Returns the starting character index for each display line segment.
fn line_breaks(chars: &[char], max_width: usize) -> Vec<usize> {
    if chars.is_empty() || max_width == 0 {
        return vec![0];
    }

    let mut breaks = vec![0usize];
    let mut line_start: usize = 0;
    let mut last_space: Option<usize> = None;

    for (i, &ch) in chars.iter().enumerate() {
        if ch == ' ' {
            last_space = Some(i);
        }

        let col = i - line_start;
        if col >= max_width {
            if let Some(sp) = last_space {
                if sp >= line_start {
                    line_start = sp + 1;
                    breaks.push(line_start);
                    last_space = None;
                    for j in line_start..=i {
                        if chars[j] == ' ' {
                            last_space = Some(j);
                        }
                    }
                } else {
                    line_start = i;
                    breaks.push(line_start);
                    last_space = None;
                }
            } else {
                line_start = i;
                breaks.push(line_start);
            }
        }
    }

    breaks
}

impl Widget for Editor {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return vec![String::new()];
        }

        let (mut lines, crow, ccol) = self.layout(width);

        // Draw reverse-video cursor character on the cursor line.
        if let Some(line) = lines.get_mut(crow) {
            let chars: Vec<char> = line.chars().collect();
            if ccol < chars.len() {
                let before: String = chars[..ccol].iter().collect();
                let cursor_ch = chars[ccol];
                let after: String = chars[ccol + 1..].iter().collect();
                *line = format!("{before}\x1b[7m{cursor_ch}\x1b[27m{after}");
            } else {
                line.push_str("\x1b[7m \x1b[27m");
            }
        }

        lines
    }

    fn cursor(&mut self, width: u16) -> Option<(usize, usize)> {
        let width = width as usize;
        if width == 0 {
            return Some((0, 0));
        }
        let (_, row, col) = self.layout(width);
        Some((row, col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        Editor::new()
    }

    fn type_str(p: &mut Editor, s: &str) {
        for c in s.chars() {
            p.buf.insert(p.cursor, c);
            p.cursor += 1;
        }
    }

    #[test]
    fn single_line_render() {
        let mut p = editor();
        type_str(&mut p, "hello");
        let lines = p.render(80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("hello"));
    }

    #[test]
    fn multiline_render() {
        let mut p = editor();
        type_str(&mut p, "aaa\nbbb");
        let lines = p.render(80);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("aaa"));
        assert!(lines[1].starts_with("bbb"));
    }

    #[test]
    fn word_wrapping() {
        let mut p = editor();
        type_str(&mut p, "aaaa bbbb");
        // Width 6 => "aaaa" fits, "bbbb" wraps
        let lines = p.render(6);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("aaaa"));
        assert!(lines[1].starts_with("bbbb"));
    }

    #[test]
    fn cursor_position_end() {
        let mut p = editor();
        type_str(&mut p, "hi");
        let (_, row, col) = p.layout(80);
        assert_eq!(row, 0);
        assert_eq!(col, 2);
    }

    #[test]
    fn cursor_position_second_line() {
        let mut p = editor();
        type_str(&mut p, "aaa\nb");
        let (_, row, col) = p.layout(80);
        assert_eq!(row, 1);
        assert_eq!(col, 1);
    }

    #[test]
    fn line_navigation() {
        let mut p = editor();
        type_str(&mut p, "abcd\nef");
        assert_eq!(p.cursor, 7);
        p.move_vertical(-1);
        assert_eq!(p.cursor, 2);
        p.move_vertical(1);
        assert_eq!(p.cursor, 7);
    }

    #[test]
    fn line_navigation_clamps_column() {
        let mut p = editor();
        type_str(&mut p, "abcdef\nhi");
        p.move_vertical(-1);
        assert_eq!(p.cursor, 2);
        p.cursor = 6;
        p.move_vertical(1);
        assert_eq!(p.cursor, 9);
    }

    #[test]
    fn home_end_multiline() {
        let mut p = editor();
        type_str(&mut p, "aaa\nbbb");
        let start = p.line_start(p.cursor);
        assert_eq!(start, 4);
        let end = p.line_end(p.cursor);
        assert_eq!(end, 7);
    }

    #[test]
    fn ctrl_u_multiline() {
        let mut p = editor();
        type_str(&mut p, "aaa\nbbb");
        let start = p.line_start(p.cursor);
        p.buf.drain(start..p.cursor);
        p.cursor = start;
        assert_eq!(p.text(), "aaa\n");
    }

    #[test]
    fn empty_editor_render() {
        let mut p = editor();
        let lines = p.render(80);
        assert_eq!(lines.len(), 1);
        // Empty line with just a cursor
        assert!(lines[0].contains("\x1b[7m"));
    }

    #[test]
    fn cursor_after_trailing_space() {
        let mut p = editor();
        type_str(&mut p, "hello ");
        let (_, row, col) = p.layout(80);
        assert_eq!(row, 0);
        assert_eq!(col, 6);
    }

    #[test]
    fn cursor_after_multiple_spaces() {
        let mut p = editor();
        type_str(&mut p, "hi   ");
        let (_, row, col) = p.layout(80);
        assert_eq!(row, 0);
        assert_eq!(col, 5);
    }

    #[test]
    fn cursor_at_end_of_wrapped_line() {
        let mut p = editor();
        type_str(&mut p, "hello world");
        let (_, row, col) = p.layout(8);
        assert_eq!(row, 1);
        assert_eq!(col, 5);
    }

    #[test]
    fn cursor_on_space_at_wrap_point() {
        let mut p = editor();
        type_str(&mut p, "hello world");
        p.cursor = 5; // on the space
        let (_, row, col) = p.layout(8);
        assert_eq!(row, 0);
        assert_eq!(col, 5);
    }

    #[test]
    fn cursor_on_first_char_of_wrapped_line() {
        let mut p = editor();
        type_str(&mut p, "hello world");
        p.cursor = 6; // on 'w'
        let (_, row, col) = p.layout(8);
        assert_eq!(row, 1);
        assert_eq!(col, 0);
    }

    #[test]
    fn hard_break_long_word() {
        let mut p = editor();
        type_str(&mut p, "abcdefghij");
        let (lines, _, _) = p.layout(5);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "abcde");
        assert_eq!(lines[1], "fghij");
    }

    #[test]
    fn line_breaks_basic() {
        let chars: Vec<char> = "hello world".chars().collect();
        assert_eq!(line_breaks(&chars, 8), vec![0, 6]);
    }

    #[test]
    fn line_breaks_no_wrap() {
        let chars: Vec<char> = "hello".chars().collect();
        assert_eq!(line_breaks(&chars, 80), vec![0]);
    }

    #[test]
    fn line_breaks_hard() {
        let chars: Vec<char> = "abcdefghij".chars().collect();
        assert_eq!(line_breaks(&chars, 5), vec![0, 5]);
    }

    #[test]
    fn line_breaks_empty() {
        assert_eq!(line_breaks(&[], 10), vec![0]);
    }
}
