use std::iter;
use unicode_width::UnicodeWidthChar;

/// A segment of text: either an ANSI escape sequence or a visible character.
enum Segment<'a> {
    /// An escape sequence (zero visible width).
    Escape(&'a str),
    /// A visible character with its display width.
    Char(char, usize),
    /// A control character (zero visible width).
    Control(char),
}

/// Iterate over segments of a string, separating escape sequences from visible characters.
fn segments(s: &str) -> impl Iterator<Item = Segment<'_>> {
    let mut i = 0;
    let bytes = s.as_bytes();
    let len = bytes.len();

    iter::from_fn(move || {
        if i >= len {
            return None;
        }

        // Check for ESC
        if bytes[i] == 0x1b {
            let start = i;
            i += 1;
            if i >= len {
                return Some(Segment::Escape(&s[start..i]));
            }

            match bytes[i] {
                b'[' => {
                    // CSI: ESC [ ... <0x40-0x7E>
                    i += 1;
                    while i < len {
                        let b = bytes[i];
                        i += 1;
                        if (0x40..=0x7E).contains(&b) {
                            break;
                        }
                    }
                }
                b']' => {
                    // OSC: ESC ] ... BEL or ESC ] ... ST (ESC \)
                    i += 1;
                    while i < len {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                b'_' => {
                    // APC: ESC _ ... BEL or ESC _ ... ST (ESC \)
                    i += 1;
                    while i < len {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Other: ESC + one char (SS3, etc.)
                    i += 1;
                }
            }

            return Some(Segment::Escape(&s[start..i]));
        }

        // Decode one char
        let ch = &s[i..];
        let c = ch.chars().next().unwrap();
        i += c.len_utf8();

        if c.is_control() {
            Some(Segment::Control(c))
        } else {
            let w = UnicodeWidthChar::width(c).unwrap_or(0);
            Some(Segment::Char(c, w))
        }
    })
}

/// Calculate the visible width of a string in terminal columns.
/// Ignores ANSI escape sequences. Handles wide characters (CJK, emoji).
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    for seg in segments(s) {
        if let Segment::Char(_, w) = seg {
            width += w;
        }
    }
    width
}

/// Truncate a string to fit within `max_width` visible columns.
/// Appends `ellipsis` if truncation occurs.
pub fn truncate_to_width(s: &str, max_width: usize, ellipsis: &str) -> String {
    let s_width = visible_width(s);
    if s_width <= max_width {
        return s.to_string();
    }

    let ellipsis_width = visible_width(ellipsis);
    if ellipsis_width >= max_width {
        return take_width(s, max_width);
    }

    let target = max_width - ellipsis_width;
    let mut result = take_width(s, target);
    result.push_str(ellipsis);
    result
}

pub fn pad_to_width(s: &str, width: usize, space: &str) -> String {
    let mut result = s.to_string();
    let w = visible_width(s);
    if w <= width {
        result.push_str(&repeat_to_width(space, width - w));
    }

    result
}

pub fn repeat_to_width(s: &str, max_width: usize) -> String {
    let w = visible_width(s);
    let n = if w > 0 { max_width / w } else { 0 };
    s.repeat(n)
}

pub fn concatenate_to_width<I, S>(seq: I, max_width: usize) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut result = String::new();
    let mut width = 0;
    for s in seq {
        let w = visible_width(s.as_ref());
        if width + w < max_width {
            result.push_str(s.as_ref());
            width += w;
        }
    }

    result
}

/// Take characters from `s` up to `max_width` visible columns.
/// Includes ANSI escape sequences in the output (they contribute zero width).
fn take_width(s: &str, max_width: usize) -> String {
    let mut result = String::with_capacity(s.len());
    let mut width = 0;

    for seg in segments(s) {
        match seg {
            Segment::Escape(esc) => result.push_str(esc),
            Segment::Control(c) => result.push(c),
            Segment::Char(c, w) => {
                if width + w > max_width {
                    break;
                }
                width += w;
                result.push(c);
            }
        }
    }

    result
}

