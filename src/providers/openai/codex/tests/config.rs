use super::common::{env_lock, EnvGuard};
use super::super::config::{
    build_responses_url, default_zeroclaw_dir, is_default_responses_url, resolve_codex_config,
    resolve_instructions, resolve_reasoning_effort, resolve_responses_url, resolve_transport_mode,
    CodexEnvConfig,
};
use super::super::{
    CodexTransport, OpenAiCodexProvider, ReasoningEffort, CODEX_BASE_URL_ENV,
    CODEX_PROVIDER_TRANSPORT_ENV, CODEX_RESPONSES_URL_ENV, CODEX_TRANSPORT_ENV,
    DEFAULT_CODEX_INSTRUCTIONS, DEFAULT_CODEX_RESPONSES_URL,
};
use crate::providers::ProviderRuntimeOptions;

#[test]
fn default_state_dir_is_non_empty() {
    let path = default_zeroclaw_dir();
    assert!(!path.as_os_str().is_empty());
}

#[test]
fn build_responses_url_appends_suffix_for_base_url() {
    assert_eq!(
        build_responses_url("https://api.tonsof.blue/v1").unwrap(),
        "https://api.tonsof.blue/v1/responses"
    );
}

#[test]
fn build_responses_url_keeps_existing_responses_endpoint() {
    assert_eq!(
        build_responses_url("https://api.tonsof.blue/v1/responses").unwrap(),
        "https://api.tonsof.blue/v1/responses"
    );
}

#[test]
fn resolve_responses_url_prefers_explicit_endpoint_env() {
    let _env_lock = env_lock();
    let _endpoint_guard = EnvGuard::set(
        CODEX_RESPONSES_URL_ENV,
        Some("https://env.example.com/v1/responses"),
    );
    let _base_guard = EnvGuard::set(CODEX_BASE_URL_ENV, Some("https://base.example.com/v1"));

    let options = ProviderRuntimeOptions::default();
    let env = CodexEnvConfig::load().unwrap();
    assert_eq!(
        resolve_responses_url(&options, &env).unwrap(),
        "https://env.example.com/v1/responses"
    );
}

#[test]
fn resolve_responses_url_uses_provider_api_url_override() {
    let _env_lock = env_lock();
    let _endpoint_guard = EnvGuard::set(CODEX_RESPONSES_URL_ENV, None);
    let _base_guard = EnvGuard::set(CODEX_BASE_URL_ENV, None);

    let options = ProviderRuntimeOptions {
        provider_api_url: Some("https://proxy.example.com/v1".to_string()),
        ..ProviderRuntimeOptions::default()
    };
    let env = CodexEnvConfig::load().unwrap();

    assert_eq!(
        resolve_responses_url(&options, &env).unwrap(),
        "https://proxy.example.com/v1/responses"
    );
}

#[test]
fn resolve_transport_mode_defaults_to_auto() {
    let _env_lock = env_lock();
    let _transport_guard = EnvGuard::set(CODEX_TRANSPORT_ENV, None);
    let _provider_guard = EnvGuard::set(CODEX_PROVIDER_TRANSPORT_ENV, None);
    let env = CodexEnvConfig::load().unwrap();

    assert_eq!(
        resolve_transport_mode(&ProviderRuntimeOptions::default(), &env).unwrap(),
        CodexTransport::Auto
    );
}

#[test]
fn resolve_transport_mode_accepts_runtime_override() {
    let _env_lock = env_lock();
    let _transport_guard = EnvGuard::set(CODEX_TRANSPORT_ENV, Some("sse"));

    let options = ProviderRuntimeOptions {
        provider_transport: Some("websocket".to_string()),
        ..ProviderRuntimeOptions::default()
    };
    let env = CodexEnvConfig::load().unwrap();

    assert_eq!(
        resolve_transport_mode(&options, &env).unwrap(),
        CodexTransport::WebSocket
    );
}

