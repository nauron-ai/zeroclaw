use super::super::request::{ChatRequest, Message};
use super::super::response::ChatResponse;
use super::super::OpenAiProvider;
use crate::providers::traits::Provider;

#[test]
fn creates_with_key() {
    let provider = OpenAiProvider::new(Some("openai-test-credential"));
    assert_eq!(provider.credential.as_deref(), Some("openai-test-credential"));
}

#[test]
fn creates_without_key() {
    let provider = OpenAiProvider::new(None);
    assert!(provider.credential.is_none());
}

#[test]
fn creates_with_empty_key() {
    let provider = OpenAiProvider::new(Some(""));
    assert_eq!(provider.credential.as_deref(), Some(""));
}

#[tokio::test]
async fn chat_fails_without_key() {
    let provider = OpenAiProvider::new(None);
    let result = provider.chat_with_system(None, "hello", "gpt-4o", 0.7).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("API key not set"));
}

#[tokio::test]
async fn chat_with_system_fails_without_key() {
    let provider = OpenAiProvider::new(None);
    let result = provider
        .chat_with_system(Some("You are ZeroClaw"), "test", "gpt-4o", 0.5)
        .await;
    assert!(result.is_err());
}

#[test]
fn request_serializes_with_system_message() {
    let request = ChatRequest::new(
        "gpt-4o",
        vec![Message::system("You are ZeroClaw"), Message::user("hello")],
        0.7,
        None,
    );

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"role\":\"system\""));
    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("gpt-4o"));
}

#[test]
fn request_serializes_without_system() {
    let request = ChatRequest::new("gpt-4o", vec![Message::user("hello")], 0.0, None);
    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("system"));
    assert!(json.contains("\"temperature\":0.0"));
}

#[test]
fn response_deserializes_single_choice() {
    let response: ChatResponse =
        serde_json::from_str(r#"{"choices":[{"message":{"content":"Hi!"}}]}"#).unwrap();
    assert_eq!(response.choices().len(), 1);
    assert_eq!(response.choices()[0].message().effective_content(), "Hi!");
}

#[test]
fn response_deserializes_empty_choices() {
    let response: ChatResponse = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
    assert!(response.choices().is_empty());
}

#[test]
fn response_deserializes_multiple_choices() {
    let response: ChatResponse = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"A"}},{"message":{"content":"B"}}]}"#,
    )
    .unwrap();
    assert_eq!(response.choices().len(), 2);
    assert_eq!(response.choices()[0].message().effective_content(), "A");
}

#[test]
fn response_with_unicode() {
    let response: ChatResponse =
        serde_json::from_str(r#"{"choices":[{"message":{"content":"Hello \u03A9"}}]}"#)
            .unwrap();
    assert_eq!(response.choices()[0].message().effective_content(), "Hello \u{03A9}");
}

#[test]
fn response_with_long_content() {
    let long = "x".repeat(100_000);
    let json = format!(r#"{{"choices":[{{"message":{{"content":"{long}"}}}}]}}"#);
    let response: ChatResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(response.choices()[0].message().content().unwrap().len(), 100_000);
}

#[tokio::test]
async fn warmup_without_key_is_noop() {
    let provider = OpenAiProvider::new(None);
    assert!(provider.warmup().await.is_ok());
}
