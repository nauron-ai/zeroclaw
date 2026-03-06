use super::super::accumulator::ResponsesEventAccumulator;
use super::super::request::{ResponsesCreateEvent, ResponsesRequest};
use super::super::{
    CODEX_WS_CONNECT_TIMEOUT, CODEX_WS_READ_TIMEOUT, CODEX_WS_SEND_TIMEOUT,
    OpenAiCodexProvider, WebsocketRequestError,
};
use super::super::payload::parse_responses_chat_response;
use crate::providers::traits::ChatResponse as ProviderChatResponse;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{
            header::{AUTHORIZATION, USER_AGENT},
            HeaderValue as WsHeaderValue,
        },
        Message as WsMessage,
    },
};

impl OpenAiCodexProvider {
    pub(in crate::providers::openai::codex) fn responses_websocket_url(
        &self,
        model: &str,
    ) -> anyhow::Result<String> {
        let mut url = reqwest::Url::parse(&self.responses_url)?;
        let next_scheme = match url.scheme() {
            "https" | "wss" => "wss",
            "http" | "ws" => "ws",
            other => {
                anyhow::bail!(
                    "OpenAI Codex websocket transport does not support URL scheme: {}",
                    other
                );
            }
        };
        url.set_scheme(next_scheme)
            .map_err(|()| anyhow::anyhow!("failed to set websocket URL scheme"))?;
        if !url.query_pairs().any(|(key, _)| key == "model") {
            url.query_pairs_mut().append_pair("model", model);
        }
        Ok(url.into())
    }

    pub(in crate::providers::openai::codex) fn apply_auth_headers_ws(
        &self,
        request: &mut tokio_tungstenite::tungstenite::http::Request<()>,
        bearer_token: &str,
        account_id: Option<&str>,
        access_token: Option<&str>,
        use_gateway_api_key_auth: bool,
    ) -> anyhow::Result<()> {
        let headers = request.headers_mut();
        headers.insert(
            AUTHORIZATION,
            WsHeaderValue::from_str(&format!("Bearer {bearer_token}"))?,
        );
        headers.insert(
            "OpenAI-Beta",
            WsHeaderValue::from_static("responses=experimental"),
        );
        headers.insert("originator", WsHeaderValue::from_static("pi"));
        headers.insert("accept", WsHeaderValue::from_static("text/event-stream"));
        headers.insert(USER_AGENT, WsHeaderValue::from_static("zeroclaw"));

        if let Some(account_id) = account_id {
            headers.insert("chatgpt-account-id", WsHeaderValue::from_str(account_id)?);
        }
        if use_gateway_api_key_auth {
            if let Some(access_token) = access_token {
                headers.insert(
                    "x-openai-access-token",
                    WsHeaderValue::from_str(access_token)?,
                );
            }
            if let Some(account_id) = account_id {
                headers.insert("x-openai-account-id", WsHeaderValue::from_str(account_id)?);
            }
        }

        Ok(())
    }

    pub(in crate::providers::openai::codex) async fn send_responses_websocket_request(
        &self,
        request: &ResponsesRequest,
        model: &str,
        bearer_token: &str,
        account_id: Option<&str>,
        access_token: Option<&str>,
        use_gateway_api_key_auth: bool,
    ) -> Result<ProviderChatResponse, WebsocketRequestError> {
        let ws_url = self
            .responses_websocket_url(model)
            .map_err(WebsocketRequestError::transport_unavailable)?;
        let mut ws_request = ws_url.into_client_request().map_err(|error| {
            WebsocketRequestError::transport_unavailable(anyhow::anyhow!(
                "invalid websocket request URL: {error}"
            ))
        })?;
        self.apply_auth_headers_ws(
            &mut ws_request,
            bearer_token,
            account_id,
            access_token,
            use_gateway_api_key_auth,
        )
        .map_err(WebsocketRequestError::transport_unavailable)?;
        let payload = ResponsesCreateEvent::new(request);

        let (mut ws_stream, _) = timeout(CODEX_WS_CONNECT_TIMEOUT, connect_async(ws_request))
            .await
            .map_err(|_| {
                WebsocketRequestError::transport_unavailable(anyhow::anyhow!(
                    "OpenAI Codex websocket connect timed out after {}s",
                    CODEX_WS_CONNECT_TIMEOUT.as_secs()
                ))
            })?
            .map_err(WebsocketRequestError::transport_unavailable)?;
        timeout(
            CODEX_WS_SEND_TIMEOUT,
            ws_stream.send(WsMessage::Text(
                serde_json::to_string(&payload)
                    .map_err(WebsocketRequestError::transport_unavailable)?
                    .into(),
            )),
        )
        .await
        .map_err(|_| {
            WebsocketRequestError::transport_unavailable(anyhow::anyhow!(
                "OpenAI Codex websocket send timed out after {}s",
                CODEX_WS_SEND_TIMEOUT.as_secs()
            ))
        })?
        .map_err(WebsocketRequestError::transport_unavailable)?;

        let mut accumulator = ResponsesEventAccumulator::default();
        let mut timed_out = false;

        loop {
            let frame = match timeout(CODEX_WS_READ_TIMEOUT, ws_stream.next()).await {
                Ok(frame) => frame,
                Err(_) => {
                    let _ = ws_stream.close(None).await;
                    if accumulator.has_partial_output() {
                        timed_out = true;
                        break;
                    }
                    return Err(WebsocketRequestError::stream(anyhow::anyhow!(
                        "OpenAI Codex websocket stream timed out after {}s waiting for events",
                        CODEX_WS_READ_TIMEOUT.as_secs()
                    )));
                }
            };

            let Some(frame) = frame else {
                break;
            };
            let frame = frame.map_err(WebsocketRequestError::stream)?;
            let event: Value = match frame {
                WsMessage::Text(text) => {
                    serde_json::from_str(text.as_ref()).map_err(WebsocketRequestError::stream)?
                }
                WsMessage::Binary(binary) => {
                    let text = String::from_utf8(binary.to_vec()).map_err(|error| {
                        WebsocketRequestError::stream(anyhow::anyhow!(
                            "invalid UTF-8 websocket frame from OpenAI Codex: {error}"
                        ))
                    })?;
                    serde_json::from_str(&text).map_err(WebsocketRequestError::stream)?
                }
                WsMessage::Ping(payload) => {
                    ws_stream
                        .send(WsMessage::Pong(payload))
                        .await
                        .map_err(WebsocketRequestError::stream)?;
                    continue;
                }
                WsMessage::Close(_) => break,
                _ => continue,
            };

            if let Some(response) = accumulator
                .apply_event(event)
                .map_err(WebsocketRequestError::stream)?
            {
                let _ = ws_stream.close(None).await;
                return Ok(parse_responses_chat_response(response));
            }
        }

        if let Some(response) = accumulator.fallback_response() {
            return Ok(parse_responses_chat_response(response));
        }
        if timed_out {
            return Err(WebsocketRequestError::stream(anyhow::anyhow!(
                "No response from OpenAI Codex websocket stream before timeout"
            )));
        }

        Err(WebsocketRequestError::stream(anyhow::anyhow!(
            "No response from OpenAI Codex websocket stream"
        )))
    }
}