#[test]
fn resolve_transport_mode_rejects_invalid_runtime_override() {
    let options = ProviderRuntimeOptions {
        provider_transport: Some("udp".to_string()),
        ..ProviderRuntimeOptions::default()
    };
    let env = CodexEnvConfig::default();

    let err =
        resolve_transport_mode(&options, &env).expect_err("invalid runtime transport must fail");
    assert!(err
        .to_string()
        .contains("Invalid OpenAI Codex transport override 'udp'"));
}

#[test]
fn default_responses_url_detector_handles_equivalent_urls() {
    assert!(is_default_responses_url(DEFAULT_CODEX_RESPONSES_URL));
    assert!(is_default_responses_url(
        "https://chatgpt.com/backend-api/codex/responses/"
    ));
    assert!(!is_default_responses_url(
        "https://api.tonsof.blue/v1/responses"
    ));
}

#[test]
fn constructor_enables_custom_endpoint_key_mode() {
    let _env_lock = env_lock();
    let _endpoint_guard = EnvGuard::set(CODEX_RESPONSES_URL_ENV, None);
    let _base_guard = EnvGuard::set(CODEX_BASE_URL_ENV, None);

    let options = ProviderRuntimeOptions {
        provider_api_url: Some("https://api.tonsof.blue/v1".to_string()),
        ..ProviderRuntimeOptions::default()
    };

    let provider = OpenAiCodexProvider::new(&options, Some("test-key")).unwrap();
    assert!(provider.custom_endpoint);
    assert_eq!(provider.gateway_api_key.as_deref(), Some("test-key"));
}

#[test]
fn resolve_instructions_uses_default_when_missing() {
    assert_eq!(
        resolve_instructions(None),
        DEFAULT_CODEX_INSTRUCTIONS.to_string()
    );
}

#[test]
fn resolve_instructions_uses_default_when_blank() {
    assert_eq!(
        resolve_instructions(Some("   ")),
        DEFAULT_CODEX_INSTRUCTIONS.to_string()
    );
}

#[test]
fn resolve_instructions_uses_system_prompt_when_present() {
    assert_eq!(resolve_instructions(Some("Be strict")), "Be strict".to_string());
}

#[test]
fn resolve_reasoning_effort_prefers_config_override() {
    let _env_lock = env_lock();
    let _reasoning_guard = EnvGuard::set("ZEROCLAW_CODEX_REASONING_EFFORT", Some("low"));
    let env = CodexEnvConfig::load().unwrap();

    assert_eq!(
        resolve_reasoning_effort(Some("xhigh"), &env),
        ReasoningEffort::Xhigh
    );
}

#[test]
fn resolve_reasoning_effort_falls_back_to_env_when_override_invalid() {
    let _env_lock = env_lock();
    let _reasoning_guard = EnvGuard::set("ZEROCLAW_CODEX_REASONING_EFFORT", Some("medium"));
    let env = CodexEnvConfig::load().unwrap();

    assert_eq!(
        resolve_reasoning_effort(Some("banana"), &env),
        ReasoningEffort::Medium
    );
}

#[test]
fn resolve_codex_config_loads_env_once_for_init() {
    let _env_lock = env_lock();
    let _url_guard = EnvGuard::set(CODEX_RESPONSES_URL_ENV, Some("https://env.example.com/v1"));
    let _transport_guard = EnvGuard::set(CODEX_TRANSPORT_ENV, Some("ws"));
    let _reasoning_guard = EnvGuard::set("ZEROCLAW_CODEX_REASONING_EFFORT", Some("high"));

    let resolved = resolve_codex_config(&ProviderRuntimeOptions::default())
        .expect("codex init config should resolve");
    assert_eq!(resolved.responses_url, "https://env.example.com/v1/responses");
    assert_eq!(resolved.transport, CodexTransport::WebSocket);
    assert_eq!(resolved.reasoning_effort, ReasoningEffort::High);
}
