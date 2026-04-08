use crate::format;
use crate::widget::Widget;
use std::iter;

pub struct HorizontalBorder {
    s: String,
    l: Option<String>,
    r: Option<String>,
    h: usize,
}

impl HorizontalBorder {
    pub fn new(s: String) -> Self {
        Self {
            s,
            l: None,
            r: None,
            h: 1,
        }
    }

    pub fn height(mut self, h: usize) -> Self {
        self.h = h;
        self
    }

    pub fn pad(h: usize) -> Self {
        Self {
            s: " ".to_string(),
            l: None,
            r: None,
            h,
        }
    }

    pub fn left(mut self, l: String) -> Self {
        self.l = Some(l);
        self
    }

    pub fn right(mut self, r: String) -> Self {
        self.r = Some(r);
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
        Self::repeat(" ".to_string().repeat(w))
    }

    pub fn repeat(s: String) -> Self {
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

/// A widget that wraps another widget with optional edges on any side.
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
}

impl<'a, W: Widget> Widget for Block<'a, W> {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();

        if let Some(top) = &self.top {
            lines.extend(top.render(width));
        }

        let mut child_width = width;

        if let Some(left) = self.left.as_mut() {
            child_width -= left.w;
        }

        if let Some(right) = self.right.as_mut() {
            child_width -= right.w;
        }

        for child_line in self.child.render(child_width) {
            let mut line = String::with_capacity(child_line.len());
            if let Some(left) = self.left.as_mut() {
                if let Some(s) = left.s.next() {
                    line.push_str(&s);
                }
            }

            line.push_str(&child_line);

            if let Some(right) = self.right.as_mut() {
                if let Some(s) = right.s.next() {
                    line = format::pad_to_width(&line, width.saturating_sub(right.w), " ");
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

    fn cursor(&mut self, width: usize) -> Option<(usize, usize)> {
        let row_offset = self.top.as_ref().map_or(0, |top| top.h);
        let col_offset = self.left.as_ref().map_or(0, |left| left.w);

        // Must mirror the width math in `render` so the child computes its
        // layout (including word wrapping) against the same width it was
        // rendered with. Otherwise the reverse-video cursor drawn inside the
        // child and the hardware cursor end up at different positions,
        // making it look like there are multiple cursors.
        let mut child_width = width;
        if let Some(left) = self.left.as_ref() {
            child_width = child_width.saturating_sub(left.w);
        }
        if let Some(right) = self.right.as_ref() {
            child_width = child_width.saturating_sub(right.w);
        }

        self.child
            .cursor(child_width)
            .map(|(row, col)| (row + row_offset, col + col_offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Editor;
    use crate::widget::WidgetExt;

    /// Regression: `Block::cursor` used to forward the outer width to the
    /// child, so the child laid out its content against a different width
    /// than `Block::render` had used. When the editor content wrapped, the
    /// hardware cursor and the editor's reverse-video cursor block landed on
    /// different cells, visually producing two cursors.
    #[test]
    fn cursor_matches_render_when_child_wraps() {
        let mut editor = Editor::new();
        for c in "hello world".chars() {
            editor.handle(crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(c),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ));
        }

        // Outer width 12, with 2 columns of left border and 1 column of
        // right border → inner width 9, which forces "hello world" to wrap
        // into ["hello", "world"].
        let outer_width = 12;
        let mut block = Block::new(&mut editor)
            .left(VerticalBorder::pad(2))
            .right(VerticalBorder::pad(1));

        let lines = block.render(outer_width);
        let (row, col) = block.cursor(outer_width).expect("cursor");

        // The cursor must be on the line that contains the reverse-video
        // escape sequence — i.e. the same line the editor drew the cursor on.
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
    /// of a bordered block, the cursor used to land on the right border
    /// column. It should wrap onto a fresh visual row instead.
    #[test]
    fn cursor_does_not_land_on_right_border_at_exact_fill() {
        let mut editor = Editor::new();
        for c in "abcdefghi".chars() {
            editor.handle(crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(c),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ));
        }

        // Outer width 12 → inner width 9, matches the typed length exactly.
        let outer_width = 12;
        let mut block = Block::new(&mut editor)
            .left(VerticalBorder::pad(2))
            .right(VerticalBorder::pad(1));

        let lines = block.render(outer_width);
        let (row, col) = block.cursor(outer_width).expect("cursor");

        // Cursor wraps to a new visual row at column 0 of the inner area.
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
