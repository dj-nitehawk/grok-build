use std::collections::HashMap;
use std::time::Duration;

use axum::Router;
use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration as ChronoDuration, Utc};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::store::ChatgptAuth;
use super::{OAUTH_CALLBACK_PORT, OAUTH_CLIENT_ID, OAUTH_ISSUER, OAUTH_REDIRECT_URI, ORIGINATOR};

const AUTHORIZE_PATH: &str = "/oauth/authorize";
const TOKEN_PATH: &str = "/oauth/token";
const DEVICE_USERCODE_PATH: &str = "/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_PATH: &str = "/api/accounts/deviceauth/token";
const DEVICE_PAGE: &str = "https://auth.openai.com/codex/device";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const TOKEN_HTTP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DeviceUserCode {
    #[serde(alias = "usercode")]
    user_code: String,
    device_auth_id: String,
    #[serde(default, deserialize_with = "deserialize_device_interval")]
    interval: Option<u64>,
}

fn deserialize_device_interval<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Interval {
        Number(u64),
        Text(String),
    }
    match Option::<Interval>::deserialize(deserializer)? {
        Some(Interval::Number(value)) => Ok(Some(value)),
        Some(Interval::Text(value)) => value
            .trim()
            .parse()
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

#[derive(Deserialize)]
struct DeviceAuthorization {
    authorization_code: String,
    code_verifier: String,
}

pub(crate) async fn browser_login() -> anyhow::Result<ChatgptAuth> {
    let pkce = xai_grok_login::oidc::protocol::generate_pkce();
    let state = uuid::Uuid::now_v7().to_string();
    let listener = TcpListener::bind(("127.0.0.1", OAUTH_CALLBACK_PORT))
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                anyhow::anyhow!(
                    "port {OAUTH_CALLBACK_PORT} is already in use (OpenCode or Codex CLI?). \
                     Stop the other app, or run `grok chatgpt-login --device`"
                )
            } else {
                anyhow::anyhow!("failed to bind localhost:{OAUTH_CALLBACK_PORT}: {e}")
            }
        })?;

    let auth_url = authorize_url(&pkce.code_challenge, &state);
    eprintln!("Opening browser for ChatGPT login.");
    eprintln!("If it does not open, visit:\n  {auth_url}");
    if let Err(e) = webbrowser::open(&auth_url) {
        tracing::debug!(error = %e, "chatgpt: failed to open browser");
    }

    let code = wait_for_callback(listener, &state).await?;
    let tokens = exchange_code(&code, &pkce.code_verifier).await?;
    tokens_to_auth(tokens)
}

pub(crate) async fn device_login() -> anyhow::Result<ChatgptAuth> {
    let started = xai_grok_http::shared_client()
        .post(format!("{OAUTH_ISSUER}{DEVICE_USERCODE_PATH}"))
        .json(&serde_json::json!({ "client_id": OAUTH_CLIENT_ID }))
        .timeout(TOKEN_HTTP_TIMEOUT)
        .send()
        .await?;
    if !started.status().is_success() {
        anyhow::bail!("ChatGPT device login failed ({})", started.status());
    }
    let started: DeviceUserCode = started.json().await?;
    eprintln!("Enter this code at {DEVICE_PAGE}");
    eprintln!("  {}", started.user_code);
    eprintln!("Waiting for authorization...");

    complete_device_login(OAUTH_ISSUER, started).await
}

async fn complete_device_login(
    issuer: &str,
    started: DeviceUserCode,
) -> anyhow::Result<ChatgptAuth> {
    let interval = Duration::from_secs(started.interval.unwrap_or(5).max(1));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            anyhow::bail!("timed out waiting for ChatGPT device authorization");
        }
        let resp = xai_grok_http::shared_client()
            .post(format!("{issuer}{DEVICE_TOKEN_PATH}"))
            .json(&serde_json::json!({
                "device_auth_id": started.device_auth_id,
                "user_code": started.user_code,
            }))
            .timeout(TOKEN_HTTP_TIMEOUT.min(remaining))
            .send()
            .await?;
        if resp.status().is_success() {
            let authorization: DeviceAuthorization = resp.json().await?;
            let tokens = exchange_code_at(
                issuer,
                &authorization.authorization_code,
                &authorization.code_verifier,
                &format!("{issuer}/deviceauth/callback"),
            )
            .await?;
            return tokens_to_auth(tokens);
        }
        let status = resp.status();
        if matches!(status.as_u16(), 403 | 404) {
            tokio::time::sleep(
                interval.min(deadline.saturating_duration_since(tokio::time::Instant::now())),
            )
            .await;
            continue;
        }
        anyhow::bail!("ChatGPT device login failed ({status})");
    }
}

