use crate::widget::Widget;

/// A set of frames that define a spinner animation.
#[derive(Debug, Clone, Copy)]
pub enum SpinnerVariant {
    /// Classic rotating line: | / - \
    Line,
}

impl SpinnerVariant {
    /// Returns the frames for this variant.
    fn frames(self) -> &'static [&'static str] {
        match self {
            SpinnerVariant::Line => &["|", "/", "-", "\\"],
        }
    }
}

/// A simple character spinner that cycles through animation frames.
pub struct Spinner {
    variant: SpinnerVariant,
    index: usize,
}

impl Spinner {
    /// Creates a new spinner with the given variant.
    pub fn new(variant: SpinnerVariant) -> Self {
        Self { variant, index: 0 }
    }

    /// Returns the current frame without advancing.
    fn get(&self) -> &'static str {
        let frames = self.variant.frames();
        frames[self.index]
    }

    /// Advances the spinner by one step and returns the new frame.
    pub fn step(&mut self) -> &'static str {
        let frames = self.variant.frames();
        self.index = (self.index + 1) % frames.len();
        frames[self.index]
    }
}

impl Widget for Spinner {
    fn render(&self, _: u16) -> Vec<String> {
        vec![self.get().into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_cycles_through_frames() {
        let mut s = Spinner::new(SpinnerVariant::Line);
        assert_eq!(s.get(), "|");
        assert_eq!(s.step(), "/");
        assert_eq!(s.step(), "-");
        assert_eq!(s.step(), "\\");
        assert_eq!(s.step(), "|");
    }
}
