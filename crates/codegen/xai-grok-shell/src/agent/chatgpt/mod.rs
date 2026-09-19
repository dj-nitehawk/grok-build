//! ChatGPT Plus/Pro via the unofficial Codex Responses backend.
//!
//! Feature body lives here. Upstream switchboards call
//! [`catalog_entries`], [`inject_synthetic_providers`], and
//! [`stamp_sampler_config`].

pub(crate) mod bearer;
pub(crate) mod catalog;
pub(crate) mod extras;
pub(crate) mod headers;
pub(crate) mod oauth;
pub mod quota;
pub(crate) mod store;

use xai_grok_sampler::{AuthScheme, SamplerConfig};

use crate::agent::config::ModelEntry;
use crate::agent::model_providers::ModelProviderConfig;

pub(crate) const AUTH_PROVIDER_NAME: &str = "chatgpt";
pub(crate) const MODEL_PROVIDER_NAME: &str = "chatgpt";
pub(crate) const MODEL_ID: &str = "gpt-6-astra";
pub(crate) const MODEL_DISPLAY_NAME: &str = "GPT-6 Astra";
pub(crate) const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub(crate) const ORIGINATOR: &str = "grok-build";
pub(crate) const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(crate) const OAUTH_ISSUER: &str = "https://auth.openai.com";
pub(crate) const OAUTH_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub(crate) const OAUTH_CALLBACK_PORT: u16 = 1455;
pub(crate) const CONTEXT_WINDOW: u64 = 272_000;

pub(crate) use catalog::{catalog_entries, merge_catalog};
pub(crate) use extras::is_first_party_responses_host;

/// Insert the synthetic ChatGPT auth and model providers when the user did not define them.
pub(crate) fn inject_synthetic_providers(
    auth_providers: &mut indexmap::IndexMap<String, xai_grok_config_types::AuthProviderConfig>,
    model_providers: &mut indexmap::IndexMap<String, ModelProviderConfig>,
) {
    auth_providers
        .entry(AUTH_PROVIDER_NAME.to_owned())
        .or_insert_with(catalog::synthetic_auth_provider);
    model_providers
        .entry(MODEL_PROVIDER_NAME.to_owned())
        .or_insert_with(catalog::synthetic_model_provider);
}

/// Attach Codex bearer/header policy. No-op unless this config is the Codex backend.
pub(crate) fn stamp_sampler_config(config: &mut SamplerConfig, model: &ModelEntry) {
    extras::apply_extras_policy(config);
    if !is_codex_model(model, &config.base_url) {
        return;
    }
    attach_credentials(config);
}

/// Stamp Codex policy when only the reconstructed `SamplerConfig` is available.
pub(crate) fn stamp_sampler_config_from_url(config: &mut SamplerConfig) {
    extras::apply_extras_policy(config);
    if !extras::is_codex_backend_url(&config.base_url) {
        return;
    }
    attach_credentials(config);
}

fn attach_credentials(config: &mut SamplerConfig) {
    config.auth_scheme = AuthScheme::Bearer;
    if !reqwest::Url::parse(&config.base_url)
        .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some())
    {
        // A resolver is required even when disabled, to prevent fallback to a seed token.
        config.api_key = None;
        config.bearer_resolver = Some(std::sync::Arc::new(bearer::DisabledBearerResolver));
        tracing::warn!("ChatGPT credentials require an HTTPS endpoint");
        return;
    }
    if config
        .header_injector
        .as_ref()
        .is_some_and(|injector| injector.auth_provider_name() == Some(AUTH_PROVIDER_NAME))
        && config
            .bearer_resolver
            .as_ref()
            .is_some_and(|resolver| resolver.auth_provider_name() == Some(AUTH_PROVIDER_NAME))
    {
        return;
    }
    let store = store::SharedStore::new();
    config.bearer_resolver = Some(bearer::ChatGPTBearerResolver::shared(store.clone()));
    if !config
        .header_injector
        .as_ref()
        .is_some_and(|injector| injector.auth_provider_name() == Some(AUTH_PROVIDER_NAME))
    {
        let inner = config.header_injector.take();
        config.header_injector = Some(headers::ChatGPTHeaderInjector::shared(store, inner));
    }
}

fn is_codex_model(model: &ModelEntry, base_url: &str) -> bool {
    extras::is_codex_backend_url(base_url)
        || model
            .auth_provider
            .as_ref()
            .is_some_and(|p| p.name == AUTH_PROVIDER_NAME)
}