pub(crate) async fn ensure_fresh(auth: ChatgptAuth) -> anyhow::Result<ChatgptAuth> {
    ensure_fresh_with(auth, |token| async move { refresh_tokens(&token).await }).await
}

async fn ensure_fresh_with<F, Fut>(auth: ChatgptAuth, refresh: F) -> anyhow::Result<ChatgptAuth>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<TokenResponse>>,
{
    if !auth.needs_refresh() {
        return Ok(auth);
    }
    if let Some(refresh_token) = auth
        .refresh_token
        .as_ref()
        .filter(|token| !token.is_empty())
    {
        match refresh(refresh_token.clone())
            .await
            .and_then(|tokens| tokens_to_auth_preserving(tokens, &auth))
        {
            Ok(fresh) => return require_unexpired(fresh),
            Err(_) => tracing::debug!("chatgpt: refresh failed"),
        }
    }
    require_unexpired(auth)
}

fn require_unexpired(auth: ChatgptAuth) -> anyhow::Result<ChatgptAuth> {
    if auth.expires_at.is_some_and(|expiry| expiry <= Utc::now()) {
        anyhow::bail!("ChatGPT authentication expired; run `grok chatgpt-login` again");
    }
    Ok(auth)
}

pub(crate) fn identity_label(auth: &ChatgptAuth) -> String {
    claims_from_jwt(auth.id_token.as_deref().unwrap_or(&auth.access_token))
        .and_then(|v| {
            v.get("email")
                .or_else(|| v.get("preferred_username"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if auth.account_id.is_empty() {
                "ChatGPT".to_owned()
            } else {
                format!("ChatGPT account {}", auth.account_id)
            }
        })
}

pub(crate) fn account_id_from_jwt(token: &str) -> Option<String> {
    let claims = claims_from_jwt(token)?;
    if let Some(id) = claims.get("chatgpt_account_id").and_then(|v| v.as_str()) {
        return Some(id.to_owned());
    }
    claims
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

pub(crate) fn residency_from_jwt(token: &str) -> Option<String> {
    residency_claim_from_jwt(token).flatten()
}

// An absent claim preserves residency; an explicit no_constraint clears it.
fn residency_claim_from_jwt(token: &str) -> Option<Option<String>> {
    let claims = claims_from_jwt(token)?;
    let value = claims.get("chatgpt_compute_residency")?.as_str()?;
    Some(if value.is_empty() || value == "no_constraint" {
        None
    } else {
        Some(value.to_owned())
    })
}

fn authorize_url(code_challenge: &str, state: &str) -> String {
    let mut url = format!("{OAUTH_ISSUER}{AUTHORIZE_PATH}");
    url.push('?');
    let params = [
        ("response_type", "code"),
        ("client_id", OAUTH_CLIENT_ID),
        ("redirect_uri", OAUTH_REDIRECT_URI),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", ORIGINATOR),
        ("state", state),
    ];
    url.push_str(
        &params
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&"),
    );
    url
}

#[derive(Clone)]
struct CallbackState {
    tx: tokio::sync::mpsc::Sender<anyhow::Result<String>>,
    expected: String,
}

async fn handle_callback(
    axum::extract::State(state): axum::extract::State<CallbackState>,
    Query(query): Query<HashMap<String, String>>,
) -> Html<String> {
    if query.get("state") != Some(&state.expected) {
        return Html("<html><body><p>Invalid login state.</p></body></html>".to_owned());
    }
    let result = if query.contains_key("error") {
        Err(anyhow::anyhow!("ChatGPT authorization was denied"))
    } else if let Some(code) = query.get("code").filter(|code| !code.is_empty()) {
        Ok(code.clone())
    } else {
        return Html("<html><body><p>Missing login code.</p></body></html>".to_owned());
    };
    let html = if result.is_ok() {
        "<html><body><p>Signed in. You can close this window.</p></body></html>"
    } else {
        "<html><body><p>Login failed. Return to the terminal.</p></body></html>"
    };
    let _ = state.tx.try_send(result);
    Html(html.to_owned())
}

async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> anyhow::Result<String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<anyhow::Result<String>>(1);
    let app = Router::new()
        .route("/auth/callback", get(handle_callback))
        .with_state(CallbackState {
            tx,
            expected: expected_state.to_owned(),
        });
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });
    let result = tokio::time::timeout(CALLBACK_TIMEOUT, rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for ChatGPT login callback"))?
        .ok_or_else(|| anyhow::anyhow!("ChatGPT login callback channel closed"))?;
    let _ = shutdown_tx.send(());
    let _ = server.await;
    result
}

async fn exchange_code(code: &str, code_verifier: &str) -> anyhow::Result<TokenResponse> {
    exchange_code_at(OAUTH_ISSUER, code, code_verifier, OAUTH_REDIRECT_URI).await
}

async fn exchange_code_at(
    issuer: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<TokenResponse> {
    let resp = xai_grok_http::shared_client()
        .post(format!("{issuer}{TOKEN_PATH}"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", OAUTH_CLIENT_ID),
            ("code_verifier", code_verifier),
        ])
        .timeout(TOKEN_HTTP_TIMEOUT)
        .send()
        .await?;
    parse_token_response(resp).await
}

async fn refresh_tokens(refresh_token: &str) -> anyhow::Result<TokenResponse> {
    let resp = xai_grok_http::shared_client()
        .post(format!("{OAUTH_ISSUER}{TOKEN_PATH}"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OAUTH_CLIENT_ID),
        ])
        .timeout(TOKEN_HTTP_TIMEOUT)
        .send()
        .await?;
    parse_token_response(resp).await
}

async fn parse_token_response(resp: reqwest::Response) -> anyhow::Result<TokenResponse> {
    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("ChatGPT token endpoint failed ({status})");
    }
    Ok(resp.json().await?)
}

