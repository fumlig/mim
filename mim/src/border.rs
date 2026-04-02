use crate::format::visible_width;
use crate::widget::Widget;

/// An edge of a border — used for all four sides.
///
/// For vertical edges (left/right), `thickness` is the width in columns.
/// For horizontal edges (top/bottom), `thickness` is the number of rows.
struct Edge<'a> {
    iter: Box<dyn Iterator<Item = String> + 'a>,
    thickness: usize,
}

/// A border that wraps another widget.
///
/// All edges and corners are optional. Corners are only rendered when both
/// adjacent edges are present. Horizontal edges are iterators that yield one
/// string per column; vertical edges yield one string per content row.
///
/// Primitive methods (`.top()`, `.left()`, `.corners()`, etc.) modify the
/// current border's edges via builder pattern. Constructors (`pad()`,
/// `line_numbers()`, `ascii()`) create pre-configured borders. Layers nest:
///
/// ```ignore
/// let mut b1 = Border::pad(&mut editor, 0, 0, 0, 1);
/// let mut b2 = Border::line_numbers(&mut b1, 4);
/// let mut b3 = Border::ascii(&mut b2);
/// frame.add_focused(&mut b3);
/// ```
pub struct Border<'a, W: Widget> {
    child: &'a mut W,

    top_left: Option<&'static str>,
    top_right: Option<&'static str>,
    bottom_left: Option<&'static str>,
    bottom_right: Option<&'static str>,

    top: Option<Edge<'a>>,
    bottom: Option<Edge<'a>>,
    left: Option<Edge<'a>>,
    right: Option<Edge<'a>>,
}

impl<'a, W: Widget> Border<'a, W> {
    /// Create a border with no edges (invisible wrapper).
    pub fn new(child: &'a mut W) -> Self {
        Self {
            child,
            top_left: None,
            top_right: None,
            bottom_left: None,
            bottom_right: None,
            top: None,
            bottom: None,
            left: None,
            right: None,
        }
    }

    // ── Primitive edge methods (modify current border) ──────────────

    /// Set the top edge to a repeated character with the given thickness (row count).
    pub fn top(mut self, ch: &'static str, thickness: usize) -> Self {
        self.top = Some(Edge {
            iter: Box::new(std::iter::repeat_with(|| ch.to_string())),
            thickness,
        });
        self
    }

    /// Set the bottom edge to a repeated character with the given thickness (row count).
    pub fn bottom(mut self, ch: &'static str, thickness: usize) -> Self {
        self.bottom = Some(Edge {
            iter: Box::new(std::iter::repeat_with(|| ch.to_string())),
            thickness,
        });
        self
    }

    /// Set the left edge to a repeated string with the given thickness (column count).
    pub fn left(mut self, ch: &'static str, thickness: usize) -> Self {
        self.left = Some(Edge {
            iter: Box::new(std::iter::repeat_with(|| ch.to_string())),
            thickness,
        });
        self
    }

    /// Set the left edge to a custom iterator with the given thickness (column count).
    pub fn left_iter(mut self, iter: impl Iterator<Item = String> + 'a, thickness: usize) -> Self {
        self.left = Some(Edge {
            iter: Box::new(iter),
            thickness,
        });
        self
    }

    /// Set the right edge to a repeated string with the given thickness (column count).
    pub fn right(mut self, ch: &'static str, thickness: usize) -> Self {
        self.right = Some(Edge {
            iter: Box::new(std::iter::repeat_with(|| ch.to_string())),
            thickness,
        });
        self
    }

    /// Set the right edge to a custom iterator with the given thickness (column count).
    pub fn right_iter(mut self, iter: impl Iterator<Item = String> + 'a, thickness: usize) -> Self {
        self.right = Some(Edge {
            iter: Box::new(iter),
            thickness,
        });
        self
    }

    /// Set all four corners.
    pub fn corners(
        mut self,
        top_left: &'static str,
        top_right: &'static str,
        bottom_left: &'static str,
        bottom_right: &'static str,
    ) -> Self {
        self.top_left = Some(top_left);
        self.top_right = Some(top_right);
        self.bottom_left = Some(bottom_left);
        self.bottom_right = Some(bottom_right);
        self
    }

    // ── Pre-configured constructors ────────────────────────────────

