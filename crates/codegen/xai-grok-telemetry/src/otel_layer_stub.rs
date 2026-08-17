//! No-op OpenTelemetry layer when feature `export-otel` is off.
//!
//! Keeps type names and function paths stable for shell/pager/bin constructors
//! without linking opentelemetry / otlp / tracing-opentelemetry.

use std::sync::Arc;

use tracing_subscriber::registry::LookupSpan;
use xai_grok_auth::AuthCredentialProvider;

/// Configuration for [`build_otel_layer`] (type-stable with the real module).
pub struct OtelLayerConfig {
    pub credentials: Arc<dyn AuthCredentialProvider>,
    pub token_header_value: String,
    pub alpha_test_key: Option<String>,
    pub exporter: OtelExporterConfig,
}

/// Static identity of the client emitting telemetry.
#[derive(Debug, Clone, Copy)]
pub struct OtelClientInfo {
    pub client_name: &'static str,
    pub client_version: &'static str,
    pub service_version: &'static str,
    pub app_entrypoint: &'static str,
}

/// OTLP trace-export transport settings (unused when export is off).
#[derive(Debug, Default, Clone)]
pub struct OtelExporterConfig {
    pub traces_url: String,
    pub extra_headers: Vec<(String, String)>,
    pub export_interval: Option<std::time::Duration>,
    pub timeout: Option<std::time::Duration>,
    pub enabled: bool,
}

/// Builds a no-op tracing layer (export compiled out).
pub fn build_otel_layer<S>(
    _client: OtelClientInfo,
    _config: OtelLayerConfig,
) -> impl tracing_subscriber::layer::Layer<S>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    tracing_subscriber::layer::Identity::default()
}

/// Flush/shutdown (no-op). Also shuts down the external stream facade.
pub fn shutdown_otel() {
    crate::external::shutdown();
}

/// RAII guard that calls [`shutdown_otel`] on drop.
pub struct OtelGuard;

impl Drop for OtelGuard {
    fn drop(&mut self) {
        shutdown_otel();
    }
}

/// Create an [`OtelGuard`].
pub fn otel_guard() -> OtelGuard {
    OtelGuard
}
