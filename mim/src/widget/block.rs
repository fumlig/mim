use super::Widget;
use crate::format;
use std::iter;

pub struct HorizontalBorder {
    s: String,
    l: Option<String>,
    r: Option<String>,
    h: usize,
}

impl HorizontalBorder {
    pub fn new(s: impl Into<String>) -> Self {
        Self {
            s: s.into(),
            l: None,
            r: None,
            h: 1,
        }
    }

    /// A single-row horizontal rule made of `-` characters.
    pub fn line() -> Self {
        Self::new("-")
    }

    pub fn height(mut self, h: usize) -> Self {
        self.h = h;
        self
    }

    /// A blank horizontal band of `h` rows.
    pub fn blank(h: usize) -> Self {
        Self::pad(h)
    }

    pub fn pad(h: usize) -> Self {
        Self {
            s: " ".to_string(),
            l: None,
            r: None,
            h,
        }
    }

    pub fn left(mut self, l: impl Into<String>) -> Self {
        self.l = Some(l.into());
        self
    }

    pub fn right(mut self, r: impl Into<String>) -> Self {
        self.r = Some(r.into());
        self
    }

    fn render(&self, width: usize) -> Vec<String> {
        let (l_s, l_w) = self
            .l
            .as_ref()
            .map_or(("", 0), |s| (&s, format::visible_width(&s)));

        let (r_s, r_w) = self
            .r
            .as_ref()
            .map_or(("", 0), |s| (&s, format::visible_width(&s)));

        let w = if l_w + r_w > width {
            width
        } else {
            width - (l_w + r_w)
        };

        let s = format::repeat_to_width(&self.s, w);

        vec![format!("{l_s}{s}{r_s}"); self.h]
    }
}

pub struct VerticalBorder {
    s: Box<dyn Iterator<Item = String>>,
    w: usize,
}

impl VerticalBorder {
    pub fn new(s: Box<dyn Iterator<Item = String>>, w: usize) -> Self {
        Self { s, w }
    }

    pub fn pad(w: usize) -> Self {
        Self::repeat(" ".repeat(w))
    }

    pub fn repeat(s: impl Into<String>) -> Self {
        let s = s.into();
        let w = format::visible_width(&s);
        let s = Box::new(iter::repeat(s));
        Self { s, w }
    }

    pub fn counter(w: usize) -> Self {
        let s = Box::new((1usize..).map(move |n| {
            let s = n.to_string();
            if s.len() <= w {
                format!("{n:>w$}")
            } else {
                format!(">{}", &s[s.len() - (w - 1)..])
            }
        }));

        Self { s, w }
    }
}

/// A widget that wraps another widget with an optional border on any
/// side.
///
/// A `Block` holds a *single* border per side. To compose multiple bands
/// — e.g. a horizontal rule with a blank padding row beneath it — wrap
/// the inner `Block` in another `Block`. Padding is not a special axis,
/// it's just a border instance ([`HorizontalBorder::blank`] /
/// [`VerticalBorder::pad`]).
pub struct Block<'a, W: Widget> {
    child: &'a mut W,
    top: Option<HorizontalBorder>,
    right: Option<VerticalBorder>,
    bottom: Option<HorizontalBorder>,
    left: Option<VerticalBorder>,
}

impl<'a, W: Widget> Block<'a, W> {
    /// Create a block with no edges (invisible wrapper).
    pub fn new(child: &'a mut W) -> Self {
        Self {
            child,
            top: None,
            bottom: None,
            left: None,
            right: None,
        }
    }

    pub fn top(mut self, border: HorizontalBorder) -> Self {
        self.top = Some(border);
        self
    }

    pub fn bottom(mut self, border: HorizontalBorder) -> Self {
        self.bottom = Some(border);
        self
    }

    pub fn right(mut self, border: VerticalBorder) -> Self {
        self.right = Some(border);
        self
    }

    pub fn left(mut self, border: VerticalBorder) -> Self {
        self.left = Some(border);
        self
    }

    // ── Padding shortcuts ─────────────────────────────────────────────
    //
    // These set the corresponding side to a blank
    // [`HorizontalBorder::blank`] / [`VerticalBorder::pad`], overwriting
    // any existing border on that side. If you need both a decorative
    // border *and* a blank pad on the same side, wrap one `Block` in
    // another.

    /// Blank top border of `n` rows.
    pub fn pad_top(self, n: usize) -> Self {
        self.top(HorizontalBorder::blank(n))
    }

    /// Blank bottom border of `n` rows.
    pub fn pad_bottom(self, n: usize) -> Self {
        self.bottom(HorizontalBorder::blank(n))
    }

    /// Blank left border of `n` columns.
    pub fn pad_left(self, n: usize) -> Self {
        self.left(VerticalBorder::pad(n))
    }

    /// Blank right border of `n` columns.
    pub fn pad_right(self, n: usize) -> Self {
        self.right(VerticalBorder::pad(n))
    }

    /// Blank left and right borders of `n` columns each.
    pub fn pad_h(self, n: usize) -> Self {
        self.pad_left(n).pad_right(n)
    }

