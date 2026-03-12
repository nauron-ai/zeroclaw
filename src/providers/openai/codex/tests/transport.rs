use super::super::{OpenAiCodexProvider, CODEX_BASE_URL_ENV, CODEX_RESPONSES_URL_ENV};
use super::common::{env_lock, EnvGuard};
use crate::providers::traits::Provider;
use crate::providers::ProviderRuntimeOptions;

#[test]
fn websocket_url_uses_ws_scheme_and_model_query() {
    let _env_lock = env_lock();
    let _endpoint_guard = EnvGuard::set(CODEX_RESPONSES_URL_ENV, None);
    let _base_guard = EnvGuard::set(CODEX_BASE_URL_ENV, None);

    let options = ProviderRuntimeOptions::default();
    let provider = OpenAiCodexProvider::new(&options, None).expect("provider should init");
    let ws_url = provider
        .responses_websocket_url("gpt-5.3-codex")
        .expect("websocket URL should be derived");

    assert_eq!(
        ws_url,
        "wss://chatgpt.com/backend-api/codex/responses?model=gpt-5.3-codex"
    );
}

#[test]
fn capabilities_includes_vision() {
    let options = ProviderRuntimeOptions {
        provider_api_url: None,
        provider_transport: None,
        labaclaw_dir: None,
        secrets_encrypt: false,
        auth_profile_override: None,
        reasoning_enabled: None,
        reasoning_level: None,
        custom_provider_api_mode: None,
        custom_provider_auth_header: None,
        max_tokens_override: None,
        model_support_vision: None,
        provider_timeout_secs: None,
    };
    let provider = OpenAiCodexProvider::new(&options, None).expect("provider should initialize");
    let caps = provider.capabilities();

    assert!(caps.native_tool_calling);
    assert!(caps.vision);
}
