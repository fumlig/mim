/// A self-rendering UI element.
///
/// Widgets hold their own state and produce lines when asked.
/// Each line must not exceed `width` visible columns.
pub trait Widget {
    /// Render to lines for the given terminal width.
    fn render(&self, width: u16) -> Vec<String>;

    /// Return cursor position (row, col) relative to this widget's output.
    /// Only meaningful for interactive widgets like Prompt.
    fn cursor(&self, _width: u16) -> Option<(usize, usize)> {
        None
    }
}
