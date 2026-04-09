use crate::format;
use crate::widget::Widget;

/// Helper widget that word-wraps pre-existing text.
pub struct Paragraph<'a> {
    text: &'a str,
}

impl<'a> Paragraph<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl Widget for Paragraph<'_> {
    fn render(&mut self, width: usize) -> Vec<String> {
        let w = width as usize;
        if w == 0 {
            return vec![];
        }
        format::wrap_text(self.text, w, "-")
    }
}
