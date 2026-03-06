use super::super::config;
use super::super::request::{CodexTransport, ResponsesInput, ResponsesRequest};
use super::super::{OpenAiCodexProvider, WebsocketRequestError};
use crate::auth::openai_oauth::extract_account_id_from_jwt;
use crate::providers::traits::ChatResponse as ProviderChatResponse;
use serde_json::Value;

impl OpenAiCodexProvider {
    pub(in crate::providers::openai::codex) async fn send_responses_sse_request(
        &self,
        request: &ResponsesRequest,
        bearer_token: &str,
        account_id: Option<&str>,
        access_token: Option<&str>,
        use_gateway_api_key_auth: bool,
    ) -> anyhow::Result<ProviderChatResponse> {
        let mut request_builder = self
            .client
            .post(&self.responses_url)
            .header("Authorization", format!("Bearer {bearer_token}"))
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "pi")
            .header("accept", "text/event-stream")
            .header("Content-Type", "application/json");

        if let Some(account_id) = account_id {
            request_builder = request_builder.header("chatgpt-account-id", account_id);
        }
        if use_gateway_api_key_auth {
            if let Some(access_token) = access_token {
                request_builder = request_builder.header("x-openai-access-token", access_token);
            }
            if let Some(account_id) = account_id {
                request_builder = request_builder.header("x-openai-account-id", account_id);
            }
        }

        let response = request_builder.json(request).send().await?;
        if !response.status().is_success() {
            return Err(crate::providers::api_error("OpenAI Codex", response).await);
        }

        super::super::stream::decode_responses_body(response).await
    }

    pub(in crate::providers::openai::codex) async fn send_responses_request(
        &self,
        input: Vec<ResponsesInput>,
        instructions: String,
        model: &str,
        tools: Option<&[Value]>,
    ) -> anyhow::Result<ProviderChatResponse> {
        let use_gateway_api_key_auth = self.custom_endpoint && self.gateway_api_key.is_some();
        let profile = match self
            .auth
            .get_profile("openai-codex", self.auth_profile_override.as_deref())
            .await
        {
            Ok(profile) => profile,
            Err(error) if use_gateway_api_key_auth => {
                tracing::warn!(
                    error = %error,
                    "failed to load OpenAI Codex profile; continuing with custom endpoint API key mode"
                );
                None
            }
            Err(error) => return Err(error),
        };
        let oauth_access_token = match self
            .auth
            .get_valid_openai_access_token(self.auth_profile_override.as_deref())
            .await
        {
            Ok(token) => token,
            Err(error) if use_gateway_api_key_auth => {
                tracing::warn!(
                    error = %error,
                    "failed to refresh OpenAI token; continuing with custom endpoint API key mode"
                );
                None
            }
            Err(error) => return Err(error),
        };

        let account_id = profile.and_then(|profile| profile.account_id).or_else(|| {
            oauth_access_token
                .as_deref()
                .and_then(extract_account_id_from_jwt)
        });
        let access_token = if use_gateway_api_key_auth {
            oauth_access_token
        } else {
            Some(oauth_access_token.ok_or_else(|| {
                anyhow::anyhow!(
                    "OpenAI Codex auth profile not found. Run `zeroclaw auth login --provider openai-codex`."
                )
            })?)
        };
        let account_id = if use_gateway_api_key_auth {
            account_id
        } else {
            Some(account_id.ok_or_else(|| {
                anyhow::anyhow!(
                    "OpenAI Codex account id not found in auth profile/token. Run `zeroclaw auth login --provider openai-codex` again."
                )
            })?)
        };
        let normalized_model = config::normalize_model_id(model);
        let request = ResponsesRequest::new(
            normalized_model,
            input,
            instructions,
            self.reasoning_effort,
            tools.map(|tool_list| tool_list.to_vec()),
        );

        let bearer_token = if use_gateway_api_key_auth {
            self.gateway_api_key.as_deref().unwrap_or_default()
        } else {
            access_token.as_deref().unwrap_or_default()
        };

        match self.transport {
            CodexTransport::WebSocket => self
                .send_responses_websocket_request(
                    &request,
                    normalized_model,
                    bearer_token,
                    account_id.as_deref(),
                    access_token.as_deref(),
                    use_gateway_api_key_auth,
                )
                .await
                .map_err(Into::into),
            CodexTransport::Sse => {
                self.send_responses_sse_request(
                    &request,
                    bearer_token,
                    account_id.as_deref(),
                    access_token.as_deref(),
                    use_gateway_api_key_auth,
                )
                .await
            }
            CodexTransport::Auto => match self
                .send_responses_websocket_request(
                    &request,
                    normalized_model,
                    bearer_token,
                    account_id.as_deref(),
                    access_token.as_deref(),
                    use_gateway_api_key_auth,
                )
                .await
            {
                Ok(text) => Ok(text),
                Err(WebsocketRequestError::TransportUnavailable(error)) => {
                    tracing::warn!(
                        error = %error,
                        "OpenAI Codex websocket request failed; falling back to SSE"
                    );
                    self.send_responses_sse_request(
                        &request,
                        bearer_token,
                        account_id.as_deref(),
                        access_token.as_deref(),
                        use_gateway_api_key_auth,
                    )
                    .await
                }
                Err(WebsocketRequestError::Stream(error)) => Err(error),
            },
        }
    }
}
