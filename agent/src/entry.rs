use serde_json::Value;

#[derive(Clone, Debug)]
pub enum ImageSource {
    Url(String), // https://... or data:image/...;base64,...
    Base64 { media_type: String, data: Vec<u8> },
}

#[derive(Clone, Debug)]
pub enum FileSource {
    Url(String),
    Base64 { filename: String, data: Vec<u8> },
}

#[derive(Clone, Debug)]
pub enum Role {
    User,
    Assistant,
    System,
    Developer,
}

#[derive(Clone, Debug)]
pub enum MessageContent {
    Text(String),
    Image(ImageSource),
    File(FileSource),
    // Output-only: model refused to answer
    Refusal(String),
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub content: Vec<MessageContent>,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub call_id: String,
    pub output: Value,
}

#[derive(Clone, Debug)]
pub struct ReasoningContent {
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Reasoning {
    pub id: String,
    pub summary: Vec<String>,
    pub content: Option<Vec<ReasoningContent>>,
    pub encrypted_content: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Compaction {
    pub encrypted_content: String,
}

#[derive(Clone, Debug)]
pub enum Entry {
    Message(Message),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Reasoning(Reasoning),
    Compaction(Compaction),
}
