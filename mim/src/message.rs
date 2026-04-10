use agent::entry::{self, Entry, MessageContent};

use crate::widget::{Block, Paragraph, VerticalBorder, Widget};

enum Role {
    User,
    Assistant,
}

pub struct Message {
    role: Role,
    text: String,
}

impl Message {
    /// Build a renderable message from any [`Entry`].
    pub fn from_entry(entry: &Entry) -> Self {
        match entry {
            Entry::Message(m) => {
                let role = match m.role {
                    entry::Role::User => Role::User,
                    _ => Role::Assistant,
                };
                let text = m
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        MessageContent::Text { text } => Some(text.as_str()),
                        MessageContent::Refusal { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                Self { role, text }
            }
            Entry::Reasoning(r) => {
                let text = r
                    .content
                    .as_ref()
                    .map(|parts| {
                        parts
                            .iter()
                            .map(|c| c.text.as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();
                Self {
                    role: Role::Assistant,
                    text,
                }
            }
            Entry::ToolCall(tc) => Self {
                role: Role::Assistant,
                text: format!("[call {}({})]", tc.name, tc.arguments),
            },
            Entry::ToolResult(tr) => Self {
                role: Role::Assistant,
                text: format!("[result: {}]", tr.output),
            },
            Entry::Compaction(_) => Self {
                role: Role::Assistant,
                text: String::new(),
            },
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
    use agent::entry;

    fn user_entry(text: &str) -> Entry {
        Entry::Message(entry::Message {
            role: entry::Role::User,
            content: vec![entry::MessageContent::Text {
                text: text.to_string(),
            }],
        })
    }

    fn assistant_entry(text: &str) -> Entry {
        Entry::Message(entry::Message {
            role: entry::Role::Assistant,
            content: vec![entry::MessageContent::Text {
                text: text.to_string(),
            }],
        })
    }

    #[test]
    fn user_render() {
        let mut msg = Message::from_entry(&user_entry("hello world this is a test"));
        let lines = msg.render(20);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("> hello world this"));
        assert!(lines[1].starts_with("> is a test"));
    }

    #[test]
    fn assistant_render() {
        let mut msg = Message::from_entry(&assistant_entry("short"));
        let lines = msg.render(80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("| short"));
    }

    #[test]
    fn empty_assistant_render() {
        let mut msg = Message::from_entry(&assistant_entry(""));
        let lines = msg.render(80);
        assert!(lines.is_empty());
    }
}
