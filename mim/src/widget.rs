pub mod block;
pub mod editor;
pub mod layout;
pub mod message;
pub mod paragraph;
pub mod spinner;

pub use block::{Block, HorizontalBorder, VerticalBorder};
pub use editor::{Editor, EditorAction};
pub use layout::{HStack, VStack};
pub use message::Message;
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
