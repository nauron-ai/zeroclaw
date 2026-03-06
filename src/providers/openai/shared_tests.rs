use super::shared::{parse_assistant_tool_calls_payload, parse_tool_result_payload};

#[test]
fn parses_assistant_payload_with_tool_calls() {
    let payload = parse_assistant_tool_calls_payload(
        r#"{"content":"checking","reasoning_content":"r","tool_calls":[{"id":"t1","name":"shell","arguments":"{}"}]}"#,
    )
    .expect("payload should parse")
    .expect("structured payload should be detected");

    assert_eq!(payload.content(), Some("checking"));
    assert_eq!(payload.reasoning_content(), Some("r"));
    assert_eq!(payload.tool_calls().len(), 1);
    assert_eq!(payload.tool_calls()[0].id, "t1");
}

#[test]
fn parses_tool_result_with_alias_id_field() {
    let payload = parse_tool_result_payload(r#"{"toolUseId":"abc","content":"done"}"#)
        .expect("payload should parse")
        .expect("structured payload should be detected");

    assert_eq!(payload.tool_call_id(), Some("abc"));
    assert_eq!(payload.content(), Some("done"));
}

#[test]
fn plain_text_is_not_treated_as_structured_payload() {
    assert!(parse_assistant_tool_calls_payload("run uptime")
        .expect("plain text must not fail")
        .is_none());
    assert!(parse_tool_result_payload("tool finished")
        .expect("plain text must not fail")
        .is_none());
}

#[test]
fn malformed_structured_payload_returns_error() {
    let error = parse_assistant_tool_calls_payload(r#"{"tool_calls":123}"#)
        .expect_err("malformed structured payload must fail");
    assert!(error.to_string().contains("invalid shape"));
}

#[test]
fn invalid_field_type_returns_error() {
    let error = parse_tool_result_payload(r#"{"tool_call_id":123,"content":"done"}"#)
        .expect_err("invalid field type must fail");
    assert!(error.to_string().contains("invalid field type"));
}

#[test]
fn module_stays_under_250_loc_budget() {
    let loc = include_str!("shared.rs").lines().count();
    assert!(loc <= 250, "openai/shared.rs exceeded 250 LOC budget: {loc}");
}
