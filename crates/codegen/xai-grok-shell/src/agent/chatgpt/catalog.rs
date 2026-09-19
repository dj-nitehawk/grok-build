use std::num::NonZeroU64;

use indexmap::IndexMap;
use xai_grok_config_types::AuthProviderConfig;
use xai_grok_sampler::AuthScheme;
use xai_grok_sampling_types::{
    ApiBackend, ReasoningEffort, ReasoningEffortOption, ReasoningSummary,
};

use crate::agent::config::{ModelEntry, ModelEntryConfig, ModelInfo};
use crate::agent::model_providers::ModelProviderConfig;

use super::{
    AUTH_PROVIDER_NAME, CODEX_BASE_URL, CONTEXT_WINDOW, MODEL_DISPLAY_NAME, MODEL_ID, ORIGINATOR,
    SOL_CONTEXT_WINDOW, SOL_MODEL_DISPLAY_NAME, SOL_MODEL_ID,
};

pub(crate) fn catalog_entries() -> IndexMap<String, ModelEntry> {
    let mut map = IndexMap::new();
    map.insert(
        MODEL_ID.to_owned(),
        catalog_entry(
            MODEL_ID,
            MODEL_DISPLAY_NAME,
            CONTEXT_WINDOW,
            codex_reasoning_efforts(false),
        ),
    );
    map.insert(
        SOL_MODEL_ID.to_owned(),
        catalog_entry(
            SOL_MODEL_ID,
            SOL_MODEL_DISPLAY_NAME,
            SOL_CONTEXT_WINDOW,
            codex_reasoning_efforts(true),
        ),
    );
    map
}

fn catalog_entry(
    model_id: &str,
    display_name: &str,
    context_window: u64,
    reasoning_efforts: Vec<ReasoningEffortOption>,
) -> ModelEntry {
    let mut extra_headers = IndexMap::new();
    extra_headers.insert("originator".to_owned(), ORIGINATOR.to_owned());
    let config = ModelEntryConfig {
        id: Some(model_id.to_owned()),
        model: model_id.to_owned(),
        model_family: Some("openai".to_owned()),
        base_url: CODEX_BASE_URL.to_owned(),
        name: Some(display_name.to_owned()),
        context_window: NonZeroU64::new(context_window)
            .expect("catalog context window is non-zero"),
        api_backend: ApiBackend::Responses,
        auth_scheme: Some(AuthScheme::Bearer),
        extra_headers,
        hidden: false,
        supported_in_api: true,
        supports_backend_search: false,
        supports_reasoning_effort: true,
        reasoning_effort: Some(ReasoningEffort::Medium),
        reasoning_efforts,
        stream_tool_calls: Some(false),
        reasoning_summary: Some(ReasoningSummary::None),
        max_completion_tokens: None,
        ..Default::default()
    };
    ModelEntry {
        info: ModelInfo::from_config(&config),
        mtls_cert_dir: None,
        api_key: None,
        env_key: None,
        auth_provider: Some(xai_grok_login::AuthProviderRef::unresolved(
            AUTH_PROVIDER_NAME.to_owned(),
        )),
        api_base_url: None,
    }
}

/// Codex rejects `none` (HTTP 400). Menu matches `/model` and `/effort`.
/// Astra: xhigh, high, medium (default), low.
/// Sol also advertises `max` (Codex CLI / GPT-5.6). Ultra is subagent mode, not an effort.
fn codex_reasoning_efforts(include_max: bool) -> Vec<ReasoningEffortOption> {
    let mut options = Vec::new();
    if include_max {
        options.push((
            ReasoningEffort::Max,
            "Max Effort",
            "Maximum reasoning for the hardest problems",
            false,
        ));
    }
    options.extend([
        (
            ReasoningEffort::Xhigh,
            "Extra High Effort",
            "Extended reasoning for long agentic work",
            false,
        ),
        (
            ReasoningEffort::High,
            "High Effort",
            "Heavier reasoning for complex implementation",
            false,
        ),
        (
            ReasoningEffort::Medium,
            "Medium Effort",
            "Balanced reasoning (default)",
            true,
        ),
        (
            ReasoningEffort::Low,
            "Low Effort",
            "Faster, lighter reasoning",
            false,
        ),
    ]);
    options
        .into_iter()
        .map(
            |(value, label, description, default)| ReasoningEffortOption {
                id: value.as_ref().to_string(),
                value,
                label: label.to_owned(),
                description: Some(description.to_owned()),
                default,
            },
        )
        .collect()
}

/// Keep Codex seeds after remote prefetch replaces the default map.
/// Existing keys (prefetch or user `[model.*]`) win.
pub(crate) fn merge_catalog(resolved: &mut IndexMap<String, ModelEntry>) {
    for (key, entry) in catalog_entries() {
        resolved.entry(key).or_insert(entry);
    }
}

pub(crate) fn synthetic_auth_provider() -> AuthProviderConfig {
    AuthProviderConfig {
        command: "grok".to_owned(),
        args: Some(vec!["chatgpt-token".to_owned()]),
        token_ttl_secs: Some(3300),
        timeout_secs: Some(30),
        cwd: None,
    }
}

pub(crate) fn synthetic_model_provider() -> ModelProviderConfig {
    let mut extra_headers = IndexMap::new();
    extra_headers.insert("originator".to_owned(), ORIGINATOR.to_owned());
    ModelProviderConfig {
        base_url: Some(CODEX_BASE_URL.to_owned()),
        api_backend: Some(ApiBackend::Responses),
        auth_provider: Some(AUTH_PROVIDER_NAME.to_owned()),
        extra_headers,
        ..Default::default()
    }
}
