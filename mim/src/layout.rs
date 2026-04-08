//! Linear layout widgets: [`VStack`] and [`HStack`].
//!
//! Both use a builder pattern with borrowed children, mirroring [`Border`].
//!
//! [`Border`]: crate::border::Border
//!
//! ```ignore
//! let mut header = Text("title");
//! let mut body   = Editor::new();
//! let mut footer = Text("status");
//!
//! // Stack vertically; each child gets the full width.
//! let mut col = VStack::new()
//!     .add(&mut header)
//!     .add(&mut body)
//!     .add(&mut footer);
//! frame.add(&mut col);
//!
//! // Two fixed-width sidebars and a flexible center column.
//! let mut row = HStack::new()
//!     .fixed(&mut left,   10)
//!     .fill(&mut center)
//!     .fixed(&mut right,  20);
//! frame.add(&mut row);
//! ```

use crate::format::pad_to_width;
use crate::widget::Widget;

// ── VStack ──────────────────────────────────────────────────────────

/// Stacks children vertically, top to bottom.
///
/// Every child is rendered at the full available width and the resulting
/// lines are concatenated. Cursor position from the first child that
/// reports one is forwarded, with rows offset by the heights of the
/// preceding children.
pub struct VStack<'a> {
    children: Vec<&'a mut dyn Widget>,
    /// Heights of each child from the most recent `render` call. Used by
    /// `cursor` so we don't have to re-render to compute row offsets.
    heights: Vec<usize>,
}

impl<'a> VStack<'a> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            heights: Vec::new(),
        }
    }

    /// Append a child to the stack.
    pub fn add(mut self, child: &'a mut dyn Widget) -> Self {
        self.children.push(child);
        self
    }
}

impl<'a> Default for VStack<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Widget for VStack<'a> {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.heights.clear();
        self.heights.reserve(self.children.len());

        let mut lines = Vec::new();
        for child in &mut self.children {
            let child_lines = child.render(width);
            self.heights.push(child_lines.len());
            lines.extend(child_lines);
        }
        lines
    }

    fn cursor(&mut self, width: usize) -> Option<(usize, usize)> {
        let mut row = 0;
        for (i, child) in self.children.iter_mut().enumerate() {
            if let Some((cr, cc)) = child.cursor(width) {
                return Some((row + cr, cc));
            }
            row += self.heights.get(i).copied().unwrap_or(0);
        }
        None
    }
}

// ── HStack ──────────────────────────────────────────────────────────

/// How a child of [`HStack`] should size itself horizontally.
#[derive(Clone, Copy, Debug)]
enum Size {
    /// Take exactly this many columns.
    Fixed(usize),
    /// Split remaining columns equally with other `Fill` children.
    Fill,
}

/// Whether a child contributes to the HStack's row count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Height {
    /// Child's rendered line count participates in the row-count max.
    Expand,
    /// Child is truncated to the row count determined by `Expand`
    /// children (or to the natural max if every child is `Clip`).
    Clip,
}

/// Stacks children horizontally, left to right.
///
/// Every child is given a column count up front: either a fixed width or
/// `fill`, which means "take whatever space the fixed children leave
/// behind". Multiple `fill` children split the remaining space equally
/// (any leftover columns are handed to the leftmost fill children one at
/// a time).
///
/// Children may produce different numbers of lines; HStack pads shorter
/// children with blank rows so the output is rectangular. Each rendered
/// child line is also right-padded to its column width so columns stay
/// aligned even when a child returns a shorter line.
///
/// Row count is determined by the children added via [`fixed`] / [`fill`]
/// (the "expand" children). Children added via [`fixed_clip`] /
/// [`fill_clip`] are truncated to that row count if they produce more
/// lines — useful for decorations or sidebars that shouldn't stretch
/// the row taller than the main content. If every child uses a `_clip`
/// variant the stack falls back to the natural maximum height, so the
/// output is never silently empty.
///
/// Cursor position from the first child that reports one is forwarded,
/// with the column offset by the widths of the preceding children.
///
/// [`fixed`]: HStack::fixed
/// [`fill`]: HStack::fill
/// [`fixed_clip`]: HStack::fixed_clip
/// [`fill_clip`]: HStack::fill_clip
pub struct HStack<'a> {
    children: Vec<(&'a mut dyn Widget, Size, Height)>,
    /// Column widths assigned to each child by the most recent `render`
    /// call. Used by `cursor`.
    widths: Vec<usize>,
}