/// Browser or device-code ChatGPT login. Writes `chatgpt-auth.json`.
pub async fn run_chatgpt_login(device: bool) -> anyhow::Result<()> {
    let auth = if device {
        oauth::device_login().await?
    } else {
        oauth::browser_login().await?
    };
    store::save_login(&auth).await?;
    println!("signed in as {}", oauth::identity_label(&auth));
    Ok(())
}

/// Delete cached ChatGPT tokens.
pub fn run_chatgpt_logout() -> anyhow::Result<()> {
    store::clear()?;
    println!("signed out of ChatGPT");
    Ok(())
}

/// Print `{access_token, expires_in}` for `[auth_provider.chatgpt]`. Never hangs on a missing store.
pub async fn print_chatgpt_token() -> anyhow::Result<()> {
    let json = token_json().await?;
    println!("{json}");
    Ok(())
}

async fn token_json() -> anyhow::Result<String> {
    serialize_token(&store::fresh().await?)
}

fn serialize_token(auth: &store::ChatgptAuth) -> anyhow::Result<String> {
    let mut output = serde_json::json!({ "access_token": auth.access_token });
    if let Some(expires_in) = auth.expires_in_secs() {
        output["expires_in"] = serde_json::json!(expires_in);
    }
    Ok(serde_json::to_string(&output)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::config::{
        Config, EndpointsConfig, default_model_entries, resolve_credentials, resolve_model_list,
        sampling_config_for_model, to_acp_model_info,
    };
    use crate::sampling::ApiBackend;
    use xai_grok_sampling_types::{
        ReasoningEffort, ReasoningSummary, parse_reasoning_efforts_meta,
        supports_reasoning_effort_meta,
    };
    use xai_grok_test_support::EnvGuard;

    fn default_entries() -> indexmap::IndexMap<String, ModelEntry> {
        default_model_entries(&EndpointsConfig::default())
    }

    #[test]
    fn catalog_adds_only_gpt_6_astra() {
        let entries = default_entries();
        assert!(entries.contains_key(MODEL_ID));
        let gpt_ids: Vec<_> = entries
            .keys()
            .filter(|k| k.starts_with("gpt-"))
            .cloned()
            .collect();
        assert_eq!(gpt_ids, vec![MODEL_ID.to_owned()]);
        let astra = entries.get(MODEL_ID).expect("seeded astra");
        assert_eq!(astra.info.model, MODEL_ID);
        assert_eq!(astra.info.name.as_deref(), Some(MODEL_DISPLAY_NAME));
        assert_eq!(astra.info.model_family.as_deref(), Some("openai"));
        assert_eq!(astra.info.api_backend, ApiBackend::Responses);
        assert_eq!(astra.info.base_url, CODEX_BASE_URL);
        assert_eq!(astra.info.context_window.get(), CONTEXT_WINDOW);
        assert!(!astra.info.supports_backend_search);
        assert_eq!(astra.info.stream_tool_calls, Some(false));
        assert_eq!(astra.info.reasoning_summary, Some(ReasoningSummary::None));
        assert!(astra.info.supports_reasoning_effort);
        assert_eq!(astra.info.reasoning_effort, Some(ReasoningEffort::Medium));
        assert_eq!(
            astra
                .info
                .reasoning_efforts
                .iter()
                .map(|opt| opt.value)
                .collect::<Vec<_>>(),
            vec![
                ReasoningEffort::Xhigh,
                ReasoningEffort::High,
                ReasoningEffort::Medium,
                ReasoningEffort::Low,
            ]
        );
        assert!(astra.has_own_credentials());
        assert!(
            entries.keys().any(|k| k.starts_with("grok")),
            "grok defaults must still be present"
        );
    }

    #[test]
    fn catalog_advertises_astra_reasoning_menu_for_model_picker() {
        let models = resolve_model_list(&Config::default(), None);
        let astra = models.get(MODEL_ID).expect("seeded astra");
        assert!(astra.info.supports_reasoning_effort);
        assert_eq!(astra.info.reasoning_effort, Some(ReasoningEffort::Medium));
        assert!(
            !astra
                .info
                .reasoning_efforts
                .iter()
                .any(|opt| opt.value == ReasoningEffort::None),
            "Astra 400s on none; do not offer it in /model"
        );

        let acp = to_acp_model_info(&models);
        let info = acp
            .get(&agent_client_protocol::ModelId::new(MODEL_ID))
            .expect("acp astra");
        assert!(supports_reasoning_effort_meta(info.meta.as_ref()));
        let options = parse_reasoning_efforts_meta(info.meta.as_ref()).expect("reasoningEfforts");
        assert_eq!(
            options.iter().map(|opt| opt.value).collect::<Vec<_>>(),
            vec![
                ReasoningEffort::Xhigh,
                ReasoningEffort::High,
                ReasoningEffort::Medium,
                ReasoningEffort::Low,
            ]
        );
    }

    #[test]
    fn catalog_survives_remote_prefetch_replace() {
        let cfg = Config::default();
        let grok = default_entries()
            .into_iter()
            .find(|(key, _)| key.starts_with("grok"))
            .expect("grok default");
        let grok_key = grok.0.clone();
        let mut prefetched = indexmap::IndexMap::new();
        prefetched.insert(grok.0, grok.1);
        let resolved = resolve_model_list(&cfg, Some(prefetched));
        assert!(
            resolved.contains_key(MODEL_ID),
            "prefetch must not drop gpt-6-astra"
        );
        assert!(resolved.contains_key(&grok_key));
        let astra = resolved.get(MODEL_ID).unwrap();
        assert_eq!(astra.info.base_url, CODEX_BASE_URL);
        assert!(astra.has_own_credentials());
    }

    #[test]
    fn user_override_wins_on_catalog_seed() {
        let raw: toml::Value = toml::from_str(
            r#"
[model.gpt-6-astra]
name = "Custom Astra"
context_window = 100000
"#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();
        let models = resolve_model_list(&cfg, None);
        let astra = models.get(MODEL_ID).expect("seeded astra");
        assert_eq!(astra.info.name.as_deref(), Some("Custom Astra"));
        assert_eq!(astra.info.context_window.get(), 100_000);
        assert_eq!(astra.info.model_family.as_deref(), Some("openai"));
        assert_eq!(astra.info.base_url, CODEX_BASE_URL);
    }

    #[test]
    fn resolve_credentials_never_uses_grok_session_for_codex() {
        let astra = default_entries().swap_remove(MODEL_ID).unwrap();
        let creds = resolve_credentials(&astra, Some("grok-session-token"));
        assert_ne!(creds.api_key.as_deref(), Some("grok-session-token"));
        assert_eq!(creds.base_url, CODEX_BASE_URL);
        assert_eq!(creds.auth_type, xai_chat_state::AuthType::ApiKey);
    }

    #[test]
    #[serial_test::serial]
    fn repeated_stamp_reuses_wrapper_and_preserves_inner_injector() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", home.path());
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        #[derive(Debug)]
        struct CountingInjector(Arc<AtomicUsize>);
        impl xai_grok_sampler::HeaderInjector for CountingInjector {
            fn inject(&self, headers: &mut reqwest::header::HeaderMap) {
                self.0.fetch_add(1, Ordering::SeqCst);
                headers.insert(
                    "x-test-inner",
                    reqwest::header::HeaderValue::from_static("preserved"),
                );
            }
        }
        let entries = default_entries();
        let astra = entries.get(MODEL_ID).unwrap();
        let mut config = sampling_config_for_model(
            astra,
            resolve_credentials(astra, None),
            None,
            None,
            None,
            None,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        config.header_injector = Some(Arc::new(CountingInjector(calls.clone())));
        stamp_sampler_config(&mut config, astra);
        let wrapper = config.header_injector.clone().unwrap();
        let resolver = config.bearer_resolver.clone().unwrap();
        for _ in 0..3 {
            stamp_sampler_config(&mut config, astra);
            stamp_sampler_config_from_url(&mut config);
        }
        assert!(Arc::ptr_eq(
            &wrapper,
            config.header_injector.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &resolver,
            config.bearer_resolver.as_ref().unwrap()
        ));
        let mut headers = reqwest::header::HeaderMap::new();
        config
            .header_injector
            .as_ref()
            .unwrap()
            .inject(&mut headers);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(headers["x-test-inner"], "preserved");
    }

    #[test]
    #[serial_test::serial]
    fn token_serialization_distinguishes_expired_and_unknown_expiry() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", home.path());
        let parse = |text: String| {
            let output = std::process::Output {
                status: std::process::Command::new("true").status().unwrap(),
                stdout: text.into_bytes(),
                stderr: Vec::new(),
            };
            xai_grok_login::token_output::parse_token_output(&output).unwrap()
        };
        let mut auth = store::ChatgptAuth {
            access_token: "token".into(),
            refresh_token: None,
            expires_at: None,
            account_id: "account".into(),
            id_token: None,
            residency: None,
        };
        assert_eq!(auth.expires_in_secs(), None);
        let unknown = serialize_token(&auth).unwrap();
        let output: serde_json::Value = serde_json::from_str(&unknown).unwrap();
        assert!(output.get("expires_in").is_none());
        let parsed = parse(unknown);
        assert!(parsed.expires_at.is_none());
        auth.expires_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));
        assert_eq!(auth.expires_in_secs(), Some(0));
        let expired = serialize_token(&auth).unwrap();
        let output: serde_json::Value = serde_json::from_str(&expired).unwrap();
        assert_eq!(output["expires_in"], 0);
        let parsed = parse(expired);
        assert!(parsed.expires_at.unwrap() <= chrono::Utc::now());
    }

    #[test]
    #[serial_test::serial]
    fn explicit_chatgpt_proxy_requires_https_and_can_be_restamped() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", home.path());
        let entries = default_entries();
        let astra = entries.get(MODEL_ID).unwrap();
        let mut config = sampling_config_for_model(
            astra,
            resolve_credentials(astra, None),
            None,
            None,
            None,
            None,
        );
        let wrapper = config.header_injector.clone().unwrap();
        config.base_url = "http://proxy.example/codex".into();
        config.api_key = Some("must-not-leak".into());
        stamp_sampler_config(&mut config, astra);
        assert!(config.api_key.is_none());
        assert!(
            config
                .bearer_resolver
                .as_ref()
                .unwrap()
                .current_bearer()
                .is_none()
        );
        assert!(
            config
                .bearer_resolver
                .as_ref()
                .unwrap()
                .auth_provider_name()
                .is_none()
        );
        config.base_url = "https://proxy.example/codex".into();
        stamp_sampler_config(&mut config, astra);
        assert_eq!(
            config
                .bearer_resolver
                .as_ref()
                .unwrap()
                .auth_provider_name(),
            Some(AUTH_PROVIDER_NAME)
        );
        assert!(std::sync::Arc::ptr_eq(
            &wrapper,
            config.header_injector.as_ref().unwrap()
        ));
    }

    #[test]
    #[serial_test::serial]
    fn stamp_attaches_resolver_only_for_codex() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", home.path());
        let entries = default_entries();
        let astra = entries.get(MODEL_ID).unwrap();
        let astra_cfg = sampling_config_for_model(
            astra,
            resolve_credentials(astra, Some("grok-session-token")),
            None,
            None,
            None,
            None,
        );
        assert!(astra_cfg.bearer_resolver.is_some());
        assert!(astra_cfg.header_injector.is_some());
        assert!(!astra_cfg.include_encrypted_reasoning);
        assert!(astra_cfg.responses_system_as_instructions);
        assert_eq!(astra_cfg.auth_scheme, AuthScheme::Bearer);
        assert_eq!(astra_cfg.base_url, CODEX_BASE_URL);
        assert_eq!(astra_cfg.reasoning_effort, Some(ReasoningEffort::Medium));

        let grok = entries
            .values()
            .find(|e| e.info.model.starts_with("grok"))
            .expect("grok default");
        let grok_cfg = sampling_config_for_model(
            grok,
            resolve_credentials(grok, Some("grok-session-token")),
            None,
            None,
            None,
            None,
        );
        assert!(grok_cfg.bearer_resolver.is_none());
        assert!(grok_cfg.header_injector.is_none());
        assert!(grok_cfg.include_encrypted_reasoning);
        assert!(!grok_cfg.responses_system_as_instructions);
    }

    #[test]
    #[serial_test::serial]
    fn chatgpt_token_json_parses_via_login_crate() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", home.path());
        let auth = store::ChatgptAuth {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            account_id: "acct".into(),
            id_token: None,
            residency: None,
        };
        store::save(&auth).unwrap();
        let json = serde_json::json!({
            "access_token": "tok",
            "expires_in": auth.expires_in_secs().unwrap(),
        });
        let stdout = serde_json::to_string(&json).unwrap();
        let output = std::process::Output {
            status: std::process::Command::new("true").status().unwrap(),
            stdout: stdout.into_bytes(),
            stderr: Vec::new(),
        };
        let parsed = xai_grok_login::token_output::parse_token_output(&output).unwrap();
        assert_eq!(parsed.access_token, "tok");
    }

    #[test]
    fn model_family_is_openai_for_lossy_compact_on_switch() {
        let astra = default_entries().swap_remove(MODEL_ID).unwrap();
        assert_eq!(astra.info.model_family.as_deref(), Some("openai"));
        let grok = default_model_entries(&EndpointsConfig::default())
            .into_values()
            .find(|e| e.info.model.starts_with("grok"))
            .unwrap();
        assert_ne!(
            grok.info.model_family.as_deref(),
            astra.info.model_family.as_deref()
        );
    }
}
