use super::super::native::parse_native_tool_spec;
use super::super::request::NativeToolKind;
use super::super::response::NativeChatResponse;
use super::super::OpenAiProvider;
use crate::providers::traits::{ChatMessage, Provider};

#[tokio::test]
async fn chat_with_tools_fails_without_key() {
    let provider = OpenAiProvider::new(None);
    let messages = vec![ChatMessage::user("hello".to_string())];
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "shell",
            "description": "Run a shell command",
            "parameters": {
                "type": "object",
                "properties": {"command": { "type": "string"}},
                "required": ["command"]
            }
        }
    })];

    let result = provider.chat_with_tools(&messages, &tools, "gpt-4o", 0.7).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("API key not set"));
}

#[tokio::test]
async fn chat_with_tools_rejects_invalid_tool_shape() {
    let provider = OpenAiProvider::new(Some("openai-test-credential"));
    let messages = vec![ChatMessage::user("hello".to_string())];
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "shell",
            "parameters": {
                "type": "object",
                "properties": {"command": { "type": "string"}},
                "required": ["command"]
            }
        }
    })];

    let result = provider.chat_with_tools(&messages, &tools, "gpt-4o", 0.7).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid OpenAI tool specification"));
}

#[test]
fn native_tool_spec_deserializes_from_openai_format() {
    let spec = parse_native_tool_spec(serde_json::json!({
        "type": "function",
        "function": {
            "name": "shell",
            "description": "Run a shell command",
            "parameters": {
                "type": "object",
                "properties": {"command": { "type": "string"}},
                "required": ["command"]
            }
        }
    }))
    .unwrap();

    assert_eq!(spec.kind(), NativeToolKind::Function);
    assert_eq!(spec.function().name(), "shell");
}

#[test]
fn native_response_parses_usage() {
    let response: NativeChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"Hello"}}],"usage":{"prompt_tokens":100,"completion_tokens":50}}"#,
    )
    .unwrap();

    let usage = response.usage().unwrap();
    assert_eq!(usage.prompt_tokens(), Some(100));
    assert_eq!(usage.completion_tokens(), Some(50));
}

#[test]
fn native_response_parses_without_usage() {
    let response: NativeChatResponse =
        serde_json::from_str(r#"{"choices":[{"message":{"content":"Hello"}}]}"#).unwrap();
    assert!(response.usage().is_none());
}

#[test]
fn convert_messages_round_trips_reasoning_content() {
    let messages = vec![ChatMessage::assistant(
        serde_json::json!({
            "content": "I will check",
            "tool_calls": [{"id": "tc_1", "name": "shell", "arguments": "{}"}],
            "reasoning_content": "Let me think..."
        })
        .to_string(),
    )];

    let native = OpenAiProvider::convert_messages(&messages).unwrap();
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].reasoning_content(), Some("Let me think..."));
}

#[test]
fn convert_messages_no_reasoning_content_when_absent() {
    let messages = vec![ChatMessage::assistant(
        serde_json::json!({
            "content": "I will check",
            "tool_calls": [{"id": "tc_1", "name": "shell", "arguments": "{}"}]
        })
        .to_string(),
    )];

    let native = OpenAiProvider::convert_messages(&messages).unwrap();
    assert_eq!(native.len(), 1);
    assert!(native[0].reasoning_content().is_none());
}

#[test]
fn convert_messages_fails_on_malformed_assistant_structured_payload() {
    let messages = vec![ChatMessage::assistant(r#"{"tool_calls":123}"#)];
    let error = OpenAiProvider::convert_messages(&messages)
        .expect_err("malformed payload must fail");
    assert!(error.to_string().contains("invalid shape"));
}

#[test]
fn convert_messages_fails_on_malformed_tool_result_payload() {
    let messages = vec![ChatMessage::tool(r#"{"tool_call_id":123,"content":"done"}"#)];
    let error = OpenAiProvider::convert_messages(&messages)
        .expect_err("malformed payload must fail");
    assert!(error.to_string().contains("invalid field type"));
}
