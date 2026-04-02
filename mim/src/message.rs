use agent::provider::ResponseEvent;

use crate::format::{prefix_lines, wrap_text};
use crate::widget::Widget;

pub enum Role {
    User,
    Assistant,
}

pub struct MessageBlock {
    role: Role,
    text: String,
}

impl MessageBlock {
    pub fn user(text: &str) -> Self {
        Self {
            role: Role::User,
            text: text.to_string(),
        }
    }

    pub fn assistant() -> Self {
        Self {
            role: Role::Assistant,
            text: String::new(),
        }
    }

    pub fn push_event(&mut self, event: &ResponseEvent) {
        match event {
            ResponseEvent::TextDelta(delta) => {
                self.text.push_str(delta);
            }
            ResponseEvent::ToolCall(tc) => {
                self.ensure_newline();
                self.text
                    .push_str(&format!("[call {}({})]", tc.name, tc.arguments));
                self.text.push('\n');
            }
            ResponseEvent::ToolResult(tr) => {
                self.text.push_str(&format!("[result: {}]", tr.output));
                self.text.push('\n');
            }
            _ => {}
        }
    }

    pub fn push_error(&mut self, error: &str) {
        self.ensure_newline();
        self.text.push_str(&format!("[error: {}]", error));
    }

    fn ensure_newline(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }
}

impl Widget for MessageBlock {
    fn render(&self, width: u16) -> Vec<String> {
        if self.text.is_empty() {
            return vec![];
        }

        let w = width as usize;
        if w == 0 {
            return vec![];
        }

        let (prefix, indent) = match self.role {
            Role::User => ("> ", "  "),
            Role::Assistant => ("", ""),
        };
        let wrap_w = w.saturating_sub(prefix.len()).max(1);
        let lines = wrap_text(&self.text, wrap_w, "-");
        prefix_lines(&lines, prefix, indent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_render() {
        let block = MessageBlock::user("hello world this is a test");
        let lines = block.render(20);
        assert_eq!(lines[0], "> hello world this");
        assert_eq!(lines[1], "  is a test");
    }

    #[test]
    fn assistant_render() {
        let block = MessageBlock {
            role: Role::Assistant,
            text: "short".to_string(),
        };
        assert_eq!(block.render(80), vec!["short"]);
    }

    #[test]
    fn empty_assistant_render() {
        let block = MessageBlock::assistant();
        let lines = block.render(80);
        assert!(lines.is_empty());
    }
}
