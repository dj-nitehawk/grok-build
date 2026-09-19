//! Responses compatibility defaults and URL-specific Codex restrictions.
//! Custom providers retain their explicitly configured capabilities.

use xai_grok_sampler::SamplerConfig;

#[cfg(test)]
use super::CODEX_BASE_URL;

/// cli-chat-proxy or `*.x.ai` HTTPS. Same trusted-route set as
/// [`crate::util::is_trusted_xai_https_url`].
pub(crate) fn is_first_party_responses_host(base_url: &str) -> bool {
    crate::util::is_trusted_cli_chat_proxy_url(base_url)
        || crate::util::is_trusted_xai_https_url(base_url)
}

pub(crate) fn is_codex_backend_url(base_url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(base_url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let host = parsed.host_str().unwrap_or("");
    if host != "chatgpt.com" && host != "www.chatgpt.com" {
        return false;
    }
    parsed.path() == "/backend-api/codex" || parsed.path().starts_with("/backend-api/codex/")
}

pub(crate) fn include_encrypted_reasoning(base_url: &str) -> bool {
    is_first_party_responses_host(base_url)
}

/// Restrict known Codex incompatibilities without overriding custom provider capabilities.
pub(crate) fn apply_extras_policy(config: &mut SamplerConfig) {
    if is_codex_backend_url(&config.base_url) {
        config.include_encrypted_reasoning = false;
        config.supports_backend_search = false;
        config.compactions_remaining = None;
        config.compaction_at_tokens = None;
        config.extra_response_includes.clear();
        config.responses_system_as_instructions = true;
    } else if is_first_party_responses_host(&config.base_url) {
        config.include_encrypted_reasoning = true;
        config.responses_system_as_instructions = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::config::response_include_extensions;
    use crate::sampling::ApiBackend;

    #[test]
    fn extras_gate_proxy_vs_codex() {
        let proxy = crate::env::PROD_CLI_CHAT_PROXY_BASE_URL;
        assert!(is_first_party_responses_host(proxy));
        assert!(include_encrypted_reasoning(proxy));
        assert!(!is_codex_backend_url(proxy));

        assert!(!is_first_party_responses_host(CODEX_BASE_URL));
        assert!(!include_encrypted_reasoning(CODEX_BASE_URL));
        assert!(is_codex_backend_url(CODEX_BASE_URL));
        assert!(is_codex_backend_url(&format!("{CODEX_BASE_URL}/")));

        let mut codex = SamplerConfig {
            base_url: CODEX_BASE_URL.to_string(),
            ..Default::default()
        };
        apply_extras_policy(&mut codex);
        assert!(codex.responses_system_as_instructions);
        assert!(!codex.include_encrypted_reasoning);

        let mut proxy_cfg = SamplerConfig {
            base_url: proxy.to_string(),
            ..Default::default()
        };
        apply_extras_policy(&mut proxy_cfg);
        assert!(!proxy_cfg.responses_system_as_instructions);
        assert!(proxy_cfg.include_encrypted_reasoning);
    }

    #[test]
    fn codex_compatibility_requires_https_and_a_path_boundary() {
        for url in [
            "http://chatgpt.com/backend-api/codex",
            "https://chatgpt.com/backend-api/codex-other",
            "https://chatgpt.com.example.org/backend-api/codex",
            "https://example.org/backend-api/codex",
        ] {
            assert!(!is_codex_backend_url(url), "{url}");
        }
        assert!(is_codex_backend_url(
            "https://www.chatgpt.com/backend-api/codex/responses"
        ));
    }

    #[test]
    fn custom_provider_capabilities_are_preserved() {
        use xai_grok_sampling_types::{CompactionAtTokens, CompactionsRemaining};

        let mut config = SamplerConfig {
            base_url: "https://grok-proxy.acme.com/v1".to_owned(),
            supports_backend_search: true,
            compactions_remaining: Some(CompactionsRemaining::Fixed(1)),
            compaction_at_tokens: Some(CompactionAtTokens::Fixed(100_000)),
            include_encrypted_reasoning: true,
            responses_system_as_instructions: true,
            extra_response_includes: vec!["provider.custom".to_owned()],
            ..Default::default()
        };
        apply_extras_policy(&mut config);
        assert!(config.supports_backend_search);
        assert!(config.compactions_remaining.is_some());
        assert!(config.compaction_at_tokens.is_some());
        assert!(config.include_encrypted_reasoning);
        assert!(config.responses_system_as_instructions);
        assert_eq!(config.extra_response_includes, ["provider.custom"]);
        assert!(
            response_include_extensions(true, &ApiBackend::Responses, &config.base_url).is_empty()
        );

        config.base_url = CODEX_BASE_URL.to_owned();
        apply_extras_policy(&mut config);
        assert!(!config.supports_backend_search);
        assert!(config.compactions_remaining.is_none());
        assert!(config.compaction_at_tokens.is_none());
        assert!(!config.include_encrypted_reasoning);
        assert!(config.responses_system_as_instructions);
        assert!(config.extra_response_includes.is_empty());
    }

    #[test]
    fn response_include_extensions_skips_codex() {
        let proxy = crate::env::PROD_CLI_CHAT_PROXY_BASE_URL;
        let proxy_includes = response_include_extensions(true, &ApiBackend::Responses, proxy);
        assert_eq!(proxy_includes, ["no_inline_citations"]);

        let codex_includes =
            response_include_extensions(true, &ApiBackend::Responses, CODEX_BASE_URL);
        assert!(codex_includes.is_empty());
        assert!(
            !codex_includes
                .iter()
                .any(|v| v == "reasoning.encrypted_content")
        );
    }
}
