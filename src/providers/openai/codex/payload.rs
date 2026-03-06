use super::DEFAULT_CODEX_INSTRUCTIONS;
use super::config::first_nonempty;
use super::request::{ResponsesInput, ResponsesInputContent, ResponsesRole};
use super::response::{
    ResponsesContentKind, ResponsesOutput, ResponsesResponse,
};
use super::schema::normalize_responses_tool_schema;
use crate::multimodal;
use crate::providers::openai::shared::{
    parse_assistant_tool_calls_payload, parse_tool_result_payload,
};
use crate::providers::traits::{
    ChatMessage, ChatResponse as ProviderChatResponse, NormalizedStopReason,
    ToolCall as ProviderToolCall,
};
use serde_json::Value;

pub(super) fn convert_tool_specs(tools: Option<&[crate::tools::ToolSpec]>) -> Vec<Value> {
    tools
        .unwrap_or_default()
        .iter()
        .map(|tool| {
            let mut parameters = tool.parameters.clone();
            normalize_responses_tool_schema(&mut parameters);
            serde_json::json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": parameters,
            })
        })
        .collect()
}

pub(super) fn build_responses_input(
    messages: &[ChatMessage],
) -> anyhow::Result<(String, Vec<ResponsesInput>)> {
    let mut system_parts: Vec<&str> = Vec::new();
    let mut input = Vec::new();

    for message in messages {
        match message.role.as_str() {
            "system" => system_parts.push(&message.content),
            "user" => input.push(build_user_input(&message.content)),
            "tool" => {
                let Some(tool_text) = parse_tool_result_content(&message.content)? else {
                    let fallback = format!("[tool_result]\n{}", message.content);
                    if fallback.trim().is_empty() {
                        continue;
                    }
                    input.push(ResponsesInput::new(
                        ResponsesRole::User,
                        vec![ResponsesInputContent::input_text(fallback)],
                    ));
                    continue;
                };
                if tool_text.trim().is_empty() {
                    continue;
                }
                input.push(ResponsesInput::new(
                    ResponsesRole::User,
                    vec![ResponsesInputContent::input_text(tool_text)],
                ));
            }
            "assistant" => {
                let assistant_text = parse_assistant_native_content(&message.content)?
                    .unwrap_or_else(|| message.content.clone());
                if assistant_text.trim().is_empty() {
                    continue;
                }
                input.push(ResponsesInput::new(
                    ResponsesRole::Assistant,
                    vec![ResponsesInputContent::output_text(assistant_text)],
                ));
            }
            _ => {}
        }
    }

    let instructions = if system_parts.is_empty() {
        DEFAULT_CODEX_INSTRUCTIONS.to_string()
    } else {
        system_parts.join("\n\n")
    };

    Ok((instructions, input))
}

fn build_user_input(content: &str) -> ResponsesInput {
    let (cleaned_text, image_refs) = multimodal::parse_image_markers(content);
    let mut content_items = Vec::new();

    if !cleaned_text.trim().is_empty() {
        content_items.push(ResponsesInputContent::input_text(cleaned_text));
    }
    for image_ref in image_refs {
        content_items.push(ResponsesInputContent::input_image(image_ref));
    }
    if content_items.is_empty() {
        content_items.push(ResponsesInputContent::input_text(String::new()));
    }

    ResponsesInput::new(ResponsesRole::User, content_items)
}

fn parse_assistant_native_content(raw: &str) -> anyhow::Result<Option<String>> {
    let Some(payload) = parse_assistant_tool_calls_payload(raw)? else {
        return Ok(None);
    };

    Ok(first_nonempty(payload.content()).or_else(|| first_nonempty(payload.reasoning_content())))
}

fn parse_tool_result_content(raw: &str) -> anyhow::Result<Option<String>> {
    let Some(payload) = parse_tool_result_payload(raw)? else {
        return Ok(None);
    };

    let header = payload
        .tool_call_id()
        .map(|id| format!("[tool_result:{id}]"))
        .unwrap_or_else(|| "[tool_result]".to_string());
    let Some(rendered_content) = first_nonempty(payload.content()) else {
        return Ok(None);
    };

    Ok(Some(format!("{header}\n{rendered_content}")))
}

pub(super) fn extract_responses_text(response: &ResponsesResponse) -> Option<String> {
    if let Some(text) = first_nonempty(response.output_text()) {
        return Some(text);
    }

    for item in response.outputs() {
        if let Some(text) = first_output_text(item) {
            return Some(text);
        }
    }

    response
        .outputs()
        .iter()
        .flat_map(ResponsesOutput::content)
        .find_map(|content| first_nonempty(content.text()))
}

pub(super) fn extract_responses_tool_calls(response: &ResponsesResponse) -> Vec<ProviderToolCall> {
    response
        .outputs()
        .iter()
        .filter(|item| item.is_function_call())
        .filter_map(|item| {
            let name = item.call_name()?.to_string();
            let arguments = item.call_arguments().unwrap_or("{}").to_string();
            Some(ProviderToolCall {
                id: item
                    .call_id()
                    .or_else(|| item.item_id())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                arguments: normalize_tool_arguments(arguments, &name),
                name,
            })
        })
        .collect()
}

pub(super) fn parse_responses_chat_response(response: ResponsesResponse) -> ProviderChatResponse {
    let text = extract_responses_text(&response);
    let tool_calls = extract_responses_tool_calls(&response);
    let (stop_reason, raw_stop_reason) = if tool_calls.is_empty() {
        (None, None)
    } else {
        (
            Some(NormalizedStopReason::ToolCall),
            Some("tool_calls".to_string()),
        )
    };

    ProviderChatResponse {
        text,
        tool_calls,
        usage: None,
        reasoning_content: None,
        quota_metadata: None,
        stop_reason,
        raw_stop_reason,
    }
}

fn first_output_text(item: &ResponsesOutput) -> Option<String> {
    item.content().iter().find_map(|content| {
        (content.kind() == ResponsesContentKind::OutputText)
            .then(|| first_nonempty(content.text()))
            .flatten()
    })
}

fn normalize_tool_arguments(arguments: String, call_name: &str) -> String {
    if serde_json::from_str::<Value>(&arguments).is_ok() {
        arguments
    } else {
        tracing::warn!(
            function = %call_name,
            arguments = %arguments,
            "Invalid JSON in OpenAI Codex tool-call arguments, using empty object"
        );
        "{}".to_string()
    }
}
