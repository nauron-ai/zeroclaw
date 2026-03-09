use super::request::{CodexTransport, ReasoningEffort};
use super::{
    CODEX_BASE_URL_ENV, CODEX_PROVIDER_TRANSPORT_ENV, CODEX_RESPONSES_URL_ENV, CODEX_TRANSPORT_ENV,
    DEFAULT_CODEX_INSTRUCTIONS, DEFAULT_CODEX_RESPONSES_URL,
};
use crate::providers::ProviderRuntimeOptions;
use std::path::PathBuf;

const CODEX_REASONING_EFFORT_ENV: &str = "ZEROCLAW_CODEX_REASONING_EFFORT";

fn env_nonempty(var_name: &str) -> Option<String> {
    std::env::var(var_name)
        .ok()
        .and_then(|value| first_nonempty(Some(value.as_str())))
}

#[derive(Debug, Clone, Default)]
pub(super) struct CodexEnvConfig {
    responses_url: Option<String>,
    base_url: Option<String>,
    transport: Option<CodexTransport>,
    provider_transport: Option<CodexTransport>,
    reasoning_effort: Option<ReasoningEffort>,
}

impl CodexEnvConfig {
    pub(super) fn load() -> anyhow::Result<Self> {
        Ok(Self {
            responses_url: env_nonempty(CODEX_RESPONSES_URL_ENV),
            base_url: env_nonempty(CODEX_BASE_URL_ENV),
            transport: load_transport_override(CODEX_TRANSPORT_ENV)?,
            provider_transport: load_transport_override(CODEX_PROVIDER_TRANSPORT_ENV)?,
            reasoning_effort: load_reasoning_effort_override(CODEX_REASONING_EFFORT_ENV),
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct CodexResolvedConfig {
    pub(super) responses_url: String,
    pub(super) transport: CodexTransport,
    pub(super) reasoning_effort: ReasoningEffort,
}

pub(super) fn resolve_codex_config(
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<CodexResolvedConfig> {
    let env = CodexEnvConfig::load()?;
    Ok(CodexResolvedConfig {
        responses_url: resolve_responses_url(options, &env)?,
        transport: resolve_transport_mode(options, &env)?,
        reasoning_effort: resolve_reasoning_effort(options.reasoning_level.as_deref(), &env),
    })
}

pub(super) fn default_zeroclaw_dir() -> PathBuf {
    directories::UserDirs::new().map_or_else(
        || PathBuf::from(".zeroclaw"),
        |dirs| dirs.home_dir().join(".zeroclaw"),
    )
}

pub(super) fn build_responses_url(base_or_endpoint: &str) -> anyhow::Result<String> {
    let candidate = base_or_endpoint.trim();
    if candidate.is_empty() {
        anyhow::bail!("OpenAI Codex endpoint override cannot be empty");
    }

    let mut parsed = reqwest::Url::parse(candidate)
        .map_err(|_| anyhow::anyhow!("OpenAI Codex endpoint override must be a valid URL"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => anyhow::bail!("OpenAI Codex endpoint override must use http:// or https://"),
    }

    let path = parsed.path().trim_end_matches('/');
    if !path.ends_with("/responses") {
        let with_suffix = if path.is_empty() || path == "/" {
            "/responses".to_string()
        } else {
            format!("{path}/responses")
        };
        parsed.set_path(&with_suffix);
    }

    parsed.set_query(None);
    parsed.set_fragment(None);
    Ok(parsed.to_string())
}

pub(super) fn resolve_responses_url(
    options: &ProviderRuntimeOptions,
    env: &CodexEnvConfig,
) -> anyhow::Result<String> {
    [
        env.responses_url.as_deref(),
        env.base_url.as_deref(),
        options.provider_api_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| first_nonempty(Some(value)))
    .map_or_else(
        || Ok(DEFAULT_CODEX_RESPONSES_URL.to_string()),
        |override_value| build_responses_url(&override_value),
    )
}

pub(super) fn canonical_endpoint(url: &str) -> Option<(String, String, u16, String)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let port = parsed.port_or_known_default()?;
    let path = parsed.path().trim_end_matches('/').to_string();
    Some((parsed.scheme().to_ascii_lowercase(), host, port, path))
}

pub(super) fn is_default_responses_url(url: &str) -> bool {
    canonical_endpoint(url) == canonical_endpoint(DEFAULT_CODEX_RESPONSES_URL)
}

pub(super) fn first_nonempty(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn parse_transport_override(
    raw: Option<&str>,
    source: &str,
) -> anyhow::Result<Option<CodexTransport>> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<CodexTransport>().map_err(|error| {
                anyhow::anyhow!(
                    "Invalid OpenAI Codex transport override '{value}' from {source}; {error}"
                )
            })
        })
        .transpose()
}

pub(super) fn resolve_transport_mode(
    options: &ProviderRuntimeOptions,
    env: &CodexEnvConfig,
) -> anyhow::Result<CodexTransport> {
    if let Some(mode) = parse_transport_override(
        options.provider_transport.as_deref(),
        "provider.transport runtime override",
    )? {
        return Ok(mode);
    }

    Ok([env.transport, env.provider_transport]
        .into_iter()
        .flatten()
        .next()
        .unwrap_or(CodexTransport::Auto))
}

pub(super) fn resolve_instructions(system_prompt: Option<&str>) -> String {
    first_nonempty(system_prompt).unwrap_or_else(|| DEFAULT_CODEX_INSTRUCTIONS.to_string())
}

pub(super) fn normalize_model_id(model: &str) -> &str {
    model.rsplit('/').next().unwrap_or(model)
}

fn parse_reasoning_effort(raw: &str, source: &str) -> Option<ReasoningEffort> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }

    match value.parse::<ReasoningEffort>() {
        Ok(effort) => Some(effort),
        Err(error) => {
            tracing::warn!(
                reasoning_level = %value,
                source,
                %error,
                "Ignoring invalid reasoning level override"
            );
            None
        }
    }
}

fn load_transport_override(var_name: &str) -> anyhow::Result<Option<CodexTransport>> {
    parse_transport_override(env_nonempty(var_name).as_deref(), var_name)
}

fn load_reasoning_effort_override(var_name: &str) -> Option<ReasoningEffort> {
    env_nonempty(var_name).and_then(|value| parse_reasoning_effort(&value, var_name))
}

pub(super) fn resolve_reasoning_effort(
    override_level: Option<&str>,
    env: &CodexEnvConfig,
) -> ReasoningEffort {
    override_level
        .and_then(|value| parse_reasoning_effort(value, "provider.reasoning_level"))
        .or(env.reasoning_effort)
        .unwrap_or(ReasoningEffort::High)
}

pub(super) fn nonempty_preserve(text: Option<&str>) -> Option<String> {
    text.filter(|value| !value.is_empty())
        .map(ToString::to_string)
}
