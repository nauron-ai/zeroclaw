use super::super::payload::{build_responses_input, convert_tool_specs};
use super::super::request::{
    ReasoningEffort, ResponsesCreateEvent, ResponsesInputContentKind, ResponsesRequest,
    ResponsesRole,
};
use crate::providers::ChatMessage;
use serde_json::Value;

#[test]
fn convert_tool_specs_maps_toolspec_to_openai_function_shape() {
    let tools = vec![crate::tools::ToolSpec {
        name: "shell".to_string(),
        description: "Run shell command".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string"}},
            "required": ["command"]
        }),
    }];

    let converted = convert_tool_specs(Some(&tools));
    assert_eq!(converted.len(), 1);
    assert_eq!(converted[0]["type"], "function");
    assert_eq!(converted[0]["name"], "shell");
    assert_eq!(converted[0]["description"], "Run shell command");
    assert_eq!(converted[0]["parameters"]["required"][0], "command");
}

#[test]
fn convert_tool_specs_adds_items_for_array_without_items() {
    let tools = vec![crate::tools::ToolSpec {
        name: "channel_ack_config".to_string(),
        description: "test".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"rules": {"type": ["array", "null"]}},
            "required": ["rules"]
        }),
    }];

    let converted = convert_tool_specs(Some(&tools));
    assert_eq!(converted.len(), 1);
    assert_eq!(
        converted[0]["parameters"]["properties"]["rules"]["items"],
        serde_json::json!({})
    );
}

#[test]
fn build_responses_input_maps_content_types_by_role() {
    let messages = vec![
        ChatMessage { role: "system".into(), content: "You are helpful.".into() },
        ChatMessage { role: "user".into(), content: "Hi".into() },
        ChatMessage { role: "assistant".into(), content: "Hello!".into() },
        ChatMessage { role: "user".into(), content: "Thanks".into() },
    ];

    let (instructions, input) = build_responses_input(&messages).unwrap();
    assert_eq!(instructions, "You are helpful.");
    assert_eq!(input.len(), 3);

    let json: Vec<Value> = input
        .iter()
        .map(|item| serde_json::to_value(item).unwrap())
        .collect();
    assert_eq!(json[0]["role"], "user");
    assert_eq!(json[0]["content"][0]["type"], "input_text");
    assert_eq!(json[1]["role"], "assistant");
    assert_eq!(json[1]["content"][0]["type"], "output_text");
    assert_eq!(json[2]["role"], "user");
    assert_eq!(json[2]["content"][0]["type"], "input_text");
}

#[test]
fn build_responses_input_uses_default_instructions_without_system() {
    let messages = vec![ChatMessage { role: "user".into(), content: "Hello".into() }];
    let (instructions, input) = build_responses_input(&messages).unwrap();
    assert_eq!(instructions, super::super::DEFAULT_CODEX_INSTRUCTIONS);
    assert_eq!(input.len(), 1);
}

#[test]
fn build_responses_input_ignores_unknown_roles() {
    let messages = vec![
        ChatMessage { role: "tool".into(), content: "result".into() },
        ChatMessage { role: "user".into(), content: "Go".into() },
    ];

    let (instructions, input) = build_responses_input(&messages).unwrap();
    assert_eq!(instructions, super::super::DEFAULT_CODEX_INSTRUCTIONS);
    assert_eq!(input.len(), 2);
    let first = serde_json::to_value(&input[0]).unwrap();
    assert_eq!(first["role"], "user");
    assert_eq!(first["content"][0]["text"], "[tool_result]\nresult");
    assert_eq!(serde_json::to_value(&input[1]).unwrap()["role"], "user");
}

#[test]
fn build_responses_input_parses_native_tool_result_payload() {
    let messages = vec![ChatMessage::tool(
        r#"{"tool_call_id":"call_123","content":"uptime output"}"#,
    )];
    let (_, input) = build_responses_input(&messages).unwrap();

    assert_eq!(input.len(), 1);
    let json = serde_json::to_value(&input[0]).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"][0]["text"], "[tool_result:call_123]\nuptime output");
}

#[test]
fn build_responses_input_strips_assistant_native_payload_wrapper() {
    let messages = vec![ChatMessage::assistant(
        r#"{"content":"checking","tool_calls":[{"id":"tc1","name":"shell","arguments":"{}"}]}"#,
    )];
    let (_, input) = build_responses_input(&messages).unwrap();

    assert_eq!(input.len(), 1);
    let json = serde_json::to_value(&input[0]).unwrap();
    assert_eq!(json["role"], "assistant");
    assert_eq!(json["content"][0]["text"], "checking");
}

#[test]
fn build_responses_input_handles_image_markers() {
    let messages = vec![ChatMessage::user("Describe this\n\n[IMAGE:data:image/png;base64,abc]")];
    let (_, input) = build_responses_input(&messages).unwrap();

    assert_eq!(input.len(), 1);
    assert_eq!(input[0].role(), ResponsesRole::User);
    assert_eq!(input[0].content()[0].kind(), ResponsesInputContentKind::InputText);
    assert_eq!(input[0].content()[1].kind(), ResponsesInputContentKind::InputImage);
}

#[test]
fn build_responses_input_preserves_text_only_messages() {
    let messages = vec![ChatMessage::user("Hello without images")];
    let (_, input) = build_responses_input(&messages).unwrap();

    assert_eq!(input.len(), 1);
    assert_eq!(input[0].content().len(), 1);
    let json = serde_json::to_value(&input[0].content()[0]).unwrap();
    assert_eq!(json["type"], "input_text");
    assert_eq!(json["text"], "Hello without images");
}

#[test]
fn build_responses_input_handles_multiple_images() {
    let messages = vec![ChatMessage::user(
        "Compare these: [IMAGE:data:image/png;base64,img1] and [IMAGE:data:image/jpeg;base64,img2]",
    )];
    let (_, input) = build_responses_input(&messages).unwrap();
    assert_eq!(input.len(), 1);
    assert_eq!(input[0].content().len(), 3);
}

#[test]
fn build_responses_input_fails_on_malformed_assistant_payload() {
    let messages = vec![ChatMessage::assistant(r#"{"tool_calls":123}"#)];
    let error = build_responses_input(&messages).expect_err("malformed payload must fail");
    assert!(error.to_string().contains("invalid shape"));
}

#[test]
fn build_responses_input_fails_on_malformed_tool_payload() {
    let messages = vec![ChatMessage::tool(r#"{"tool_call_id":123,"content":"done"}"#)];
    let error = build_responses_input(&messages).expect_err("malformed payload must fail");
    assert!(error.to_string().contains("invalid field type"));
}

#[test]
fn websocket_create_event_flattens_request_payload() {
    let request = ResponsesRequest::new(
        "gpt-5.3-codex",
        vec![],
        "test",
        ReasoningEffort::High,
        None,
    );

    let payload = serde_json::to_value(ResponsesCreateEvent::new(&request)).unwrap();
    assert_eq!(payload["type"], "response.create");
    assert_eq!(payload["model"], "gpt-5.3-codex");
    assert_eq!(payload["parallel_tool_calls"], true);
}
