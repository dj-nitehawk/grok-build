use std::sync::Arc;

use xai_grok_sampler::{BearerResolver, SharedBearerResolver};

use super::store::SharedStore;

#[derive(Debug)]
pub(crate) struct ChatGPTBearerResolver {
    store: SharedStore,
}

impl ChatGPTBearerResolver {
    pub(crate) fn shared(store: SharedStore) -> SharedBearerResolver {
        Arc::new(Self { store })
    }
}

impl BearerResolver for ChatGPTBearerResolver {
    fn auth_provider_name(&self) -> Option<&'static str> {
        Some(super::AUTH_PROVIDER_NAME)
    }

    fn current_bearer(&self) -> Option<String> {
        self.store
            .snapshot()
            .filter(|auth| {
                auth.expires_at
                    .is_none_or(|expiry| expiry > chrono::Utc::now())
            })
            .map(|auth| auth.access_token)
    }

    fn prepare_for_send(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if let Err(error) = self.store.fresh().await {
                tracing::debug!(error = %error, "chatgpt: credential preparation failed");
            }
        })
    }
}

#[derive(Debug)]
pub(crate) struct DisabledBearerResolver;

impl BearerResolver for DisabledBearerResolver {
    fn current_bearer(&self) -> Option<String> {
        None
    }
}
