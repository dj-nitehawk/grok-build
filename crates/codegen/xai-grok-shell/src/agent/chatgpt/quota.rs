//! ChatGPT subscription quota, separate from Grok billing and payment types.
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::store::ChatgptAuth;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TIMEOUT: Duration = Duration::from_secs(15);
pub const PROVIDER_META_KEY: &str = "quotaProvider";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Quota {
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimit {
    pub primary_window: Option<Window>,
    pub secondary_window: Option<Window>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Window {
    pub used_percent: Option<f64>,
    pub limit_window_seconds: Option<i64>,
    pub reset_at: Option<i64>,
    pub reset_after_seconds: Option<i64>,
}

/// Only non-secret routing metadata crosses the ACP boundary.
pub(crate) fn stamp_provider(
    model: &crate::agent::config::ModelEntry,
    meta: &mut serde_json::Map<String, serde_json::Value>,
) {
    let provider = if super::is_codex_model(model, &model.info.base_url) {
        "chatgpt"
    } else if crate::util::is_cli_chat_proxy_url(&model.info.base_url)
        || crate::util::is_xai_api_url(&model.info.base_url)
    {
        "grok"
    } else {
        "unsupported"
    };
    meta.insert(PROVIDER_META_KEY.into(), provider.into());
}

/// Account identity only, never a bearer token. Missing credentials invalidate caches.
pub fn identity() -> Option<String> {
    super::store::load().ok().map(|auth| auth.account_id)
}

pub async fn fetch(expected_identity: Option<&str>) -> Result<Quota, String> {
    tokio::time::timeout(TIMEOUT, async {
        if identity().as_deref() != expected_identity {
            return Err("ChatGPT authentication changed; retry quota refresh".into());
        }
        let fresh = super::store::fresh()
            .await
            .map_err(|_| "ChatGPT credential refresh failed; run grok chatgpt-login".to_owned())?;
        if Some(fresh.account_id.as_str()) != expected_identity {
            return Err("ChatGPT authentication changed; retry quota refresh".into());
        }
        let quota =
            fetch_with_auth(&crate::http::shared_client(), USAGE_URL, &fresh, TIMEOUT).await?;
        if identity().as_deref() != expected_identity {
            return Err("ChatGPT authentication changed; retry quota refresh".into());
        }
        Ok(quota)
    })
    .await
    .map_err(|_| "ChatGPT quota request timed out".to_owned())?
}

async fn fetch_with_auth(
    client: &reqwest::Client,
    url: &str,
    auth: &ChatgptAuth,
    timeout: Duration,
) -> Result<Quota, String> {
    if auth.access_token.is_empty()
        || auth.account_id.is_empty()
        || auth.expires_at.is_some_and(|at| at <= chrono::Utc::now())
    {
        return Err("ChatGPT credentials expired or missing; run grok chatgpt-login".into());
    }
    let mut request = client
        .get(url)
        .bearer_auth(&auth.access_token)
        .header(super::headers::ACCOUNT_ID_HEADER, &auth.account_id)
        .header(super::headers::ORIGINATOR_HEADER, super::ORIGINATOR)
        .timeout(timeout);
    if let Some(residency) = &auth.residency {
        request = request.header(super::headers::RESIDENCY_HEADER, residency);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "ChatGPT quota request failed or timed out".to_owned())?;
    if !response.status().is_success() {
        return Err(format!("ChatGPT quota HTTP {}", response.status().as_u16()));
    }
    let mut quota: Quota = response
        .json()
        .await
        .map_err(|_| "Invalid ChatGPT quota response".to_owned())?;
    let now = chrono::Utc::now().timestamp();
    if let Some(limits) = &mut quota.rate_limit {
        for window in [&mut limits.primary_window, &mut limits.secondary_window]
            .into_iter()
            .flatten()
        {
            if window.reset_at.is_none() {
                window.reset_at = window
                    .reset_after_seconds
                    .and_then(|seconds| now.checked_add(seconds.max(0)));
            }
        }
    }
    Ok(quota)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn auth() -> ChatgptAuth {
        ChatgptAuth {
            access_token: "test-bearer".into(),
            account_id: "test-account".into(),
            refresh_token: None,
            expires_at: None,
            id_token: None,
            residency: Some("test-residency".into()),
        }
    }

    async fn server(
        status: u16,
        body: &'static str,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/backend-api/wham/usage",
            listener.local_addr().unwrap()
        );
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let n = stream.read(&mut request).await.unwrap();
            tokio::time::sleep(delay).await;
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            String::from_utf8(request[..n].to_vec()).unwrap()
        });
        (url, task)
    }

    #[tokio::test]
    async fn quota_http_headers_and_both_windows() {
        let (url, task) = server(200, r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":18000,"reset_after_seconds":120},"secondary_window":{"used_percent":25,"limit_window_seconds":604800,"reset_at":2000000000}}}"#, Duration::ZERO).await;
        let quota = fetch_with_auth(&reqwest::Client::new(), &url, &auth(), TIMEOUT)
            .await
            .unwrap();
        let limits = quota.rate_limit.unwrap();
        assert_eq!(
            limits.primary_window.as_ref().unwrap().used_percent,
            Some(10.0)
        );
        assert!(limits.primary_window.unwrap().reset_at.is_some());
        assert_eq!(limits.secondary_window.unwrap().reset_at, Some(2000000000));
        let request = task.await.unwrap().to_lowercase();
        assert!(request.starts_with("get /backend-api/wham/usage "));
        for header in [
            "authorization: bearer test-bearer",
            "chatgpt-account-id: test-account",
            "x-openai-internal-codex-residency: test-residency",
            "originator:",
        ] {
            assert!(request.contains(header), "missing {header}");
        }
        assert!(!request.contains("x-xai-token-auth"));
        assert!(!request.contains("x-userid"));
    }

    #[tokio::test]
    async fn quota_http_errors_are_redacted_and_timeout_is_bounded() {
        for (status, body) in [
            (401, "secret response"),
            (500, "secret response"),
            (200, "not json: secret response"),
        ] {
            let (url, task) = server(status, body, Duration::ZERO).await;
            let error = fetch_with_auth(&reqwest::Client::new(), &url, &auth(), TIMEOUT)
                .await
                .unwrap_err();
            assert!(!error.contains("secret"));
            task.await.unwrap();
        }
        let (url, task) = server(200, "{}", Duration::from_millis(100)).await;
        assert!(
            fetch_with_auth(
                &reqwest::Client::new(),
                &url,
                &auth(),
                Duration::from_millis(10)
            )
            .await
            .is_err()
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn quota_rejects_expired_or_missing_credentials_before_http() {
        let mut expired = auth();
        expired.expires_at = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
        for credentials in [
            expired,
            ChatgptAuth {
                access_token: String::new(),
                ..auth()
            },
            ChatgptAuth {
                account_id: String::new(),
                ..auth()
            },
        ] {
            let error = fetch_with_auth(
                &reqwest::Client::new(),
                "http://127.0.0.1:1",
                &credentials,
                TIMEOUT,
            )
            .await
            .unwrap_err();
            assert!(error.contains("credentials expired or missing"));
        }
        let fresh = super::super::oauth::ensure_fresh(auth()).await.unwrap();
        assert_eq!(fresh.access_token, "test-bearer");
    }

    #[test]
    fn quota_provider_stamp_uses_backend_and_auth_not_openai_family() {
        use crate::agent::config::{ModelEntry, ModelInfo};
        let mut model = ModelEntry {
            info: ModelInfo::default(),
            mtls_cert_dir: None,
            api_key: None,
            env_key: None,
            auth_provider: None,
            api_base_url: None,
        };
        let mut meta = serde_json::Map::new();
        for (url, expected) in [
            ("https://api.openai.com/v1", "unsupported"),
            (super::super::CODEX_BASE_URL, "chatgpt"),
            ("https://api.x.ai/v1", "grok"),
        ] {
            model.info.base_url = url.into();
            stamp_provider(&model, &mut meta);
            assert_eq!(meta[PROVIDER_META_KEY], expected);
        }
    }

    #[test]
    fn quota_missing_null_and_partial_fields_do_not_invent_usage() {
        for fixture in ["{}", r#"{"rate_limit":null}"#] {
            assert!(
                serde_json::from_str::<Quota>(fixture)
                    .unwrap()
                    .rate_limit
                    .is_none()
            );
        }
        let parsed: Quota =
            serde_json::from_str(r#"{"rate_limit":{"primary_window":{},"secondary_window":null}}"#)
                .unwrap();
        let limits = parsed.rate_limit.unwrap();
        assert!(limits.primary_window.unwrap().used_percent.is_none());
        assert!(limits.secondary_window.is_none());
    }
}
