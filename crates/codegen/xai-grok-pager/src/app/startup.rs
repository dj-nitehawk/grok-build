//! TUI startup helpers that keep network work off the critical path to first paint.
//!
//! Auth refresh and early models/settings prefetch still run at process start, but
//! the pager no longer awaits them before local leader resolution, session
//! materialization, or terminal init. Prefetch is joined just before ACP connect
//! so the agent still receives remote settings when the network is healthy.
//!
//! First paint shows a real welcome shell (non-minimal) so startup feels instant;
//! input is not live until ACP connect finishes and the event loop starts.
//!
//! Fork-owned TTFP surface: keep new startup logic here and only thin call-sites
//! in `app::run` / `event_loop` / `acp::connect`.

use std::time::Duration;

use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::widgets::Paragraph;
use xai_acp_lib::acp_channels;
use xai_grok_login::GrokComConfig;
use xai_grok_shell::agent::config::UiConfig;
use xai_grok_shell::agent::remote_config::settings_get::{self, SettingsQuery};
use xai_grok_shell::util::config::RemoteSettings;
use xai_grok_telemetry::region;
use xai_grok_telemetry::region::Parent;

use super::app_view::{AppView, AuthState};
use super::{EARLY_PREFETCH_WAIT, PagerTerminal, ScreenMode};
use crate::acp::model_state::ModelState;

/// Handle returned by [`kick_auth_and_prefetch`] so join can consume the same query.
pub struct EarlyPrefetch {
    query: SettingsQuery,
    grok_com_config: GrokComConfig,
}

/// Kick auth refresh (background) and early settings prefetch (current credentials).
///
/// Does **not** await OIDC refresh before launching prefetch. Prefetch uses the
/// on-disk/current token so models/settings can overlap session setup; a
/// successful background refresh warms credentials for later agent work.
pub fn kick_auth_and_prefetch(
    grok_com_config: GrokComConfig,
    proxy_base_url: String,
) -> EarlyPrefetch {
    let auth_cfg = grok_com_config.clone();
    tokio::spawn(async move {
        let _ = tokio::time::timeout(
            xai_grok_shell::http::STARTUP_AUTH_REFRESH_TIMEOUT,
            xai_grok_login::try_ensure_fresh_auth(&auth_cfg, proxy_base_url),
        )
        .await;
    });
    let query = SettingsQuery::resolve(None, Some(grok_com_config.clone()));
    settings_get::warm_startup_settings(query.clone());
    EarlyPrefetch {
        query,
        grok_com_config,
    }
}

/// Join the process-global prefetch for remote settings (capped wait).
pub async fn join_early_prefetch(early: EarlyPrefetch) -> Option<RemoteSettings> {
    let EarlyPrefetch {
        query,
        grok_com_config,
    } = early;
    if !settings_get::is_eligible(&query) {
        return None;
    }
    let wait_started = std::time::Instant::now();
    let _wait_span = region!("startup.prefetch_join_wait", Parent::Inherit);
    let warmed_auth = query.auth().cloned();
    let wait = {
        let _settings = region!(
            "startup.prefetch_join_wait.settings",
            Parent::Explicit(_wait_span.span())
        );
        settings_get::await_startup_settings(
            query,
            EARLY_PREFETCH_WAIT,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
    };
    let settings = settings_get::consume_wait(wait, warmed_auth.as_ref(), &grok_com_config);
    xai_grok_telemetry::startup::record_prefetch_wait(wait_started.elapsed());
    settings
}

/// Apply process-global remote side effects after prefetch join.
pub fn apply_remote_settings_caches(remote: Option<&RemoteSettings>) {
    xai_grok_shell::util::config::cache_remote_auto_mode(remote.and_then(|s| s.auto_mode.clone()));
    xai_grok_shell::util::config::cache_remote_prompt_suggestions(
        remote.and_then(|s| s.prompt_suggestions.clone()),
    );
    xai_grok_shell::util::config::set_remote_campaigns_from_settings(remote);
}

/// Reload effective config only when remote settings may have seeded campaigns.
pub fn reload_config_after_remote(
    existing: toml::Value,
    remote: Option<&RemoteSettings>,
) -> anyhow::Result<toml::Value> {
    if remote.is_some() {
        xai_grok_shell::config::load_effective_config()
            .map_err(|e| anyhow::anyhow!("Failed to load config: {e}"))
    } else {
        Ok(existing)
    }
}

/// `UiConfig` from an already-loaded effective config root (no disk re-read).
pub fn ui_config_from_effective(root: &toml::Value) -> UiConfig {
    let Some(ui_value) = root.get("ui").cloned() else {
        return UiConfig::default();
    };
    ui_value.try_into::<UiConfig>().unwrap_or_default()
}

/// Optional `[ui]` table clone from a preloaded effective root.
pub fn ui_table_from_effective(root: &toml::Value) -> Option<toml::Value> {
    root.get("ui").cloned()
}

/// Optional CLI bool from a preloaded effective root (`[cli].key`).
pub fn cli_bool_from_effective(root: &toml::Value, key: &str) -> Option<bool> {
    root.get("cli")?.get(key)?.as_bool()
}

/// Paint as soon as the terminal is live so the user is not on a blank screen
/// during prefetch join / ACP connect (best-TTFF path).
///
/// Non-minimal: real welcome layout (not interactive; event loop owns input).
/// Minimal: single-line skeleton (no welcome surface in that mode).
///
/// Call [`discard_pending_input`] after connect so keys typed on the frozen
/// frame are not applied when the live loop starts.
pub fn paint_connecting_frame(
    terminal: &mut PagerTerminal,
    raw_config: &toml::Value,
    screen_mode: ScreenMode,
) {
    if screen_mode.is_minimal() {
        paint_minimal_connecting_skeleton(terminal);
        return;
    }

    // Placeholder ACP channel: nothing should send; hold agent side open.
    let (client, _agent_hold) = acp_channels();
    let mut shell = AppView::new(
        client.tx,
        ModelState::default(),
        Vec::new(),
        crate::render::draw::EscapeWriter::disconnected(),
    );
    shell.auth_state = AuthState::Done;
    shell.screen_mode = screen_mode;
    shell.current_ui = ui_config_from_effective(raw_config);
    crate::appearance::cache::prime(&shell.current_ui);
    shell.welcome_prompt.set_screen_mode(screen_mode);
    shell.welcome_prompt_focused = true;
    shell.show_toast("Connecting…");
    shell.draw(terminal);
}

fn paint_minimal_connecting_skeleton(terminal: &mut PagerTerminal) {
    let theme = crate::theme::Theme::current();
    // Best-effort: a paint failure must not abort startup.
    let _ = terminal.draw(|frame| {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Length(1),
                Constraint::Percentage(55),
            ])
            .split(area);
        let label = Paragraph::new("Starting…")
            .style(theme.muted())
            .alignment(Alignment::Center);
        frame.render_widget(label, chunks[1]);
    });
}

/// Drop key/paste/resize events queued while the connecting frame was frozen.
pub fn discard_pending_input() {
    while crossterm::event::poll(Duration::ZERO).unwrap_or(false) {
        let _ = crossterm::event::read();
    }
}