fn tokens_to_auth(tokens: TokenResponse) -> anyhow::Result<ChatgptAuth> {
    tokens_to_auth_preserving(
        tokens,
        &ChatgptAuth {
            access_token: String::new(),
            refresh_token: None,
            expires_at: None,
            account_id: String::new(),
            id_token: None,
            residency: None,
        },
    )
}

fn tokens_to_auth_preserving(
    tokens: TokenResponse,
    previous: &ChatgptAuth,
) -> anyhow::Result<ChatgptAuth> {
    if tokens.access_token.trim().is_empty() {
        anyhow::bail!("ChatGPT token response missing access_token");
    }
    let id_token = tokens
        .id_token
        .clone()
        .or_else(|| previous.id_token.clone());
    let jwt_source = id_token.as_deref().unwrap_or(tokens.access_token.as_str());
    let account_id = account_id_from_jwt(jwt_source)
        .or_else(|| account_id_from_jwt(&tokens.access_token))
        .unwrap_or_else(|| previous.account_id.clone());
    let residency = tokens
        .id_token
        .as_deref()
        .and_then(residency_claim_from_jwt)
        .or_else(|| residency_claim_from_jwt(&tokens.access_token))
        .unwrap_or_else(|| previous.residency.clone());
    let expires_at = tokens.expires_in.and_then(|secs| {
        i64::try_from(secs)
            .ok()
            .and_then(ChronoDuration::try_seconds)
            .and_then(|duration| Utc::now().checked_add_signed(duration))
    });
    Ok(ChatgptAuth {
        access_token: tokens.access_token,
        refresh_token: tokens
            .refresh_token
            .or_else(|| previous.refresh_token.clone()),
        expires_at,
        account_id,
        id_token,
        residency,
    })
}

