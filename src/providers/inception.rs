//! Native Inception Labs provider.
//!
//! Inception exposes an OpenAI-compatible chat completions API, but we keep a
//! first-class typed provider so the runtime can treat it like any other native
//! provider surface.

use crate::providers::compatible::{AuthStyle, CompatibleApiMode, OpenAiCompatibleProvider};
use crate::providers::traits::{
    ChatMessage, ChatRequest, ChatResponse, Provider, ProviderCapabilities, StreamChunk,
    StreamOptions, StreamResult, ToolsPayload,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::stream;

pub(crate) const INCEPTION_BASE_URL: &str = "https://api.inceptionlabs.ai/v1";
pub(crate) const INCEPTION_MODELS_URL: &str = "https://api.inceptionlabs.ai/v1/models";
pub(crate) const INCEPTION_DEFAULT_MODEL: &str = "mercury-2";

/// Native Inception Labs provider wrapper around the compatible transport.
pub struct InceptionProvider {
    inner: OpenAiCompatibleProvider,
}

impl InceptionProvider {
    pub fn new(credential: Option<&str>, max_tokens_override: Option<u32>) -> Self {
        Self::with_base_url(INCEPTION_BASE_URL, credential, max_tokens_override)
    }

    pub fn with_base_url(
        base_url: &str,
        credential: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new_custom_with_mode(
                "Inception Labs",
                base_url,
                credential,
                AuthStyle::Bearer,
                false,
                CompatibleApiMode::OpenAiChatCompletions,
                max_tokens_override,
            ),
        }
    }
}

#[async_trait]
impl Provider for InceptionProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> ToolsPayload {
        self.inner.convert_tools(tools)
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.inner
            .chat_with_system(system_prompt, message, model, temperature)
            .await
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.inner
            .chat_with_history(messages, model, temperature)
            .await
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        self.inner.chat(request, model, temperature).await
    }

    fn supports_native_tools(&self) -> bool {
        self.inner.supports_native_tools()
    }

    fn supports_vision(&self) -> bool {
        self.inner.supports_vision()
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        self.inner.warmup().await
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        self.inner
            .chat_with_tools(messages, tools, model, temperature)
            .await
    }

    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        self.inner
            .stream_chat_with_system(system_prompt, message, model, temperature, options)
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        self.inner
            .stream_chat_with_history(messages, model, temperature, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inception_provider_uses_official_api_endpoints() {
        assert_eq!(INCEPTION_BASE_URL, "https://api.inceptionlabs.ai/v1");
        assert_eq!(
            INCEPTION_MODELS_URL,
            "https://api.inceptionlabs.ai/v1/models"
        );
    }

    #[test]
    fn inception_provider_defaults_to_mercury_2() {
        assert_eq!(INCEPTION_DEFAULT_MODEL, "mercury-2");
    }

    #[test]
    fn inception_provider_supports_native_tools_but_not_vision() {
        let provider = InceptionProvider::new(Some("test-key"), None);
        assert!(provider.supports_native_tools());
        assert!(!provider.supports_vision());
        assert!(provider.supports_streaming());
    }
}
