use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct ChatResponse {
    choices: Vec<Choice>,
}

impl ChatResponse {
    pub(super) fn choices(&self) -> &[Choice] {
        &self.choices
    }

    pub(super) fn into_first_choice(self) -> Option<Choice> {
        self.choices.into_iter().next()
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

impl Choice {
    pub(super) fn message(&self) -> &ResponseMessage {
        &self.message
    }

    pub(super) fn into_parts(self) -> (ResponseMessage, Option<String>) {
        (self.message, self.finish_reason)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

impl ResponseMessage {
    pub(super) fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    pub(super) fn effective_content(&self) -> String {
        match &self.content {
            Some(content) if !content.is_empty() => content.clone(),
            _ => self.reasoning_content.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct NativeChatResponse {
    choices: Vec<NativeChoice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

impl NativeChatResponse {
    pub(super) fn choices(&self) -> &[NativeChoice] {
        &self.choices
    }

    pub(super) fn usage(&self) -> Option<&UsageInfo> {
        self.usage.as_ref()
    }

    pub(super) fn into_first_choice_and_usage(self) -> Option<(NativeChoice, Option<UsageInfo>)> {
        self.choices.into_iter().next().map(|choice| (choice, self.usage))
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub(super) struct UsageInfo {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

impl UsageInfo {
    pub(super) fn prompt_tokens(self) -> Option<u64> {
        self.prompt_tokens
    }

    pub(super) fn completion_tokens(self) -> Option<u64> {
        self.completion_tokens
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct NativeChoice {
    message: NativeResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

impl NativeChoice {
    pub(super) fn message(&self) -> &NativeResponseMessage {
        &self.message
    }

    pub(super) fn into_parts(self) -> (NativeResponseMessage, Option<String>) {
        (self.message, self.finish_reason)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct NativeResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<super::request::NativeToolCall>>,
}

impl NativeResponseMessage {
    pub(super) fn effective_content(&self) -> Option<String> {
        match &self.content {
            Some(content) if !content.is_empty() => Some(content.clone()),
            _ => self.reasoning_content.clone(),
        }
    }

    pub(super) fn reasoning_content(&self) -> Option<&str> {
        self.reasoning_content.as_deref()
    }

    pub(super) fn take_tool_calls(self) -> Vec<super::request::NativeToolCall> {
        self.tool_calls.unwrap_or_default()
    }
}
