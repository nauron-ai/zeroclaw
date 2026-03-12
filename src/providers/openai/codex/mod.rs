use crate::auth::AuthService;
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities,
};
use crate::providers::ProviderRuntimeOptions;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

mod accumulator;
mod config;
mod payload;
mod request;
mod response;
mod schema;
mod stream;
#[cfg(test)]
mod tests;
mod transport;

use payload::{build_responses_input, convert_tool_specs};
use request::{CodexTransport, ReasoningEffort};

const DEFAULT_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const CODEX_RESPONSES_URL_ENV: &str = "LABACLAW_CODEX_RESPONSES_URL";
const CODEX_BASE_URL_ENV: &str = "LABACLAW_CODEX_BASE_URL";
const CODEX_TRANSPORT_ENV: &str = "LABACLAW_CODEX_TRANSPORT";
const CODEX_PROVIDER_TRANSPORT_ENV: &str = "LABACLAW_PROVIDER_TRANSPORT";
const DEFAULT_CODEX_INSTRUCTIONS: &str =
    "You are LabaClaw, a concise and helpful coding assistant.";
const CODEX_WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const CODEX_WS_SEND_TIMEOUT: Duration = Duration::from_secs(15);
const CODEX_WS_READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
enum WebsocketRequestError {
    TransportUnavailable(anyhow::Error),
    Stream(anyhow::Error),
}

impl WebsocketRequestError {
    fn transport_unavailable<E>(error: E) -> Self
    where
        E: Into<anyhow::Error>,
    {
        Self::TransportUnavailable(error.into())
    }

    fn stream<E>(error: E) -> Self
    where
        E: Into<anyhow::Error>,
    {
        Self::Stream(error.into())
    }
}

impl std::fmt::Display for WebsocketRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TransportUnavailable(error) | Self::Stream(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for WebsocketRequestError {}

pub struct OpenAiCodexProvider {
    auth: AuthService,
    auth_profile_override: Option<String>,
    responses_url: String,
    transport: CodexTransport,
    custom_endpoint: bool,
    gateway_api_key: Option<String>,
    reasoning_effort: ReasoningEffort,
    client: Client,
}

impl OpenAiCodexProvider {
    pub fn new(
        options: &ProviderRuntimeOptions,
        gateway_api_key: Option<&str>,
    ) -> anyhow::Result<Self> {
        let state_dir = options
            .labaclaw_dir
            .clone()
            .unwrap_or_else(config::default_labaclaw_dir);
        let auth = AuthService::new(&state_dir, options.secrets_encrypt);
        let resolved = config::resolve_codex_config(options)?;
        let custom_endpoint = !config::is_default_responses_url(&resolved.responses_url);

        Ok(Self {
            auth,
            auth_profile_override: options.auth_profile_override.clone(),
            custom_endpoint,
            responses_url: resolved.responses_url,
            transport: resolved.transport,
            gateway_api_key: gateway_api_key.map(ToString::to_string),
            reasoning_effort: resolved.reasoning_effort,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(
                    options.provider_timeout_secs.unwrap_or(120),
                ))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        })
    }
}

#[async_trait]
impl Provider for OpenAiCodexProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(ChatMessage::system(sys));
        }
        messages.push(ChatMessage::user(message));

        let config = crate::config::MultimodalConfig::default();
        let prepared = crate::multimodal::prepare_messages_for_provider_with_provider_hint(
            &messages,
            &config,
            Some("openai"),
        )
        .await?;

        let (instructions, input) = build_responses_input(&prepared.messages)?;
        let response = self
            .send_responses_request(input, instructions, model, None)
            .await?;
        response
            .text
            .ok_or_else(|| anyhow::anyhow!("No text response from OpenAI Codex"))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        let config = crate::config::MultimodalConfig::default();
        let prepared = crate::multimodal::prepare_messages_for_provider_with_provider_hint(
            messages,
            &config,
            Some("openai"),
        )
        .await?;

        let (instructions, input) = build_responses_input(&prepared.messages)?;
        let response = self
            .send_responses_request(input, instructions, model, None)
            .await?;
        response
            .text
            .ok_or_else(|| anyhow::anyhow!("No text response from OpenAI Codex"))
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
        model: &str,
        _temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let config = crate::config::MultimodalConfig::default();
        let prepared = crate::multimodal::prepare_messages_for_provider_with_provider_hint(
            messages,
            &config,
            Some("openai"),
        )
        .await?;

        let (instructions, input) = build_responses_input(&prepared.messages)?;
        let mut response = self
            .send_responses_request(input, instructions, model, Some(tools))
            .await?;
        if !response.tool_calls.is_empty() {
            response.text = None;
        }
        Ok(response)
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let tools = convert_tool_specs(request.tools);
        if tools.is_empty() {
            let text = self
                .chat_with_history(request.messages, model, temperature)
                .await?;
            return Ok(ProviderChatResponse {
                text: Some(text),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
                quota_metadata: None,
                stop_reason: None,
                raw_stop_reason: None,
            });
        }

        self.chat_with_tools(request.messages, &tools, model, temperature)
            .await
    }
}
