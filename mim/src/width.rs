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
}
