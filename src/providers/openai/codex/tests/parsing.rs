use super::super::payload::{
    extract_responses_text, extract_responses_tool_calls, parse_responses_chat_response,
};
use super::super::response::{
    ResponsesContent, ResponsesOutput, ResponsesOutputKind, ResponsesResponse,
};
use super::super::accumulator::ResponsesEventAccumulator;
use super::super::stream::parse_sse_response;
use crate::providers::traits::NormalizedStopReason;

#[test]
fn extracts_output_text_first() {
    let response = ResponsesResponse::new(vec![], Some("hello".into()));
    assert_eq!(extract_responses_text(&response).as_deref(), Some("hello"));
}

#[test]
fn extracts_nested_output_text() {
    let response = ResponsesResponse::new(
        vec![ResponsesOutput::new(
            ResponsesOutputKind::Other,
            None,
            None,
            None,
            None,
            vec![ResponsesContent::output_text("nested")],
        )],
        None,
    );
    assert_eq!(extract_responses_text(&response).as_deref(), Some("nested"));
}

#[test]
fn extracts_function_calls_from_response_output() {
    let response = ResponsesResponse::new(
        vec![ResponsesOutput::new(
            ResponsesOutputKind::FunctionCall,
            Some("item_1".into()),
            Some("call_1".into()),
            Some("shell".into()),
            Some("{\"command\":\"date\"}".into()),
            vec![],
        )],
        None,
    );

    let calls = extract_responses_tool_calls(&response);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].name, "shell");
    assert_eq!(calls[0].arguments, "{\"command\":\"date\"}");
}

#[test]
fn parse_response_prefers_tool_calls_and_marks_stop_reason() {
    let response = ResponsesResponse::new(
        vec![ResponsesOutput::new(
            ResponsesOutputKind::FunctionCall,
            Some("item_2".into()),
            Some("call_2".into()),
            Some("shell".into()),
            Some("{\"command\":\"uptime\"}".into()),
            vec![],
        )],
        Some("I will check uptime now".into()),
    );

    let parsed = parse_responses_chat_response(response);
    assert_eq!(parsed.tool_calls.len(), 1);
    assert_eq!(parsed.text.as_deref(), Some("I will check uptime now"));
    assert_eq!(parsed.stop_reason, Some(NormalizedStopReason::ToolCall));
    assert_eq!(parsed.raw_stop_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn invalid_tool_arguments_are_normalized_to_empty_object() {
    let response = ResponsesResponse::new(
        vec![ResponsesOutput::new(
            ResponsesOutputKind::FunctionCall,
            None,
            Some("call_bad".into()),
            Some("shell".into()),
            Some("{not json}".into()),
            vec![],
        )],
        None,
    );

    let calls = extract_responses_tool_calls(&response);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments, "{}");
}

#[test]
fn accumulator_reports_partial_output_without_field_access() {
    let mut accumulator = ResponsesEventAccumulator::default();
    assert!(!accumulator.has_partial_output());
    accumulator
        .apply_event(serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": "call_acc",
                "name": "shell",
                "arguments": "{}"
            }
        }))
        .unwrap();
    assert!(accumulator.has_output_items());
    assert!(accumulator.has_partial_output());
}

#[test]
fn parse_sse_response_reads_output_text_delta() {
    let payload = r#"data: {"type":"response.created","response":{"id":"resp_123"}}

data: {"type":"response.output_text.delta","delta":"Hello"}
data: {"type":"response.output_text.delta","delta":" world"}
data: {"type":"response.completed","response":{"output_text":"Hello world"}}
data: [DONE]
"#;

    assert_eq!(
        parse_sse_response(payload)
            .unwrap()
            .and_then(|response| extract_responses_text(&response))
            .as_deref(),
        Some("Hello world")
    );
}

#[test]
fn parse_sse_response_falls_back_to_completed_response() {
    let payload = r#"data: {"type":"response.completed","response":{"output_text":"Done"}}
data: [DONE]
"#;

    assert_eq!(
        parse_sse_response(payload)
            .unwrap()
            .and_then(|response| extract_responses_text(&response))
            .as_deref(),
        Some("Done")
    );
}

#[test]
fn parse_sse_response_reads_function_call_output_item() {
    let payload = r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_sse","name":"shell","arguments":"{\"command\":\"whoami\"}"}}
data: {"type":"response.completed","response":{"output":[]}}
data: [DONE]
"#;

    let response = parse_sse_response(payload)
        .expect("payload should parse")
        .expect("response should exist");
    let calls = extract_responses_tool_calls(&response);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_sse");
    assert_eq!(calls[0].name, "shell");
}

#[test]
fn parse_sse_response_fails_on_malformed_output_item() {
    let payload = r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_bad","name":"shell","arguments":123}}
data: [DONE]
"#;

    let error = parse_sse_response(payload).expect_err("malformed item must fail");
    assert!(error.to_string().contains("malformed OpenAI Codex output item"));
}
