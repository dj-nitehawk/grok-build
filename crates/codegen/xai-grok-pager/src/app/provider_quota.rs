//! Provider-aware Alt+Q state, admission and completion, isolated from billing.
use super::{
    actions::Effect,
    app_view::{ActiveView, AppView},
};
use std::time::Instant;
use xai_grok_shell::agent::chatgpt::quota::{self, Quota};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider {
    Grok,
    Chatgpt,
    Unsupported,
}

pub(crate) fn provider(models: &crate::acp::model_state::ModelState) -> Provider {
    match models
        .current
        .as_ref()
        .and_then(|id| models.available.get(id))
        .and_then(|model| model.meta.as_ref())
        .and_then(|meta| meta.get(quota::PROVIDER_META_KEY))
        .and_then(|value| value.as_str())
    {
        Some("grok") => Provider::Grok,
        Some("chatgpt") => Provider::Chatgpt,
        _ => Provider::Unsupported,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub identity: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Default)]
pub struct State {
    identity: Option<String>,
    pub snapshot: Option<Quota>,
    fetched_at: Option<Instant>,
    in_flight: Option<Request>,
    generation: u64,
    refresh_failed: bool,
    authentication_checked_at: Option<Instant>,
}

impl State {
    pub(crate) fn sync_authentication(&mut self, force: bool) {
        let now = Instant::now();
        if (self.identity.is_some() || self.in_flight.is_some() || self.snapshot.is_some())
            && (force || self.authentication_check_due(now))
        {
            self.authentication_checked_at = Some(now);
            self.sync_identity(quota::identity());
        }
    }

    fn authentication_check_due(&self, now: Instant) -> bool {
        self.authentication_checked_at
            .is_none_or(|at| now.duration_since(at) >= std::time::Duration::from_secs(1))
    }

    fn sync_identity(&mut self, identity: Option<String>) {
        if self.identity != identity {
            self.identity = identity;
            self.snapshot = None;
            self.fetched_at = None;
            self.in_flight = None;
            self.refresh_failed = false;
        }
    }

    fn begin(&mut self, identity: Option<String>, now: Instant) -> Option<Request> {
        self.sync_identity(identity);
        if self.in_flight.is_some()
            || self.fetched_at.is_some_and(|at| {
                now.duration_since(at) < super::dispatch::billing::BILLING_CACHE_TTL
            })
        {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        let request = Request {
            identity: self.identity.clone(),
            generation: self.generation,
        };
        self.in_flight = Some(request.clone());
        Some(request)
    }

    fn finish(
        &mut self,
        request: Request,
        result: Result<Quota, String>,
        identity: Option<String>,
        now: Instant,
    ) {
        self.sync_identity(identity);
        if self.in_flight.as_ref() != Some(&request) {
            return;
        }
        self.in_flight = None;
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.fetched_at = Some(now);
                self.refresh_failed = false;
            }
            Err(_) => {
                self.fetched_at = None;
                self.refresh_failed = true;
            }
        }
    }

    pub(crate) fn chip(&self) -> String {
        if self.in_flight.is_some() {
            return "refreshing...".into();
        }
        let chip = crate::views::credit_bar::format_chatgpt_quota(
            self.snapshot.as_ref(),
            chrono::Utc::now().timestamp(),
        );
        if self.refresh_failed {
            if self.snapshot.is_some() {
                format!("{chip} (stale; refresh failed)")
            } else {
                "ChatGPT quota unavailable (refresh failed)".into()
            }
        } else {
            chip
        }
    }
}

pub(crate) fn refresh(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get(&id) else {
        return vec![];
    };
    match provider(&agent.session.models) {
        Provider::Grok if app.usage_visible => {
            super::dispatch::billing::fetch_billing_if_allowed(app, id, true)
        }
        Provider::Chatgpt => app
            .chatgpt_quota
            .begin(quota::identity(), Instant::now())
            .map(|request| vec![Effect::FetchChatgptQuota { request }])
            .unwrap_or_default(),
        _ => vec![],
    }
}