fn claims_from_jwt(token: &str) -> Option<serde_json::Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt_from_payload(payload: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let body = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        format!("{header}.{body}.sig")
    }

    #[test]
    fn jwt_extracts_account_id_and_residency() {
        let token = jwt_from_payload(
            r#"{"chatgpt_account_id":"acct-nested","chatgpt_compute_residency":"us-west"}"#,
        );
        assert_eq!(account_id_from_jwt(&token).as_deref(), Some("acct-nested"));
        assert_eq!(residency_from_jwt(&token).as_deref(), Some("us-west"));
    }

    #[test]
    fn jwt_extracts_namespaced_account_id() {
        let token =
            jwt_from_payload(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-ns"}}"#);
        assert_eq!(account_id_from_jwt(&token).as_deref(), Some("acct-ns"));
    }

    #[test]
    fn residency_no_constraint_is_none() {
        let token = jwt_from_payload(r#"{"chatgpt_compute_residency":"no_constraint"}"#);
        assert_eq!(residency_from_jwt(&token), None);
    }

    fn cached_auth(expires_in: Option<i64>) -> ChatgptAuth {
        ChatgptAuth {
            access_token: "cached".to_owned(),
            refresh_token: None,
            expires_at: expires_in.map(|seconds| Utc::now() + ChronoDuration::seconds(seconds)),
            account_id: "account".to_owned(),
            id_token: None,
            residency: Some("us-west".to_owned()),
        }
    }

    #[tokio::test]
    async fn freshness_rejects_expired_without_refresh_but_not_unknown_expiry() {
        assert!(ensure_fresh(cached_auth(Some(-1))).await.is_err());
        assert!(ensure_fresh(cached_auth(None)).await.is_ok());
        assert!(ensure_fresh(cached_auth(Some(30))).await.is_ok());
    }

    #[tokio::test]
    async fn refresh_failure_only_allows_still_valid_credentials() {
        for (expiry, allowed) in [(-1, false), (30, true)] {
            let mut auth = cached_auth(Some(expiry));
            auth.refresh_token = Some("refresh".to_owned());
            let result = ensure_fresh_with(auth, |_| async { anyhow::bail!("unavailable") }).await;
            assert_eq!(result.is_ok(), allowed);
        }
    }

    #[tokio::test]
    async fn refresh_success_replaces_expired_credentials() {
        let mut auth = cached_auth(Some(-1));
        auth.refresh_token = Some("refresh".to_owned());
        let refreshed = ensure_fresh_with(auth, |_| async {
            Ok(TokenResponse {
                access_token: "new".to_owned(),
                refresh_token: None,
                id_token: None,
                expires_in: Some(3600),
            })
        })
        .await
        .unwrap();
        assert_eq!(refreshed.access_token, "new");
        assert!(!refreshed.needs_refresh());
    }

    #[tokio::test]
    async fn invalid_refresh_response_cannot_rescue_expired_credentials() {
        for access_token in ["", "replacement"] {
            let mut auth = cached_auth(Some(-1));
            auth.refresh_token = Some("refresh".to_owned());
            let result = ensure_fresh_with(auth, |_| async {
                Ok(TokenResponse {
                    access_token: access_token.to_owned(),
                    refresh_token: None,
                    id_token: None,
                    expires_in: Some(0),
                })
            })
            .await;
            assert!(result.is_err());
        }
    }

    #[test]
    fn residency_refresh_distinguishes_absent_and_explicit_clear() {
        let mut previous = cached_auth(None);
        previous.id_token = Some(jwt_from_payload(
            r#"{"chatgpt_compute_residency":"us-west"}"#,
        ));
        for (payload, expected) in [
            (r#"{}"#, Some("us-west")),
            (r#"{"chatgpt_compute_residency":"no_constraint"}"#, None),
            (r#"{"chatgpt_compute_residency":"eu"}"#, Some("eu")),
        ] {
            for from_id_token in [false, true] {
                let token = jwt_from_payload(payload);
                let auth = tokens_to_auth_preserving(
                    TokenResponse {
                        access_token: if from_id_token {
                            "opaque".to_owned()
                        } else {
                            token.clone()
                        },
                        refresh_token: None,
                        id_token: from_id_token.then_some(token),
                        expires_in: None,
                    },
                    &previous,
                )
                .unwrap();
                assert_eq!(auth.residency.as_deref(), expected);
            }
        }
    }

    #[tokio::test]
    #[ignore = "manual browser callback verification; listens on localhost:18765"]
    async fn manual_browser_callback_fixture() {
        let listener = TcpListener::bind("127.0.0.1:18765").await.unwrap();
        eprintln!("Dummy callback: http://127.0.0.1:18765/auth/callback; state=manual-dummy-state");
        let code = tokio::time::timeout(
            Duration::from_secs(180),
            wait_for_callback(listener, "manual-dummy-state"),
        )
        .await
        .expect("manual browser callback timed out after 180 seconds")
        .expect("manual browser callback failed");
        assert_eq!(code, "manual-dummy-code");
    }

    #[tokio::test]
    async fn callback_http_ignores_unrelated_requests_before_valid_code() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let callback = tokio::spawn(wait_for_callback(listener, "expected"));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        for query in [
            "state=wrong&code=unrelated",
            "state=wrong&error=%3Cscript%3Eevil%3C%2Fscript%3E",
            "error=denied",
            "state=expected",
            "state=expected&code=",
        ] {
            let response = client
                .get(format!("http://{address}/auth/callback?{query}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            assert!(!response.contains("<script>"));
            assert!(!callback.is_finished());
        }
        client
            .get(format!(
                "http://{address}/auth/callback?state=expected&code=accepted"
            ))
            .send()
            .await
            .unwrap();
        let code = tokio::time::timeout(Duration::from_secs(3), callback)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(code, "accepted");
    }

    #[tokio::test]
    async fn callback_http_valid_state_error_is_terminal_and_not_reflected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let callback = tokio::spawn(wait_for_callback(listener, "expected"));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let html = client.get(format!("http://{address}/auth/callback?state=expected&error=%3Cscript%3Eevil%3C%2Fscript%3E"))
            .send().await.unwrap().text().await.unwrap();
        assert!(!html.contains("<script>"));
        assert!(!html.contains("evil"));
        let error = tokio::time::timeout(Duration::from_secs(3), callback)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(!error.to_string().contains("evil"));
    }

    #[test]
    fn device_interval_accepts_upstream_string_and_legacy_number() {
        for interval in [serde_json::json!("5"), serde_json::json!(5)] {
            let code: DeviceUserCode = serde_json::from_value(serde_json::json!({
                "usercode": "code", "device_auth_id": "device", "interval": interval,
            }))
            .unwrap();
            assert_eq!(code.interval, Some(5));
            assert_eq!(code.user_code, "code");
        }
    }

    #[tokio::test]
    async fn device_http_polls_then_exchanges_authorization_code() {
        use axum::extract::{Form, State};
        use axum::routing::post;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let polls = Arc::new(AtomicUsize::new(0));
        let expected_redirect = format!("{issuer}/deviceauth/callback");
        let app = Router::new()
            .route(DEVICE_TOKEN_PATH, post(|State(polls): State<Arc<AtomicUsize>>, axum::Json(body): axum::Json<serde_json::Value>| async move {
                assert_eq!(body, serde_json::json!({"device_auth_id": "device", "user_code": "user"}));
                let attempt = polls.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    let status = if attempt == 0 { axum::http::StatusCode::FORBIDDEN } else { axum::http::StatusCode::NOT_FOUND };
                    return (status, axum::Json(serde_json::json!({})));
                }
                (axum::http::StatusCode::OK, axum::Json(serde_json::json!({
                    "authorization_code": "authorized", "code_verifier": "verifier", "code_challenge": "challenge",
                })))
            }))
            .route(TOKEN_PATH, post(move |Form(body): Form<HashMap<String, String>>| async move {
                assert_eq!(body.get("grant_type").unwrap(), "authorization_code");
                assert_eq!(body.get("code").unwrap(), "authorized");
                assert_eq!(body.get("code_verifier").unwrap(), "verifier");
                assert_eq!(body.get("redirect_uri").unwrap(), &expected_redirect);
                assert_eq!(body.get("client_id").unwrap(), OAUTH_CLIENT_ID);
                axum::Json(serde_json::json!({"access_token": "access", "expires_in": 3600}))
            }))
            .with_state(polls.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            complete_device_login(
                &issuer,
                DeviceUserCode {
                    user_code: "user".to_owned(),
                    device_auth_id: "device".to_owned(),
                    interval: Some(1),
                },
            ),
        )
        .await;
        server.abort();
        let auth = result.unwrap().unwrap();
        assert_eq!(auth.access_token, "access");
        assert_eq!(polls.load(Ordering::SeqCst), 3);
    }
}
