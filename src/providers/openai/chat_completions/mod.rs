use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, TokenUsage,
};
use async_trait::async_trait;

mod native;
mod request;
mod response;
#[cfg(test)]
mod tests;

use native::parse_native_tool_spec;
use request::{ChatRequest, Message, NativeChatRequest};
use response::{ChatResponse, NativeChatResponse};

pub struct OpenAiProvider {
    base_url: String,
    credential: Option<String>,
    max_tokens_override: Option<u32>,
}

impl OpenAiProvider {
    pub fn new(credential: Option<&str>) -> Self {
        Self::with_base_url_and_max_tokens(None, credential, None)
    }

    pub fn with_base_url(base_url: Option<&str>, credential: Option<&str>) -> Self {
        Self::with_base_url_and_max_tokens(base_url, credential, None)
    }

    pub fn with_base_url_and_max_tokens(
        base_url: Option<&str>,
        credential: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Self {
        Self {
            base_url: base_url
                .map(|url| url.trim_end_matches('/').to_string())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            credential: credential.map(ToString::to_string),
            max_tokens_override: max_tokens_override.filter(|value| *value > 0),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: false,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(Message::system(sys));
        }
        messages.push(Message::user(message));

        let request = ChatRequest::new(model, messages, temperature, self.max_tokens_override);
        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::providers::api_error("OpenAI", response).await);
        }

        let chat_response: ChatResponse = response.json().await?;
        chat_response
            .into_first_choice()
            .map(|choice| choice.into_parts().0.effective_content())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let tools = Self::convert_tools(request.tools);
        let native_request = NativeChatRequest::new(
            model,
            Self::convert_messages(request.messages)?,
            temperature,
            self.max_tokens_override,
            tools,
        );

        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::providers::api_error("OpenAI", response).await);
        }

        let quota_extractor = crate::providers::quota_adapter::UniversalQuotaExtractor::new();
        let quota_metadata = quota_extractor.extract("openai", response.headers(), None);

        let native_response: NativeChatResponse = response.json().await?;
        let (choice, usage) = native_response
            .into_first_choice_and_usage()
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))?;
        let mut result = Self::parse_native_response(choice);
        result.usage = usage.map(|usage| TokenUsage {
            input_tokens: usage.prompt_tokens(),
            output_tokens: usage.completion_tokens(),
        });
        result.quota_metadata = quota_metadata;
        Ok(result)
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let native_tools = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .cloned()
                    .map(parse_native_tool_spec)
                    .collect::<Result<Vec<_>, _>>()?,
            )
        };

        let native_request = NativeChatRequest::new(
            model,
            Self::convert_messages(messages)?,
            temperature,
            self.max_tokens_override,
            native_tools,
        );

        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::providers::api_error("OpenAI", response).await);
        }

        let quota_extractor = crate::providers::quota_adapter::UniversalQuotaExtractor::new();
        let quota_metadata = quota_extractor.extract("openai", response.headers(), None);

        let native_response: NativeChatResponse = response.json().await?;
        let (choice, usage) = native_response
            .into_first_choice_and_usage()
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))?;
        let mut result = Self::parse_native_response(choice);
        result.usage = usage.map(|usage| TokenUsage {
            input_tokens: usage.prompt_tokens(),
            output_tokens: usage.completion_tokens(),
        });
        result.quota_metadata = quota_metadata;
        Ok(result)
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        if let Some(credential) = self.credential.as_ref() {
            self.http_client()
                .get(format!("{}/models", self.base_url))
                .header("Authorization", format!("Bearer {credential}"))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }
}