pub(crate) fn complete(
    app: &mut AppView,
    request: Request,
    result: Result<Quota, String>,
) -> Vec<Effect> {
    app.chatgpt_quota
        .finish(request, result, quota::identity(), Instant::now());
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn quota_cache_coalesces_expires_and_retries_without_losing_snapshot() {
        let mut state = State::default();
        let now = Instant::now();
        let identity = Some("test-account".to_owned());
        let first = state.begin(identity.clone(), now).unwrap();
        assert!(state.begin(identity.clone(), now).is_none());
        state.finish(first, Ok(Quota::default()), identity.clone(), now);
        assert!(
            state
                .begin(identity.clone(), now + Duration::from_secs(59))
                .is_none()
        );
        let expired = state
            .begin(identity.clone(), now + Duration::from_secs(60))
            .unwrap();
        state.finish(
            expired,
            Err("offline".into()),
            identity.clone(),
            now + Duration::from_secs(60),
        );
        assert!(state.snapshot.is_some());
        assert!(
            state
                .begin(identity, now + Duration::from_secs(60))
                .is_some()
        );
    }

    #[test]
    fn quota_failures_are_visible_and_success_clears_them() {
        let mut state = State::default();
        let now = Instant::now();
        let identity = Some("account".to_owned());
        let first = state.begin(identity.clone(), now).unwrap();
        state.finish(
            first,
            Err("secret error detail".into()),
            identity.clone(),
            now,
        );
        assert_eq!(state.chip(), "ChatGPT quota unavailable (refresh failed)");
        let retry = state.begin(identity.clone(), now).unwrap();
        assert_eq!(state.chip(), "refreshing...");
        let quota = serde_json::from_value(serde_json::json!({"rate_limit": {
            "primary_window": {"used_percent": 25, "limit_window_seconds": 18000}
        }}))
        .unwrap();
        state.finish(retry, Ok(quota), identity.clone(), now);
        assert_eq!(state.chip(), "25%");
        let later = now + Duration::from_secs(60);
        let refresh = state.begin(identity.clone(), later).unwrap();
        state.finish(refresh, Err("offline".into()), identity.clone(), later);
        assert_eq!(state.chip(), "25% (stale; refresh failed)");
        assert!(state.fetched_at.is_none());
        assert!(state.begin(identity, later).is_some());
        state.sync_identity(None);
        assert_eq!(state.chip(), "ChatGPT quota unavailable");
    }

    #[test]
    fn superseded_generation_cannot_replace_retry_for_same_identity() {
        let mut state = State::default();
        let now = Instant::now();
        let identity = Some("account".to_owned());
        let old = state.begin(identity.clone(), now).unwrap();
        state.finish(old.clone(), Err("timeout".into()), identity.clone(), now);
        let retry = state.begin(identity.clone(), now).unwrap();
        state.finish(old, Ok(Quota::default()), identity, now);
        assert_eq!(state.in_flight.as_ref(), Some(&retry));
        assert!(state.snapshot.is_none());
    }

    #[test]
    fn authentication_checks_are_throttled_without_quota_polling() {
        let now = Instant::now();
        let mut state = State::default();
        assert!(state.authentication_check_due(now));
        state.authentication_checked_at = Some(now);
        assert!(!state.authentication_check_due(now + Duration::from_millis(999)));
        assert!(state.authentication_check_due(now + Duration::from_secs(1)));
    }

    #[test]
    fn quota_identity_change_discards_snapshot_and_late_results() {
        let mut state = State::default();
        let now = Instant::now();
        let old = state.begin(Some("old".into()), now).unwrap();
        let new = state.begin(Some("new".into()), now).unwrap();
        state.finish(old, Ok(Quota::default()), Some("new".into()), now);
        assert_eq!(state.in_flight.as_ref(), Some(&new));
        assert!(state.snapshot.is_none());
        state.finish(new, Ok(Quota::default()), Some("new".into()), now);
        assert!(state.snapshot.is_some());
        state.sync_identity(None);
        assert!(state.snapshot.is_none());
        assert!(state.fetched_at.is_none());
    }

    #[test]
    fn quota_render_selection_never_leaks_grok_to_other_providers() {
        use crate::views::credit_bar::{PromptBorderChips, provider_border_chips};
        use agent_client_protocol as acp;
        let mut models = crate::acp::model_state::ModelState::default();
        let id = acp::ModelId::new("test");
        models.current = Some(id.clone());
        let mut state = State::default();
        state.begin(Some("test-account".into()), Instant::now());
        for (name, expected) in [
            ("grok", Some("Grok cached quota")),
            ("chatgpt", Some("refreshing...")),
            ("unsupported", None),
        ] {
            models.available.insert(
                id.clone(),
                acp::ModelInfo::new(id.clone(), "test").meta(serde_json::Map::from_iter([(
                    quota::PROVIDER_META_KEY.into(),
                    name.into(),
                )])),
            );
            let chips = PromptBorderChips {
                context: None,
                quota: Some("Grok cached quota".into()),
            };
            let result = provider_border_chips(chips, &models, Some(&state), false);
            assert_eq!(result.quota.as_deref(), expected);
        }
    }

    #[test]
    fn quota_model_routing_uses_metadata_not_names() {
        use agent_client_protocol as acp;
        let mut models = crate::acp::model_state::ModelState::default();
        let id = acp::ModelId::new("gpt-6-astra");
        models.current = Some(id.clone());
        models
            .available
            .insert(id.clone(), acp::ModelInfo::new(id.clone(), "Grok"));
        assert_eq!(provider(&models), Provider::Unsupported);
        for (name, expected) in [
            ("chatgpt", Provider::Chatgpt),
            ("grok", Provider::Grok),
            ("unsupported", Provider::Unsupported),
        ] {
            models.available.get_mut(&id).unwrap().meta = Some(serde_json::Map::from_iter([(
                quota::PROVIDER_META_KEY.into(),
                name.into(),
            )]));
            assert_eq!(provider(&models), expected);
        }
    }
}
