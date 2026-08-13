//! No-op Sentry facade when feature `export-sentry` is off.
//!
//! Keeps `xai_grok_telemetry::sentry::{Config, init, flush_on_shutdown}` stable
//! so bin/pager call sites compile without linking the `sentry` crate.

/// Per-host config; everything that varies between binaries lives here.
pub struct Config {
    /// Sentry tag `client`, e.g. `"grok-pager"`.
    pub client: &'static str,
    pub client_version: &'static str,
    pub release: &'static str,
    /// When `true`, [`init`] is a no-op (also always true for this stub).
    pub disabled: bool,
}

/// Drop guard returned by [`init`]. Real Sentry holds client state; stub is unit.
#[derive(Debug, Default)]
pub struct ClientInitGuard;

/// Init Sentry. No-op when feature `export-sentry` is off.
pub fn init(_config: Config) -> ClientInitGuard {
    ClientInitGuard
}

/// Flush in-flight events. No-op when export is compiled out.
pub fn flush_on_shutdown() {}
