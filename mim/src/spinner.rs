use crate::widget::Widget;

/// A simple character spinner that cycles through animation frames.
pub struct Spinner {
    frames: Box<dyn Iterator<Item = &'static str>>,
    current: &'static str,
}

impl Spinner {
    pub const ASCII: &[&str] = &["|", "/", "-", "\\"];

    /// Creates a new spinner with the given style.
    pub fn new(frames: &'static [&'static str]) -> Self {
        let mut iter = frames.iter().copied().cycle();
        let current = iter.next().unwrap();
        Self {
            frames: Box::new(iter),
            current,
        }
    }

    /// Returns the current frame without advancing.
    pub fn get(&self) -> &str {
        self.current
    }

    /// Advances the spinner by one step and returns the new frame.
    pub fn step(&mut self) -> &str {
        self.current = self.frames.next().unwrap();
        self.current
    }
}

impl Widget for Spinner {
    fn render(&mut self, _: u16) -> Vec<String> {
        vec![self.current.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_through_frames() {
        let mut s = Spinner::new(Spinner::ASCII);
        assert_eq!(s.get(), "|");
        assert_eq!(s.step(), "/");
        assert_eq!(s.step(), "-");
        assert_eq!(s.step(), "\\");
        assert_eq!(s.step(), "|");
    }
}
