use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::format::CURSOR_MARKER;
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
///
/// The buffer is stored as a vector of logical lines (one `Vec<char>` per
/// line) and the cursor lives in the same coordinate system as the buffer:
/// `row` indexes into `lines`, `col` indexes into `lines[row]`. There is
/// no separate "buffer position → display position" translation step.
///
/// During [`Widget::render`], the cursor is materialised by injecting
/// [`CURSOR_MARKER`] (and a reverse-video block) directly into the segment
/// of the wrapped output that contains it. The screen extracts the marker
/// later to position the hardware cursor; nothing in this module needs to
/// know or return display row/col coordinates.
pub struct Editor {
    /// Logical lines. Always non-empty: an "empty" editor is
    /// `vec![Vec::new()]`, not `vec![]`. This invariant lets every method
    /// index `lines[row]` without bounds checks.
    lines: Vec<Vec<char>>,
    /// Cursor row (`0..lines.len()`).
    row: usize,
    /// Cursor column within `lines[row]` (`0..=lines[row].len()`).
    col: usize,
    /// When true, `render` paints the reverse-video cursor block and embeds
    /// [`CURSOR_MARKER`] next to it. Defaults to true because mim currently
    /// only ever shows a single editor and it is always focused.
    pub focused: bool,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
            focused: true,
        }
    }

    /// The current input text, with logical lines joined by `\n`.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.extend(line.iter());
        }
        s
    }

    /// Whether the input buffer is empty (single empty line).
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Clear the input buffer and reset cursor.
    pub fn clear(&mut self) {
        self.lines = vec![Vec::new()];
        self.row = 0;
        self.col = 0;
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
                if self.is_empty() {
                    return Some(EditorAction::Eof);
                }
            }
            KeyCode::Char('c') if ctrl => {
                if self.is_empty() {
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
                let text = self.text();
                self.clear();
                return Some(EditorAction::Submit(text));
            }

            // Insert newline
            KeyCode::Enter if shift || alt => {
                self.insert_newline();
            }
            KeyCode::Char('j') if ctrl => {
                self.insert_newline();
            }

            // Navigation
            KeyCode::Left if !ctrl => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                }
            }
            KeyCode::Right if !ctrl => {
                if self.col < self.lines[self.row].len() {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
            KeyCode::Up if !ctrl => {
                if self.row > 0 {
                    self.row -= 1;
                    self.col = self.col.min(self.lines[self.row].len());
                }
            }
            KeyCode::Down if !ctrl => {
                if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = self.col.min(self.lines[self.row].len());
                }
            }
            KeyCode::Home | KeyCode::Char('a') if ctrl || matches!(key.code, KeyCode::Home) => {
                self.col = 0;
            }
            KeyCode::End | KeyCode::Char('e') if ctrl || matches!(key.code, KeyCode::End) => {
                self.col = self.lines[self.row].len();
            }

            // Deletion
            KeyCode::Backspace if !ctrl => {
                if self.col > 0 {
                    self.col -= 1;
                    self.lines[self.row].remove(self.col);
                } else if self.row > 0 {
                    let curr = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                    self.lines[self.row].extend(curr);
                }
            }
            KeyCode::Delete if !ctrl => {
                if self.col < self.lines[self.row].len() {
                    self.lines[self.row].remove(self.col);
                } else if self.row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.row + 1);
                    self.lines[self.row].extend(next);
                }
            }
            KeyCode::Char('u') if ctrl => {
                self.lines[self.row].drain(..self.col);
                self.col = 0;
            }
            KeyCode::Char('k') if ctrl => {
                if self.col == self.lines[self.row].len() {
                    // At end of line — join with next line if any.
                    if self.row + 1 < self.lines.len() {
                        let next = self.lines.remove(self.row + 1);
                        self.lines[self.row].extend(next);
                    }
                } else {
                    self.lines[self.row].truncate(self.col);
                }
            }
            KeyCode::Char('w') if ctrl => {
                self.delete_word_back();
            }

            // Character input
            KeyCode::Char(c) if !ctrl => {
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
            }

            _ => {}
        }
        None
    }

    /// Split the current line at the cursor and move the cursor to the
    /// start of the new line.
    fn insert_newline(&mut self) {
        let tail = self.lines[self.row].split_off(self.col);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.col = 0;
    }

    /// Delete the word before the cursor (Ctrl+W), stopping at line start.
    fn delete_word_back(&mut self) {
        let line = &mut self.lines[self.row];
        while self.col > 0 && line[self.col - 1] == ' ' {
            self.col -= 1;
            line.remove(self.col);
        }
        while self.col > 0 && line[self.col - 1] != ' ' {
            self.col -= 1;
            line.remove(self.col);
        }
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
    fn render(&mut self, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![String::new()];
        }

        let cursor_glyph = format!("{CURSOR_MARKER}\x1b[7m \x1b[27m");
        let mut display: Vec<String> = Vec::new();

        for (li, line) in self.lines.iter().enumerate() {
            let breaks = line_breaks(line, width);

            for (si, &seg_start) in breaks.iter().enumerate() {
                let seg_end = breaks.get(si + 1).copied().unwrap_or(line.len());
                let is_last_seg = si + 1 == breaks.len();

                // Trim trailing spaces at a wrap point so the next segment
                // starts cleanly after the break (matches `line_breaks`).
                let mut content_end = seg_end;
                if !is_last_seg {
                    while content_end > seg_start && line[content_end - 1] == ' ' {
                        content_end -= 1;
                    }
                }
                let seg_chars = &line[seg_start..content_end];

                // Does the cursor live on this segment? It does if it's on
                // the cursor line and either falls strictly inside the
                // segment or sits exactly at the end of the *last* segment
                // of the line.
                let on_cursor = self.focused
                    && li == self.row
                    && self.col >= seg_start
                    && (self.col < seg_end || (self.col == seg_end && is_last_seg));

                if !on_cursor {
                    display.push(seg_chars.iter().collect());
                    continue;
                }

                let local = self.col - seg_start;
                let visible_len = content_end - seg_start;

                if local < visible_len {
                    // Cursor on a visible char — replace it with marker +
                    // reverse-video.
                    let before: String = seg_chars[..local].iter().collect();
                    let after: String = seg_chars[local + 1..].iter().collect();
                    display.push(format!(
                        "{before}{CURSOR_MARKER}\x1b[7m{}\x1b[27m{after}",
                        seg_chars[local]
                    ));
                } else if local >= width {
                    // Cursor would land at column == width of a fully-filled
                    // segment. Push the segment unchanged and emit a fresh
                    // visual row carrying just the cursor, so the caret
                    // never lands on whatever sits to the right of the
                    // editor (e.g. a block border).
                    display.push(seg_chars.iter().collect());
                    display.push(cursor_glyph.clone());
                } else {
                    // Cursor at end of segment, fits within width — append.
                    let mut segment: String = seg_chars.iter().collect();
                    segment.push_str(&cursor_glyph);
                    display.push(segment);
                }
            }
        }

        if display.is_empty() {
            display.push(String::new());
        }
        display
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::extract_cursor;

    fn editor() -> Editor {
        Editor::new()
    }

    fn type_str(p: &mut Editor, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                p.insert_newline();
            } else {
                p.lines[p.row].insert(p.col, c);
                p.col += 1;
            }
        }
    }

    /// Render with the cursor disabled, so callers can compare against
    /// plain text without dealing with the marker / reverse-video bytes.
    fn render_plain(p: &mut Editor, width: usize) -> Vec<String> {
        let was_focused = p.focused;
        p.focused = false;
        let lines = p.render(width);
        p.focused = was_focused;
        lines
    }

    /// Render with the cursor enabled and extract its (row, col) via the
    /// same path the screen uses.
    fn render_with_cursor(p: &mut Editor, width: usize) -> (Vec<String>, usize, usize) {
        p.focused = true;
        let mut lines = p.render(width);
        let (row, col) = extract_cursor(&mut lines).expect("cursor marker present");
        (lines, row, col)
    }

    // ── Render shape ────────────────────────────────────────────────

    #[test]
    fn single_line_render() {
        let mut p = editor();
        type_str(&mut p, "hello");
        let lines = render_plain(&mut p, 80);
        assert_eq!(lines, vec!["hello".to_string()]);
    }

    #[test]
    fn multiline_render() {
        let mut p = editor();
        type_str(&mut p, "aaa\nbbb");
        let lines = render_plain(&mut p, 80);
        assert_eq!(lines, vec!["aaa".to_string(), "bbb".to_string()]);
    }

    #[test]
    fn word_wrapping() {
        let mut p = editor();
        type_str(&mut p, "aaaa bbbb");
        let lines = render_plain(&mut p, 6);
        assert_eq!(lines, vec!["aaaa".to_string(), "bbbb".to_string()]);
    }

    #[test]
    fn empty_editor_render() {
        let mut p = editor();
        let lines = p.render(80);
        assert_eq!(lines.len(), 1);
        // Empty line with just a cursor.
        assert!(lines[0].contains("\x1b[7m"));
        assert!(lines[0].contains(CURSOR_MARKER));
    }

    #[test]
    fn hard_break_long_word() {
        let mut p = editor();
        type_str(&mut p, "abcdefghij");
        // Place the cursor at the start so we don't add a trailing visual row.
        p.col = 0;
        let lines = render_plain(&mut p, 5);
        assert_eq!(lines, vec!["abcde".to_string(), "fghij".to_string()]);
    }

    // ── Cursor position via render+extract ──────────────────────────

    #[test]
    fn cursor_position_end() {
        let mut p = editor();
        type_str(&mut p, "hi");
        let (_, row, col) = render_with_cursor(&mut p, 80);
        assert_eq!((row, col), (0, 2));
    }

    #[test]
    fn cursor_position_second_line() {
        let mut p = editor();
        type_str(&mut p, "aaa\nb");
        let (_, row, col) = render_with_cursor(&mut p, 80);
        assert_eq!((row, col), (1, 1));
    }

    #[test]
    fn cursor_after_trailing_space() {
        let mut p = editor();
        type_str(&mut p, "hello ");
        let (_, row, col) = render_with_cursor(&mut p, 80);
        assert_eq!((row, col), (0, 6));
    }

    #[test]
    fn cursor_after_multiple_spaces() {
        let mut p = editor();
        type_str(&mut p, "hi   ");
        let (_, row, col) = render_with_cursor(&mut p, 80);
        assert_eq!((row, col), (0, 5));
    }

    #[test]
    fn cursor_at_end_of_wrapped_line() {
        let mut p = editor();
        type_str(&mut p, "hello world");
        let (_, row, col) = render_with_cursor(&mut p, 8);
        assert_eq!((row, col), (1, 5));
    }

    #[test]
    fn cursor_on_space_at_wrap_point() {
        let mut p = editor();
        type_str(&mut p, "hello world");
        p.col = 5; // on the space
        let (_, row, col) = render_with_cursor(&mut p, 8);
        assert_eq!((row, col), (0, 5));
    }

    #[test]
    fn cursor_on_first_char_of_wrapped_line() {
        let mut p = editor();
        type_str(&mut p, "hello world");
        p.col = 6; // on 'w'
        let (_, row, col) = render_with_cursor(&mut p, 8);
        assert_eq!((row, col), (1, 0));
    }

    #[test]
    fn cursor_wraps_at_exact_width() {
        // Typing exactly `width` characters should push the cursor onto a
        // new visual line instead of sitting at column `width` where the
        // right border of an enclosing block lives.
        let mut p = editor();
        type_str(&mut p, "abcde");
        let (lines, row, col) = render_with_cursor(&mut p, 5);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "abcde");
        assert_eq!((row, col), (1, 0));
    }

    #[test]
    fn cursor_wraps_at_exact_width_after_newline() {
        let mut p = editor();
        type_str(&mut p, "ab\ncdefg");
        let (lines, row, col) = render_with_cursor(&mut p, 5);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "ab");
        assert_eq!(lines[1], "cdefg");
        assert_eq!((row, col), (2, 0));
    }

    #[test]
    fn cursor_just_below_width_does_not_wrap() {
        let mut p = editor();
        type_str(&mut p, "abcd");
        let (lines, row, col) = render_with_cursor(&mut p, 5);
        assert_eq!(lines.len(), 1);
        assert_eq!((row, col), (0, 4));
    }

    // ── Edit operations driven through fields ──────────────────────

    #[test]
    fn line_navigation() {
        let mut p = editor();
        type_str(&mut p, "abcd\nef");
        assert_eq!((p.row, p.col), (1, 2));
        // Up: back into "abcd" at column 2.
        p.row -= 1;
        p.col = p.col.min(p.lines[p.row].len());
        assert_eq!((p.row, p.col), (0, 2));
        // Down: forward into "ef" at column 2.
        p.row += 1;
        p.col = p.col.min(p.lines[p.row].len());
        assert_eq!((p.row, p.col), (1, 2));
    }

    #[test]
    fn line_navigation_clamps_column() {
        let mut p = editor();
        type_str(&mut p, "abcdef\nhi");
        // Cursor on line 1, col 2 (end of "hi"). Up should clamp to col 2.
        p.row -= 1;
        p.col = p.col.min(p.lines[p.row].len());
        assert_eq!((p.row, p.col), (0, 2));
        // Move to col 6 (end of "abcdef"); Down should clamp to col 2 (end of "hi").
        p.col = 6;
        p.row += 1;
        p.col = p.col.min(p.lines[p.row].len());
        assert_eq!((p.row, p.col), (1, 2));
    }

    #[test]
    fn home_end_multiline() {
        let mut p = editor();
        type_str(&mut p, "aaa\nbbb");
        // We're on line 1; Home → col 0, End → col 3.
        p.col = 0;
        assert_eq!((p.row, p.col), (1, 0));
        p.col = p.lines[p.row].len();
        assert_eq!((p.row, p.col), (1, 3));
    }

    #[test]
    fn ctrl_u_drains_to_line_start() {
        let mut p = editor();
        type_str(&mut p, "aaa\nbbb");
        // On line 1 at col 3; Ctrl+U should leave the previous line untouched
        // and clear the current line, leaving the buffer at "aaa\n".
        p.lines[p.row].drain(..p.col);
        p.col = 0;
        assert_eq!(p.text(), "aaa\n");
    }

    // ── line_breaks helper ─────────────────────────────────────────

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