/// Word-wrap a single line of text at word boundaries.
/// Words longer than `max_width` are kept intact (the caller can truncate).
/// Returns `vec![""]` for empty input.
/// Word-wrap a single line of text at word boundaries.
/// Words longer than `max_width` are broken, with `hyphen` appended at each break.
/// Returns `vec![""]` for empty input.
pub fn word_wrap(text: &str, max_width: usize, hyphen: &str) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut line_len: usize = 0;

    for word in text.split_whitespace() {
        if line.is_empty() {
            push_word(
                &mut lines,
                &mut line,
                &mut line_len,
                word,
                max_width,
                hyphen,
            );
        } else if line_len + 1 + word.len() <= max_width {
            line.push(' ');
            line.push_str(word);
            line_len += 1 + word.len();
        } else {
            lines.push(std::mem::take(&mut line));
            line_len = 0;
            push_word(
                &mut lines,
                &mut line,
                &mut line_len,
                word,
                max_width,
                hyphen,
            );
        }
    }

    if !line.is_empty() {
        lines.push(line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Push a word onto the current line, breaking it into chunks if it exceeds `max_width`.
/// Non-final chunks get `hyphen` appended.
fn push_word(
    lines: &mut Vec<String>,
    line: &mut String,
    line_len: &mut usize,
    word: &str,
    max_width: usize,
    hyphen: &str,
) {
    if word.len() <= max_width {
        line.push_str(word);
        *line_len = word.len();
        return;
    }

    // Reserve room for the hyphen on each broken line.
    let chunk_width = max_width.saturating_sub(hyphen.len()).max(1);

    let mut remaining = word;
    while !remaining.is_empty() {
        if !line.is_empty() {
            lines.push(std::mem::take(line));
        }

        let chunk_end = char_boundary_at(remaining, chunk_width);
        let (chunk, rest) = remaining.split_at(chunk_end);

        if rest.is_empty() {
            // Last chunk — keep it in `line` so the next word can join.
            line.push_str(chunk);
            *line_len = chunk.len();
        } else {
            lines.push(format!("{chunk}{hyphen}"));
            *line_len = 0;
        }
        remaining = rest;
    }
}

/// Find the largest byte offset <= `max_bytes` that falls on a char boundary.
fn char_boundary_at(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut i = max_bytes;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    // Ensure we make progress even with a very narrow width.
    if i == 0 {
        let c = s.chars().next().unwrap();
        i = c.len_utf8();
    }
    i
}

/// Split text on newlines, word-wrap each paragraph, and strip a trailing
/// empty line (from a final `\n`).
pub fn wrap_text(text: &str, max_width: usize, hyphen: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(word_wrap(paragraph, max_width, hyphen));
        }
    }
    if lines.last().map_or(false, |l| l.is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_width_ascii() {
        assert_eq!(visible_width("hello"), 5);
        assert_eq!(visible_width(""), 0);
        assert_eq!(visible_width("abc"), 3);
    }

    #[test]
    fn test_visible_width_ansi() {
        assert_eq!(visible_width("\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(visible_width("\x1b[1;32mfoo\x1b[0m"), 3);
    }

    #[test]
    fn test_visible_width_osc() {
        // OSC 8 hyperlink
        assert_eq!(
            visible_width("\x1b]8;;http://example.com\x07link\x1b]8;;\x07"),
            4
        );
    }

    #[test]
    fn test_truncate_to_width() {
        assert_eq!(truncate_to_width("hello", 10, "..."), "hello");
        assert_eq!(truncate_to_width("hello world", 8, "..."), "hello...");
        assert_eq!(truncate_to_width("hi", 2, "..."), "hi");
    }

    #[test]
    fn test_truncate_preserves_ansi() {
        let s = "\x1b[31mhello world\x1b[0m";
        let t = truncate_to_width(s, 8, "...");
        assert_eq!(t, "\x1b[31mhello...");
        assert_eq!(visible_width(&t), 8);
    }

    #[test]
    fn word_wrap_short() {
        assert_eq!(word_wrap("hello world", 80, ""), vec!["hello world"]);
    }

    #[test]
    fn word_wrap_breaks() {
        assert_eq!(
            word_wrap("hello world foo", 11, ""),
            vec!["hello world", "foo"]
        );
    }

    #[test]
    fn word_wrap_long_word_no_hyphen() {
        assert_eq!(word_wrap("abcdefghij", 5, ""), vec!["abcde", "fghij"]);
    }

    #[test]
    fn word_wrap_long_word_uneven() {
        assert_eq!(word_wrap("abcdefgh", 3, ""), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn word_wrap_long_then_short() {
        assert_eq!(word_wrap("abcdefgh x", 5, ""), vec!["abcde", "fgh x"]);
    }

    #[test]
    fn word_wrap_short_then_long() {
        assert_eq!(word_wrap("hi abcdefgh", 5, ""), vec!["hi", "abcde", "fgh"]);
    }

    #[test]
    fn word_wrap_long_word_unicode() {
        // Each 'あ' is 3 bytes; width=2 means chunk_width=2,
        // char_boundary_at rounds up to include one full char.
        assert_eq!(word_wrap("ああああ", 2, ""), vec!["あ", "あ", "あ", "あ"]);
    }

    #[test]
    fn word_wrap_empty() {
        assert_eq!(word_wrap("", 10, ""), vec![""]);
    }

    #[test]
    fn word_wrap_hyphen() {
        assert_eq!(word_wrap("abcdefghij", 6, "-"), vec!["abcde-", "fghij"]);
    }

    #[test]
    fn word_wrap_hyphen_multiple_breaks() {
        assert_eq!(
            word_wrap("abcdefghij", 4, "-"),
            vec!["abc-", "def-", "ghi-", "j"]
        );
    }

    #[test]
    fn word_wrap_hyphen_fits_exactly() {
        // Word fits in max_width — no hyphen needed.
        assert_eq!(word_wrap("abcde", 5, "-"), vec!["abcde"]);
    }

    #[test]
    fn word_wrap_hyphen_mixed() {
        assert_eq!(
            word_wrap("hi abcdefgh ok", 5, "-"),
            vec!["hi", "abcd-", "efgh", "ok"]
        );
    }

    #[test]
    fn wrap_text_paragraphs() {
        assert_eq!(
            wrap_text("aaa bbb\nccc ddd", 7, ""),
            vec!["aaa bbb", "ccc ddd"]
        );
    }

    #[test]
    fn wrap_text_trailing_newline() {
        assert_eq!(wrap_text("hello\n", 80, ""), vec!["hello"]);
    }

    #[test]
    fn wrap_text_blank_line() {
        assert_eq!(wrap_text("a\n\nb", 80, ""), vec!["a", "", "b"]);
    }
}
