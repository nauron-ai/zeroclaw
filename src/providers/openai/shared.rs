use crate::providers::ToolCall as ProviderToolCall;
use serde_json::{Map, Value};
use thiserror::Error;

const ASSISTANT_PAYLOAD_CONTEXT: &str = "assistant tool-calls payload";
const TOOL_RESULT_PAYLOAD_CONTEXT: &str = "tool result payload";

#[derive(Debug, Clone)]
pub(in crate::providers::openai) struct AssistantToolCallsPayload {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Vec<ProviderToolCall>,
}

impl AssistantToolCallsPayload {
    pub(in crate::providers::openai) fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    pub(in crate::providers::openai) fn reasoning_content(&self) -> Option<&str> {
        self.reasoning_content.as_deref()
    }

    pub(in crate::providers::openai) fn tool_calls(&self) -> &[ProviderToolCall] {
        &self.tool_calls
    }

    pub(in crate::providers::openai) fn into_parts(
        self,
    ) -> (Option<String>, Option<String>, Vec<ProviderToolCall>) {
        (self.content, self.reasoning_content, self.tool_calls)
    }
}

#[derive(Debug, Clone)]
pub(in crate::providers::openai) struct ToolResultPayload {
    tool_call_id: Option<String>,
    content: Option<String>,
}

impl ToolResultPayload {
    pub(in crate::providers::openai) fn tool_call_id(&self) -> Option<&str> {
        self.tool_call_id.as_deref()
    }

    pub(in crate::providers::openai) fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    pub(in crate::providers::openai) fn into_parts(self) -> (Option<String>, Option<String>) {
        (self.tool_call_id, self.content)
    }
}

#[derive(Debug, Error)]
pub(in crate::providers::openai) enum OpenAiPayloadParseError {
    #[error("invalid JSON in {context}: {source}")]
    InvalidJson {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid shape for {context}: {details}")]
    InvalidShape {
        context: &'static str,
        details: String,
    },
    #[error("invalid field type for {field} in {context}; expected {expected}")]
    InvalidFieldType {
        context: &'static str,
        field: &'static str,
        expected: &'static str,
    },
}

pub(in crate::providers::openai) fn parse_assistant_tool_calls_payload(
    raw: &str,
) -> Result<Option<AssistantToolCallsPayload>, OpenAiPayloadParseError> {
    let Some(map) = parse_candidate_object(raw, ASSISTANT_PAYLOAD_CONTEXT, &["tool_calls"])? else {
        return Ok(None);
    };

    let content = optional_string_field(&map, ASSISTANT_PAYLOAD_CONTEXT, "content")?;
    let reasoning_content =
        optional_string_field(&map, ASSISTANT_PAYLOAD_CONTEXT, "reasoning_content")?;
    let tool_calls = serde_json::from_value::<Vec<ProviderToolCall>>(
        map.get("tool_calls").cloned().expect("candidate key must exist"),
    )
    .map_err(|source| OpenAiPayloadParseError::InvalidShape {
        context: ASSISTANT_PAYLOAD_CONTEXT,
        details: format!("tool_calls: {source}"),
    })?;

    Ok(Some(AssistantToolCallsPayload {
        content,
        reasoning_content,
        tool_calls,
    }))
}

pub(in crate::providers::openai) fn parse_tool_result_payload(
    raw: &str,
) -> Result<Option<ToolResultPayload>, OpenAiPayloadParseError> {
    let Some(map) = parse_candidate_object(
        raw,
        TOOL_RESULT_PAYLOAD_CONTEXT,
        &["tool_call_id", "toolUseId", "tool_use_id"],
    )? else {
        return Ok(None);
    };

    let tool_call_id = first_present_string_field(
        &map,
        TOOL_RESULT_PAYLOAD_CONTEXT,
        &["tool_call_id", "toolUseId", "tool_use_id"],
    )?;
    let content = match map.get("content") {
        Some(Value::Null) | None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => Some(other.to_string()),
    };

    Ok(Some(ToolResultPayload {
        tool_call_id,
        content,
    }))
}

fn parse_candidate_object(
    raw: &str,
    context: &'static str,
    candidate_keys: &[&str],
) -> Result<Option<Map<String, Value>>, OpenAiPayloadParseError> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !contains_candidate_key(trimmed, candidate_keys) {
        return Ok(None);
    }

    let parsed = serde_json::from_str::<Value>(trimmed).map_err(|source| {
        OpenAiPayloadParseError::InvalidJson { context, source }
    })?;
    let Value::Object(map) = parsed else {
        return Err(OpenAiPayloadParseError::InvalidShape {
            context,
            details: "expected top-level object".to_string(),
        });
    };

    if !candidate_keys.iter().any(|key| map.contains_key(*key)) {
        return Ok(None);
    }

    Ok(Some(map))
}

fn optional_string_field(
    map: &Map<String, Value>,
    context: &'static str,
    field: &'static str,
) -> Result<Option<String>, OpenAiPayloadParseError> {
    match map.get(field) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(OpenAiPayloadParseError::InvalidFieldType {
            context,
            field,
            expected: "string",
        }),
    }
}

fn first_present_string_field(
    map: &Map<String, Value>,
    context: &'static str,
    fields: &[&'static str],
) -> Result<Option<String>, OpenAiPayloadParseError> {
    for field in fields {
        match map.get(*field) {
            Some(Value::Null) | None => continue,
            Some(Value::String(text)) => return Ok(Some(text.clone())),
            Some(_) => {
                return Err(OpenAiPayloadParseError::InvalidFieldType {
                    context,
                    field,
                    expected: "string",
                });
            }
        }
    }

    Ok(None)
}

fn contains_candidate_key(raw: &str, candidate_keys: &[&str]) -> bool {
    candidate_keys
        .iter()
        .any(|key| raw.contains(&format!("\"{key}\"")))
}
