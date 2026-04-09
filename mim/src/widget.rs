pub mod block;
pub mod editor;
pub mod layout;
pub mod paragraph;
pub mod spinner;

pub use block::{Block, HorizontalBorder, VerticalBorder};
pub use editor::{Editor, EditorAction};
pub use layout::{HStack, VStack};
pub use paragraph::Paragraph;
pub use spinner::Spinner;

/// A self-rendering UI element.
///
/// Widgets hold their own state and produce lines when asked.
/// Each line must not exceed `width` visible columns.
///
/// Widgets that want to claim the hardware cursor embed
/// [`crate::format::CURSOR_MARKER`] at the desired position in their own
/// rendered output while they're focused. Containers don't need to know
/// anything about it — the marker rides along inside the rendered strings
/// and is extracted by the screen at the end of the render pass.
pub trait Widget {
    /// Render to lines for the given terminal width.
    fn render(&mut self, width: usize) -> Vec<String>;
}

/// Extension helpers for wrapping any widget in a [`Block`].
pub trait WidgetExt<'a>: Widget + Sized {
    /// Wrap `self` in an empty [`Block`] for further configuration.
    fn block(&'a mut self) -> Block<'a, Self> {
        Block::new(self)
    }
}

impl<'a, W: Widget> WidgetExt<'a> for W {}