    /// Wrap with padding (space edges at the given thicknesses).
    pub fn pad(child: &'a mut W, top: usize, right: usize, bottom: usize, left: usize) -> Self {
        let mut b = Self::new(child);
        if top > 0 {
            b = b.top(" ", top);
        }
        if bottom > 0 {
            b = b.bottom(" ", bottom);
        }
        if left > 0 {
            b = b.left(" ", left);
        }
        if right > 0 {
            b = b.right(" ", right);
        }
        let has_h = top > 0 || bottom > 0;
        let has_v = left > 0 || right > 0;
        if has_h && has_v {
            b = b.corners(" ", " ", " ", " ");
        }
        b
    }

    /// Wrap with line numbers on the left. Numbers are right-aligned within
    /// `width` columns. Overflowing numbers show `>` followed by the rightmost
    /// digits that fit (e.g. width 3, line 1234 → `>34`).
    pub fn line_numbers(child: &'a mut W, width: usize) -> Self {
        Self::new(child).left_iter(
            (1usize..).map(move |n| {
                let s = n.to_string();
                if s.len() <= width {
                    format!("{n:>width$}")
                } else {
                    format!(">{}", &s[s.len() - (width - 1)..])
                }
            }),
            width,
        )
    }

    /// ASCII border: +-+|+-+|
    pub fn ascii(child: &'a mut W) -> Self {
        Self::new(child)
            .top("-", 1)
            .bottom("-", 1)
            .left("|", 1)
            .right("|", 1)
            .corners("+", "+", "+", "+")
    }

    // ── Rendering helpers ───────────────────────────────────────────

    /// Build a horizontal line with optional corners.
    /// Corners are placed only when the corresponding vertical edge exists.
    fn h_line(
        iter: &mut dyn Iterator<Item = String>,
        width: usize,
        corner_left: Option<&str>,
        corner_right: Option<&str>,
        has_left: bool,
        has_right: bool,
    ) -> String {
        let cl = if has_left {
            corner_left.unwrap_or("")
        } else {
            ""
        };
        let cr = if has_right {
            corner_right.unwrap_or("")
        } else {
            ""
        };
        let fill_w = width.saturating_sub(visible_width(cl) + visible_width(cr));
        let fill: String = iter.take(fill_w).collect();
        format!("{cl}{fill}{cr}")
    }

    /// Render N horizontal rows for a top or bottom edge.
    ///
    /// For top: row 0 is outermost (has corners), rows 1..N-1 are inner.
    /// For bottom: rows 0..N-2 are inner, row N-1 is outermost (has corners).
    ///
    /// Inner rows use vertical edge characters on the sides instead of corners.
    fn h_rows(
        h_iter: &mut dyn Iterator<Item = String>,
        thickness: usize,
        width: usize,
        corner_left: Option<&str>,
        corner_right: Option<&str>,
        has_left: bool,
        has_right: bool,
        left: &mut Option<Edge<'_>>,
        right: &mut Option<Edge<'_>>,
        top: bool,
    ) -> Vec<String> {
        let mut rows = Vec::with_capacity(thickness);
        for i in 0..thickness {
            let is_outermost = if top { i == 0 } else { i == thickness - 1 };
            if is_outermost {
                rows.push(Self::h_line(
                    h_iter,
                    width,
                    corner_left,
                    corner_right,
                    has_left,
                    has_right,
                ));
            } else {
                // Inner row: use vertical edges on the sides, fill in the middle.
                let left_str = left
                    .as_mut()
                    .map(|e| {
                        let s = e.iter.next().unwrap_or_default();
                        pad_or_truncate(&s, e.thickness)
                    })
                    .unwrap_or_default();
                let right_str = right
                    .as_mut()
                    .map(|e| {
                        let s = e.iter.next().unwrap_or_default();
                        pad_or_truncate(&s, e.thickness)
                    })
                    .unwrap_or_default();
                let fill_w =
                    width.saturating_sub(visible_width(&left_str) + visible_width(&right_str));
                let fill: String = h_iter.take(fill_w).collect();
                rows.push(format!("{left_str}{fill}{right_str}"));
            }
        }
        rows
    }
}

