use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    pub(super) fn from_chat_role(role: &str) -> Self {
        match role {
            "system" => Self::System,
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "tool" => Self::Tool,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

impl ChatRequest {
    pub(super) fn new(model: impl Into<String>, messages: Vec<Message>, temperature: f64, max_tokens: Option<u32>) -> Self {
        Self { model: model.into(), messages, temperature, max_tokens }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct Message {
    role: MessageRole,
    content: String,
}

impl Message {
    pub(super) fn system(content: impl Into<String>) -> Self { Self::new(MessageRole::System, content) }
    pub(super) fn user(content: impl Into<String>) -> Self { Self::new(MessageRole::User, content) }
    fn new(role: MessageRole, content: impl Into<String>) -> Self { Self { role, content: content.into() } }
}

#[derive(Debug, Serialize)]
pub(super) struct NativeChatRequest {
    model: String,
    messages: Vec<NativeMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<NativeToolChoice>,
}

impl NativeChatRequest {
    pub(super) fn new(
        model: impl Into<String>,
        messages: Vec<NativeMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
        tools: Option<Vec<NativeToolSpec>>,
    ) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature,
            max_tokens,
            tool_choice: tools.as_ref().map(|_| NativeToolChoice::Auto),
            tools,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct NativeMessage {
    role: MessageRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<NativeToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

impl NativeMessage {
    pub(super) fn plain(role: MessageRole, content: impl Into<String>) -> Self {
        Self { role, content: Some(content.into()), tool_call_id: None, tool_calls: None, reasoning_content: None }
    }

    pub(super) fn assistant_tool_calls(content: Option<String>, reasoning_content: Option<String>, tool_calls: Vec<NativeToolCall>) -> Self {
        Self { role: MessageRole::Assistant, content, tool_call_id: None, tool_calls: Some(tool_calls), reasoning_content }
    }

    pub(super) fn tool_result(content: Option<String>, tool_call_id: Option<String>) -> Self {
        Self { role: MessageRole::Tool, content, tool_call_id, tool_calls: None, reasoning_content: None }
    }

    pub(super) fn reasoning_content(&self) -> Option<&str> { self.reasoning_content.as_deref() }
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum NativeToolChoice {
    Auto,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct NativeToolSpec {
    #[serde(rename = "type")]
    kind: NativeToolKind,
    function: NativeToolFunctionSpec,
}

impl NativeToolSpec {
    pub(super) fn new_function(name: impl Into<String>, description: impl Into<String>, parameters: serde_json::Value) -> Self {
        Self { kind: NativeToolKind::Function, function: NativeToolFunctionSpec::new(name, description, parameters) }
    }

    pub(super) fn kind(&self) -> NativeToolKind { self.kind }
    pub(super) fn function(&self) -> &NativeToolFunctionSpec { &self.function }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum NativeToolKind {
    Function,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct NativeToolFunctionSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

impl NativeToolFunctionSpec {
    pub(super) fn new(name: impl Into<String>, description: impl Into<String>, parameters: serde_json::Value) -> Self {
        Self { name: name.into(), description: description.into(), parameters }
    }

    pub(super) fn name(&self) -> &str { &self.name }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct NativeToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<NativeToolKind>,
    function: NativeFunctionCall,
}

impl NativeToolCall {
    pub(super) fn function_call(id: Option<String>, name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self { id, kind: Some(NativeToolKind::Function), function: NativeFunctionCall::new(name, arguments) }
    }

    pub(super) fn into_parts(self) -> (Option<String>, NativeFunctionCall) { (self.id, self.function) }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct NativeFunctionCall {
    name: String,
    arguments: String,
}

impl NativeFunctionCall {
    fn new(name: impl Into<String>, arguments: impl Into<String>) -> Self { Self { name: name.into(), arguments: arguments.into() } }
    pub(super) fn into_parts(self) -> (String, String) { (self.name, self.arguments) }
}
