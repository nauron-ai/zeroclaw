use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, Clone)]
pub(super) struct ResponsesResponse {
    #[serde(default)]
    output: Vec<ResponsesOutput>,
    #[serde(default)]
    output_text: Option<String>,
}

impl ResponsesResponse {
    pub(super) fn new(output: Vec<ResponsesOutput>, output_text: Option<String>) -> Self {
        Self { output, output_text }
    }
    pub(super) fn output_text(&self) -> Option<&str> { self.output_text.as_deref() }
    pub(super) fn outputs(&self) -> &[ResponsesOutput] { &self.output }
    pub(super) fn into_parts(self) -> (Vec<ResponsesOutput>, Option<String>) {
        (self.output, self.output_text)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct ResponsesOutput {
    #[serde(rename = "type", default)]
    kind: ResponsesOutputKind,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    content: Vec<ResponsesContent>,
}

impl ResponsesOutput {
    pub(super) fn new(
        kind: ResponsesOutputKind,
        id: Option<String>,
        call_id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
        content: Vec<ResponsesContent>,
    ) -> Self {
        Self { kind, id, call_id, name, arguments, content }
    }

    pub(super) fn is_function_call(&self) -> bool { self.kind == ResponsesOutputKind::FunctionCall }
    pub(super) fn item_id(&self) -> Option<&str> { self.id.as_deref() }
    pub(super) fn call_id(&self) -> Option<&str> { self.call_id.as_deref() }
    pub(super) fn call_name(&self) -> Option<&str> { self.name.as_deref() }
    pub(super) fn call_arguments(&self) -> Option<&str> { self.arguments.as_deref() }
    pub(super) fn content(&self) -> &[ResponsesContent] { &self.content }
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct ResponsesContent {
    #[serde(rename = "type", default)]
    kind: ResponsesContentKind,
    text: Option<String>,
}

impl ResponsesContent {
    pub(super) fn output_text(text: impl Into<String>) -> Self {
        Self { kind: ResponsesContentKind::OutputText, text: Some(text.into()) }
    }
    pub(super) fn text(&self) -> Option<&str> { self.text.as_deref() }
    pub(super) fn kind(&self) -> ResponsesContentKind { self.kind }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponsesOutputKind {
    FunctionCall,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponsesContentKind {
    OutputText,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamEventType {
    OutputTextDelta,
    OutputTextDone,
    OutputItemDone,
    Completed,
    Done,
    Error,
    Failed,
    Other,
}

impl StreamEventType {
    pub(super) fn from_event(event: &Value) -> Self {
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => Self::OutputTextDelta,
            Some("response.output_text.done") => Self::OutputTextDone,
            Some("response.output_item.done") => Self::OutputItemDone,
            Some("response.completed") => Self::Completed,
            Some("response.done") => Self::Done,
            Some("error") => Self::Error,
            Some("response.failed") => Self::Failed,
            _ => Self::Other,
        }
    }
}
