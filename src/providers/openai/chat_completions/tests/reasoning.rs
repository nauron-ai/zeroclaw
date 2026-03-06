use super::super::request::{MessageRole, NativeMessage};
use super::super::response::{ChatResponse, NativeChatResponse};
use super::super::OpenAiProvider;
use crate::providers::traits::NormalizedStopReason;

#[test]
fn reasoning_content_fallback_empty_content() {
    let response: ChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"","reasoning_content":"Thinking..."}}]}"#,
    )
    .unwrap();
    assert_eq!(response.choices()[0].message().effective_content(), "Thinking...");
}

#[test]
fn reasoning_content_fallback_null_content() {
    let response: ChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":null,"reasoning_content":"Thinking..."}}]}"#,
    )
    .unwrap();
    assert_eq!(response.choices()[0].message().effective_content(), "Thinking...");
}

#[test]
fn reasoning_content_not_used_when_content_present() {
    let response: ChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"Hello","reasoning_content":"Ignored"}}]}"#,
    )
    .unwrap();
    assert_eq!(response.choices()[0].message().effective_content(), "Hello");
}

#[test]
fn native_response_reasoning_content_fallback() {
    let response: NativeChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"","reasoning_content":"Native thinking"}}]}"#,
    )
    .unwrap();
    let message = response.choices()[0].message();
    assert_eq!(message.effective_content(), Some("Native thinking".to_string()));
}

#[test]
fn native_response_reasoning_content_ignored_when_content_present() {
    let response: NativeChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"Real answer","reasoning_content":"Ignored"}}]}"#,
    )
    .unwrap();
    let message = response.choices()[0].message();
    assert_eq!(message.effective_content(), Some("Real answer".to_string()));
}

#[test]
fn parse_native_response_captures_reasoning_content() {
    let response: NativeChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"answer","reasoning_content":"thinking step","tool_calls":[{"id":"call_1","type":"function","function":{"name":"shell","arguments":"{}"}}]},"finish_reason":"length"}]}"#,
    )
    .unwrap();
    let choice = response.into_first_choice_and_usage().unwrap().0;
    let parsed = OpenAiProvider::parse_native_response(choice);
    assert_eq!(parsed.reasoning_content.as_deref(), Some("thinking step"));
    assert_eq!(parsed.tool_calls.len(), 1);
    assert_eq!(parsed.stop_reason, Some(NormalizedStopReason::MaxTokens));
    assert_eq!(parsed.raw_stop_reason.as_deref(), Some("length"));
}

#[test]
fn parse_native_response_none_reasoning_content_for_normal_model() {
    let response: NativeChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]}"#,
    )
    .unwrap();
    let choice = response.into_first_choice_and_usage().unwrap().0;
    let parsed = OpenAiProvider::parse_native_response(choice);
    assert!(parsed.reasoning_content.is_none());
    assert_eq!(parsed.stop_reason, Some(NormalizedStopReason::EndTurn));
    assert_eq!(parsed.raw_stop_reason.as_deref(), Some("stop"));
}

#[test]
fn native_message_omits_reasoning_content_when_none() {
    let message = NativeMessage::plain(MessageRole::Assistant, "hi");
    let json = serde_json::to_string(&message).unwrap();
    assert!(!json.contains("reasoning_content"));
}

#[test]
fn native_message_includes_reasoning_content_when_some() {
    let message = NativeMessage::assistant_tool_calls(
        Some("hi".to_string()),
        Some("thinking...".to_string()),
        vec![],
    );
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("reasoning_content"));
    assert!(json.contains("thinking..."));
}
