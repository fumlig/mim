use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Url { url: String },
    Base64 { media_type: String, data: Vec<u8> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileSource {
    Url { url: String },
    Base64 { filename: String, data: Vec<u8> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
    Developer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text { text: String },
    Image { source: ImageSource },
    File { source: FileSource },
    Refusal { text: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<MessageContent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: String,
    pub output: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReasoningContent {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reasoning {
    pub id: String,
    pub summary: Vec<String>,
    pub content: Option<Vec<ReasoningContent>>,
    pub encrypted_content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Compaction {
    pub encrypted_content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Message(Message),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Reasoning(Reasoning),
    Compaction(Compaction),
}
