use crate::block::{Block, HorizontalBorder, VerticalBorder};

/// A self-rendering UI element.
///
/// Widgets hold their own state and produce lines when asked.
/// Each line must not exceed `width` visible columns.
pub trait Widget {
    /// Render to lines for the given terminal width.
    fn render(&mut self, width: usize) -> Vec<String>;

    /// Return cursor position (row, col) relative to this widget's output.
    /// Only meaningful for interactive widgets like Editor.
    fn cursor(&mut self, _width: usize) -> Option<(usize, usize)> {
        None
    }
}

pub trait WidgetExt<'a>: Widget + Sized {
    fn pad(&'a mut self, top: usize, right: usize, bottom: usize, left: usize) -> Block<'a, Self> {
        Block::new(self)
            .top(HorizontalBorder::pad(top))
            .right(VerticalBorder::pad(right))
            .bottom(HorizontalBorder::pad(bottom))
            .left(VerticalBorder::pad(left))
    }

    fn ascii(&'a mut self) -> Block<'a, Self> {
        Block::new(self)
            .top(
                HorizontalBorder::new("-".to_string())
                    .left("+".to_string())
                    .right("+".to_string()),
            )
            .bottom(
                HorizontalBorder::new("-".to_string())
                    .left("+".to_string())
                    .right("+".to_string()),
            )
            .left(VerticalBorder::repeat("|".to_string()))
            .right(VerticalBorder::repeat("|".to_string()))
        //.bottom(BlockBorder::new("-".to_string(), 1))
    }

    fn line_numbers(&'a mut self, w: usize) -> Block<'a, Self> {
        Block::new(self).left(VerticalBorder::counter(w))
    }
}

impl<'a, W: Widget> WidgetExt<'a> for W {}
