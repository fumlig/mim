use crate::widget::Widget;

/// A simple character spinner driven by an iterator of frames.
pub struct Spinner {
    frames: Box<dyn Iterator<Item = String>>,
    current: String,
}

impl Spinner {
    pub const ASCII: &[&str] = &["|", "/", "-", "\\"];

    /// Create a spinner that cycles forever through a fixed set of frames.
    pub fn cycle<I>(frames: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String> + 'static,
        I::IntoIter: Clone + 'static,
    {
        Self::from_iter(frames.into_iter().map(Into::into).cycle())
    }

    /// Create a spinner from any iterator of frames. Wrap in `.cycle()`
    /// yourself if you want looping. Stays on the last frame when exhausted.
    pub fn from_iter<I>(frames: I) -> Self
    where
        I: IntoIterator<Item = String> + 'static,
    {
        let mut iter = frames.into_iter();
        let current = iter.next().unwrap_or_default();
        Self {
            frames: Box::new(iter),
            current,
        }
    }

    /// Returns the current frame without advancing.
    pub fn get(&self) -> &str {
        &self.current
    }

    /// Advances the spinner by one step and returns the new frame.
    /// Stays on the last frame if the iterator is exhausted.
    pub fn step(&mut self) -> &str {
        if let Some(next) = self.frames.next() {
            self.current = next;
        }
        &self.current
    }
}

impl Widget for Spinner {
    fn render(&mut self, _: usize) -> Vec<String> {
        vec![self.current.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_through_frames() {
        let mut s = Spinner::cycle(Spinner::ASCII.iter().copied());
        assert_eq!(s.get(), "|");
        assert_eq!(s.step(), "/");
        assert_eq!(s.step(), "-");
        assert_eq!(s.step(), "\\");
        assert_eq!(s.step(), "|");
    }

    #[test]
    fn finite_iterator_sticks_on_last_frame() {
        let mut s = Spinner::from_iter(["a".to_string(), "b".to_string()].into_iter());
        assert_eq!(s.get(), "a");
        assert_eq!(s.step(), "b");
        assert_eq!(s.step(), "b");
        assert_eq!(s.step(), "b");
    }

    #[test]
    fn procedural_frames() {
        let mut s = Spinner::from_iter((1..).map(|n| format!("{n}")));
        assert_eq!(s.get(), "1");
        assert_eq!(s.step(), "2");
        assert_eq!(s.step(), "3");
    }
}
