use super::config::{first_nonempty, nonempty_preserve};
use super::payload::extract_responses_text;
use super::response::{ResponsesOutput, ResponsesResponse, StreamEventType};
use serde_json::Value;

#[derive(Debug, Default)]
pub(super) struct ResponsesEventAccumulator {
    saw_delta: bool,
    delta_accumulator: String,
    fallback_text: Option<String>,
    output_items: Vec<ResponsesOutput>,
}

impl ResponsesEventAccumulator {
    pub(super) fn final_text(&self) -> Option<String> {
        if self.saw_delta {
            nonempty_preserve(Some(&self.delta_accumulator))
        } else {
            self.fallback_text.clone()
        }
    }

    pub(super) fn has_output_items(&self) -> bool { !self.output_items.is_empty() }
    pub(super) fn has_partial_output(&self) -> bool { self.final_text().is_some() || self.has_output_items() }

    pub(super) fn fallback_response(&self) -> Option<ResponsesResponse> {
        let output_text = self.final_text();
        if output_text.is_none() && self.output_items.is_empty() {
            return None;
        }
        Some(ResponsesResponse::new(self.output_items.clone(), output_text))
    }

    pub(super) fn apply_event(&mut self, event: Value) -> anyhow::Result<Option<ResponsesResponse>> {
        if let Some(message) = extract_stream_error_message(&event) {
            anyhow::bail!("OpenAI Codex stream error: {message}");
        }

        self.record_output_item(&event)?;
        self.record_text(&event);

        match StreamEventType::from_event(&event) {
            StreamEventType::Completed | StreamEventType::Done => self.completed_response(event),
            _ => Ok(None),
        }
    }

    fn record_output_item(&mut self, event: &Value) -> anyhow::Result<()> {
        if StreamEventType::from_event(event) != StreamEventType::OutputItemDone {
            return Ok(());
        }

        let item = event
            .get("item")
            .or_else(|| event.get("output_item"))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing output item in OpenAI Codex stream event"))?;
        let parsed = serde_json::from_value::<ResponsesOutput>(item).map_err(|error| {
            anyhow::anyhow!("malformed OpenAI Codex output item in stream payload: {error}")
        })?;
        self.output_items.push(parsed);
        Ok(())
    }

    fn record_text(&mut self, event: &Value) {
        if let Some(text) = extract_stream_event_text(event, self.saw_delta) {
            if StreamEventType::from_event(event) == StreamEventType::OutputTextDelta {
                self.saw_delta = true;
                self.delta_accumulator.push_str(&text);
            } else if self.fallback_text.is_none() {
                self.fallback_text = Some(text);
            }
        }
    }

    fn completed_response(&self, event: Value) -> anyhow::Result<Option<ResponsesResponse>> {
        let Some(value) = event.get("response").cloned() else {
            return Ok(self.fallback_response());
        };

        let parsed = serde_json::from_value::<ResponsesResponse>(value).map_err(|error| {
            anyhow::anyhow!("malformed OpenAI Codex completed response payload: {error}")
        })?;
        let (output, output_text) = parsed.into_parts();
        let output = if output.is_empty() && self.has_output_items() {
            self.output_items.clone()
        } else {
            output
        };
        let output_text = output_text.or_else(|| self.final_text());
        Ok(Some(ResponsesResponse::new(output, output_text)))
    }
}

fn extract_stream_event_text(event: &Value, saw_delta: bool) -> Option<String> {
    match StreamEventType::from_event(event) {
        StreamEventType::OutputTextDelta => {
            nonempty_preserve(event.get("delta").and_then(Value::as_str))
        }
        StreamEventType::OutputTextDone if !saw_delta => {
            nonempty_preserve(event.get("text").and_then(Value::as_str))
        }
        StreamEventType::Completed | StreamEventType::Done => event
            .get("response")
            .and_then(|value| serde_json::from_value::<ResponsesResponse>(value.clone()).ok())
            .and_then(|response| extract_responses_text(&response)),
        _ => None,
    }
}

fn extract_stream_error_message(event: &Value) -> Option<String> {
    match StreamEventType::from_event(event) {
        StreamEventType::Error => first_nonempty(
            event
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| event.get("code").and_then(Value::as_str))
                .or_else(|| {
                    event
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                }),
        ),
        StreamEventType::Failed => first_nonempty(
            event
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str),
        ),
        _ => None,
    }
}
