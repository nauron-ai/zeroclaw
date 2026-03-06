use super::accumulator::ResponsesEventAccumulator;
use super::payload::parse_responses_chat_response;
use super::response::ResponsesResponse;
use crate::providers;
use crate::providers::traits::ChatResponse as ProviderChatResponse;
use serde_json::Value;
use std::borrow::Cow;

pub(super) fn parse_sse_response(body: &str) -> anyhow::Result<Option<ResponsesResponse>> {
    let mut accumulator = ResponsesEventAccumulator::default();
    let mut completed_response = None;

    for chunk in sse_chunks(body) {
        process_sse_chunk(chunk, &mut accumulator, &mut completed_response)?;
    }

    Ok(completed_response.or_else(|| accumulator.fallback_response()))
}

pub(super) async fn decode_responses_body(
    response: reqwest::Response,
) -> anyhow::Result<ProviderChatResponse> {
    let body = response.text().await?;

    if let Some(parsed) = parse_sse_response(&body)? {
        return Ok(parse_responses_chat_response(parsed));
    }

    let body_trimmed = body.trim_start();
    let looks_like_sse = body_trimmed.starts_with("event:") || body_trimmed.starts_with("data:");
    if looks_like_sse {
        return Err(anyhow::anyhow!(
            "No response from OpenAI Codex stream payload: {}",
            providers::sanitize_api_error(&body)
        ));
    }

    let parsed: ResponsesResponse = serde_json::from_str(&body).map_err(|error| {
        anyhow::anyhow!(
            "OpenAI Codex JSON parse failed: {error}. Payload: {}",
            providers::sanitize_api_error(&body)
        )
    })?;
    Ok(parse_responses_chat_response(parsed))
}

fn process_sse_chunk(
    chunk: &str,
    accumulator: &mut ResponsesEventAccumulator,
    completed_response: &mut Option<ResponsesResponse>,
) -> anyhow::Result<()> {
    let Some(data) = extract_chunk_data(chunk) else {
        return Ok(());
    };

    let trimmed = data.payload.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return Ok(());
    }

    if let Ok(event) = serde_json::from_str::<Value>(trimmed) {
        if let Some(response) = accumulator.apply_event(event)? {
            *completed_response = Some(response);
        }
        return Ok(());
    }

    if !data.multiline {
        return Err(stream_parse_error(trimmed));
    }

    let mut parsed_any = false;
    for line in data_lines(chunk) {
        if line.is_empty() || line == "[DONE]" {
            continue;
        }
        let event = serde_json::from_str::<Value>(line).map_err(|_| stream_parse_error(line))?;
        parsed_any = true;
        if let Some(response) = accumulator.apply_event(event)? {
            *completed_response = Some(response);
        }
    }

    if parsed_any { Ok(()) } else { Err(stream_parse_error(trimmed)) }
}

fn stream_parse_error(payload: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "OpenAI Codex stream payload parse failed: {}",
        providers::sanitize_api_error(payload)
    )
}

fn sse_chunks(body: &str) -> impl Iterator<Item = &str> { body.split("\n\n") }

fn data_lines(chunk: &str) -> impl Iterator<Item = &str> {
    chunk.lines().filter_map(|line| line.strip_prefix("data:").map(str::trim))
}

struct SseChunkData<'a> {
    payload: Cow<'a, str>,
    multiline: bool,
}

fn extract_chunk_data(chunk: &str) -> Option<SseChunkData<'_>> {
    let mut first: Option<&str> = None;
    let mut combined: Option<String> = None;
    let mut multiline = false;

    for line in data_lines(chunk) {
        if let Some(joined) = combined.as_mut() {
            joined.push('\n');
            joined.push_str(line);
            multiline = true;
            continue;
        }
        if let Some(initial) = first {
            let mut joined = String::with_capacity(initial.len() + line.len() + 1);
            joined.push_str(initial);
            joined.push('\n');
            joined.push_str(line);
            combined = Some(joined);
            multiline = true;
            continue;
        }
        first = Some(line);
    }

    combined
        .map(|payload| SseChunkData { payload: Cow::Owned(payload), multiline })
        .or_else(|| first.map(|payload| SseChunkData { payload: Cow::Borrowed(payload), multiline }))
}
