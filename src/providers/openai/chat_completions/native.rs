use super::OpenAiProvider;
use super::request::{
    MessageRole, NativeMessage, NativeToolCall, NativeToolSpec,
};
use super::response::{NativeChoice, NativeResponseMessage};
use crate::providers::openai::shared::{
    parse_assistant_tool_calls_payload, parse_tool_result_payload,
};
use crate::providers::traits::{
    ChatMessage, ChatResponse as ProviderChatResponse, NormalizedStopReason,
    ToolCall as ProviderToolCall,
};
use crate::tools::ToolSpec;
use reqwest::Client;

pub(super) fn parse_native_tool_spec(value: serde_json::Value) -> anyhow::Result<NativeToolSpec> {
    serde_json::from_value(value)
        .map_err(|error| anyhow::anyhow!("Invalid OpenAI tool specification: {error}"))
}

impl OpenAiProvider {
    pub(super) fn convert_tools(tools: Option<&[ToolSpec]>) -> Option<Vec<NativeToolSpec>> {
        tools.map(|items| {
            items
                .iter()
                .map(|tool| {
                    NativeToolSpec::new_function(
                        tool.name.clone(),
                        tool.description.clone(),
                        tool.parameters.clone(),
                    )
                })
                .collect()
        })
    }

    pub(super) fn convert_messages(messages: &[ChatMessage]) -> anyhow::Result<Vec<NativeMessage>> {
        messages
            .iter()
            .map(|message| {
                if message.role == "assistant" {
                    if let Some(payload) = parse_assistant_tool_calls_payload(&message.content)? {
                        let (content, reasoning_content, tool_calls) = payload.into_parts();
                        let tool_calls = tool_calls
                            .into_iter()
                            .map(|tool_call| {
                                NativeToolCall::function_call(
                                    Some(tool_call.id),
                                    tool_call.name,
                                    tool_call.arguments,
                                )
                            })
                            .collect();
                        return Ok(NativeMessage::assistant_tool_calls(
                            content,
                            reasoning_content,
                            tool_calls,
                        ));
                    }
                }

                if message.role == "tool" {
                    if let Some(payload) = parse_tool_result_payload(&message.content)? {
                        let (tool_call_id, content) = payload.into_parts();
                        return Ok(NativeMessage::tool_result(content, tool_call_id));
                    }
                }

                Ok(NativeMessage::plain(
                    MessageRole::from_chat_role(&message.role),
                    message.content.clone(),
                ))
            })
            .collect()
    }

    pub(super) fn parse_native_response(choice: NativeChoice) -> ProviderChatResponse {
        let (message, raw_stop_reason) = choice.into_parts();
        let stop_reason = raw_stop_reason
            .as_deref()
            .map(NormalizedStopReason::from_openai_finish_reason);
        let text = message.effective_content();
        let reasoning_content = message.reasoning_content().map(ToString::to_string);
        let tool_calls = native_tool_calls_to_provider(message);

        ProviderChatResponse {
            text,
            tool_calls,
            usage: None,
            reasoning_content,
            quota_metadata: None,
            stop_reason,
            raw_stop_reason,
        }
    }

    pub(super) fn http_client(&self) -> Client {
        crate::config::build_runtime_proxy_client_with_timeouts("provider.openai", 120, 10)
    }
}

fn native_tool_calls_to_provider(message: NativeResponseMessage) -> Vec<ProviderToolCall> {
    message
        .take_tool_calls()
        .into_iter()
        .map(|tool_call| {
            let (id, function) = tool_call.into_parts();
            let (name, arguments) = function.into_parts();
            ProviderToolCall {
                id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                name,
                arguments,
            }
        })
        .collect()
}
