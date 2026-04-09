use agent::provider::ResponseEvent;

use super::block::{Block, VerticalBorder};
use super::paragraph::Paragraph;
use super::Widget;

enum Role {
    User,
    Assistant,
}

pub struct Message {
    role: Role,
    text: String,
}

impl Message {
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

    fn prefix(&self) -> &'static str {
        match self.role {
            Role::User => "> ",
            Role::Assistant => "| ",
        }
    }
}

impl Widget for Message {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.text.is_empty() {
            return vec![];
        }

        let prefix = self.prefix();
        let mut content = Paragraph::new(&self.text);
        let mut border = Block::new(&mut content).left(VerticalBorder::repeat(prefix.to_string()));
        border.render(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_render() {
        let mut msg = Message::user("hello world this is a test");
        let lines = msg.render(20);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("> hello world this"));
        assert!(lines[1].starts_with("> is a test"));
    }

    #[test]
    fn assistant_render() {
        let mut msg = Message {
            role: Role::Assistant,
            text: "short".to_string(),
        };
        let lines = msg.render(80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("| short"));
    }

    #[test]
    fn empty_assistant_render() {
        let mut msg = Message::assistant();
        let lines = msg.render(80);
        assert!(lines.is_empty());
    }
}
