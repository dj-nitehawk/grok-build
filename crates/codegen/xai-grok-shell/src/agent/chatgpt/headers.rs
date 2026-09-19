use std::sync::Arc;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use xai_grok_sampler::{HeaderInjector, SharedHeaderInjector};

use super::ORIGINATOR;
use super::store::SharedStore;

pub(super) const ACCOUNT_ID_HEADER: HeaderName = HeaderName::from_static("chatgpt-account-id");
pub(super) const RESIDENCY_HEADER: HeaderName =
    HeaderName::from_static("x-openai-internal-codex-residency");
pub(super) const ORIGINATOR_HEADER: HeaderName = HeaderName::from_static("originator");

#[derive(Debug)]
pub(crate) struct ChatGPTHeaderInjector {
    store: SharedStore,
    inner: Option<SharedHeaderInjector>,
}

impl ChatGPTHeaderInjector {
    pub(crate) fn shared(
        store: SharedStore,
        inner: Option<SharedHeaderInjector>,
    ) -> SharedHeaderInjector {
        Arc::new(Self { store, inner })
    }
}

impl HeaderInjector for ChatGPTHeaderInjector {
    fn auth_provider_name(&self) -> Option<&'static str> {
        Some(super::AUTH_PROVIDER_NAME)
    }

    fn inject(&self, headers: &mut HeaderMap) {
        if let Some(inner) = &self.inner {
            inner.inject(headers);
        }
        headers.remove(ACCOUNT_ID_HEADER);
        headers.remove(RESIDENCY_HEADER);
        let Some(auth) = self.store.snapshot() else {
            headers.remove(reqwest::header::AUTHORIZATION);
            return;
        };
        // The account may have changed since the resolver stamped its bearer.
        let expected = format!("Bearer {}", auth.access_token);
        if headers
            .get(reqwest::header::AUTHORIZATION)
            .map(|value| value.as_bytes())
            != Some(expected.as_bytes())
        {
            headers.remove(reqwest::header::AUTHORIZATION);
            return;
        }
        if let Ok(value) = HeaderValue::from_str(&auth.account_id) {
            headers.insert(ACCOUNT_ID_HEADER, value);
        }
        if let Some(residency) = auth.residency.as_deref()
            && let Ok(value) = HeaderValue::from_str(residency)
        {
            headers.insert(RESIDENCY_HEADER, value);
        }
        if !headers.contains_key(ORIGINATOR_HEADER) {
            headers.insert(ORIGINATOR_HEADER, HeaderValue::from_static(ORIGINATOR));
        }
    }

    fn set_span_parent(&self, span: &tracing::Span, traceparent: &str) {
        if let Some(inner) = &self.inner {
            inner.set_span_parent(span, traceparent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_test_support::EnvGuard;

    #[tokio::test]
    #[serial_test::serial]
    async fn long_lived_resolver_observes_logout_and_account_switch() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set("GROK_HOME", dir.path());
        let auth = super::super::store::ChatgptAuth {
            access_token: "old".into(),
            refresh_token: None,
            expires_at: None,
            account_id: "old-account".into(),
            id_token: None,
            residency: Some("us".into()),
        };
        super::super::store::save(&auth).unwrap();
        let store = SharedStore::new();
        let resolver = super::super::bearer::ChatGPTBearerResolver::shared(store.clone());
        let injector = ChatGPTHeaderInjector::shared(store, None);
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer old"),
        );
        injector.inject(&mut headers);
        assert_eq!(headers[ACCOUNT_ID_HEADER], "old-account");
        let mut next = auth;
        next.access_token = "new".into();
        next.account_id = "new-account".into();
        next.residency = None;
        super::super::store::save(&next).unwrap();
        injector.inject(&mut headers);
        assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
        assert!(!headers.contains_key(ACCOUNT_ID_HEADER));
        assert!(!headers.contains_key(RESIDENCY_HEADER));
        assert_eq!(resolver.current_bearer().as_deref(), Some("new"));
        super::super::store::clear().unwrap();
        resolver.prepare_for_send().await;
        assert!(resolver.current_bearer().is_none());
        assert!(!super::super::store::auth_path().exists());
    }
}