impl<'a, W: Widget> Widget for Border<'a, W> {
    fn render(&mut self, width: u16) -> Vec<String> {
        let w = width as usize;
        let left_w = self.left.as_ref().map_or(0, |e| e.thickness);
        let right_w = self.right.as_ref().map_or(0, |e| e.thickness);
        let has_left = self.left.is_some();
        let has_right = self.right.is_some();
        let inner_w = w.saturating_sub(left_w + right_w);

        let mut lines = Vec::new();

        // Top edge
        if let Some(ref mut top) = self.top {
            lines.extend(Self::h_rows(
                top.iter.as_mut(),
                top.thickness,
                w,
                self.top_left,
                self.top_right,
                has_left,
                has_right,
                &mut self.left,
                &mut self.right,
                true,
            ));
        }

        // Child content
        let child_lines = self.child.render(inner_w as u16);

        let content = if child_lines.is_empty() {
            vec![" ".repeat(inner_w)]
        } else {
            child_lines
        };

        for cl in &content {
            let padding = inner_w.saturating_sub(visible_width(cl));
            let left_str = self
                .left
                .as_mut()
                .map(|e| {
                    let s = e.iter.next().unwrap_or_default();
                    pad_or_truncate(&s, e.thickness)
                })
                .unwrap_or_default();
            let right_str = self
                .right
                .as_mut()
                .map(|e| {
                    let s = e.iter.next().unwrap_or_default();
                    pad_or_truncate(&s, e.thickness)
                })
                .unwrap_or_default();
            lines.push(format!(
                "{}{}{}{}",
                left_str,
                cl,
                " ".repeat(padding),
                right_str
            ));
        }

        // Bottom edge
        if let Some(ref mut bottom) = self.bottom {
            lines.extend(Self::h_rows(
                bottom.iter.as_mut(),
                bottom.thickness,
                w,
                self.bottom_left,
                self.bottom_right,
                has_left,
                has_right,
                &mut self.left,
                &mut self.right,
                false,
            ));
        }

        lines
    }

    fn cursor(&mut self, width: u16) -> Option<(usize, usize)> {
        let w = width as usize;
        let left_w = self.left.as_ref().map_or(0, |e| e.thickness);
        let right_w = self.right.as_ref().map_or(0, |e| e.thickness);
        let inner_w = w.saturating_sub(left_w + right_w);
        let top_rows = self.top.as_ref().map_or(0, |e| e.thickness);

        self.child
            .cursor(inner_w as u16)
            .map(|(row, col)| (row + top_rows, col + left_w))
    }
}