impl<'a> HStack<'a> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            widths: Vec::new(),
        }
    }

    /// Append a child with a fixed column width. Contributes to the
    /// HStack's row count.
    pub fn fixed(mut self, child: &'a mut dyn Widget, width: usize) -> Self {
        self.children.push((child, Size::Fixed(width), Height::Expand));
        self
    }

    /// Append a child that takes the remaining space (after subtracting
    /// the fixed-width children). If multiple `fill` children are added,
    /// the remaining space is split equally between them. Contributes to
    /// the HStack's row count.
    pub fn fill(mut self, child: &'a mut dyn Widget) -> Self {
        self.children.push((child, Size::Fill, Height::Expand));
        self
    }

    /// Like [`fixed`], but this child does **not** contribute to the
    /// HStack's row count — it is truncated to the height of the other
    /// (non-clip) children instead.
    ///
    /// [`fixed`]: HStack::fixed
    pub fn fixed_clip(mut self, child: &'a mut dyn Widget, width: usize) -> Self {
        self.children.push((child, Size::Fixed(width), Height::Clip));
        self
    }

    /// Like [`fill`], but this child does **not** contribute to the
    /// HStack's row count — it is truncated to the height of the other
    /// (non-clip) children instead.
    ///
    /// [`fill`]: HStack::fill
    pub fn fill_clip(mut self, child: &'a mut dyn Widget) -> Self {
        self.children.push((child, Size::Fill, Height::Clip));
        self
    }

    /// Compute per-child column widths for a given total width.
    fn layout(&self, total: usize) -> Vec<usize> {
        let fixed_sum: usize = self
            .children
            .iter()
            .filter_map(|(_, s, _)| match s {
                Size::Fixed(w) => Some(*w),
                Size::Fill => None,
            })
            .sum();
        let fill_count = self
            .children
            .iter()
            .filter(|(_, s, _)| matches!(s, Size::Fill))
            .count();

        let remaining = total.saturating_sub(fixed_sum);
        let (per_fill, leftover) = if fill_count > 0 {
            (remaining / fill_count, remaining % fill_count)
        } else {
            (0, 0)
        };

        let mut seen_fills = 0;
        self.children
            .iter()
            .map(|(_, s, _)| match s {
                Size::Fixed(w) => *w,
                Size::Fill => {
                    let extra = if seen_fills < leftover { 1 } else { 0 };
                    seen_fills += 1;
                    per_fill + extra
                }
            })
            .collect()
    }
}

impl<'a> Default for HStack<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Widget for HStack<'a> {
    fn render(&mut self, width: usize) -> Vec<String> {
        let widths = self.layout(width);
        self.widths = widths.clone();

        // Snapshot height policies before we borrow `children` mutably
        // to render them.
        let heights: Vec<Height> =
            self.children.iter().map(|(_, _, h)| *h).collect();

        // Render each child to its assigned column width.
        let rendered: Vec<Vec<String>> = self
            .children
            .iter_mut()
            .zip(widths.iter())
            .map(|((child, _, _), w)| child.render(*w))
            .collect();

        // Row count is the max over `Expand` children. If there are no
        // `Expand` children at all we fall back to the natural max so a
        // stack of only `_clip` children still renders something.
        let any_expand = heights.iter().any(|h| *h == Height::Expand);
        let max_h = if any_expand {
            rendered
                .iter()
                .zip(heights.iter())
                .filter(|(_, h)| **h == Height::Expand)
                .map(|(v, _)| v.len())
                .max()
                .unwrap_or(0)
        } else {
            rendered.iter().map(|v| v.len()).max().unwrap_or(0)
        };

        let mut lines = Vec::with_capacity(max_h);
        let last = rendered.len().saturating_sub(1);
        for row in 0..max_h {
            let mut combined = String::new();
            for (i, child_lines) in rendered.iter().enumerate() {
                let cell = child_lines.get(row).map(String::as_str).unwrap_or("");
                if i == last {
                    // Don't pad the trailing column — let the consumer
                    // (or terminal) handle the right edge.
                    combined.push_str(cell);
                } else {
                    // Pad (but don't truncate) — if a child overflows
                    // its assigned width we leave the overflow visible
                    // rather than silently hiding the bug.
                    combined.push_str(&pad_to_width(cell, widths[i], " "));
                }
            }
            lines.push(combined);
        }
        lines
    }

