//! No-op external OTEL stream when feature `export-otel` is off.
//!
//! Reuses always-on [`config`] / [`schema`] / [`truncate`] so product event
//! types and shell config resolution keep stable paths; runtime emit is a
//! pure no-op and no OTLP crates are linked from this facade.

#[path = "external/config.rs"]
pub mod config;
#[path = "external/schema.rs"]
pub mod schema;
#[path = "external/truncate.rs"]
pub mod truncate;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

pub use config::{ContentGates, ExternalOtelConfig, ExternalOtelFileConfig};

/// Identity attributes (plain id strings). Stored nowhere when export is off.
#[derive(Debug, Clone, Default)]
pub struct IdentityAttrs {
    pub user_id: Option<String>,
    /// OAuth/gateway email. Present for field compatibility with export-on;
    /// unused when export is off.
    pub email: Option<String>,
    pub organization_id: Option<String>,
    pub team_id: Option<String>,
    pub deployment_id: Option<String>,
}

impl IdentityAttrs {
    pub fn from_snapshot(snapshot: &xai_grok_auth::CredentialSnapshot) -> Self {
        Self {
            user_id: snapshot.user_id.clone(),
            email: None,
            organization_id: snapshot.organization_id.clone(),
            team_id: snapshot.team_id.clone(),
            deployment_id: snapshot.deployment_id.clone(),
        }
    }
}

/// Remote-settings policy (restrictive-only). No-op target when export is off.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExternalOtelRemotePolicy {
    pub force_disable: bool,
    pub lock_content_gates: bool,
}

// Settings gate mirrors the real module so shell tests and leader preinit keep
// the same fail-closed / open / bounded-window semantics even when no exporter
// is linked.
static SETTINGS_RESOLVED: AtomicBool = AtomicBool::new(true);

const DEFAULT_SETTINGS_GATE_MAX_WAIT: Duration = Duration::from_secs(30);

static SETTINGS_GATE_MAX_WAIT_MS: AtomicU64 =
    AtomicU64::new(DEFAULT_SETTINGS_GATE_MAX_WAIT.as_millis() as u64);

static GATE_CLOSED_AT_MS: AtomicU64 = AtomicU64::new(0);

fn process_uptime_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    u64::try_from(
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

/// Set the bound on the fail-closed window.
pub fn set_settings_gate_max_wait(max_wait: Duration) {
    SETTINGS_GATE_MAX_WAIT_MS.store(
        u64::try_from(max_wait.as_millis()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
}

/// The current bound on the fail-closed window.
pub fn settings_gate_max_wait() -> Duration {
    Duration::from_millis(SETTINGS_GATE_MAX_WAIT_MS.load(Ordering::Relaxed))
}

/// Initialize the external stream. No-op (export compiled out).
pub fn init(_cfg: Option<ExternalOtelConfig>) {}

/// Close the settings gate (leader preinit + account switch).
pub fn suppress_external_otel_until_settings() {
    GATE_CLOSED_AT_MS.store(process_uptime_ms(), Ordering::Relaxed);
    SETTINGS_RESOLVED.store(false, Ordering::Release);
}

/// Open the settings gate.
pub fn mark_external_otel_settings_resolved() {
    SETTINGS_RESOLVED.store(true, Ordering::Release);
}

/// Read the settings gate (resolved OR bounded window expired).
#[inline]
pub fn is_settings_gate_open() -> bool {
    SETTINGS_RESOLVED.load(Ordering::Acquire) || settings_gate_window_expired()
}

fn settings_gate_window_expired() -> bool {
    let waited = process_uptime_ms().saturating_sub(GATE_CLOSED_AT_MS.load(Ordering::Relaxed));
    waited >= SETTINGS_GATE_MAX_WAIT_MS.load(Ordering::Relaxed)
}

/// Stream never active when export is compiled out.
pub fn is_active() -> bool {
    false
}

/// Map and emit one typed telemetry event. No-op.
pub fn emit<T: crate::events::TelemetryEvent>(_data: &T) {}

/// Update identity attrs. No-op.
pub fn set_identity(_attrs: IdentityAttrs) {}

/// Apply remote policy. No-op.
pub fn apply_remote_policy(_policy: ExternalOtelRemotePolicy) {}

/// Flush providers. No-op.
pub fn flush() {}

/// Shutdown providers. No-op.
pub fn shutdown() {}