/// Pad a string with spaces to `width` columns, or truncate if wider.
fn pad_or_truncate(s: &str, width: usize) -> String {
    let vw = visible_width(s);
    if vw >= width {
        crate::format::truncate_to_width(s, width, "")
    } else {
        format!("{}{}", s, " ".repeat(width - vw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Text(&'static str);

    impl Widget for Text {
        fn render(&mut self, _width: u16) -> Vec<String> {
            vec![self.0.to_string()]
        }
    }

    #[test]
    fn ascii_box() {
        let mut child = Text("hi");
        let mut b = Border::ascii(&mut child);
        let lines = b.render(10);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "+--------+");
        assert_eq!(lines[1], "|hi      |");
        assert_eq!(lines[2], "+--------+");
    }

    #[test]
    fn multiline_child() {
        struct Multi;
        impl Widget for Multi {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["aaa".into(), "b".into()]
            }
        }
        let mut child = Multi;
        let mut b = Border::ascii(&mut child);
        let lines = b.render(8);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "+------+");
        assert_eq!(lines[1], "|aaa   |");
        assert_eq!(lines[2], "|b     |");
        assert_eq!(lines[3], "+------+");
    }

    #[test]
    fn cursor_offset() {
        struct Cur;
        impl Widget for Cur {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["ab".into()]
            }
            fn cursor(&mut self, _width: u16) -> Option<(usize, usize)> {
                Some((0, 1))
            }
        }
        let mut child = Cur;
        let mut b = Border::ascii(&mut child);
        let pos = b.cursor(10);
        assert_eq!(pos, Some((1, 2)));
    }

    #[test]
    fn empty_child() {
        struct Empty;
        impl Widget for Empty {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec![]
            }
        }
        let mut child = Empty;
        let mut b = Border::ascii(&mut child);
        let lines = b.render(6);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "+----+");
        assert_eq!(lines[1], "|    |");
        assert_eq!(lines[2], "+----+");
    }

    #[test]
    fn left_only() {
        let mut child = Text("hello");
        let mut b = Border::new(&mut child).left("|", 1);
        let lines = b.render(10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "|hello    ");
    }

    #[test]
    fn top_only() {
        let mut child = Text("hello");
        let mut b = Border::new(&mut child).top("-", 1);
        let lines = b.render(10);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "----------");
        assert_eq!(lines[1], "hello     ");
    }

    #[test]
    fn no_corners_without_both_edges() {
        let mut child = Text("hi");
        let mut b = Border::new(&mut child)
            .top("-", 1)
            .left("|", 1)
            .corners("+", "+", "+", "+");
        let lines = b.render(8);
        assert_eq!(lines[0], "+-------");
        assert_eq!(lines[1], "|hi     ");
    }

    #[test]
    fn line_numbers() {
        struct Lines;
        impl Widget for Lines {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["aaa".into(), "bbb".into(), "ccc".into()]
            }
        }
        let mut child = Lines;
        let mut b = Border::line_numbers(&mut child, 3);
        let lines = b.render(10);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "  1aaa    ");
        assert_eq!(lines[1], "  2bbb    ");
        assert_eq!(lines[2], "  3ccc    ");
    }

    #[test]
    fn line_numbers_overflow() {
        struct ManyLines;
        impl Widget for ManyLines {
            fn render(&mut self, _width: u16) -> Vec<String> {
                (0..12).map(|_| "x".into()).collect()
            }
        }
        let mut child = ManyLines;
        let mut b = Border::line_numbers(&mut child, 2);
        let lines = b.render(6);
        assert_eq!(lines.len(), 12);
        assert_eq!(lines[0], " 1x   ");
        assert_eq!(lines[8], " 9x   ");
        assert_eq!(lines[9], "10x   ");
        assert_eq!(lines[10], "11x   ");
        assert_eq!(lines[11], "12x   ");
    }

    #[test]
    fn line_numbers_overflow_truncation() {
        struct ManyLines;
        impl Widget for ManyLines {
            fn render(&mut self, _width: u16) -> Vec<String> {
                (0..1001).map(|_| "x".into()).collect()
            }
        }
        let mut child = ManyLines;
        let mut b = Border::line_numbers(&mut child, 3);
        let lines = b.render(8);
        assert_eq!(&lines[0][..3], "  1");
        assert_eq!(&lines[998][..3], "999");
        assert_eq!(&lines[999][..3], ">00");
        assert_eq!(&lines[1000][..3], ">01");
    }

    #[test]
    fn padding_single() {
        let mut child = Text("hi");
        let mut b = Border::new(&mut child).left(" ", 1).right(" ", 1);
        let lines = b.render(6);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], " hi   ");
    }

    #[test]
    fn padding_all_sides() {
        let mut child = Text("hi");
        let mut b = Border::pad(&mut child, 1, 1, 1, 1);
        let lines = b.render(6);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "      ");
        assert_eq!(lines[1], " hi   ");
        assert_eq!(lines[2], "      ");
    }

    #[test]
    fn padding_left_only() {
        let mut child = Text("hi");
        let mut b = Border::pad(&mut child, 0, 0, 0, 2);
        let lines = b.render(6);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "  hi  ");
    }

    #[test]
    fn padding_asymmetric() {
        let mut child = Text("hi");
        let mut b = Border::pad(&mut child, 2, 1, 0, 0);
        let lines = b.render(6);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "      ");
        assert_eq!(lines[1], "      ");
        assert_eq!(lines[2], "hi    ");
    }

    #[test]
    fn padding_zero() {
        let mut child = Text("hi");
        let mut b = Border::pad(&mut child, 0, 0, 0, 0);
        let lines = b.render(6);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "hi    ");
    }

    #[test]
    fn thick_top_ascii() {
        let mut child = Text("hi");
        let mut b = Border::new(&mut child)
            .top("-", 2)
            .left("|", 1)
            .right("|", 1)
            .corners("+", "+", "+", "+");
        let lines = b.render(8);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "+------+"); // outermost: corners
        assert_eq!(lines[1], "|------|"); // inner: vertical edges + fill
        assert_eq!(lines[2], "|hi    |");
    }

    #[test]
    fn thick_bottom_ascii() {
        let mut child = Text("hi");
        let mut b = Border::new(&mut child)
            .bottom("-", 3)
            .left("|", 1)
            .right("|", 1)
            .corners("+", "+", "+", "+");
        let lines = b.render(8);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "|hi    |");
        assert_eq!(lines[1], "|------|"); // inner: vertical edges + fill
        assert_eq!(lines[2], "|------|"); // inner: vertical edges + fill
        assert_eq!(lines[3], "+------+"); // outermost: corners
    }

    #[test]
    fn nested_border_thickness() {
        let mut child = Text("x");
        let mut inner = Border::ascii(&mut child);
        let mut outer = Border::ascii(&mut inner);
        let lines = outer.render(10);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "+--------+");
        assert_eq!(lines[1], "|+------+|");
        assert_eq!(lines[2], "||x     ||");
        assert_eq!(lines[3], "|+------+|");
        assert_eq!(lines[4], "+--------+");
    }

    #[test]
    fn cursor_offset_no_top() {
        struct Cur;
        impl Widget for Cur {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["ab".into()]
            }
            fn cursor(&mut self, _width: u16) -> Option<(usize, usize)> {
                Some((0, 1))
            }
        }
        let mut child = Cur;
        let mut b = Border::new(&mut child).left("|", 1);
        let pos = b.cursor(10);
        assert_eq!(pos, Some((0, 2)));
    }

    #[test]
    fn cursor_offset_thick_top() {
        struct Cur;
        impl Widget for Cur {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["ab".into()]
            }
            fn cursor(&mut self, _width: u16) -> Option<(usize, usize)> {
                Some((0, 1))
            }
        }
        let mut child = Cur;
        let mut b = Border::new(&mut child).top("-", 3).left("|", 2);
        let pos = b.cursor(10);
        assert_eq!(pos, Some((3, 3)));
    }

    // ── Chaining tests ──────────────────────────────────────────────

    #[test]
    fn nested_pad_then_line_numbers() {
        let mut child = Text("hi");
        let mut b1 = Border::pad(&mut child, 0, 0, 0, 1);
        let mut b2 = Border::line_numbers(&mut b1, 3);
        let lines = b2.render(10);
        assert_eq!(lines.len(), 1);
        // line_numbers(3) + pad left(1) + content
        // "  1" + " " + "hi" + padding
        assert_eq!(lines[0], "  1 hi    ");
    }

    #[test]
    fn nested_pad_then_ascii() {
        let mut child = Text("hi");
        let mut b1 = Border::pad(&mut child, 0, 0, 0, 1);
        let mut b2 = Border::ascii(&mut b1);
        let lines = b2.render(8);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "+------+");
        assert_eq!(lines[1], "| hi   |");
        assert_eq!(lines[2], "+------+");
    }

    #[test]
    fn nested_line_numbers_then_ascii() {
        struct Lines;
        impl Widget for Lines {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["aa".into(), "bb".into()]
            }
        }
        let mut child = Lines;
        let mut b1 = Border::line_numbers(&mut child, 2);
        let mut b2 = Border::ascii(&mut b1);
        let lines = b2.render(10);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "+--------+");
        assert_eq!(lines[1], "| 1aa    |");
        assert_eq!(lines[2], "| 2bb    |");
        assert_eq!(lines[3], "+--------+");
    }

    #[test]
    fn nested_cursor_offset() {
        struct Cur;
        impl Widget for Cur {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["ab".into()]
            }
            fn cursor(&mut self, _width: u16) -> Option<(usize, usize)> {
                Some((0, 1))
            }
        }
        let mut child = Cur;
        // pad left 1 + line_numbers 3 + ascii border (left 1, top 1)
        let mut b1 = Border::pad(&mut child, 0, 0, 0, 1);
        let mut b2 = Border::line_numbers(&mut b1, 3);
        let mut b3 = Border::ascii(&mut b2);
        // cursor at (0,1) in child
        // + pad left 1 → (0, 2)
        // + line_numbers left 3 → (0, 5)
        // + ascii top 1, left 1 → (1, 6)
        let pos = b3.cursor(20);
        assert_eq!(pos, Some((1, 6)));
    }
}