    fn cursor(&mut self, _width: usize) -> Option<(usize, usize)> {
        // Snapshot widths so we can iterate `children` mutably.
        let widths = self.widths.clone();
        let mut col = 0;
        for (i, (child, _, _)) in self.children.iter_mut().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            if let Some((cr, cc)) = child.cursor(w) {
                return Some((cr, col + cc));
            }
            col += w;
        }
        None
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Single-line text widget.
    struct Text(&'static str);
    impl Widget for Text {
        fn render(&mut self, _width: usize) -> Vec<String> {
            vec![self.0.to_string()]
        }
    }

    /// Multi-line widget for height tests.
    struct Multi(Vec<&'static str>);
    impl Widget for Multi {
        fn render(&mut self, _width: usize) -> Vec<String> {
            self.0.iter().map(|s| s.to_string()).collect()
        }
    }

    /// Widget with a cursor at a fixed (row, col).
    struct Cur(&'static str, usize, usize);
    impl Widget for Cur {
        fn render(&mut self, _width: usize) -> Vec<String> {
            vec![self.0.to_string()]
        }
        fn cursor(&mut self, _width: usize) -> Option<(usize, usize)> {
            Some((self.1, self.2))
        }
    }

    // ── VStack ──────────────────────────────────────────────────────

    #[test]
    fn vstack_concatenates_children() {
        let mut a = Text("hello");
        let mut b = Multi(vec!["x", "y"]);
        let mut c = Text("world");
        let mut v = VStack::new().add(&mut a).add(&mut b).add(&mut c);
        let lines = v.render(10);
        assert_eq!(lines, vec!["hello", "x", "y", "world"]);
    }

    #[test]
    fn vstack_cursor_offset() {
        let mut a = Multi(vec!["aa", "bb", "cc"]); // 3 rows
        let mut b = Cur("xy", 0, 1); // cursor at (0, 1) in itself
        let mut v = VStack::new().add(&mut a).add(&mut b);
        // Render first so heights are populated.
        let _ = v.render(10);
        // Cursor row should be offset by 3 rows of `a`.
        assert_eq!(v.cursor(10), Some((3, 1)));
    }

    #[test]
    fn vstack_empty() {
        let mut v = VStack::new();
        assert!(v.render(10).is_empty());
        assert_eq!(v.cursor(10), None);
    }

    // ── HStack ──────────────────────────────────────────────────────

    #[test]
    fn hstack_layout_fixed_and_fill() {
        let v = HStack {
            children: Vec::new(),
            widths: Vec::new(),
        }
        .layout(20);
        assert!(v.is_empty());

        // 5 + fill + 3, total 20 → fill = 12.
        let mut a = Text("");
        let mut b = Text("");
        let mut c = Text("");
        let h = HStack::new().fixed(&mut a, 5).fill(&mut b).fixed(&mut c, 3);
        assert_eq!(h.layout(20), vec![5, 12, 3]);
    }

    #[test]
    fn hstack_layout_two_fills_split_evenly() {
        let mut a = Text("");
        let mut b = Text("");
        let mut c = Text("");
        let h = HStack::new().fixed(&mut a, 4).fill(&mut b).fill(&mut c);
        // Remaining = 11, two fills → 5 + 6 (leftover goes to first fill).
        assert_eq!(h.layout(15), vec![4, 6, 5]);
    }

    #[test]
    fn hstack_layout_overflows_to_zero_fill() {
        let mut a = Text("");
        let mut b = Text("");
        let h = HStack::new().fixed(&mut a, 30).fill(&mut b);
        assert_eq!(h.layout(10), vec![30, 0]);
    }

    #[test]
    fn hstack_renders_side_by_side() {
        let mut a = Text("hi");
        let mut b = Text("world");
        let mut h = HStack::new().fixed(&mut a, 4).fill(&mut b);
        // a: "hi" padded to 4 → "hi  ", b: "world" not padded.
        assert_eq!(h.render(10), vec!["hi  world"]);
    }

    #[test]
    fn hstack_pads_shorter_columns() {
        let mut a = Multi(vec!["a1", "a2", "a3"]);
        let mut b = Text("b");
        let mut h = HStack::new().fixed(&mut a, 3).fill(&mut b);
        // 3 rows from a; b only has 1 row → trailing column not padded,
        // so rows where b is absent end right after column a.
        assert_eq!(
            h.render(7),
            vec![
                "a1 b", // row 0
                "a2 ",  // row 1: a2 padded, b absent
                "a3 ",  // row 2
            ]
        );
    }

    #[test]
    fn hstack_cursor_offset() {
        let mut a = Text("aaa"); // width 5
        let mut b = Cur("xy", 0, 1); // width = remaining
        let mut h = HStack::new().fixed(&mut a, 5).fill(&mut b);
        let _ = h.render(10);
        // Cursor in b is (0, 1); column offset by a's width (5).
        assert_eq!(h.cursor(10), Some((0, 6)));
    }

    #[test]
    fn hstack_empty() {
        let mut h = HStack::new();
        assert!(h.render(10).is_empty());
        assert_eq!(h.cursor(10), None);
    }

    #[test]
    fn hstack_clip_truncates_tall_child() {
        // body is 2 rows (Expand), sidebar is 4 rows (Clip) → 2 rows total.
        // Total width 6: body fill = 4, sidebar fixed = 2. Trailing column
        // (sidebar) is not padded.
        let mut body = Multi(vec!["b1", "b2"]);
        let mut sidebar = Multi(vec!["s1", "s2", "s3", "s4"]);
        let mut h = HStack::new().fill(&mut body).fixed_clip(&mut sidebar, 2);
        assert_eq!(
            h.render(6),
            vec![
                "b1  s1", // row 0 — body padded to 4, sidebar trailing
                "b2  s2", // row 1 — sidebar's s3 and s4 are dropped
            ]
        );
    }

    #[test]
    fn hstack_clip_short_child_unchanged() {
        // sidebar is shorter than body — clip is a one-sided bound, so
        // it still gets blank trailing cells on the extra rows.
        // Total width 4: body fill = 3, sidebar fixed = 1 (trailing).
        let mut body = Multi(vec!["b1", "b2", "b3"]);
        let mut sidebar = Text("s");
        let mut h = HStack::new().fill(&mut body).fixed_clip(&mut sidebar, 1);
        assert_eq!(h.render(4), vec!["b1 s", "b2 ", "b3 "]);
    }

    #[test]
    fn hstack_clip_uses_expand_max_ignoring_other_clip() {
        // body (Expand) is 2 rows. Two clip sidebars of different heights.
        // Row count should be driven by body alone, not by the tallest clip.
        // Total width 8: left fixed = 2, body fill = 4, right fixed = 2
        // (right is trailing, unpadded).
        let mut body = Multi(vec!["b1", "b2"]);
        let mut left = Multi(vec!["L1", "L2", "L3"]);
        let mut right = Multi(vec!["R1", "R2", "R3", "R4"]);
        let mut h = HStack::new()
            .fixed_clip(&mut left, 2)
            .fill(&mut body)
            .fixed_clip(&mut right, 2);
        assert_eq!(h.render(8), vec!["L1b1  R1", "L2b2  R2"]);
    }

    #[test]
    fn hstack_all_clip_falls_back_to_natural_max() {
        // No expand children: should render the natural max (3 rows)
        // rather than collapsing to zero. Total width 5: a fixed = 3,
        // b fill = 2 (trailing, unpadded).
        let mut a = Multi(vec!["a1", "a2", "a3"]);
        let mut b = Text("b");
        let mut h = HStack::new().fixed_clip(&mut a, 3).fill_clip(&mut b);
        assert_eq!(h.render(5), vec!["a1 b", "a2 ", "a3 "]);
    }

    #[test]
    fn hstack_clip_preserves_width_layout() {
        // Make sure the new Height axis didn't break width accounting:
        // total 10 → body gets 6, side fixed_clip gets 4 (trailing, unpadded).
        let mut body = Multi(vec!["b1", "b2"]);
        let mut side = Text("ss");
        let mut h = HStack::new().fill(&mut body).fixed_clip(&mut side, 4);
        assert_eq!(h.render(10), vec!["b1    ss", "b2    "]);
    }
}
