//! Telemetry engine for Grok Build sessions: product events + Mixpanel emission +
//! Sentry error reporting + OpenTelemetry tracing + structured unified log.
//!
//! Extracted from `xai-file-utils` per review feedback so telemetry has
//! its own ownership boundary (see CODEOWNERS) and so downstream consumers
//! that only want event tracking + inference metrics no longer pull in
//! Mixpanel/HTTP/identity dependencies.

mod appender;
pub mod client;
pub mod config;
pub mod context;
pub mod debug_log;
pub mod enums;
pub mod events;
// External customer OTEL stream: real exporters when `export-otel`, else stub.
#[cfg(feature = "export-otel")]
pub mod external;
#[cfg(not(feature = "export-otel"))]
#[path = "external_stub.rs"]
pub mod external;
pub mod hooks_log;
pub mod http;
pub mod id;
pub mod instrumentation;
pub mod memory_log;
pub mod memory_telemetry;
// Internal OTLP span layer: real when `export-otel`, else no-op Identity layer.
#[cfg(feature = "export-otel")]
pub mod otel_layer;
#[cfg(not(feature = "export-otel"))]
#[path = "otel_layer_stub.rs"]
pub mod otel_layer;
#[cfg(feature = "export-otel")]
pub(crate) mod otlp_http;
pub mod prompt_timing;
pub(crate) mod redact_common;
pub mod sampling_log;
// Sentry: real client when `export-sentry`, else no-op facade.
#[cfg(feature = "export-sentry")]
pub mod sentry;
#[cfg(not(feature = "export-sentry"))]
#[path = "sentry_stub.rs"]
pub mod sentry;
pub mod session_ctx;
pub mod session_metrics;
pub mod startup;
pub mod unified_log;

pub use client::{
    Metadata, TelemetryClient, UserContext, init, init_if_needed, is_enabled,
    is_session_metrics_enabled,
};
pub use events::TelemetryEvent;
pub use session_ctx::{
    EmitterOrigin, TelemetryCtx, emit_event, emit_event_with_origin, log_event, log_session_event,
    log_session_event_with_origin, with_session_ctx,
};
