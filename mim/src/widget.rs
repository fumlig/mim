use crate::border::Border;

/// A self-rendering UI element.
///
/// Widgets hold their own state and produce lines when asked.
/// Each line must not exceed `width` visible columns.
pub trait Widget {
    /// Render to lines for the given terminal width.
    fn render(&mut self, width: u16) -> Vec<String>;

    /// Return cursor position (row, col) relative to this widget's output.
    /// Only meaningful for interactive widgets like Editor.
    fn cursor(&mut self, _width: u16) -> Option<(usize, usize)> {
        None
    }
}

pub trait WidgetExt<'a>: Widget + Sized {
    fn pad(&'a mut self, top: usize, right: usize, bottom: usize, left: usize) -> Border<'a, Self> {
        Border::pad(self, top, right, bottom, left)
    }

    fn ascii(&'a mut self) -> Border<'a, Self> {
        Border::ascii(self)
    }

    fn line_numbers(&'a mut self, width: usize) -> Border<'a, Self> {
        Border::line_numbers(self, width)
    }
}

impl<'a, W: Widget> WidgetExt<'a> for W {}