    /// Blank top and bottom borders of `n` rows each.
    pub fn pad_v(self, n: usize) -> Self {
        self.pad_top(n).pad_bottom(n)
    }

    /// Blank borders of `n` on all four sides.
    pub fn pad_all(self, n: usize) -> Self {
        self.pad_v(n).pad_h(n)
    }

    // ── Recipes ─────────────────────────────────────────────────────

    /// Wrap the child in an ASCII `+--+ / |…| / +--+` box.
    pub fn ascii(self) -> Self {
        self.top(HorizontalBorder::new("-").left("+").right("+"))
            .bottom(HorizontalBorder::new("-").left("+").right("+"))
            .left(VerticalBorder::repeat("|"))
            .right(VerticalBorder::repeat("|"))
    }

    /// Attach a left-side line-number gutter of width `w`.
    pub fn line_numbers(self, w: usize) -> Self {
        self.left(VerticalBorder::counter(w))
    }
}

impl<'a, W: Widget> Widget for Block<'a, W> {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();

        if let Some(top) = &self.top {
            lines.extend(top.render(width));
        }

        let left_w = self.left.as_ref().map(|b| b.w).unwrap_or(0);
        let right_w = self.right.as_ref().map(|b| b.w).unwrap_or(0);
        let child_width = width.saturating_sub(left_w + right_w);

        for child_line in self.child.render(child_width) {
            let mut line = String::with_capacity(child_line.len() + left_w + right_w);

            if let Some(left) = self.left.as_mut() {
                if let Some(s) = left.s.next() {
                    line.push_str(&s);
                }
            }

            line.push_str(&child_line);

            if let Some(right) = self.right.as_mut() {
                line = format::pad_to_width(&line, width.saturating_sub(right_w), " ");
                if let Some(s) = right.s.next() {
                    line.push_str(&s);
                }
            }

            lines.push(line);
        }

        if let Some(bottom) = self.bottom.as_mut() {
            lines.extend(bottom.render(width));
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::super::editor::Editor;
    use super::*;
    use crate::format::extract_cursor;

    fn type_into(editor: &mut Editor, s: &str) {
        for c in s.chars() {
            editor.handle(crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(c),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ));
        }
    }

    /// Regression: the cursor extracted from the rendered output must land
    /// on the same line and inside the same inner area as the painted
    /// reverse-video cursor, even when the child wraps.
    ///
    /// Before the marker-based approach this was enforced by having
    /// `Block::cursor` mirror the width math in `Block::render` and was
    /// easy to get wrong — the two had to agree by hand. With the marker
    /// approach both come from the same `render` call, but this test still
    /// exercises the "bordered block + wrapping child" path end-to-end.
    #[test]
    fn extracted_cursor_matches_rendered_cursor_when_child_wraps() {
        let mut editor = Editor::new();
        type_into(&mut editor, "hello world");

        // Outer width 12, with 2 columns of left border and 1 column of
        // right border → inner width 9, which forces "hello world" to wrap
        // into ["hello", "world"].
        let outer_width = 12;
        let mut block = Block::new(&mut editor)
            .left(VerticalBorder::pad(2))
            .right(VerticalBorder::pad(1));

        let mut lines = block.render(outer_width);
        let (row, col) = extract_cursor(&mut lines).expect("cursor");

        // The cursor must be on the line that contains the reverse-video
        // escape sequence — the marker was injected on exactly that line.
        assert!(
            lines[row].contains("\x1b[7m"),
            "cursor row {row} does not contain the rendered cursor: {lines:?}"
        );
        // And it must be the only such line.
        let cursor_rows: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("\x1b[7m"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(cursor_rows, vec![row]);
        // Column must be inside the inner area (past the left border).
        assert!(col >= 2, "cursor col {col} should be past left border");
        // ...and strictly before the right border column.
        assert!(
            col < outer_width - 1,
            "cursor col {col} should not enter the right border (outer_width={outer_width})"
        );
    }

    /// Regression: when the editor's content exactly fills the inner width
    /// of a bordered block, the cursor must wrap to a new visual row at
    /// column 0 of the inner area instead of landing on the right border.
    #[test]
    fn extracted_cursor_does_not_land_on_right_border_at_exact_fill() {
        let mut editor = Editor::new();
        type_into(&mut editor, "abcdefghi");

        // Outer width 12 → inner width 9, matches the typed length exactly.
        let outer_width = 12;
        let mut block = Block::new(&mut editor)
            .left(VerticalBorder::pad(2))
            .right(VerticalBorder::pad(1));

        let mut lines = block.render(outer_width);
        let (row, col) = extract_cursor(&mut lines).expect("cursor");

        assert_eq!(col, 2, "cursor should sit at the start of the inner area");
        assert!(
            row >= 1,
            "cursor should wrap to a new visual row, got row {row}"
        );
        assert!(
            lines[row].contains("\x1b[7m"),
            "cursor row {row} does not contain the rendered cursor: {lines:?}"
        );
    }
}
