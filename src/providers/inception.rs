use crate::providers::compatible::{AuthStyle, CompatibleApiMode, OpenAiCompatibleProvider};
use crate::providers::traits::{
    ChatMessage, ChatRequest, ChatResponse, Provider, ProviderCapabilities, StreamChunk,
    StreamOptions, StreamResult, ToolsPayload,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::stream;

mod dashboard;

pub(crate) use dashboard::dashboard_fields;

pub(crate) const CANONICAL_NAME: &str = "inception";
pub(crate) const DISPLAY_NAME: &str = "Inception Labs";
pub(crate) const DASHBOARD_INTEGRATION_NAME: &str = "Inception";
pub(crate) const ALIASES: &[&str] = &["inceptionlabs"];
pub(crate) const API_KEY_ENV_VAR: &str = "INCEPTION_API_KEY";
pub(crate) const API_KEY_PORTAL_URL: &str = "https://platform.inceptionlabs.ai/";
pub(crate) const BASE_URL: &str = "https://api.inceptionlabs.ai/v1";
pub(crate) const MODELS_URL: &str = "https://api.inceptionlabs.ai/v1/models";
pub(crate) const PROVIDER_PICKER_LABEL: &str = "Inception Labs — Mercury 2 (ultra-low latency)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InceptionModel {
    Mercury2,
}

impl InceptionModel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mercury2 => "mercury-2",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Mercury2 => "Mercury 2",
        }
    }

    pub const fn onboarding_description(self) -> &'static str {
        match self {
            Self::Mercury2 => "ultra-low latency",
        }
    }
}

pub(crate) const DEFAULT_MODEL: InceptionModel = InceptionModel::Mercury2;
pub(crate) const DEFAULT_MODEL_ID: &str = DEFAULT_MODEL.as_str();
pub(crate) const SUPPORTED_MODELS: &[InceptionModel] = &[DEFAULT_MODEL];
pub(crate) const DASHBOARD_MODEL_OPTIONS: &[&str] = &[DEFAULT_MODEL_ID];
pub(crate) const PROVIDER_INFO: super::ProviderInfo = super::ProviderInfo {
    name: CANONICAL_NAME,
    display_name: DISPLAY_NAME,
    aliases: ALIASES,
    local: false,
};

pub(crate) fn is_alias(name: &str) -> bool {
    name == CANONICAL_NAME || ALIASES.iter().any(|alias| *alias == name)
}

pub(crate) fn canonical_name(name: &str) -> Option<&'static str> {
    is_alias(name).then_some(CANONICAL_NAME)
}

pub(crate) fn curated_model_options() -> Vec<(String, String)> {
    SUPPORTED_MODELS
        .iter()
        .map(|model| {
            (
                model.as_str().to_string(),
                format!(
                    "{} ({})",
                    model.display_name(),
                    model.onboarding_description()
                ),
            )
        })
        .collect()
}

pub struct InceptionProvider {
    inner: OpenAiCompatibleProvider,
}

impl InceptionProvider {
    pub fn new(credential: Option<&str>, max_tokens_override: Option<u32>) -> Self {
        Self::with_base_url(BASE_URL, credential, max_tokens_override)
    }

    pub fn with_base_url(
        base_url: &str,
        credential: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new_custom_with_mode(
                DISPLAY_NAME,
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
        assert_eq!(BASE_URL, "https://api.inceptionlabs.ai/v1");
        assert_eq!(MODELS_URL, "https://api.inceptionlabs.ai/v1/models");
    }

    #[test]
    fn inception_provider_defaults_to_mercury_2() {
        assert_eq!(DEFAULT_MODEL_ID, "mercury-2");
        assert_eq!(DEFAULT_MODEL.as_str(), DEFAULT_MODEL_ID);
    }

    #[test]
    fn inception_provider_exposes_canonical_metadata() {
        assert_eq!(canonical_name(CANONICAL_NAME), Some(CANONICAL_NAME));
        assert_eq!(canonical_name("inceptionlabs"), Some(CANONICAL_NAME));
        assert_eq!(API_KEY_ENV_VAR, "INCEPTION_API_KEY");
        assert_eq!(PROVIDER_INFO.name, CANONICAL_NAME);
        assert_eq!(PROVIDER_INFO.display_name, DISPLAY_NAME);
        assert_eq!(PROVIDER_INFO.aliases, ALIASES);
    }

    #[test]
    fn inception_provider_supports_native_tools_but_not_vision() {
        let provider = InceptionProvider::new(Some("test-key"), None);
        assert!(provider.supports_native_tools());
        assert!(!provider.supports_vision());
        assert!(provider.supports_streaming());
    }
}
